//! Audio extraction: any supported container -> 16 kHz mono FLAC in a temp file.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use indicatif::ProgressBar;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::cancel::{CancelToken, Cancelled};

pub const RATE: u32 = 16_000;

/// How to extract audio.
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    /// Built-in decoder, ffmpeg if it fails and is on PATH.
    #[default]
    Auto,
    /// Built-in decoder only.
    Native,
    /// ffmpeg only.
    Ffmpeg,
}

pub struct Extracted {
    pub path: PathBuf,
    pub duration_s: f64,
    /// Peak level 0..1, when known (built-in decoder only).
    pub peak: Option<f32>,
}

/// The input has nothing to transcribe: no audio track, or a track without samples.
/// Not a failure of the tool, so callers can tell it apart from real errors.
#[derive(Debug)]
pub struct NoAudio(pub String);

impl std::fmt::Display for NoAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NoAudio {}

/// Peak below this (about -60 dBFS) is treated as digital silence.
pub const SILENCE_PEAK: f32 = 0.001;

/// What a file's header says about its audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Probe {
    NoAudio,
    /// Length in seconds, when the header has it.
    Audio(Option<f64>),
}

/// Reads only the container header: is there an audio track, and how long is it. No decoding.
pub fn probe(input: &Path) -> Result<Probe> {
    let file = File::open(input).with_context(|| format!("cannot open {}", input.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = input.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .context("unsupported container")?;
    let Some(track) = format.default_track(TrackType::Audio) else { return Ok(Probe::NoAudio) };
    let rate = track.codec_params.as_ref().and_then(|p| p.audio()).and_then(|a| a.sample_rate);
    Ok(Probe::Audio(track.num_frames.zip(rate).map(|(n, r)| n as f64 / r as f64)))
}

/// Extracts the audio of `input` into `out`. Stops with [`Cancelled`] soon after `cancel` is set.
pub fn extract(input: &Path, out: &Path, backend: Backend, pb: &ProgressBar, cancel: &CancelToken) -> Result<Extracted> {
    let ffmpeg = || which::which("ffmpeg").ok();
    match backend {
        Backend::Native => native(input, out, pb, cancel),
        Backend::Ffmpeg => {
            let exe = ffmpeg().ok_or_else(|| anyhow!("ffmpeg not found on PATH"))?;
            with_ffmpeg(&exe, input, out, pb, cancel)
        }
        Backend::Auto => match native(input, out, pb, cancel) {
            Ok(x) => Ok(x),
            Err(e) if cancel.is_cancelled() || e.is::<NoAudio>() => Err(e),
            Err(e) => match ffmpeg() {
                Some(exe) => {
                    pb.println(format!("  built-in decoder failed ({e:#}), falling back to ffmpeg"));
                    log::info!("built-in decoder failed on {} ({e:#}), falling back to ffmpeg", input.display());
                    with_ffmpeg(&exe, input, out, pb, cancel)
                }
                None => Err(e.context("built-in decoder failed and ffmpeg is not on PATH")),
            },
        },
    }
}

fn native(input: &Path, out: &Path, pb: &ProgressBar, cancel: &CancelToken) -> Result<Extracted> {
    let file = File::open(input).with_context(|| format!("cannot open {}", input.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = input.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .context("unsupported container")?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| NoAudio("no audio track".into()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| anyhow!("audio track has no codec parameters"))?
        .clone();
    // Track length in frames, for the progress bar (time base is 1/sample_rate for audio tracks).
    let total = track.num_frames.filter(|_| params.sample_rate.is_some());
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .context("unsupported audio codec")?;

    if let Some(n) = total {
        pb.set_length(n);
    }
    let mut resampler: Option<Resampler> = None;
    let mut interleaved: Vec<f32> = Vec::new();
    let mut mono: Vec<f32> = Vec::new();
    let mut out_samples: Vec<i16> = Vec::new();
    let mut decoded_frames = 0u64;

    loop {
        if cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e).context("reading media"),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => continue,
            Err(e) => return Err(e).context("decoding audio"),
        };
        let channels = buf.spec().channels().count().max(1);
        let rate = buf.spec().rate();
        let frames = buf.frames();
        interleaved.resize(buf.samples_interleaved(), 0.0);
        buf.copy_to_slice_interleaved(&mut interleaved);

        mono.clear();
        mono.extend(interleaved.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
        let rs = resampler.get_or_insert_with(|| Resampler::new(rate, RATE));
        rs.push(&mono, &mut out_samples);

        decoded_frames += frames as u64;
        pb.set_position(decoded_frames);
    }
    let mut rs = resampler.ok_or_else(|| NoAudio("audio track has no samples".into()))?;
    rs.finish(&mut out_samples);
    if out_samples.is_empty() {
        return Err(NoAudio("audio track has no samples".into()).into());
    }
    let peak = out_samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32 / i16::MAX as f32;
    pb.set_message("encoding FLAC");
    write_flac(&out_samples, out)?;
    Ok(Extracted { path: out.to_path_buf(), duration_s: out_samples.len() as f64 / RATE as f64, peak: Some(peak) })
}

fn write_flac(samples: &[i16], out: &Path) -> Result<()> {
    let signal: Vec<i32> = samples.iter().map(|&s| s as i32).collect();
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, e)| anyhow!("flac config: {e:?}"))?;
    let source = flacenc::source::MemSource::from_samples(&signal, 1, 16, RATE as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| anyhow!("flac encoding failed: {e:?}"))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| anyhow!("flac encoding failed: {e:?}"))?;
    let mut bytes = sink.as_slice().to_vec();
    // flacenc counts the short last block in STREAMINFO's minimum block size; the spec excludes it,
    // and strict decoders (symphonia) then take the stream for variable-blocksize and fail.
    // Layout: "fLaC", 4-byte block header, min block size (u16 BE), max block size (u16 BE).
    if bytes.len() > 12 && &bytes[..4] == b"fLaC" {
        bytes.copy_within(10..12, 8);
    }
    std::fs::write(out, bytes).with_context(|| format!("cannot write {}", out.display()))
}

fn with_ffmpeg(exe: &Path, input: &Path, out: &Path, pb: &ProgressBar, cancel: &CancelToken) -> Result<Extracted> {
    let duration = probe_duration(exe, input);
    if let Some(d) = duration {
        pb.set_length((d * 1000.0) as u64);
    }
    pb.set_message("ffmpeg");
    let mut child = command(exe)
        .args(["-hide_banner", "-nostats", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", &RATE.to_string(), "-c:a", "flac", "-progress", "pipe:1"])
        .arg(out)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot start ffmpeg")?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut last_ms = 0u64;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Cancelled.into());
        }
        if let Some(v) = line.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<u64>().ok()) {
            last_ms = v / 1000;
            pb.set_position(last_ms);
        }
    }
    let output = child.wait_with_output().context("ffmpeg failed")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        if err.contains("does not contain any stream") || err.contains("matches no streams") {
            return Err(NoAudio("no audio track".into()).into());
        }
        bail!("ffmpeg exited with {}: {}", output.status, err.trim());
    }
    Ok(Extracted { path: out.to_path_buf(), duration_s: duration.unwrap_or(last_ms as f64 / 1000.0), peak: None })
}

fn probe_duration(ffmpeg: &Path, input: &Path) -> Option<f64> {
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    let out = command(&ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(input)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// A console program started without a console window of its own: a GUI app using this crate
/// would otherwise flash one per file on Windows.
fn command(exe: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Streaming windowed-sinc resampler (polyphase table, Blackman window).
struct Resampler {
    step: f64,
    half: usize,
    table: Vec<Vec<f32>>,
    buf: Vec<f32>,
    pos: f64,
}

const PHASES: usize = 256;

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        let ratio = from as f64 / to as f64;
        // Cutoff a bit below the output Nyquist, 16 zero crossings of the output-rate sinc per side.
        let cutoff = 0.5 * 0.92 / ratio.max(1.0);
        let half = (16.0 * ratio.max(1.0)).ceil() as usize;
        let table = (0..=PHASES)
            .map(|p| {
                let frac = p as f64 / PHASES as f64;
                let mut row: Vec<f64> = (0..2 * half)
                    .map(|j| {
                        let x = j as f64 - (half as f64 - 1.0) - frac;
                        let s = if x == 0.0 { 1.0 } else { (std::f64::consts::PI * 2.0 * cutoff * x).sin() / (std::f64::consts::PI * 2.0 * cutoff * x) };
                        let t = (x / half as f64).clamp(-1.0, 1.0);
                        let w = 0.42 + 0.5 * (std::f64::consts::PI * t).cos() + 0.08 * (2.0 * std::f64::consts::PI * t).cos();
                        s * w
                    })
                    .collect();
                let sum: f64 = row.iter().sum();
                row.iter_mut().for_each(|v| *v /= sum);
                row.into_iter().map(|v| v as f32).collect()
            })
            .collect();
        // Leading zeros so that output sample 0 is centred on input sample 0.
        Self { step: ratio, half, table, buf: vec![0.0; half], pos: half as f64 }
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<i16>) {
        self.buf.extend_from_slice(input);
        loop {
            let i0 = self.pos.floor() as usize;
            if i0 + self.half >= self.buf.len() {
                break;
            }
            let frac = self.pos - i0 as f64;
            let row = &self.table[(frac * PHASES as f64).round() as usize];
            let start = i0 + 1 - self.half;
            let acc: f32 = row.iter().zip(&self.buf[start..start + 2 * self.half]).map(|(a, b)| a * b).sum();
            out.push((acc.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            self.pos += self.step;
        }
        let drop = (self.pos.floor() as usize).saturating_sub(self.half);
        if drop > 0 {
            self.buf.drain(..drop);
            self.pos -= drop as f64;
        }
    }

    fn finish(&mut self, out: &mut Vec<i16>) {
        let tail = vec![0.0; self.half + 1];
        self.push(&tail, out);
    }
}

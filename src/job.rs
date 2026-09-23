//! Everything that happens to one input file: media -> audio -> Soniox -> .srt, and cleanup.
//! Knows nothing about batches, log files or the command line.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Value;

use crate::audio;
use crate::cancel::interrupted;
use crate::soniox::{self, Client, RemoteGuard};
use crate::srt;

/// What to do with a file.
#[derive(Debug, Clone)]
pub struct Options {
    /// Soniox async model.
    pub model: String,
    /// Language hints ("ru", "en", ...).
    pub languages: Vec<String>,
    pub strict_languages: bool,
    /// Names and terms that help recognition.
    pub context: Option<String>,
    /// Speaker diarization; also makes a speaker change start a new cue.
    pub diarization: bool,
    pub audio: audio::Backend,
    /// Where the temporary audio goes (default: system temp).
    pub temp_dir: Option<PathBuf>,
    /// Keep the temporary audio instead of deleting it.
    pub keep_audio: bool,
    /// Save the raw transcript as `<output>.soniox.json`.
    pub keep_json: bool,
    /// Overwrite an existing .srt.
    pub force: bool,
    pub layout: srt::Layout,
    /// Interval between transcription status checks.
    pub poll: Duration,
    /// Printed before progress lines, e.g. "[2/5]".
    pub prefix: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            model: "stt-async-v3".into(),
            languages: vec!["ru".into(), "en".into()],
            strict_languages: false,
            context: None,
            diarization: true,
            audio: audio::Backend::Auto,
            temp_dir: None,
            keep_audio: false,
            keep_json: false,
            force: false,
            layout: srt::Layout::default(),
            poll: Duration::from_secs(3),
            prefix: String::new(),
        }
    }
}

#[derive(Debug)]
pub enum Outcome {
    Written { srt: PathBuf, cues: usize, json: Option<PathBuf> },
    /// The .srt already exists and `force` is off.
    Skipped { srt: PathBuf },
}

/// Makes subtitles for `input` and writes them to `output`.
///
/// `input` is a media file, or a `.json` Soniox transcript (then no API call is made and `client` may be `None`).
/// Temp audio and everything created on Soniox are removed before returning, on success and on error.
pub fn process(input: &Path, output: &Path, opts: &Options, client: Option<&Client>) -> Result<Outcome> {
    if !input.is_file() {
        bail!("file not found: {}", input.display());
    }
    if output.exists() && !opts.force {
        return Ok(Outcome::Skipped { srt: output.to_path_buf() });
    }

    let mut json = None;
    let transcript = if is_transcript(input) {
        let text = std::fs::read_to_string(input)?;
        serde_json::from_str(&text).with_context(|| format!("{} is not a Soniox transcript", input.display()))?
    } else {
        let t = transcribe(input, opts, client.ok_or_else(soniox::missing_key)?)?;
        if opts.keep_json {
            let path = output.with_extension("soniox.json");
            std::fs::write(&path, serde_json::to_string_pretty(&t)?)
                .with_context(|| format!("cannot write {}", path.display()))?;
            json = Some(path);
        }
        t
    };

    let layout = srt::Layout { by_speaker: opts.diarization, ..opts.layout.clone() };
    let (text, cues) = srt::build(&transcript, &layout);
    std::fs::write(output, text).with_context(|| format!("cannot write {}", output.display()))?;
    Ok(Outcome::Written { srt: output.to_path_buf(), cues, json })
}

/// Media file -> Soniox transcript JSON.
pub fn transcribe(input: &Path, opts: &Options, client: &Client) -> Result<Value> {
    // The temp dir (and the audio inside) is removed when it goes out of scope, whatever happens.
    let mut tmp = match &opts.temp_dir {
        Some(d) => tempfile::Builder::new().prefix("sonisub-").tempdir_in(d),
        None => tempfile::Builder::new().prefix("sonisub-").tempdir(),
    }
    .context("cannot create temp dir")?;
    let stem = input.file_stem().map_or("audio".into(), |s| s.to_string_lossy().into_owned());
    let audio_path = tmp.path().join(format!("{stem}.flac"));

    let pb = bar(&opts.prefix, "extract", "{prefix} {msg:9} [{bar:30.cyan/blue}] {percent:>3}%  {elapsed}");
    let audio = audio::extract(input, &audio_path, opts.audio, &pb)?;
    pb.finish_with_message(format!("extracted {} ({})", fmt_dur(audio.duration_s), fmt_size(&audio.path)));
    if opts.keep_audio {
        tmp.disable_cleanup(true);
        pb.println(format!("  audio kept: {}", audio.path.display()));
    }

    // Deletes the uploaded file and the transcription on drop, including on `?` returns below.
    let mut remote = RemoteGuard { client, file_id: None, transcription_id: None, warnings: Vec::new() };

    let pb = bar(&opts.prefix, "upload", "{prefix} {msg:9} [{bar:30.cyan/blue}] {bytes}/{total_bytes}  {bytes_per_sec}");
    let file_id = client.upload(&audio.path, &pb)?;
    remote.file_id = Some(file_id.clone());
    pb.finish_with_message("uploaded");

    let cfg = soniox::transcription_config(
        &opts.model,
        &file_id,
        &opts.languages,
        opts.strict_languages,
        opts.diarization,
        opts.context.as_deref(),
    );
    let id = client.create(&cfg)?;
    remote.transcription_id = Some(id.clone());

    let pb = ProgressBar::new_spinner().with_prefix(opts.prefix.clone());
    pb.set_style(ProgressStyle::with_template("{prefix} {spinner} {msg}  {elapsed}").unwrap());
    pb.enable_steady_tick(Duration::from_millis(120));
    loop {
        if interrupted() {
            bail!("interrupted");
        }
        let st = client.status(&id)?;
        if st.status == "completed" {
            break;
        }
        pb.set_message(format!("transcribing ({})", st.status));
        sleep(opts.poll);
    }
    pb.set_message("downloading transcript");
    let transcript = client.transcript(&id)?;
    pb.finish_with_message("transcribed");

    remote.cleanup();
    Ok(transcript)
}

/// Default .srt path: next to the input (or in `out_dir`), same name; `clip.soniox.json` -> `clip.srt`.
pub fn default_output(input: &Path, out_dir: Option<&Path>) -> PathBuf {
    let name = input.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let stem = name
        .strip_suffix(".soniox.json")
        .map(str::to_string)
        .unwrap_or_else(|| input.file_stem().map_or(name.clone(), |s| s.to_string_lossy().into_owned()));
    let dir = out_dir.map_or_else(|| input.parent().unwrap_or(Path::new(".")).to_path_buf(), Path::to_path_buf);
    dir.join(format!("{stem}.srt"))
}

/// A saved Soniox transcript rather than media.
pub fn is_transcript(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

fn bar(prefix: &str, msg: &str, template: &str) -> ProgressBar {
    let pb = ProgressBar::new(0).with_prefix(prefix.to_string()).with_message(msg.to_string());
    pb.set_style(ProgressStyle::with_template(template).unwrap().progress_chars("=> "));
    pb
}

fn fmt_dur(s: f64) -> String {
    let s = s.round() as u64;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn fmt_size(p: &Path) -> String {
    let b = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) as f64;
    if b > 1e6 { format!("{:.1} MB", b / 1e6) } else { format!("{:.0} KB", b / 1e3) }
}

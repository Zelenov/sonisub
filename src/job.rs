//! Everything that happens to one input file: media -> audio -> Soniox -> .srt (and other formats), and cleanup.
//! Knows nothing about batches, log files or the command line.

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde_json::{Value, json};

use crate::audio;
use crate::cancel::interrupted;
use crate::soniox::{self, Client, RemoteGuard};
use crate::{premiere, srt};

/// What gets written for a file.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Subtitles: `clip.srt`.
    Srt,
    /// Premiere Pro transcript (Text panel > Transcript > ... > Import Static Transcript): `clip.premiere.json`.
    Premiere,
}

impl Format {
    /// Where this format goes for the output base path (`clip.srt` -> `clip.premiere.json`).
    pub fn path(self, output: &Path) -> PathBuf {
        match self {
            Format::Srt => output.to_path_buf(),
            Format::Premiere => output.with_extension("premiere.json"),
        }
    }
}

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
    /// Save the raw transcript as `<output>.soniox.json` (an empty one is always saved, as a marker).
    pub keep_json: bool,
    /// What to write; files that already exist are left alone unless `force`.
    pub formats: Vec<Format>,
    /// Overwrite existing outputs and ignore a saved `.soniox.json` (transcribe again).
    pub force: bool,
    pub layout: srt::Layout,
    /// Interval between transcription status checks.
    pub poll: Duration,
    /// Printed before progress lines, e.g. "[2/5]".
    pub prefix: String,
    /// Sent to Soniox as `client_reference_id`, to find this run's cost in the usage logs.
    pub reference: String,
    /// Draw stage bars here, under a batch's overall bar; they are cleared when the file is done.
    pub progress: Option<MultiProgress>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            model: "stt-async-v3".into(),
            languages: vec!["en".into(), "ru".into()],
            strict_languages: false,
            context: None,
            diarization: true,
            audio: audio::Backend::Auto,
            temp_dir: None,
            keep_audio: false,
            keep_json: true,
            formats: vec![Format::Srt],
            force: false,
            layout: srt::Layout::default(),
            poll: Duration::from_secs(3),
            prefix: String::new(),
            reference: "sonisub".into(),
            progress: None,
        }
    }
}

#[derive(Debug)]
pub enum Outcome {
    /// Outputs written (`files`; `cues` counts subtitles, 0 without .srt). `cached`: the transcript came
    /// from an existing `.soniox.json`, no API call. `uploaded_s`: seconds of audio sent to Soniox (what is paid for).
    Written { files: Vec<PathBuf>, cues: usize, json: Option<PathBuf>, cached: bool, uploaded_s: Option<f64> },
    /// Every output already exists and `force` is off.
    Skipped { files: Vec<PathBuf> },
    /// Nothing to transcribe: no audio track, or a track without samples.
    NoAudio { reason: String },
    /// Audio is there but has no words: digital silence (detected locally, `uploaded_s` is `None`),
    /// or Soniox found nothing. No .srt is written; an empty transcript is kept as `marker` so later
    /// runs (and plans) know the file is checked and nothing is paid for twice.
    NoSpeech { marker: Option<PathBuf>, cached: bool, uploaded_s: Option<f64> },
}

impl Outcome {
    /// Seconds of audio this call sent to Soniox.
    pub fn uploaded_s(&self) -> Option<f64> {
        match self {
            Outcome::Written { uploaded_s, .. } | Outcome::NoSpeech { uploaded_s, .. } => *uploaded_s,
            _ => None,
        }
    }
}

/// Where the transcript for `output` is kept: `clip.srt` -> `clip.soniox.json`.
pub fn transcript_path(output: &Path) -> PathBuf {
    output.with_extension("soniox.json")
}

enum Source {
    /// Fresh from Soniox, with the seconds of audio sent.
    Api(Value, f64),
    /// Saved earlier next to the output.
    Cache(Value, PathBuf),
    /// The input itself is a transcript.
    Input(Value),
}

/// Outputs of `formats` for the base path `output` that still have to be written (all of them with `force`).
pub fn missing_outputs(output: &Path, formats: &[Format], force: bool) -> Vec<(Format, PathBuf)> {
    formats.iter().map(|f| (*f, f.path(output))).filter(|(_, p)| force || !p.exists()).collect()
}

/// Makes subtitles for `input` and writes them to `output` (the .srt path; other formats go next to it,
/// see [`Format::path`]). Outputs that already exist are kept unless `force`.
///
/// `input` is a media file, or a `.json` Soniox transcript (then no API call is made and `client` may be `None`).
/// A `.soniox.json` already lying next to `output` is used instead of calling the API, unless `force`.
/// Temp audio and everything created on Soniox are removed before returning, on success and on error.
pub fn process(input: &Path, output: &Path, opts: &Options, client: Option<&Client>) -> Result<Outcome> {
    let meta = std::fs::metadata(input).map_err(|_| anyhow!("file not found: {}", input.display()))?;
    if !meta.is_file() {
        bail!("not a file: {}", input.display());
    }
    if meta.len() == 0 {
        bail!("empty file: {}", input.display());
    }
    let targets = missing_outputs(output, &opts.formats, opts.force);
    if targets.is_empty() {
        return Ok(Outcome::Skipped { files: opts.formats.iter().map(|f| f.path(output)).collect() });
    }

    let cache = transcript_path(output);
    let source = if is_transcript(input) {
        Source::Input(read_transcript(input)?)
    } else if cache.is_file() && !opts.force {
        Source::Cache(read_transcript(&cache).context("use --force to transcribe again")?, cache.clone())
    } else {
        let client = client.ok_or_else(soniox::missing_key)?;
        match transcribe(input, opts, client) {
            Ok(Some((t, secs))) => Source::Api(t, secs),
            Ok(None) => {
                // Digital silence: nothing was sent; the marker still tells later runs (and plans) it's checked.
                let marker = json!({"text": "", "tokens": [], "sonisub": "digital silence, not sent to Soniox"});
                std::fs::write(&cache, serde_json::to_string_pretty(&marker)?)
                    .with_context(|| format!("cannot write {}", cache.display()))?;
                return Ok(Outcome::NoSpeech { marker: Some(cache), cached: false, uploaded_s: None });
            }
            Err(e) => match e.downcast::<audio::NoAudio>() {
                Ok(no) => return Ok(Outcome::NoAudio { reason: no.0 }),
                Err(e) => return Err(e),
            },
        }
    };

    let (transcript, json, cached, uploaded_s) = match source {
        Source::Input(t) => (t, None, false, None),
        Source::Cache(t, path) => (t, Some(path), true, None),
        Source::Api(t, secs) => {
            // An empty transcript is always kept: it is the "already checked, no speech" marker.
            let keep = opts.keep_json || !srt::has_speech(&t);
            if keep {
                std::fs::write(&cache, serde_json::to_string_pretty(&t)?)
                    .with_context(|| format!("cannot write {}", cache.display()))?;
            }
            (t, keep.then_some(cache), false, Some(secs))
        }
    };

    if !srt::has_speech(&transcript) {
        return Ok(Outcome::NoSpeech { marker: json, cached, uploaded_s });
    }
    let layout = srt::Layout { by_speaker: opts.diarization, ..opts.layout.clone() };
    let mut cues = 0;
    let mut files = Vec::new();
    for (format, path) in targets {
        let text = match format {
            Format::Srt => {
                let (text, n) = srt::build(&transcript, &layout);
                cues = n;
                text
            }
            Format::Premiere => {
                let hint = opts.languages.first().map(String::as_str);
                let t = premiere::build(&transcript, &layout, hint).expect("the transcript has speech");
                serde_json::to_string_pretty(&t)?
            }
        };
        std::fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))?;
        files.push(path);
    }
    Ok(Outcome::Written { files, cues, json, cached, uploaded_s })
}

fn read_transcript(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    if text.trim().is_empty() {
        bail!("empty transcript file: {}", path.display());
    }
    let v: Value = serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", path.display()))?;
    if !v["tokens"].is_array() {
        bail!("{} is not a Soniox transcript (no \"tokens\")", path.display());
    }
    Ok(v)
}

/// Media file -> Soniox transcript JSON and the seconds of audio sent; `None` if the audio is digital
/// silence (nothing is uploaded then). Fails with [`audio::NoAudio`] when there is no audio at all.
pub fn transcribe(input: &Path, opts: &Options, client: &Client) -> Result<Option<(Value, f64)>> {
    // The temp dir (and the audio inside) is removed when it goes out of scope, whatever happens.
    let mut tmp = match &opts.temp_dir {
        Some(d) => tempfile::Builder::new().prefix("sonisub-").tempdir_in(d),
        None => tempfile::Builder::new().prefix("sonisub-").tempdir(),
    }
    .context("cannot create temp dir")?;
    let stem = input.file_stem().map_or("audio".into(), |s| s.to_string_lossy().into_owned());
    let audio_path = tmp.path().join(format!("{stem}.flac"));

    let pb = bar(opts, "extract", "{prefix} {msg:9} [{bar:30.cyan/blue}] {percent:>3}%  {elapsed}");
    let audio = audio::extract(input, &audio_path, opts.audio, &pb).inspect_err(|_| pb.finish_and_clear())?;
    finish(opts, &pb, format!("extracted {} ({})", fmt_dur(audio.duration_s), fmt_size(&audio.path)));
    if opts.keep_audio {
        tmp.disable_cleanup(true);
        pb.println(format!("  audio kept: {}", audio.path.display()));
    }
    if audio.peak.is_some_and(|p| p < audio::SILENCE_PEAK) {
        return Ok(None);
    }

    // Deletes the uploaded file and the transcription on drop, including on `?` returns below.
    let mut remote = RemoteGuard { client, file_id: None, transcription_id: None, warnings: Vec::new() };

    let pb = bar(opts, "upload", "{prefix} {msg:9} [{bar:30.cyan/blue}] {bytes}/{total_bytes}  {bytes_per_sec}");
    let file_id = client.upload(&audio.path, &pb).inspect_err(|_| pb.finish_and_clear())?;
    remote.file_id = Some(file_id.clone());
    finish(opts, &pb, "uploaded".into());

    let cfg = soniox::transcription_config(
        &opts.model,
        &file_id,
        &opts.languages,
        opts.strict_languages,
        opts.diarization,
        opts.context.as_deref(),
        &opts.reference,
    );
    let id = client.create(&cfg)?;
    remote.transcription_id = Some(id.clone());

    let pb = attach(opts, ProgressBar::new_spinner().with_prefix(opts.prefix.clone()));
    pb.set_style(ProgressStyle::with_template("{prefix} {spinner} {msg}  {elapsed}").unwrap());
    pb.enable_steady_tick(Duration::from_millis(120));
    // Clears the spinner however this loop is left.
    let _clear = ClearOnDrop(&pb);
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
    finish(opts, &pb, "transcribed".into());

    remote.cleanup();
    Ok(Some((transcript, audio.duration_s)))
}

/// Default .srt path: next to the input (or in `out_dir`), same name; `clip.soniox.json` -> `clip.srt`.
pub fn default_output(input: &Path, out_dir: Option<&Path>) -> PathBuf {
    let name = input.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let stem = strip_suffix(&name, ".soniox.json")
        .map(str::to_string)
        .unwrap_or_else(|| input.file_stem().map_or(name.clone(), |s| s.to_string_lossy().into_owned()));
    let dir = out_dir.map_or_else(|| input.parent().unwrap_or(Path::new(".")).to_path_buf(), Path::to_path_buf);
    dir.join(format!("{stem}.srt"))
}

/// The .srt base for an `-o` path given with any output extension: `clip.premiere.json` -> `clip.srt`.
pub fn output_base(path: &Path) -> PathBuf {
    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let stem = [".premiere.json", ".soniox.json", ".srt", ".json"]
        .iter()
        .find_map(|s| strip_suffix(&name, s))
        .unwrap_or(&name);
    path.with_file_name(format!("{stem}.srt"))
}

fn strip_suffix<'a>(name: &'a str, suffix: &str) -> Option<&'a str> {
    let cut = name.len().checked_sub(suffix.len())?;
    (name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(suffix) && cut > 0).then(|| &name[..cut])
}

/// A saved Soniox transcript rather than media.
pub fn is_transcript(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

fn bar(opts: &Options, msg: &str, template: &str) -> ProgressBar {
    let pb = attach(opts, ProgressBar::new(0).with_prefix(opts.prefix.clone()).with_message(msg.to_string()));
    pb.set_style(ProgressStyle::with_template(template).unwrap().progress_chars("=> "));
    pb
}

fn attach(opts: &Options, pb: ProgressBar) -> ProgressBar {
    match &opts.progress {
        Some(m) => m.add(pb),
        None => pb,
    }
}

/// Alone, a finished stage stays on screen as a log line; in a batch it disappears (the batch prints a result line).
fn finish(opts: &Options, pb: &ProgressBar, msg: String) {
    if opts.progress.is_some() {
        pb.finish_and_clear();
    } else {
        pb.finish_with_message(msg);
    }
}

struct ClearOnDrop<'a>(&'a ProgressBar);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        if !self.0.is_finished() {
            self.0.finish_and_clear();
        }
    }
}

fn fmt_dur(s: f64) -> String {
    let s = s.round() as u64;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn fmt_size(p: &Path) -> String {
    let b = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) as f64;
    if b > 1e6 { format!("{:.1} MB", b / 1e6) } else { format!("{:.0} KB", b / 1e3) }
}

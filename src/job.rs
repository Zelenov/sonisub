//! Everything that happens to one input file: media -> audio -> Soniox -> .srt (and other formats), and cleanup.
//! Knows nothing about batches, log files or the command line.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde_json::{Value, json};

use crate::audio;
use crate::cancel::{CancelToken, Cancelled};
use crate::progress::{Callback, Estimate, Reporter, Stage};
use crate::soniox::{self, Client, RemoteGuard};
use crate::srt;
use captions::{ass, vtt};

/// What gets written for a file. New formats come in minor releases: a `match` needs a `_` arm.
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// Subtitles: `clip.srt`.
    Srt,
    /// WebVTT subtitles, the same cues: `clip.vtt`.
    Vtt,
    /// ASS subtitles, the same cues, a style per speaker: `clip.ass`.
    Ass,
    /// Premiere Pro transcript (Text panel > Transcript > ... > Import Static Transcript): `clip.premiere.json`.
    Premiere,
    /// CAPT, the whole transcript as small text, every other format can be made from: `clip.capt`.
    Capt,
    /// YTT, YouTube's own timed text (srv3): a time for every word, places, italic: `clip.ytt`.
    Ytt,
}

impl Format {
    /// Subtitles: a file of them with no cue is empty, not written.
    fn has_cues(self) -> bool {
        matches!(self, Format::Srt | Format::Vtt | Format::Ass | Format::Ytt)
    }

    /// Where this format goes for the output base path (`clip.srt` -> `clip.premiere.json`).
    pub fn path(self, output: &Path) -> PathBuf {
        match self {
            Format::Srt => output.to_path_buf(),
            Format::Vtt => output.with_extension("vtt"),
            Format::Ass => output.with_extension("ass"),
            Format::Premiere => output.with_extension("premiere.json"),
            Format::Capt => output.with_extension("capt"),
            Format::Ytt => output.with_extension("ytt"),
        }
    }
}

/// How the formats are written, beyond the cue layout.
#[derive(Debug, Clone, Default)]
pub struct Outputs {
    /// Speaker names and word highlight for [`Format::Ass`].
    pub ass: ass::Options,
    /// Subtitles for the deaf and hard of hearing: SRT, WebVTT and ASS get sounds as
    /// subtitles and voices as labels (`[phone ringing]`, `[whispering]`).
    pub sdh: bool,
    /// The language of sound and voice labels (`en`, `ru`); `None`: Russian for a Russian
    /// transcript, else English.
    pub labels: Option<String>,
}

/// The layout's frame into a CAPT file's header when it is not the one the file already says, so
/// the next `render` of the file lays it out the same.
pub fn frame_into(doc: &mut captions::capt::Doc, layout: &srt::Layout) {
    if doc.frame.unwrap_or(captions::cues::DEFAULT_FRAME) != layout.frame {
        doc.frame = Some(layout.frame);
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
    /// How the formats are written beyond the layout: ASS options, SDH, label language, frame.
    pub outputs: Outputs,
    /// Interval between transcription status checks.
    pub poll: Duration,
    /// Printed before progress lines, e.g. "[2/5]".
    pub prefix: String,
    /// Sent to Soniox as `client_reference_id`, to find this run's cost in the usage logs.
    pub reference: String,
    /// Draw stage bars here, under a batch's overall bar; they are cleared when the file is done.
    pub progress: Option<MultiProgress>,
    /// Called with where the job is: extract, upload, transcribe (estimated), download, write.
    /// See [`crate::progress`]; it runs on the job's threads. `None` reports nothing.
    pub on_progress: Option<Callback>,
    /// Stops this job's extraction and polling; give the [`Client`] the same token
    /// ([`Client::with_cancel`]) to stop its uploads and retries too. The process-wide
    /// [`crate::cancel::interrupt`] stops every job.
    pub cancel: CancelToken,
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
            outputs: Outputs::default(),
            poll: Duration::from_secs(3),
            prefix: String::new(),
            reference: "sonisub".into(),
            progress: None,
            on_progress: None,
            cancel: CancelToken::new(),
        }
    }
}

#[derive(Debug)]
pub enum Outcome {
    /// Outputs written (`files`; `cues` counts subtitles, 0 with only a Premiere transcript). `cached`: the transcript came
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

    let rep = Reporter::new(opts.on_progress.clone());
    let cache = transcript_path(output);
    let source = if is_transcript(input) {
        Source::Input(read_transcript(input)?)
    } else if cache.is_file() && !opts.force {
        Source::Cache(read_transcript(&cache).context("use --force to transcribe again")?, cache.clone())
    } else {
        let client = client.ok_or_else(soniox::missing_key)?;
        match transcribe_with(input, opts, client, &rep) {
            Ok(Some((t, secs))) => Source::Api(t, secs),
            Ok(None) => {
                // Digital silence: nothing was sent; the marker still tells later runs (and plans) it's checked.
                let marker = json!({"text": "", "tokens": [], "sonisub": "digital silence, not sent to Soniox"});
                std::fs::write(&cache, serde_json::to_string_pretty(&marker)?)
                    .with_context(|| format!("cannot write {}", cache.display()))?;
                rep.mark(Stage::Writing, 1.0);
                return Ok(Outcome::NoSpeech { marker: Some(cache), cached: false, uploaded_s: None });
            }
            // Anything but "no audio track" is a real error.
            Err(e) => return Ok(Outcome::NoAudio { reason: e.downcast::<audio::NoAudio>()?.0 }),
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
        rep.mark(Stage::Writing, 1.0);
        return Ok(Outcome::NoSpeech { marker: json, cached, uploaded_s });
    }
    rep.mark(Stage::Writing, 0.0);
    let layout = srt::Layout { by_speaker: opts.diarization, ..opts.layout.clone() };
    let hint = opts.languages.first().map(String::as_str);
    let seed = transcript["id"].as_str().unwrap_or_default();
    // Every output is a pass over the CAPT document, written or not, so `-t srt` and
    // `-t srt,capt` (or `render clip.capt`) give the same subtitles.
    let mut doc = soniox::capt(&transcript);
    if !is_transcript(input) {
        doc.media = input.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    name_speakers(&mut doc, &layout, &opts.outputs.ass);
    frame_into(&mut doc, &layout);
    let words = soniox::words(&transcript);
    let (files, cues) = write_outputs(&targets, &doc.to_words(), &words, &doc, &layout, &opts.outputs, hint, seed)?;
    rep.mark(Stage::Writing, 1.0);
    Ok(Outcome::Written { files, cues, json, cached, uploaded_s })
}

/// A CAPT file's names of numbered speakers in order (`speaker:2 name:Anna` is the second), as
/// [`srt::Layout::speaker_names`] takes them. A speaker with another id is named by
/// [`ass::Options::names`] only.
pub fn speaker_names(doc: &captions::capt::Doc) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for s in &doc.speakers {
        if let (Some(name), Ok(n)) = (&s.name, s.id.parse::<usize>())
            && n > 0
        {
            if names.len() < n {
                names.resize(n, String::new());
            }
            names[n - 1] = name.clone();
        }
    }
    names
}

/// The names the user gave (`--speaker-names`, ASS names) into a CAPT file's header.
pub fn name_speakers(doc: &mut captions::capt::Doc, layout: &srt::Layout, ass: &ass::Options) {
    for s in &mut doc.speakers {
        let by_number = s.id.parse::<usize>().ok().and_then(|n| layout.speaker_names.get(n.checked_sub(1)?));
        if let Some(name) = ass.names.get(&s.id).or(by_number).filter(|n| !n.trim().is_empty()) {
            s.name = Some(name.clone());
        }
    }
}

/// Writes every format of `targets`: subtitles from `cue_words`, the Premiere transcript from
/// `words`, SDH subtitles and CAPT from `doc` (the same speech with its sounds). A transcript from
/// Soniox passes `doc.to_words()` and Soniox's words (confidence to the third decimal), saved words
/// pass themselves twice. The files written and the number of cues of the first subtitles written
/// (0 with only a Premiere transcript or CAPT). `default_language` and `seed` are for
/// [`Format::Premiere`] ([`captions::premiere::build`]); with no `words` nothing is written.
#[allow(clippy::too_many_arguments)]
pub fn write_outputs(
    targets: &[(Format, PathBuf)],
    cue_words: &[captions::Word],
    words: &[captions::Word],
    doc: &captions::capt::Doc,
    layout: &srt::Layout,
    outputs: &Outputs,
    default_language: Option<&str>,
    seed: &str,
) -> Result<(Vec<PathBuf>, usize)> {
    if words.is_empty() && targets.iter().any(|(f, _)| *f == Format::Premiere) {
        bail!("no words for a Premiere transcript");
    }
    let mut cues = 0;
    let mut files = Vec::new();
    let labels = outputs.labels.clone().unwrap_or_else(|| captions::labels::for_language(doc.lang.as_deref()).into());
    // Everything is made before anything is written, so a format with nothing to show leaves no
    // files behind.
    let mut made = Vec::new();
    for (format, path) in targets {
        let (text, n) = match format {
            Format::Srt if outputs.sdh => captions::sdh::srt(doc, layout, &labels),
            Format::Vtt if outputs.sdh => captions::sdh::vtt(doc, layout, &labels),
            Format::Ass if outputs.sdh => captions::sdh::ass(doc, layout, &outputs.ass, &labels),
            Format::Srt => captions::srt::build(cue_words, layout),
            Format::Vtt => vtt::build(cue_words, layout),
            Format::Ass => ass::build(cue_words, layout, &outputs.ass),
            Format::Ytt => {
                if outputs.sdh {
                    captions::sdh::ytt(doc, layout, &labels)
                } else {
                    captions::ytt::build(doc, layout)
                }
            }
            Format::Premiere => {
                let t = captions::premiere::build(words, layout, default_language, seed)
                    .ok_or_else(|| anyhow!("no words to write"))?;
                (serde_json::to_string_pretty(&t)?, 0)
            }
            Format::Capt => (doc.write(), 0),
        };
        if n == 0 && format.has_cues() {
            bail!(
                "nothing to write in {}: no words, and --sdh adds sounds only to .srt, .vtt and .ass",
                path.display()
            );
        }
        if cues == 0 {
            cues = n;
        }
        made.push((path, text));
    }
    for (path, text) in made {
        std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))?;
        files.push(path.clone());
    }
    Ok((files, cues))
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
///
/// With `on_progress` set it reports `Extracting` through `Downloading`; `overall` stops at 0.95 because
/// `Writing` belongs to [`process`].
pub fn transcribe(input: &Path, opts: &Options, client: &Client) -> Result<Option<(Value, f64)>> {
    transcribe_with(input, opts, client, &Reporter::new(opts.on_progress.clone()))
}

fn transcribe_with(input: &Path, opts: &Options, client: &Client, rep: &Reporter) -> Result<Option<(Value, f64)>> {
    // The temp dir (and the audio inside) is removed when it goes out of scope, whatever happens.
    let mut tmp = match &opts.temp_dir {
        Some(d) => tempfile::Builder::new().prefix("sonisub-").tempdir_in(d),
        None => tempfile::Builder::new().prefix("sonisub-").tempdir(),
    }
    .context("cannot create temp dir")?;
    let stem = input.file_stem().map_or("audio".into(), |s| s.to_string_lossy().into_owned());
    let audio_path = tmp.path().join(format!("{stem}.flac"));

    let pb = bar(opts, "extract", "{prefix} {msg:9} [{bar:30.cyan/blue}] {percent:>3}%  {elapsed}");
    let audio = rep
        .watch(Stage::Extracting, &pb, || audio::extract(input, &audio_path, opts.audio, &pb, &opts.cancel))
        .inspect_err(|_| pb.finish_and_clear())?;
    finish(opts, &pb, format!("extracted {} ({})", fmt_dur(audio.duration_s), fmt_size(&audio.path)));
    if opts.keep_audio {
        tmp.disable_cleanup(true);
        let line = format!("  audio kept: {}", audio.path.display());
        match &opts.progress {
            // A hidden draw target (no terminal, --quiet) would swallow the line.
            Some(m) if m.is_hidden() => eprintln!("{line}"),
            _ => pb.println(line),
        }
    }
    if audio.peak.is_some_and(|p| p < audio::SILENCE_PEAK) {
        return Ok(None);
    }

    // Deletes the uploaded file and the transcription on drop, including on `?` returns below.
    let mut remote = RemoteGuard { client, file_id: None, transcription_id: None };
    if opts.cancel.is_cancelled() {
        return Err(Cancelled.into());
    }

    let pb = bar(opts, "upload", "{prefix} {msg:9} [{bar:30.cyan/blue}] {bytes}/{total_bytes}  {bytes_per_sec}");
    let file_id =
        rep.watch(Stage::Uploading, &pb, || client.upload(&audio.path, &pb)).inspect_err(|_| pb.finish_and_clear())?;
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

    let pb = bar(opts, "", "{prefix} {spinner} [{bar:30.cyan/blue}] {msg}  {elapsed}");
    pb.set_length(1000);
    pb.enable_steady_tick(Duration::from_millis(120));
    // Clears the spinner however this loop is left.
    let _clear = ClearOnDrop(&pb);
    let estimate = Estimate::new(audio.duration_s);
    rep.emit(Stage::Transcribing, Some(0.0), None, estimate.remaining());
    loop {
        if opts.cancel.is_cancelled() {
            return Err(Cancelled.into());
        }
        let st = client.status(&id)?;
        if st.status == "completed" {
            break;
        }
        // Wait out the poll interval in short slices, so the estimate keeps moving between status checks.
        let mut left = opts.poll;
        while !left.is_zero() {
            let slice = left.min(Duration::from_millis(250));
            if !opts.cancel.sleep(slice) {
                return Err(Cancelled.into());
            }
            left -= slice;
            let fraction = estimate.fraction();
            pb.set_position((fraction * 1000.0) as u64);
            pb.set_message(transcribing_text(fraction, &st.status));
            rep.emit(Stage::Transcribing, Some(fraction), Some(st.status.clone()), estimate.remaining());
        }
    }
    estimate.finish(audio.duration_s);
    rep.mark(Stage::Transcribing, 1.0);
    rep.mark(Stage::Downloading, 0.0);
    pb.set_message("downloading transcript");
    let transcript = client.transcript(&id)?;
    rep.mark(Stage::Downloading, 1.0);
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
    let stem = [".premiere.json", ".soniox.json", ".words.json", ".srt", ".vtt", ".ass", ".capt", ".ytt", ".json"]
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

/// "transcribing ~60% (est.), processing": Soniox gives no percentage, so it is an estimate.
fn transcribing_text(fraction: f32, status: &str) -> String {
    format!("transcribing ~{:.0}% (est.), {status}", fraction * 100.0)
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

/// Without a `MultiProgress` a finished stage stays on screen as a log line; with one it disappears (the caller
/// prints a result line).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transcribing_text_is_marked_as_an_estimate_and_keeps_the_status() {
        assert_eq!(transcribing_text(0.6, "processing"), "transcribing ~60% (est.), processing");
        assert_eq!(transcribing_text(0.95, "queued"), "transcribing ~95% (est.), queued");
    }
}

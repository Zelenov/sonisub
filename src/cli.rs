use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use sonisub::captions::{self, ass};
use sonisub::job::Format;
use sonisub::{audio, job, soniox, srt};

/// Generate .srt subtitles for video/audio files with Soniox speech-to-text.
///
/// Extracts the audio track, uploads it to Soniox, waits for the transcript,
/// builds subtitle cues and deletes everything it created on Soniox and on disk.
#[derive(Parser, Debug)]
#[command(name = "sonisub", version, args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub run: RunArgs,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// What Soniox usage cost: per model, today, price per hour of audio.
    Usage {
        /// How many days back (Soniox keeps 91).
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=91))]
        days: u64,

        #[command(flatten)]
        api: ApiArgs,
    },
    /// Languages Soniox recognises: the codes --lang takes.
    Languages {
        #[command(flatten)]
        api: ApiArgs,
    },
    /// Write subtitles from a CAPT file or saved words (a JSON list of words with timing), without transcribing again.
    Render(Box<RenderArgs>),
    /// List (and with --yes delete) all files and transcriptions stored in the Soniox account.
    Purge {
        /// Actually delete. Without it only prints what would be deleted.
        #[arg(long)]
        yes: bool,

        #[command(flatten)]
        api: ApiArgs,
    },
}

#[derive(Args, Debug, Clone)]
pub struct ApiArgs {
    /// Soniox API key.
    #[arg(long, env = "SONIOX_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,

    /// Soniox API base URL.
    #[arg(long, default_value = soniox::DEFAULT_BASE, hide = true)]
    pub api_url: String,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Media files or folders (mp4, mov, m4a, mp3, wav, flac, ogg, mkv...). A folder means all media files in it.
    /// A `.soniox.json` transcript saved earlier is re-segmented without calling the API.
    #[arg(required = true)]
    pub inputs: Vec<PathBuf>,

    /// Only files in folders whose name matches: "*.MP4", "Interview*", "DJI_2025080?_*". Repeatable;
    /// without * or ? it matches names containing the text. Case-insensitive.
    #[arg(short = 'F', long = "filter", value_name = "PATTERN")]
    pub filters: Vec<String>,

    /// Also look into subfolders.
    #[arg(short, long)]
    pub recursive: bool,

    /// Show what would be done (and roughly what it would cost), change nothing.
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    #[command(flatten)]
    pub api: ApiArgs,

    /// What to write, comma separated: srt, vtt (WebVTT), ass (a style per speaker), premiere (Premiere Pro transcript,
    /// clip.premiere.json), capt (the whole transcript as small text, clip.capt; `sonisub render` makes the other
    /// formats from it), sbv (YouTube's SBV, clip.sbv), lrc (Enhanced LRC, a time before every word, clip.lrc),
    /// transcript (plain text, a paragraph per speaker turn, clip.txt), ttml (TTML / IMSC 1.1 Text, clip.ttml), adr (a
    /// DaVinci Resolve Fairlight ADR cue list, clip.adr.csv), ytt (YouTube's srv3, a time for every word, clip.ytt).
    /// Only missing files are written, so adding a format later reuses the saved transcript for free.
    #[arg(short = 't', long = "format", value_enum, value_delimiter = ',', default_value = "srt")]
    pub formats: Vec<Format>,

    /// Output path (only with a single input); other formats get their own extension next to it.
    /// Default: next to the input, same name.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Directory for the output files. Default: next to each input.
    #[arg(short = 'd', long, conflicts_with = "output")]
    pub out_dir: Option<PathBuf>,

    /// Overwrite existing outputs and transcribe again (by default existing files are kept and a saved
    /// .soniox.json is reused).
    #[arg(short, long)]
    pub force: bool,

    /// Language hints, comma separated.
    #[arg(short, long, value_delimiter = ',', default_value = "en,ru")]
    pub lang: Vec<String>,

    /// Make Soniox stick to the hinted languages.
    #[arg(long)]
    pub strict_lang: bool,

    /// Domain context for better recognition: names, places, terms ("Nairobi, Maasai, UAT").
    #[arg(short, long)]
    pub context: Option<String>,

    /// Soniox async model.
    #[arg(short, long, default_value = "stt-async-v3")]
    pub model: String,

    /// Disable speaker diarization (by default a speaker change starts a new cue).
    #[arg(long)]
    pub no_diarization: bool,

    /// Don't keep the raw Soniox transcript (<name>.soniox.json next to the outputs). By default it is kept:
    /// other formats and layouts can be made from it later without paying again.
    #[arg(long)]
    pub no_json: bool,

    /// How to extract audio.
    #[arg(long, value_enum, default_value_t = audio::Backend::Auto)]
    pub audio: audio::Backend,

    /// Directory for temporary audio. Default: system temp.
    #[arg(long)]
    pub temp_dir: Option<PathBuf>,

    /// Keep the temporary audio file (prints its path).
    #[arg(long)]
    pub keep_audio: bool,

    /// No progress bars: only the result lines and the summary.
    #[arg(short, long)]
    pub quiet: bool,

    /// Log file. Default: sonisub.log next to each input.
    #[arg(long)]
    pub log: Option<PathBuf>,

    #[command(flatten)]
    pub seg: SegArgs,

    /// Seconds between transcription status checks.
    #[arg(long, default_value_t = 3.0, hide = true)]
    pub poll: f64,
}

#[derive(Args, Debug, Clone)]
#[command(next_help_heading = "Subtitle layout")]
pub struct SegArgs {
    /// Max characters per line (with --wrap); a cue holds up to --max-line × --max-lines characters. 0 = no limit.
    /// Sentences are cut at punctuation first, between words only if that can't fit.
    #[arg(long, default_value_t = 50)]
    pub max_line: usize,

    /// Max lines per cue.
    #[arg(long, default_value_t = 2)]
    pub max_lines: usize,

    /// Max cue duration, seconds; 0 = no limit.
    #[arg(long, default_value_t = 8.0)]
    pub max_duration: f64,

    /// Break each cue's text into lines (up to --max-lines of --max-line chars). By default a cue is one line.
    #[arg(long, short = 'w')]
    pub wrap: bool,

    /// No length or duration limit: one cue per sentence (same as --max-line 0 --max-duration 0).
    #[arg(long, short = 'u')]
    pub unlimited: bool,

    /// Silence longer than this (seconds) always ends a cue; 0 = never.
    #[arg(long, default_value_t = 2.0)]
    pub gap: f64,

    /// Minimum cue duration, seconds (extended into following silence when possible).
    #[arg(long, default_value_t = 0.5)]
    pub min_duration: f64,

    /// Prefix cues with the speaker when it changes: "Speaker 1: ...". Only if there are several speakers.
    #[arg(long, short = 's')]
    pub speakers: bool,

    /// Speaker names instead of "Speaker N", comma separated in order of appearance: "Eugene,Sasha". Implies
    /// --speakers (in .ass the names are also the speakers' styles).
    #[arg(long, value_delimiter = ',')]
    pub speaker_names: Vec<String>,

    /// In .ass, light up each word as it is spoken (\k karaoke).
    #[arg(long)]
    pub karaoke: bool,

    /// The language of sound and voice labels in -t transcript and -t adr: en ([laughs]) or ru ((смеётся)).
    /// Default: ru for a Russian transcript, else en.
    #[arg(long, value_parser = captions::labels::LANGUAGES, help_heading = "Outputs")]
    pub labels: Option<String>,

    /// Frame rate of the Fairlight timeline (-t adr): 23.976, 24, 25, 29.97, 30, 50, 59.94 or 60; in Resolve, Project
    /// Settings > Master Settings > Timeline frame rate. Default 25.
    #[arg(long, value_parser = captions::adr::Rate::parse, help_heading = "Fairlight ADR")]
    pub fps: Option<captions::adr::Rate>,

    /// Where the clip starts on the Fairlight timeline (-t adr), HH:MM:SS:FF, non-drop frame; usually the
    /// timeline's own start (Timeline > Starting Timecode). Default 01:00:00:00.
    #[arg(long, value_parser = captions::adr::Timecode::parse, help_heading = "Fairlight ADR")]
    pub timeline_start: Option<captions::adr::Timecode>,
}

impl SegArgs {
    /// The cue layout; `by_speaker`: a speaker change ends a cue.
    pub fn layout(&self, by_speaker: bool) -> srt::Layout {
        srt::Layout {
            max_line: if self.unlimited { 0 } else { self.max_line },
            max_lines: self.max_lines,
            max_duration: if self.unlimited { 0.0 } else { self.max_duration },
            gap: self.gap,
            min_duration: self.min_duration,
            by_speaker,
            speaker_labels: self.speakers || !self.speaker_names.is_empty(),
            speaker_names: self.speaker_names.clone(),
            wrap_lines: self.wrap,
        }
    }

    /// How the formats are written beyond the layout.
    pub fn outputs(&self) -> job::Outputs {
        job::Outputs {
            ass: ass::Options { karaoke: self.karaoke, ..Default::default() },
            adr: self.adr(),
            labels: self.labels.clone(),
        }
    }

    /// The ADR list's rate and start: the ones given, else 25 fps from 01:00:00:00.
    pub fn adr(&self) -> captions::adr::Options {
        let default = captions::adr::Options::default();
        captions::adr::Options {
            rate: self.fps.unwrap_or(default.rate),
            start: self.timeline_start.unwrap_or(default.start),
            ..default
        }
    }

    /// What an ADR list is timed by, for the result line: ` (ADR at 25 fps from 01:00:00:00)`, or
    /// nothing without `-t adr`.
    pub fn adr_note(&self, formats: &[Format]) -> String {
        if !formats.contains(&Format::Adr) {
            return String::new();
        }
        let o = self.adr();
        format!(" (ADR at {} fps from {})", o.rate, o.start)
    }

    /// Options that make no sense for `formats`, said instead of ignored.
    pub fn check(&self, formats: &[Format]) -> anyhow::Result<()> {
        let adr = formats.contains(&Format::Adr);
        if self.labels.is_some() && !formats.contains(&Format::Transcript) && !adr {
            anyhow::bail!("--labels needs -t transcript or -t adr: nothing else writes a label");
        }
        if !adr && (self.fps.is_some() || self.timeline_start.is_some()) {
            anyhow::bail!("--fps and --timeline-start need -t adr: nothing else is timed by frames");
        }
        if adr {
            let o = self.adr();
            o.start
                .frames_at(o.rate)
                .map_err(|e| anyhow::anyhow!("--timeline-start: {e}; pass --fps to match the timeline"))?;
        }
        Ok(())
    }
}

#[derive(Args, Debug)]
pub struct RenderArgs {
    /// A CAPT file (`clip.capt`), or words saved as JSON:
    /// [{"text": "Hello,", "start": 120, "end": 480, "speaker": "1", "language": "en"}, ...]
    /// (start and end in milliseconds; speaker and language optional).
    pub input: PathBuf,

    /// What to write, comma separated: srt, vtt, ass, premiere, capt, sbv, lrc, transcript, ttml, adr, ytt.
    #[arg(short = 't', long = "format", value_enum, value_delimiter = ',', default_value = "srt")]
    pub formats: Vec<Format>,

    /// Output path; other formats get their own extension next to it. Default: next to the input, same name.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Overwrite existing outputs.
    #[arg(short, long)]
    pub force: bool,

    #[command(flatten)]
    pub seg: SegArgs,
}

impl RunArgs {
    /// Per-file options for [`job::process`].
    pub fn job_options(&self) -> job::Options {
        job::Options {
            model: self.model.clone(),
            languages: self.lang.clone(),
            strict_languages: self.strict_lang,
            context: self.context.clone(),
            diarization: !self.no_diarization,
            audio: self.audio,
            temp_dir: self.temp_dir.clone(),
            keep_audio: self.keep_audio,
            keep_json: !self.no_json,
            formats: self.formats.clone(),
            force: self.force,
            layout: self.seg.layout(!self.no_diarization),
            outputs: self.seg.outputs(),
            poll: Duration::from_secs_f64(self.poll.max(0.2)),
            prefix: String::new(),
            reference: "sonisub".into(),
            progress: None,
            on_progress: None,
            // Ctrl+C sets the process-wide flag, which every token also obeys.
            cancel: Default::default(),
        }
    }
}

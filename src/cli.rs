use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
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

    /// Output .srt path (only with a single input). Default: next to the input, same name.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Directory for the .srt files. Default: next to each input.
    #[arg(short = 'd', long, conflicts_with = "output")]
    pub out_dir: Option<PathBuf>,

    /// Overwrite existing .srt files (by default such inputs are skipped).
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

    /// Also save the raw Soniox transcript as <name>.soniox.json next to the .srt.
    #[arg(short = 'j', long)]
    pub keep_json: bool,

    /// How to extract audio.
    #[arg(long, value_enum, default_value_t = audio::Backend::Auto)]
    pub audio: audio::Backend,

    /// Directory for temporary audio. Default: system temp.
    #[arg(long)]
    pub temp_dir: Option<PathBuf>,

    /// Keep the temporary audio file (prints its path).
    #[arg(long)]
    pub keep_audio: bool,

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

    /// Speaker names instead of "Speaker N", comma separated in order of appearance: "Eugene,Sasha". Implies --speakers.
    #[arg(long, value_delimiter = ',')]
    pub speaker_names: Vec<String>,
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
            keep_json: self.keep_json,
            force: self.force,
            layout: srt::Layout {
                max_line: if self.seg.unlimited { 0 } else { self.seg.max_line },
                max_lines: self.seg.max_lines,
                max_duration: if self.seg.unlimited { 0.0 } else { self.seg.max_duration },
                gap: self.seg.gap,
                min_duration: self.seg.min_duration,
                by_speaker: !self.no_diarization,
                speaker_labels: self.seg.speakers || !self.seg.speaker_names.is_empty(),
                speaker_names: self.seg.speaker_names.clone(),
                wrap_lines: self.seg.wrap,
            },
            poll: Duration::from_secs_f64(self.poll.max(0.2)),
            prefix: String::new(),
            reference: "sonisub".into(),
            progress: None,
            // Ctrl+C sets the process-wide flag, which every token also obeys.
            cancel: Default::default(),
        }
    }
}

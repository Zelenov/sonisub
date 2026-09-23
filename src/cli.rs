use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use sonisub::{audio, job, srt};

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
    #[arg(long, default_value = "https://api.soniox.com/v1", hide = true)]
    pub api_url: String,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Input media files (mp4, mov, m4a, mp3, wav, flac, ogg, mkv...).
    /// A `.soniox.json` transcript saved earlier with --keep-json is re-segmented without calling the API.
    #[arg(required = true)]
    pub inputs: Vec<PathBuf>,

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
    #[arg(short, long, value_delimiter = ',', default_value = "ru,en")]
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
    /// Max characters per subtitle line.
    #[arg(long, default_value_t = 42)]
    pub max_line: usize,

    /// Max lines per cue.
    #[arg(long, default_value_t = 2)]
    pub max_lines: usize,

    /// Max cue duration, seconds.
    #[arg(long, default_value_t = 6.0)]
    pub max_duration: f64,

    /// A pause longer than this (seconds) starts a new cue.
    #[arg(long, default_value_t = 0.7)]
    pub gap: f64,

    /// Minimum cue duration, seconds (extended into following silence when possible).
    #[arg(long, default_value_t = 0.5)]
    pub min_duration: f64,
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
                max_line: self.seg.max_line,
                max_lines: self.seg.max_lines,
                max_duration: self.seg.max_duration,
                gap: self.seg.gap,
                min_duration: self.seg.min_duration,
                by_speaker: !self.no_diarization,
            },
            poll: Duration::from_secs_f64(self.poll.max(0.2)),
            prefix: String::new(),
        }
    }
}

//! sonisub: subtitles for media files via Soniox speech-to-text.
//!
//! - [`job`] runs the whole pipeline for one file; [`batch`] turns folders into a list of files and a plan.
//! - [`progress`] reports where a job is, for programs without a console.
//! - [`usage`] reads what was spent, [`languages`] what each model recognises.
//! - [`audio`] extracts audio, [`soniox`] talks to the API and reads its tokens as words.
//! - [`captions`] lays words out as SRT, WebVTT, ASS or a Premiere Pro transcript; [`srt`] and
//!   [`premiere`] do it straight from a Soniox transcript.

pub use captions;

pub mod audio;
pub mod batch;
pub mod cancel;
pub mod job;
pub mod languages;
pub mod premiere;
pub mod progress;
pub mod soniox;
pub mod srt;
pub mod usage;

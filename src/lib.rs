//! sonisub: subtitles for media files via Soniox speech-to-text.
//!
//! - [`job`] runs the whole pipeline for one file.
//! - [`usage`] reads what was spent.
//! - [`audio`] extracts audio, [`soniox`] talks to the API, [`srt`] turns a transcript into subtitles.

pub mod audio;
pub mod cancel;
pub mod job;
pub mod soniox;
pub mod srt;
pub mod usage;

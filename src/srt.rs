//! Soniox transcript -> SRT. The layout rules and the SRT writer are [`captions`]'; this module keeps
//! the names callers used before they moved.

use serde_json::Value;

pub use crate::soniox::words;
pub use captions::{Layout, Word};

/// Whether the transcript contains any words at all.
pub fn has_speech(transcript: &Value) -> bool {
    !words(transcript).is_empty()
}

/// Builds SRT text from a Soniox transcript; returns the text and the number of cues.
pub fn build(transcript: &Value, s: &Layout) -> (String, usize) {
    captions::srt::build(&words(transcript), s)
}

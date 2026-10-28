//! Soniox transcript -> Premiere Pro transcript. The writer and reader are [`captions::premiere`]'s;
//! this module keeps the names callers used before they moved.

use serde_json::Value;

pub use captions::premiere::to_srt;

use crate::srt::Layout;

/// Builds a Premiere Pro transcript from a Soniox transcript; `None` if there are no words. See
/// [`captions::premiere::build`]; speaker ids are derived from the transcription's id.
pub fn build(transcript: &Value, layout: &Layout, default_language: Option<&str>) -> Option<Value> {
    let seed = transcript["id"].as_str().unwrap_or_default();
    captions::premiere::build(&crate::soniox::words(transcript), layout, default_language, seed)
}

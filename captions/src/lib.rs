//! captions: words with timing -> cues -> SRT, WebVTT, ASS, Premiere Pro transcript.
//!
//! Knows nothing about speech recognition: a provider turns its own output into [`Word`]s, and every
//! format here is laid out from them by the same rules.
//!
//! - [`cues`] cuts words into subtitle cues by a [`Layout`].
//! - [`srt`], [`vtt`] and [`ass`] write those cues; [`premiere`] writes a Premiere Pro transcript.

use serde::{Deserialize, Serialize};

pub mod ass;
pub mod capt;
pub mod cues;
pub mod premiere;
pub mod srt;
pub mod vtt;

pub use cues::{Cue, Layout};

/// One word of a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// The word with the punctuation written next to it ("Hello,"), without spaces.
    pub text: String,
    /// Start and end, milliseconds from the start of the audio.
    pub start: u64,
    pub end: u64,
    /// Speaker label ("1", "2"); `None` without diarization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// Language code ("en", "ru").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// How sure the recogniser is, 0 to 1.
    #[serde(default = "certain")]
    pub confidence: f64,
    /// Written right after the previous word, without a space ("finally—" + "and").
    #[serde(default, skip_serializing_if = "is_false")]
    pub glued: bool,
}

fn certain() -> f64 {
    1.0
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// FNV-1a of `seed` (started from `salt`) with the splitmix64 finaliser: similar seeds
/// ("…/speaker/1", "…/speaker/2") give unrelated values, and the same seed always the same one.
pub(crate) fn hash64(seed: &str, salt: u64) -> u64 {
    let h = seed.bytes().fold(0xcbf2_9ce4_8422_2325 ^ salt, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    let h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

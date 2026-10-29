//! captions: words with timing -> cues -> SRT, WebVTT, ASS, Premiere Pro transcript.
//!
//! Knows nothing about speech recognition: a provider turns its own output into [`Word`]s, and every
//! format here is laid out from them by the same rules.
//!
//! - [`cues`] cuts words into subtitle cues by a [`Layout`].
//! - [`srt`], [`vtt`] and [`ass`] write those cues; [`premiere`] writes a Premiere Pro transcript.
//! - [`capt`] reads and writes CAPT, the transcript format between a recogniser and these writers.
//! - [`sbv`] writes YouTube's SBV, the same cues as [`srt`].
//! - [`lrc`] writes Enhanced LRC, a time before every word of the same cues.
//! - [`transcript`] writes a plain text transcript from a CAPT file, a paragraph per turn, sounds and
//!   voices as labels from [`labels`].

use serde::{Deserialize, Serialize};

pub mod ass;
pub mod capt;
pub mod cues;
pub mod labels;
pub mod lrc;
pub mod premiere;
pub mod sbv;
pub mod srt;
mod style;
pub mod transcript;
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
    /// What a CAPT file says about the word beyond its text and time ([`capt::Doc::to_words`]);
    /// empty for a recogniser's words, which are laid out by their punctuation and pauses alone.
    #[serde(default, skip_serializing_if = "Marks::is_empty")]
    pub marks: Marks,
}

impl Default for Word {
    /// An empty word at 0 with full confidence: `Word { text, start, end, ..Word::default() }`.
    fn default() -> Self {
        Word {
            text: String::new(),
            start: 0,
            end: 0,
            speaker: None,
            language: None,
            confidence: 1.0,
            glued: false,
            marks: Marks::default(),
        }
    }
}

/// A word's layout and style from a CAPT file. New fields may come: build one with
/// `Marks::default()` and set what you need.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Marks {
    /// Whether a cut point (` / `) follows the word: subtitles are cut at cut points first and
    /// never between words without one, unless nothing else fits. `None`: cut at punctuation and
    /// pauses, as for a recogniser's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<bool>,
    /// Whether a sentence ends after the word, against what its punctuation says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentence: Option<bool>,
    /// Shown in italic: a title, a foreign word, a stressed word.
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// Where on screen; a subtitle is never shared by two places. `None` is the bottom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<capt::Pos>,
    /// How the word is said (`whisper`, `laughter`): a span's or a block's `voice:`; see [`labels`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// The word's language when it differs from the file's (a block's or a span's `lang:`), so
    /// outputs that can mark a language (WebVTT) do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

impl Marks {
    /// Nothing marked: the word is laid out like a recogniser's.
    pub fn is_empty(&self) -> bool {
        *self == Marks::default()
    }
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

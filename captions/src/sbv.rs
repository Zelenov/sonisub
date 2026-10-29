//! Cues -> SBV, the subtitle format YouTube Studio uploads and downloads: `H:MM:SS.mmm,H:MM:SS.mmm`,
//! then the lines, then a blank line. The same cues as SRT, as plain text (SBV has no markup);
//! [`crate::sdh::sbv`] adds sounds and voices.

use crate::Word;
use crate::cues::{Cue, Layout, cues};
use crate::style::{self, Markup, Sound};

fn timestamp(ms: u64) -> String {
    format!("{}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// SBV for cues and, for subtitles for the deaf and hard of hearing, sounds and voice labels.
pub(crate) fn write(cues: &[Cue], sounds: &[Sound], voice: Option<&dyn Fn(&str) -> String>) -> (String, usize) {
    let markup = Markup { italic: None, lang: None, voice, escape: &style::plain };
    let mut items: Vec<(u64, u64, String)> =
        cues.iter().map(|c| (c.start, c.end, style::lines(c, &markup).join("\n"))).collect();
    items.extend(sounds.iter().map(|s| (s.start, s.end, s.text.clone())));
    items.sort_by_key(|(start, _, _)| *start);
    let out = items
        .iter()
        .map(|(start, end, text)| format!("{},{}\n{text}\n\n", timestamp(*start), timestamp(*end)))
        .collect();
    (out, items.len())
}

/// SBV text for `words` laid out by `layout`, and the number of cues.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    write(&cues(words, layout), &[], None)
}

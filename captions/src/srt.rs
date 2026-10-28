//! Cues -> SRT text.
//!
//! A CAPT file's italic words are written `<i>…</i>`, and a cue in another place than the bottom
//! starts with the `{\anN}` tag most players read (the nine places of the numpad).

use crate::Word;
use crate::cues::{Cue, Layout, cues};
use crate::style::{self, Markup, Sound};

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// SRT for cues and, for subtitles for the deaf and hard of hearing, sounds and voice labels.
pub(crate) fn write(cues: &[Cue], sounds: &[Sound], voice: Option<&dyn Fn(&str) -> String>) -> (String, usize) {
    let markup = Markup { italic: Some(("<i>", "</i>")), lang: None, voice, escape: &style::plain };
    let mut items: Vec<(u64, u64, String)> = cues
        .iter()
        .map(|c| {
            let n = style::numpad(style::place(c));
            let tag = if n == 2 { String::new() } else { format!("{{\\an{n}}}") };
            (c.start, c.end, format!("{tag}{}", style::lines(c, &markup).join("\n")))
        })
        .collect();
    items.extend(sounds.iter().map(|s| (s.start, s.end, s.text.clone())));
    // Speech and sounds in order of start; speech first when both start together.
    items.sort_by_key(|(start, _, _)| *start);
    let out = items
        .iter()
        .enumerate()
        .map(|(i, (start, end, text))| format!("{}\n{} --> {}\n{text}\n\n", i + 1, timestamp(*start), timestamp(*end)))
        .collect();
    (out, items.len())
}

/// SRT text for `words` laid out by `layout`, and the number of cues.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    write(&cues(words, layout), &[], None)
}

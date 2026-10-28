//! Cues -> SRT text.
//!
//! A CAPT file's italic words are written `<i>…</i>`, and a cue in another place than the bottom
//! starts with the `{\anN}` tag most players read (the nine places of the numpad).

use crate::Word;
use crate::cues::{Cue, Layout, cues};
use crate::style::{self, Markup};

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

fn plain(text: &str) -> String {
    text.to_string()
}

/// The text of one SRT cue: its place tag and its lines, with italic and, when `voice` is given,
/// the voice labels (subtitles for the deaf and hard of hearing).
pub(crate) fn cue_text(c: &Cue, voice: Option<&dyn Fn(&str) -> String>) -> String {
    let markup = Markup { italic: Some(("<i>", "</i>")), lang: None, voice, escape: &plain };
    let lines = if style::marked(c) { style::lines(c, &markup) } else { None };
    let n = style::numpad(style::place(c));
    let tag = if n == 2 { String::new() } else { format!("{{\\an{n}}}") };
    format!("{tag}{}", lines.unwrap_or_else(|| c.lines.clone()).join("\n"))
}

/// One SRT entry.
pub(crate) fn entry(n: usize, start: u64, end: u64, text: &str) -> String {
    format!("{n}\n{} --> {}\n{text}\n\n", timestamp(start), timestamp(end))
}

/// SRT text for `words` laid out by `layout`, and the number of cues.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    let cues = cues(words, layout);
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&entry(i + 1, c.start, c.end, &cue_text(c, None)));
    }
    (out, cues.len())
}

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

/// A cue's text: `{\\anN}` when not at the bottom, then its lines with italic marked.
fn text(c: &Cue) -> String {
    let markup = Markup { italic: Some(("<i>", "</i>")), lang: None, escape: &style::plain };
    let n = style::numpad(style::place(c));
    let tag = if n == 2 { String::new() } else { format!("{{\\an{n}}}") };
    format!("{tag}{}", style::lines(c, &markup).join("\n"))
}

/// SRT text for `words` laid out by `layout`, and the number of cues.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    let cues = cues(words, layout);
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, timestamp(c.start), timestamp(c.end), text(c)));
    }
    (out, cues.len())
}

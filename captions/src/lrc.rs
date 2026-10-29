//! Cues -> Enhanced LRC (lyrics files with a time before every word, the "A2" extension that
//! karaoke players and music apps read): `[mm:ss.xx]` for the line, `<mm:ss.xx>` before each word,
//! and one after the last word for the line's end. A cue is one line.

use crate::Word;
use crate::cues::{Layout, cues};

/// `mm:ss.xx`; minutes past 99 keep their digits.
fn timestamp(ms: u64) -> String {
    let cs = ms / 10;
    format!("{:02}:{:02}.{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

/// Enhanced LRC text for `words` laid out by `layout`, and the number of lines.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    let cues = cues(words, layout);
    let mut out = String::new();
    for c in &cues {
        out.push_str(&format!("[{}]{}", timestamp(c.start), c.prefix));
        for (i, w) in c.words.iter().enumerate() {
            if i > 0 && !w.glued {
                out.push(' ');
            }
            out.push_str(&format!("<{}>{}", timestamp(w.start), w.text));
        }
        out.push_str(&format!(" <{}>\n", timestamp(c.end)));
    }
    (out, cues.len())
}

//! Cues -> SRT text.

use crate::Word;
use crate::cues::{Layout, cues};

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// SRT text for `words` laid out by `layout`, and the number of cues.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    let cues = cues(words, layout);
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, timestamp(c.start), timestamp(c.end), c.text()));
    }
    (out, cues.len())
}

//! Cues -> SBV, the subtitle format YouTube Studio uploads and downloads: `H:MM:SS.mmm,H:MM:SS.mmm`,
//! then the lines, then a blank line. The same cues as SRT.

use crate::Word;
use crate::cues::{Layout, cues};

fn timestamp(ms: u64) -> String {
    format!("{}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// SBV text for `words` laid out by `layout`, and the number of cues.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    let cues = cues(words, layout);
    let mut out = String::new();
    for c in &cues {
        out.push_str(&format!("{},{}\n{}\n\n", timestamp(c.start), timestamp(c.end), c.text()));
    }
    (out, cues.len())
}

//! Cues -> WebVTT text (W3C WebVTT: <https://www.w3.org/TR/webvtt1/>).
//!
//! The same cues as SRT, numbered the same; only the header, the timestamps (`.` before the
//! milliseconds) and the escaping of `&`, `<` and `>` differ.

use crate::Word;
use crate::cues::{Layout, cues};

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// `&` and `<` start markup in WebVTT text; `>` is escaped too so no text reads as `-->`.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// WebVTT text for `words` laid out by `layout`, and the number of cues.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    let cues = cues(words, layout);
    let mut out = String::from("WEBVTT\n\n");
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, timestamp(c.start), timestamp(c.end), escape(&c.text())));
    }
    (out, cues.len())
}

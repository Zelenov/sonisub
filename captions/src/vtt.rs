//! Cues -> WebVTT text (W3C WebVTT: <https://www.w3.org/TR/webvtt1/>).
//!
//! The same cues as SRT, numbered the same; only the header, the timestamps (`.` before the
//! milliseconds) and the escaping of `&`, `<` and `>` differ. A CAPT file's italic words are
//! `<i>…</i>`, words in another language `<lang xx>…</lang>`, and a cue's place is in its cue
//! settings (`line`, `position`, `size`); on a vertical frame the bottom stays clear of a phone
//! app's buttons.

use crate::Word;
use crate::capt::Pos;
use crate::cues::{Cue, Layout, cues};
use crate::style::{self, Markup, Sound, pct};

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// `&` and `<` start markup in WebVTT text; `>` is escaped too so no text reads as `-->`.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn lang_tags(l: &str) -> (String, String) {
    (format!("<lang {}>", escape(l)), "</lang>".into())
}

/// The cue settings that put a cue in its place.
fn settings(place: Option<Pos>, layout: &Layout) -> String {
    let vertical = layout.is_vertical();
    match place {
        None | Some(Pos::Bottom) if vertical => format!(" line:{},end", pct(1.0 - style::bottom_clear(layout))),
        None | Some(Pos::Bottom) => String::new(),
        Some(Pos::Top) if vertical => format!(" line:{},start", pct(style::top_clear(layout))),
        Some(Pos::Top) => " line:0".into(),
        Some(Pos::Middle) => " line:50%,center".into(),
        Some(Pos::Point { x, y }) => format!(" position:{},center line:{},center", pct(x), pct(y)),
        // The rectangle's width and top edge; a WebVTT cue has no height.
        Some(Pos::Rect { x, y, w, .. }) => {
            format!(" position:{},center line:{},start size:{}", pct(x + w / 2.0), pct(y), pct(w))
        }
    }
}

/// WebVTT for cues and, for subtitles for the deaf and hard of hearing, sounds and voice labels.
pub(crate) fn write(
    cues: &[Cue],
    sounds: &[Sound],
    layout: &Layout,
    voice: Option<&dyn Fn(&str) -> String>,
) -> (String, usize) {
    let markup = Markup { italic: Some(("<i>", "</i>")), lang: Some(&lang_tags), voice, escape: &escape };
    let mut items: Vec<(u64, u64, String, String)> = cues
        .iter()
        .map(|c| (c.start, c.end, settings(style::place(c), layout), style::lines(c, &markup).join("\n")))
        .collect();
    items.extend(sounds.iter().map(|s| (s.start, s.end, settings(None, layout), escape(&s.text))));
    items.sort_by_key(|i| i.0);
    let mut out = String::from("WEBVTT\n\n");
    for (i, (start, end, settings, text)) in items.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}{settings}\n{text}\n\n", i + 1, timestamp(*start), timestamp(*end)));
    }
    (out, items.len())
}

/// WebVTT text for `words` laid out by `layout`, and the number of cues.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    write(&cues(words, layout), &[], layout, None)
}

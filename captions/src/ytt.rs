//! A CAPT file -> YTT, YouTube's own timed text (also called srv3: the `<timedtext format="3">` XML
//! YouTube Studio exports as `.srv3` and accepts as a subtitle upload).
//!
//! Google does not document the format; this follows what YouTube Studio exports and what
//! YTSubConverter documents (<https://github.com/arcusmaximus/YTSubConverter>):
//!
//! - `<head>` holds pens (`<pen id i="1">` for italic), window styles (`<ws ju>`, justification) and
//!   window positions (`<wp ap ah av>`: an anchor point 0–8 like a numpad read from the top, and
//!   its place in percent of the width and height);
//! - `<body>` has a `<p t d wp ws>` per subtitle (start and duration in milliseconds) and an
//!   `<s t>` per word, `t` in milliseconds after the subtitle's start, so YouTube shows the words
//!   one by one as they are said; a line break is a newline in the text.
//!
//! Places follow the CAPT file; on a vertical frame the bottom and the top stay clear of the
//! Shorts buttons, as in the other writers. [`crate::sdh::ytt`] adds sounds and voices.

use crate::capt::{Doc, Pos};
use crate::cues::{Cue, Layout, cues, pieces};
use crate::labels;
use crate::style::{self, Sound};

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A window position: anchor point (0 top left … 4 centre … 7 bottom centre … 8 bottom right),
/// horizontal and vertical place in percent.
type Window = (u8, u32, u32);

/// Where a place goes on YouTube's player.
fn window(place: Option<Pos>, layout: &Layout) -> Window {
    let pct = |v: f64| (v * 100.0).round().clamp(0.0, 100.0) as u32;
    match place {
        None | Some(Pos::Bottom) => (7, 50, pct(1.0 - style::bottom_clear(layout))),
        Some(Pos::Top) => (1, 50, pct(style::top_clear(layout))),
        Some(Pos::Middle) => (4, 50, 50),
        Some(Pos::Point { x, y }) => (4, pct(x), pct(y)),
        // The rectangle's top edge, centred on its width; YouTube has no box to wrap in.
        Some(Pos::Rect { x, y, w, .. }) => (1, pct(x + w / 2.0), pct(y)),
    }
}

/// One `<s>`: milliseconds after the subtitle's start, italic, the text as shown.
struct Run {
    t: u64,
    italic: bool,
    text: String,
}

/// The cue as runs, a run per word (the speaker label and a voice label go with the word after
/// them); `None` if the lines do not split back into the cue's words.
fn runs(cue: &Cue, voice: Option<&dyn Fn(&str) -> String>) -> Option<Vec<Run>> {
    let mut words = cue.words.iter();
    let mut prefix = if cue.prefix.is_empty() { 0 } else { pieces(cue.prefix.trim_end()).len() };
    let mut out: Vec<Run> = Vec::new();
    let mut pending = String::new();
    let mut said: Option<&str> = None;
    let mut rest = 0;
    for (l, line) in cue.lines.iter().enumerate() {
        for (i, (piece, glued)) in pieces(line).into_iter().enumerate() {
            let gap = if i == 0 && l > 0 {
                "\n"
            } else if i > 0 && !glued {
                " "
            } else {
                ""
            };
            if prefix > 0 {
                prefix -= 1;
                pending.push_str(gap);
                pending.push_str(piece);
                continue;
            }
            if rest > 0 {
                rest -= 1;
                let run = out.last_mut()?;
                run.text.push_str(gap);
                run.text.push_str(piece);
                continue;
            }
            let w = words.next()?;
            rest = style::word_pieces(w, piece)?;
            let mut text = std::mem::take(&mut pending);
            text.push_str(gap);
            if !w.glued && w.marks.voice.as_deref() != said {
                said = w.marks.voice.as_deref();
                if let (Some(f), Some(v)) = (voice, said) {
                    text.push_str(&f(v));
                    text.push(' ');
                }
            }
            text.push_str(piece);
            out.push(Run { t: w.start.saturating_sub(cue.start), italic: w.marks.italic, text });
        }
    }
    words.next().is_none().then_some(out)
}

/// YTT text for `doc` laid out by `layout`, and the number of subtitles.
pub fn build(doc: &Doc, layout: &Layout) -> (String, usize) {
    write(doc, layout, None)
}

/// YTT; with `sdh` (the labels' language) sounds and voices too.
pub(crate) fn write(doc: &Doc, layout: &Layout, sdh: Option<&str>) -> (String, usize) {
    let cues = cues(&doc.to_words(), layout);
    let sounds: Vec<Sound> = sdh.map(|lang| style::sounds(doc, layout, lang)).unwrap_or_default();
    let lang = sdh.unwrap_or_default();
    let label = |v: &str| labels::voice(v, lang);
    let voice = sdh.map(|_| &label as &dyn Fn(&str) -> String);

    // A window position per place, in order of first use; the bottom is always the first.
    let mut windows: Vec<Window> = vec![window(None, layout)];
    let mut wp = |w: Window| match windows.iter().position(|x| *x == w) {
        Some(i) => i,
        None => {
            windows.push(w);
            windows.len() - 1
        }
    };
    let mut items: Vec<(u64, String)> = Vec::new();
    for c in &cues {
        let id = wp(window(style::place(c), layout));
        let body: String = match runs(c, voice) {
            Some(runs) => runs
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let pen = if r.italic { " p=\"2\"" } else { "" };
                    let t = if i == 0 { String::new() } else { format!(" t=\"{}\"", r.t) };
                    format!("<s{pen}{t}>{}</s>", xml(&r.text))
                })
                .collect(),
            None => xml(&c.text()),
        };
        items.push((
            c.start,
            format!("<p t=\"{}\" d=\"{}\" wp=\"{id}\" ws=\"0\">{body}</p>\n", c.start, c.end - c.start),
        ));
    }
    for s in &sounds {
        items.push((
            s.start,
            format!("<p t=\"{}\" d=\"{}\" wp=\"0\" ws=\"0\">{}</p>\n", s.start, s.end - s.start, xml(&s.text)),
        ));
    }
    items.sort_by_key(|(start, _)| *start);

    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<timedtext format=\"3\">\n<head>\n");
    out.push_str("<pen id=\"1\"/>\n<pen id=\"2\" i=\"1\"/>\n<ws id=\"0\" ju=\"2\"/>\n");
    for (i, (ap, ah, av)) in windows.iter().enumerate() {
        out.push_str(&format!("<wp id=\"{i}\" ap=\"{ap}\" ah=\"{ah}\" av=\"{av}\"/>\n"));
    }
    out.push_str("</head>\n<body>\n");
    for (_, p) in &items {
        out.push_str(p);
    }
    out.push_str("</body>\n</timedtext>\n");
    (out, items.len())
}

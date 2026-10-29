//! Cues -> YTT, YouTube's own timed text (also called srv3: the `<timedtext format="3">` XML
//! YouTube Studio exports as `.srv3` and accepts as a subtitle upload).
//!
//! Google does not document the format; this follows what YouTube Studio exports and what
//! YTSubConverter does (<https://github.com/arcusmaximus/YTSubConverter>, `YttDocument.cs`):
//!
//! - `<head>` holds a pen for italic (`<pen id="2" i="1">`), a window style (`<ws ju>`,
//!   justification) and window positions (`<wp ap ah av>`: an anchor point 0–8 like a numpad read
//!   from the top, and its place in percent; YouTube draws `v` at `2 + 0.96 × v`, so a place is
//!   written through that);
//! - `<body>` has a `<p t d wp ws>` per subtitle (start and duration in milliseconds, the start at
//!   least 1: the Android app drops the place of, or the whole, subtitle at 0) and an `<s t>` per
//!   word, `t` in milliseconds after the subtitle's start, so YouTube shows the words one by one
//!   as they are said; a line break is a newline in the text. The server takes the pen off a
//!   line's first section unless the line has text outside any section, so a zero-width space
//!   follows the first section; a line of one section has its pen on the `<p>`.
//!
//! Places follow the CAPT file; on a vertical frame the bottom and the top stay clear of the
//! Shorts buttons, as in the other writers. [`crate::sdh::ytt`] adds sounds and voices.

use crate::Word;
use crate::capt::Pos;
use crate::cues::{Cue, Layout, cues};
use crate::style::{self, Gap, Sound, StepKind};

/// Text goes only into element content: `&` and `<` start markup, `>` is escaped as well.
fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A window position: anchor point (0 top left … 4 centre … 7 bottom centre … 8 bottom right),
/// horizontal and vertical place as YouTube reads them.
type Window = (u8, u32, u32);

/// Where a place goes on YouTube's player.
fn window(place: Option<Pos>, layout: &Layout) -> Window {
    // YouTube draws a place of `v` percent at `2 + 0.96 × v`.
    let yt_place = |v: f64| ((v * 100.0 - 2.0) / 0.96).round().clamp(0.0, 100.0) as u32;
    match place {
        None | Some(Pos::Bottom) => (7, 50, yt_place(1.0 - style::bottom_clear(layout))),
        Some(Pos::Top) => (1, 50, yt_place(style::top_clear(layout))),
        Some(Pos::Middle) => (4, 50, 50),
        Some(Pos::Point { x, y }) => (4, yt_place(x), yt_place(y)),
        // The rectangle's top edge, centred on its width; YouTube has no box to wrap in.
        Some(Pos::Rect { x, y, w, .. }) => (1, yt_place(x + w / 2.0), yt_place(y)),
    }
}

/// One `<s>`: milliseconds after the subtitle's start, italic, the text as shown.
struct Run {
    t: u64,
    italic: bool,
    text: String,
}

/// The cue as runs from `t0`: a run per word, the speaker label and a voice label each a run of
/// their own (never italic, as in the other formats); `None` if the lines do not split back into
/// the cue's words.
fn runs(cue: &Cue, voice: Option<&dyn Fn(&str) -> String>, t0: u64) -> Option<Vec<Run>> {
    let mut out: Vec<Run> = Vec::new();
    let mut prefix = String::new();
    for step in style::steps(cue)? {
        let gap = match step.gap {
            Gap::Line => "\n",
            Gap::Space => " ",
            Gap::None => "",
        };
        match step.kind {
            StepKind::Prefix => {
                prefix.push_str(gap);
                prefix.push_str(step.text);
            }
            StepKind::More => {
                let run = out.last_mut()?;
                run.text.push_str(gap);
                run.text.push_str(step.text);
            }
            StepKind::Word { word, voice: starts, .. } => {
                let t = word.start.saturating_sub(t0);
                let mut gap = gap;
                if !prefix.is_empty() {
                    out.push(Run { t, italic: false, text: std::mem::take(&mut prefix) });
                }
                if let (Some(f), Some(v)) = (voice, starts) {
                    out.push(Run { t, italic: false, text: format!("{gap}{} ", f(v)) });
                    gap = "";
                }
                out.push(Run { t, italic: word.marks.italic, text: format!("{gap}{}", step.text) });
            }
        }
    }
    Some(out)
}

/// YTT text for `words` laid out by `layout`, and the number of subtitles.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    write(&cues(words, layout), &[], layout, None)
}

/// YTT for cues and, for subtitles for the deaf and hard of hearing, sounds and voice labels.
pub(crate) fn write(
    cues: &[Cue],
    sounds: &[Sound],
    layout: &Layout,
    voice: Option<&dyn Fn(&str) -> String>,
) -> (String, usize) {
    // A window position per place, in order of first use; the bottom is always the first.
    let mut windows: Vec<Window> = vec![window(None, layout)];
    let mut wp = |w: Window| match windows.iter().position(|x| *x == w) {
        Some(i) => i,
        None => {
            windows.push(w);
            windows.len() - 1
        }
    };
    // `<p>` from `t0` (the start, at 1 ms at the earliest, as the Android app needs) to `end`, with
    // a pen for the whole line or none.
    let p = |t0: u64, end: u64, wp: usize, pen: &str, body: &str| {
        format!("<p t=\"{t0}\" d=\"{}\" wp=\"{wp}\" ws=\"0\"{pen}>{body}</p>\n", end.saturating_sub(t0).max(1))
    };
    let pen = |italic: bool| if italic { " p=\"2\"" } else { "" };
    let mut items: Vec<(u64, String)> = Vec::new();
    // Where each cue was put, so a sound is not drawn over one: YouTube gives every `<p>` its own
    // window, and windows in one place are drawn on top of each other.
    let mut shown: Vec<(u64, u64, usize)> = Vec::new();
    for c in cues {
        let id = wp(window(style::place(c), layout));
        shown.push((c.start, c.end, id));
        let t0 = c.start.max(1);
        let line = match runs(c, voice, t0) {
            // One section: its pen on the line, as YTSubConverter writes it.
            Some(runs) if runs.len() == 1 => p(t0, c.end, id, pen(runs[0].italic), &xml(&runs[0].text)),
            // The server drops the first section's pen unless the line has text outside any
            // section: a zero-width space after the first one.
            Some(runs) => {
                let body: String = runs
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        // As YTSubConverter writes it: no `t` at the line's start.
                        let t = if r.t == 0 { String::new() } else { format!(" t=\"{}\"", r.t) };
                        let after = if i == 0 { "\u{200b}" } else { "" };
                        format!("<s{}{t}>{}</s>{after}", pen(r.italic), xml(&r.text))
                    })
                    .collect();
                p(t0, c.end, id, "", &body)
            }
            None => p(t0, c.end, id, "", &xml(&c.text())),
        };
        items.push((c.start, line));
    }
    let top = wp(window(Some(Pos::Top), layout));
    let busy = |id: usize, s: &Sound| shown.iter().any(|&(a, b, w)| w == id && a < s.end && s.start < b);
    for s in sounds {
        // At the bottom, or at the top while speech is at the bottom.
        let id = if busy(0, s) && !busy(top, s) { top } else { 0 };
        items.push((s.start, p(s.start.max(1), s.end, id, "", &xml(&s.text))));
    }
    items.sort_by_key(|(start, _)| *start);

    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<timedtext format=\"3\">\n<head>\n");
    out.push_str("<pen id=\"2\" i=\"1\"/>\n<ws id=\"0\" ju=\"2\"/>\n");
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

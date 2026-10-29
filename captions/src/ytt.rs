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
//!   line's first section, so a line of several starts with an empty one (a zero-width space).
//!
//! Places follow the CAPT file; on a vertical frame the bottom and the top stay clear of the
//! Shorts buttons, as in the other writers. [`crate::sdh::ytt`] adds sounds and voices.

use crate::Word;
use crate::capt::Pos;
use crate::cues::{Cue, Layout, cues};
use crate::style::{self, Sound, StepKind};

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A window position: anchor point (0 top left … 4 centre … 7 bottom centre … 8 bottom right),
/// horizontal and vertical place as YouTube reads them.
type Window = (u8, u32, u32);

/// Where a place goes on YouTube's player.
fn window(place: Option<Pos>, layout: &Layout) -> Window {
    // YouTube draws a place of `v` percent at `2 + 0.96 × v`.
    let pct = |v: f64| ((v * 100.0 - 2.0) / 0.96).round().clamp(0.0, 100.0) as u32;
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
    let mut out: Vec<Run> = Vec::new();
    let mut pending = String::new();
    for step in style::steps(cue)? {
        let gap = if step.new_line {
            "\n"
        } else if step.space {
            " "
        } else {
            ""
        };
        match step.kind {
            StepKind::Prefix => {
                pending.push_str(gap);
                pending.push_str(step.text);
            }
            StepKind::More => {
                let run = out.last_mut()?;
                run.text.push_str(gap);
                run.text.push_str(step.text);
            }
            StepKind::Word { word, voice: starts, .. } => {
                let mut text = std::mem::take(&mut pending);
                text.push_str(gap);
                if let (Some(f), Some(v)) = (voice, starts) {
                    text.push_str(&f(v));
                    text.push(' ');
                }
                text.push_str(step.text);
                out.push(Run { t: word.start.saturating_sub(cue.start), italic: word.marks.italic, text });
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
    // `<p>` from `start` to `end`: at 1 ms at the earliest, as the Android app needs.
    let p = |start: u64, end: u64, wp: usize, body: &str| {
        let t = start.max(1);
        format!("<p t=\"{t}\" d=\"{}\" wp=\"{wp}\" ws=\"0\">{body}</p>\n", end.saturating_sub(t).max(1))
    };
    let mut items: Vec<(u64, String)> = Vec::new();
    for c in cues {
        let id = wp(window(style::place(c), layout));
        let body: String = match runs(c, voice) {
            Some(runs) if runs.len() > 1 => {
                // The server drops the pen of a line's first section: let that be an empty one.
                let first = "<s>\u{200b}</s>".to_string();
                let shift = c.start.max(1) - c.start;
                std::iter::once(first)
                    .chain(runs.iter().map(|r| {
                        let pen = if r.italic { " p=\"2\"" } else { "" };
                        format!("<s{pen} t=\"{}\">{}</s>", r.t.saturating_sub(shift), xml(&r.text))
                    }))
                    .collect()
            }
            Some(runs) => runs
                .iter()
                .map(|r| format!("<s{}>{}</s>", if r.italic { " p=\"2\"" } else { "" }, xml(&r.text)))
                .collect(),
            None => xml(&c.text()),
        };
        items.push((c.start, p(c.start, c.end, id, &body)));
    }
    for s in sounds {
        items.push((s.start, p(s.start, s.end, 0, &xml(&s.text))));
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

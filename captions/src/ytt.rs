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
//! Places follow the CAPT file.

use crate::Word;
use crate::capt::Pos;
use crate::cues::{Cue, Layout, cues};
use crate::style::{self, Gap, StepKind};

/// Text goes only into element content: `&` and `<` start markup, `>` is escaped as well, and
/// characters XML forbids are left out, as YouTube refuses a file that has one.
fn xml(s: &str) -> String {
    style::xml_chars(s).replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A window position: anchor point (0 top left … 4 centre … 7 bottom centre … 8 bottom right),
/// horizontal and vertical place as YouTube reads them.
type Window = (u8, u32, u32);

/// Where a place goes on YouTube's player.
fn window(place: Option<Pos>) -> Window {
    // YouTube draws a place of `v` percent at `2 + 0.96 × v`.
    let yt_place = |v: f64| ((v * 100.0 - 2.0) / 0.96).round().clamp(0.0, 100.0) as u32;
    match place {
        None | Some(Pos::Bottom) => (7, 50, 100),
        Some(Pos::Top) => (1, 50, 0),
        Some(Pos::Middle) => (4, 50, 50),
        Some(Pos::Point { x, y }) => (4, yt_place(x), yt_place(y)),
        // The rectangle's top edge, centred on its width; YouTube has no box to wrap in.
        Some(Pos::Rect { x, y, w, .. }) => (1, yt_place(x + w / 2.0), yt_place(y)),
    }
}

/// Where the windows stacked up from the bottom are, in YouTube's percent, for lines shown at once
/// when the bottom, the top and the middle are all taken: clear of the middle window (50) and the
/// top one, for lines of up to two rows each (a taller one may reach the window above). A line
/// past these goes where it wants, over another.
const STACKED: [u32; 6] = [89, 78, 67, 34, 23, 12];

/// One `<s>`: milliseconds after the subtitle's start, italic, the text as shown.
struct Run {
    t: u64,
    italic: bool,
    text: String,
}

/// The cue as runs from `t0`: a run per word, the speaker label a run of its own (never italic, as
/// in the other formats); `None` if the lines do not split back into the cue's words.
fn runs(cue: &Cue, t0: u64) -> Option<Vec<Run>> {
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
            StepKind::Word { word, .. } => {
                let t = word.start.saturating_sub(t0);
                if !prefix.is_empty() {
                    out.push(Run { t, italic: false, text: std::mem::take(&mut prefix) });
                }
                out.push(Run { t, italic: word.marks.italic, text: format!("{gap}{}", step.text) });
            }
        }
    }
    // A section that starts with the one before is broken by the upload (YTSubConverter's
    // AvoidZeroDurationKaraoke): each starts a millisecond after the one before at least.
    for i in 1..out.len() {
        if out[i].t <= out[i - 1].t {
            out[i].t = out[i - 1].t + 1;
        }
    }
    Some(out)
}

/// YTT text for `words` laid out by `layout`, and the number of subtitles.
pub fn build(words: &[Word], layout: &Layout) -> (String, usize) {
    // In order of start, so a line only has to look at the lines still on screen.
    let mut cues = cues(words, layout);
    cues.sort_by_key(|c| c.start);
    // Window positions in order of first use, after an unused one: YTSubConverter leaves item 0 of
    // each list unused, as the iOS app mishandles the first.
    let mut windows: Vec<Window> = vec![(7, 50, 100)];
    let mut window_id = |w: Window| match windows.iter().skip(1).position(|x| *x == w) {
        Some(i) => i + 1,
        None => {
            windows.push(w);
            windows.len() - 1
        }
    };
    // `<p>` from `t0` (the start, at 1 ms at the earliest, as the Android app needs) to `end`, with
    // a pen for the whole line or none.
    let paragraph = |t0: u64, end: u64, wp: usize, pen: &str, body: &str| {
        format!("<p t=\"{t0}\" d=\"{}\" wp=\"{wp}\" ws=\"1\"{pen}>{body}</p>\n", end.saturating_sub(t0).max(1))
    };
    let pen = |italic: bool| if italic { " p=\"2\"" } else { "" };

    // YouTube gives every `<p>` its own window, and windows in one place are drawn on top of each
    // other: a line goes where it wants if that is free at the time, else to the bottom, the top
    // or the middle, whichever is free first, else to a window stacked up from the bottom: nine
    // lines at once at most each have their own.
    let free_order = [window(None), window(Some(Pos::Top)), window(Some(Pos::Middle))];
    let stacked = STACKED.map(|av| (7, 50, av));
    // The lines on screen: their end and window.
    let mut shown: Vec<(u64, Window)> = Vec::new();
    let mut items: Vec<String> = Vec::new();
    for c in &cues {
        let (start, wants) = (c.start, window(style::place(c)));
        let t0 = start.max(1);
        let runs = runs(c, t0);
        // Sections bumped a millisecond apart may run past the cue's end: the line lasts until its
        // last one starts at least, and holds its window until then.
        let end = match &runs {
            Some(runs) if runs.len() > 1 => c.end.max(t0 + runs.last().map_or(0, |r| r.t) + 1),
            _ => c.end,
        };
        shown.retain(|(until, _)| *until > start);
        let free = |w: &Window| !shown.iter().any(|(_, sw)| sw == w);
        let at = std::iter::once(wants).chain(free_order).chain(stacked).find(|w| free(w)).unwrap_or(wants);
        shown.push((end, at));
        let id = window_id(at);
        items.push(match runs {
            // One section: its pen on the line, as YTSubConverter writes it.
            Some(runs) if runs.len() == 1 => paragraph(t0, end, id, pen(runs[0].italic), &xml(&runs[0].text)),
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
                paragraph(t0, end, id, "", &body)
            }
            None => paragraph(t0, end, id, "", &xml(&c.text())),
        });
    }

    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<timedtext format=\"3\">\n<head>\n");
    out.push_str("<pen id=\"2\" i=\"1\"/>\n<ws id=\"0\"/>\n<ws id=\"1\" ju=\"2\"/>\n");
    for (i, (ap, ah, av)) in windows.iter().enumerate() {
        out.push_str(&format!("<wp id=\"{i}\" ap=\"{ap}\" ah=\"{ah}\" av=\"{av}\"/>\n"));
    }
    out.push_str("</head>\n<body>\n");
    for item in &items {
        out.push_str(item);
    }
    out.push_str("</body>\n</timedtext>\n");
    (out, items.len())
}

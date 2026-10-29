//! A CAPT file -> TTML (the W3C Timed Text Markup Language, as IMSC 1.1 Text profiles it): what
//! broadcasters and streaming services ask for.
//!
//! Speakers are `ttm:agent`s and each subtitle names its speaker; a place is a `region` (the
//! bottom, the top, the middle, and one per point or rectangle of the file, exactly); italic and
//! other languages than the file's are `<span>`s. [`crate::sdh::ttml`] adds sounds as subtitles
//! and voices as labels ([`crate::labels`]).
//!
//! IMSC wants regions shown at the same time never to overlap: a subtitle whose region would
//! overlap one already on screen goes into that region, under what is there. The font is sized
//! from the frame (`ttp:cellResolution`) so a line of the layout's length fits a region's width;
//! on a vertical frame the bottom and top regions stay clear of a phone app's buttons.

use crate::capt::{Doc, Pos};
use crate::cues::{Cue, Layout, cues};
use crate::style::{self, Markup, Sound, speaker_name};

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn lang_span(l: &str) -> (String, String) {
    (format!("<span xml:lang=\"{}\">", xml(l)), "</span>".into())
}

/// A region: its id, its box (left, top, width, height as shares of the frame) and how text sits
/// in it (`displayAlign`, `textAlign`).
struct Region {
    id: String,
    rect: (f64, f64, f64, f64),
    display: &'static str,
    text: &'static str,
}

impl Region {
    fn overlaps(&self, other: &Region) -> bool {
        let ((ax, ay, aw, ah), (bx, by, bw, bh)) = (self.rect, other.rect);
        let e = 1e-9;
        ax < bx + bw - e && bx < ax + aw - e && ay < by + bh - e && by < ay + ah - e
    }

    /// The `<region>` element. Each edge is rounded inward to 0.1%, so a region inside the frame
    /// stays inside it.
    fn line(&self) -> String {
        let (x, y, w, h) = self.rect;
        let from = |v: f64| ((v * 1000.0 - 1e-6).ceil() / 10.0).clamp(0.0, 100.0);
        let to = |v: f64| ((v * 1000.0 + 1e-6).floor() / 10.0).clamp(0.0, 100.0);
        let (x0, y0) = (from(x), from(y));
        let (ew, eh) = ((to(x + w) - x0).max(0.1), (to(y + h) - y0).max(0.1));
        let pct = |v: f64| format!("{}%", (v * 10.0).round() / 10.0 + 0.0);
        format!(
            "      <region xml:id=\"{}\" tts:origin=\"{} {}\" tts:extent=\"{} {}\" tts:displayAlign=\"{}\" tts:textAlign=\"{}\"/>\n",
            self.id,
            pct(x0),
            pct(y0),
            pct(ew),
            pct(eh),
            self.display,
            self.text
        )
    }
}

/// Rows of the cell grid (`ttp:cellResolution`), and so the font size (one cell): a line of the
/// layout's length, about half a cell a character, fills 80% of the frame's width. IMSC's default
/// is 15 rows; a frame never gets bigger text than that.
fn cell_rows(layout: &Layout) -> u32 {
    let (w, h) = (layout.frame.0 as f64, layout.frame.1 as f64);
    if layout.max_line == 0 {
        return 15;
    }
    let cell = 0.8 * w / (0.5 * layout.max_line as f64);
    ((h / cell).ceil() as u32).max(15)
}

/// The bottom, top and middle regions, 80% of the width: tall enough for the layout's lines, the
/// middle between the other two.
fn named_regions(layout: &Layout) -> [Region; 3] {
    let lines = if layout.wrap_lines { layout.max_lines.max(1) } else { 2 };
    let height = (lines as f64 * 1.25 / cell_rows(layout) as f64).max(0.2);
    // The bottom region ends 5% above the frame's edge, or where a vertical frame's clear zone starts.
    let bottom = if layout.is_vertical() { 1.0 - style::bottom_clear(layout) } else { 0.95 };
    let top = if layout.is_vertical() { style::top_clear(layout) } else { 0.05 };
    let middle = if layout.is_vertical() { (top + height).min(bottom - 2.0 * height) } else { 0.5 - height / 2.0 };
    let region =
        |id: &str, y: f64, display| Region { id: id.into(), rect: (0.1, y, 0.8, height), display, text: "center" };
    [region("bottom", bottom - height, "after"), region("top", top, "before"), region("middle", middle, "center")]
}

/// The region of a point, `id`: as big as the named ones (80% × 20%) and inside the frame, centred
/// on the point where it can be; near an edge the text is aligned toward the point.
fn point_region(id: String, x: f64, y: f64) -> Region {
    let rect = ((x - 0.4).clamp(0.0, 0.2), (y - 0.1).clamp(0.0, 0.8), 0.8, 0.2);
    let text = if x < 0.4 {
        "left"
    } else if x > 0.6 {
        "right"
    } else {
        "center"
    };
    let display = if y < 0.1 {
        "before"
    } else if y > 0.9 {
        "after"
    } else {
        "center"
    };
    Region { id, rect, display, text }
}

/// An `xml:id` for each speaker the file names or hears, in that order: `speaker_` and the id's
/// letters, digits, `-`, `_` and `.`, numbered when two come out the same.
fn agent_ids(doc: &Doc) -> Vec<(String, String)> {
    let mut speakers: Vec<&str> = doc.speakers.iter().map(|s| s.id.as_str()).collect();
    for sp in doc.blocks.iter().filter_map(|b| b.speaker.as_deref()) {
        if !speakers.contains(&sp) {
            speakers.push(sp);
        }
    }
    let mut ids: Vec<(String, String)> = Vec::new();
    for sp in speakers {
        let clean: String =
            sp.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
        let mut id = format!("speaker_{clean}");
        let mut n = 2;
        while ids.iter().any(|(_, i)| *i == id) {
            id = format!("speaker_{clean}_{n}");
            n += 1;
        }
        ids.push((sp.to_string(), id));
    }
    ids
}

/// The document's language for `<tt>`: the file's, else its first word's, else `und` (IMSC needs one).
fn doc_lang(doc: &Doc, cues: &[Cue]) -> String {
    doc.lang
        .clone()
        .or_else(|| cues.iter().flat_map(|c| &c.words).find_map(|w| w.language.clone()))
        .unwrap_or_else(|| "und".into())
}

/// TTML text for `doc` laid out by `layout`, and the number of subtitles.
pub fn build(doc: &Doc, layout: &Layout) -> (String, usize) {
    write(doc, &cues(&doc.to_words(), layout), &[], layout, None)
}

/// TTML for cues and, for subtitles for the deaf and hard of hearing, sounds and voice labels;
/// `doc` gives the title, the speakers and the language.
pub(crate) fn write(
    doc: &Doc,
    cues: &[Cue],
    sounds: &[Sound],
    layout: &Layout,
    voice: Option<&dyn Fn(&str) -> String>,
) -> (String, usize) {
    // The named regions, then one per other place of the file, in order of first use.
    let mut regions: Vec<Region> = named_regions(layout).into();
    let mut places: Vec<Pos> = Vec::new();
    for p in cues.iter().filter_map(style::place) {
        if !matches!(p, Pos::Bottom | Pos::Top | Pos::Middle) && !places.contains(&p) {
            places.push(p);
            let id = format!("r{}", places.len());
            regions.push(match p {
                Pos::Rect { x, y, w, h } => Region { id, rect: (x, y, w, h), display: "before", text: "center" },
                Pos::Point { x, y } => point_region(id, x, y),
                _ => unreachable!("named places have their own regions"),
            });
        }
    }
    let region_of = |p: Option<Pos>| match p {
        None | Some(Pos::Bottom) => 0,
        Some(Pos::Top) => 1,
        Some(Pos::Middle) => 2,
        Some(p) => 3 + places.iter().position(|q| *q == p).expect("listed"),
    };

    let agents = agent_ids(doc);
    let agent = |sp: Option<&str>| {
        sp.and_then(|s| agents.iter().find(|(a, _)| a == s))
            .map(|(_, id)| format!(" ttm:agent=\"{id}\""))
            .unwrap_or_default()
    };
    let markup = Markup {
        italic: Some(("<span tts:fontStyle=\"italic\">", "</span>")),
        lang: Some(&lang_span),
        voice,
        escape: &xml,
    };
    // (start, end, region, the rest of the <p>)
    let mut items: Vec<(u64, u64, usize, String)> = cues
        .iter()
        .map(|c| {
            let body = format!("{}>{}", agent(c.speaker.as_deref()), style::lines(c, &markup).join("<br/>"));
            (c.start, c.end, region_of(style::place(c)), body)
        })
        .collect();
    items.extend(
        sounds.iter().map(|s| (s.start, s.end, 0, format!("{}>{}", agent(s.speaker.as_deref()), xml(&s.text)))),
    );
    items.sort_by_key(|i| i.0);
    // A subtitle whose region overlaps one on screen goes into that one.
    let mut shown: Vec<(u64, usize)> = Vec::new();
    for item in &mut items {
        shown.retain(|(end, _)| *end > item.0);
        if let Some(&(_, r)) = shown.iter().find(|(_, r)| *r != item.2 && regions[*r].overlaps(&regions[item.2])) {
            item.2 = r;
        }
        shown.push((item.1, item.2));
    }

    let (w, h) = layout.frame;
    let rows = cell_rows(layout);
    let cols = ((rows as f64 * w as f64 / h as f64).round() as u32).max(1);
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <tt xmlns=\"http://www.w3.org/ns/ttml\" xmlns:tts=\"http://www.w3.org/ns/ttml#styling\" \
         xmlns:ttm=\"http://www.w3.org/ns/ttml#metadata\" xmlns:ttp=\"http://www.w3.org/ns/ttml#parameter\" \
         ttp:timeBase=\"media\" ttp:contentProfiles=\"http://www.w3.org/ns/ttml/profile/imsc1.1/text\" \
         ttp:cellResolution=\"{cols} {rows}\" tts:extent=\"{w}px {h}px\" xml:lang=\"{}\">\n  <head>\n    <metadata>\n",
        xml(&doc_lang(doc, cues))
    );
    if let Some(t) = &doc.title {
        out.push_str(&format!("      <ttm:title>{}</ttm:title>\n", xml(t)));
    }
    for (sp, id) in &agents {
        out.push_str(&format!(
            "      <ttm:agent xml:id=\"{id}\" type=\"person\"><ttm:name type=\"full\">{}</ttm:name></ttm:agent>\n",
            xml(&speaker_name(doc, layout, sp))
        ));
    }
    out.push_str("    </metadata>\n    <layout>\n");
    for r in &regions {
        out.push_str(&r.line());
    }
    out.push_str("    </layout>\n  </head>\n  <body>\n    <div>\n");
    for (start, end, r, body) in &items {
        out.push_str(&format!(
            "      <p begin=\"{}\" end=\"{}\" region=\"{}\"{body}</p>\n",
            timestamp(*start),
            timestamp(*end),
            regions[*r].id
        ));
    }
    out.push_str("    </div>\n  </body>\n</tt>\n");
    (out, items.len())
}

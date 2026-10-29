//! A CAPT file -> TTML (the W3C Timed Text Markup Language, as IMSC 1.1 Text profiles it): what
//! broadcasters and streaming services ask for.
//!
//! Speakers are `ttm:agent`s and each subtitle names its speaker; a place is a `region` (the
//! bottom, the top, the middle, and one per point or rectangle of the file, exactly); italic and
//! other languages than the file's are `<span>`s. [`crate::sdh::ttml`] adds sounds as subtitles
//! and voices as labels ([`crate::labels`]). Text is white with a black outline, in the player's
//! sans serif, so it reads on any picture.
//!
//! IMSC wants regions shown at the same time never to overlap and at most four of them: a
//! subtitle whose region would overlap one already on screen, or be a fifth, goes into one on
//! screen, under what is there, and a region grows (from its anchored edge) to hold every line
//! shown in it at once. The font is sized from the frame (`ttp:cellResolution`) so a line of the
//! layout's length fits a region's width with room to spare; on a vertical frame the bottom and top
//! regions stay clear of a phone app's buttons.

use crate::capt::{Doc, Pos};
use crate::cues::{Cue, Layout, cues};
use crate::style::{self, Markup, Sound, speaker_name};

/// The width of a character, in cells: a sans serif needs about half a cell; this leaves room for
/// Cyrillic and capitals.
const CHAR_WIDTH: f64 = 0.55;
/// The height of a line, in cells (`tts:lineHeight` 125%).
const LINE_HEIGHT: f64 = 1.25;
/// IMSC 1.1 Text shows at most this many regions at once.
const MAX_SHOWN: usize = 4;
/// The named regions, first in the list.
const BOTTOM: usize = 0;
const TOP: usize = 1;
const MIDDLE: usize = 2;
/// How many named regions come before the file's own places.
const NAMED: usize = 3;
/// A bound on the passes of placing and growing; each pass grows some region by a line, so it is
/// never reached in practice, only kept so a mistake cannot loop for ever.
const MAX_PASSES_PER_REGION: usize = 8;

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The characters a player shows for TTML text: tags left out, an entity one character.
fn shown_len(text: &str) -> usize {
    let (mut n, mut tag, mut entity) = (0, false, false);
    for c in text.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            _ if tag => {}
            '&' => {
                entity = true;
                n += 1;
            }
            ';' if entity => entity = false,
            _ if entity => {}
            _ => n += 1,
        }
    }
    n
}

fn lang_span(l: &str) -> (String, String) {
    (format!("<span xml:lang=\"{}\">", xml(l)), "</span>".into())
}

/// Where text sits in a region from top to bottom (`tts:displayAlign`), and so which way the
/// region grows: away from that edge.
#[derive(Clone, Copy, PartialEq)]
enum Display {
    Before,
    Center,
    After,
}

impl Display {
    fn as_str(self) -> &'static str {
        match self {
            Display::Before => "before",
            Display::Center => "center",
            Display::After => "after",
        }
    }
}

/// A region: its id, its box (left, top, width, height as shares of the frame) and how text sits
/// in it.
struct Region {
    id: String,
    rect: (f64, f64, f64, f64),
    display: Display,
    text_align: &'static str,
}

impl Region {
    fn overlaps(&self, other: &Region) -> bool {
        let ((ax, ay, aw, ah), (bx, by, bw, bh)) = (self.rect, other.rect);
        let e = 1e-9;
        ax < bx + bw - e && bx < ax + aw - e && ay < by + bh - e && by < ay + ah - e
    }

    fn center(&self) -> (f64, f64) {
        let (x, y, w, h) = self.rect;
        (x + w / 2.0, y + h / 2.0)
    }

    /// The `<region>` element. Each edge is rounded inward to 0.1%, so a region inside the frame
    /// stays inside it.
    fn element(&self) -> String {
        let (x, y, w, h) = self.rect;
        let from = |v: f64| ((v * 1000.0 - 1e-6).ceil() / 10.0).clamp(0.0, 100.0);
        let to = |v: f64| ((v * 1000.0 + 1e-6).floor() / 10.0).clamp(0.0, 100.0);
        let (x0, y0) = (from(x), from(y));
        let (ew, eh) = ((to(x + w) - x0).max(0.1), (to(y + h) - y0).max(0.1));
        // Not `style::pct`: the value is rounded already, and `+ 0.0` keeps `-0` out.
        let pct = |v: f64| format!("{}%", (v * 10.0).round() / 10.0 + 0.0);
        format!(
            "      <region xml:id=\"{}\" tts:origin=\"{} {}\" tts:extent=\"{} {}\" tts:displayAlign=\"{}\" tts:textAlign=\"{}\"/>\n",
            self.id,
            pct(x0),
            pct(y0),
            pct(ew),
            pct(eh),
            self.display.as_str(),
            self.text_align
        )
    }
}

/// The text cell grid: rows (`ttp:cellResolution`, and so the font size, one cell) and the size of
/// a cell as shares of the frame's width and height.
struct Grid {
    rows: u32,
    cols: u32,
    /// A cell's width as a share of the frame's width.
    cell_w: f64,
    /// A line's height as a share of the frame's height.
    line_h: f64,
}

impl Grid {
    /// A line of the layout's length fills 80% of the frame's width. IMSC's default is 15 rows; a
    /// frame never gets bigger text than that.
    fn new(layout: &Layout) -> Grid {
        let (w, h) = (layout.frame.0 as f64, layout.frame.1 as f64);
        let rows = if layout.max_line == 0 {
            15
        } else {
            let cell = 0.8 * w / (CHAR_WIDTH * layout.max_line as f64);
            ((h / cell).ceil() as u32).max(15)
        };
        let cols = ((rows as f64 * w / h).round() as u32).max(1);
        Grid { rows, cols, cell_w: h / rows as f64 / w, line_h: LINE_HEIGHT / rows as f64 }
    }

    /// How many lines a line of `chars` characters takes in a region `width` wide, once wrapped.
    fn lines(&self, chars: usize, width: f64) -> usize {
        let per_line = ((width / (CHAR_WIDTH * self.cell_w)).floor() as usize).max(1);
        chars.div_ceil(per_line).max(1)
    }
}

/// The height of the named regions: the layout's lines (two when IMSC wraps a long one) and a
/// sound under them, at least 20% of the frame.
fn region_height(layout: &Layout, grid: &Grid) -> f64 {
    let lines = if layout.wrap_lines { layout.max_lines.max(1) } else { 2 };
    ((lines + 1) as f64 * grid.line_h).max(0.2)
}

/// The bottom, top and middle regions (in the order of `BOTTOM`, `TOP`, `MIDDLE`), 80% of the
/// width: the middle right under the top one, never over the bottom one.
fn named_regions(layout: &Layout, height: f64) -> [Region; 3] {
    // The bottom region ends 5% above the frame's edge, or where a vertical frame's clear zone starts.
    let bottom = if layout.is_vertical() { 1.0 - style::bottom_clear(layout) } else { 0.95 };
    let top = if layout.is_vertical() { style::top_clear(layout) } else { 0.05 };
    let middle = if layout.is_vertical() { (top + height).min(bottom - 2.0 * height) } else { 0.5 - height / 2.0 };
    let region = |id: &str, y: f64, display| Region {
        id: id.into(),
        rect: (0.1, y, 0.8, height),
        display,
        text_align: "center",
    };
    [
        region("bottom", bottom - height, Display::After),
        region("top", top, Display::Before),
        region("middle", middle, Display::Center),
    ]
}

/// The region of a point, `id`: as big as the named ones (80% wide, `height` tall) and inside the
/// frame, centred on the point where it can be; near an edge the text is aligned toward the point.
fn point_region(id: String, x: f64, y: f64, height: f64) -> Region {
    let rect = ((x - 0.4).clamp(0.0, 0.2), (y - height / 2.0).clamp(0.0, 1.0 - height), 0.8, height);
    let text_align = if x < 0.4 {
        "left"
    } else if x > 0.6 {
        "right"
    } else {
        "center"
    };
    let display = if y < height / 2.0 {
        Display::Before
    } else if y > 1.0 - height / 2.0 {
        Display::After
    } else {
        Display::Center
    };
    Region { id, rect, display, text_align }
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
        .filter(|l| !l.is_empty())
        .or_else(|| cues.iter().flat_map(|c| &c.words).find_map(|w| w.language.clone().filter(|l| !l.is_empty())))
        .unwrap_or_else(|| "und".into())
}

/// A `<p>` to write: when, in which region, whose (an agent id), what, and the characters of each
/// of its lines (to count the lines it takes in its region).
struct Item {
    start: u64,
    end: u64,
    region: usize,
    agent: Option<String>,
    text: String,
    widths: Vec<usize>,
}

/// One pass of placing: each item goes into a region on screen when its own would overlap one, or
/// would be a fifth; into the nearest of them for a fifth.
fn place(items: &mut [Item], regions: &[Region]) {
    let mut shown: Vec<(u64, usize)> = Vec::new();
    for item in items.iter_mut() {
        shown.retain(|(end, _)| *end > item.start);
        if let Some(&(_, r)) =
            shown.iter().find(|(_, r)| *r != item.region && regions[*r].overlaps(&regions[item.region]))
        {
            item.region = r;
        }
        let mut on_screen: Vec<usize> = shown.iter().map(|(_, r)| *r).collect();
        on_screen.sort_unstable();
        on_screen.dedup();
        if on_screen.len() >= MAX_SHOWN && !on_screen.contains(&item.region) {
            let (cx, cy) = regions[item.region].center();
            let dist = |r: &usize| {
                let (x, y) = regions[*r].center();
                (x - cx).powi(2) + (y - cy).powi(2)
            };
            item.region = *on_screen.iter().min_by(|a, b| dist(a).total_cmp(&dist(b))).expect("four on screen");
        }
        shown.push((item.end, item.region));
    }
}

/// Places the items, then grows each region to the most lines shown in it at once, and again
/// until no region grows (each pass grows a region at least a line, so this ends); a last pass
/// places after the last growth.
fn place_items(items: &mut [Item], regions: &mut [Region], grid: &Grid) {
    for _ in 0..=regions.len() * MAX_PASSES_PER_REGION {
        place(items, regions);
        let mut grown = false;
        for (r, region) in regions.iter_mut().enumerate() {
            let (x, y, w, h) = region.rect;
            let lines_at = |t: u64| -> usize {
                items
                    .iter()
                    .filter(|o| o.region == r && o.start <= t && o.end > t)
                    .flat_map(|o| o.widths.iter().map(|c| grid.lines(*c, w)))
                    .sum()
            };
            let most = items.iter().filter(|i| i.region == r).map(|i| lines_at(i.start)).max().unwrap_or(0);
            // A hair more than the lines, so rounding the edges inward never cuts the last one.
            let need = (most as f64 * grid.line_h + 0.001).min(1.0);
            if most > 0 && need > h + 1e-9 {
                let y = match region.display {
                    Display::After => y + h - need,
                    Display::Center => y - (need - h) / 2.0,
                    Display::Before => y,
                };
                region.rect = (x, y.clamp(0.0, 1.0 - need), w, need);
                grown = true;
            }
        }
        if !grown {
            return;
        }
    }
    place(items, regions);
}

/// TTML text for `doc` laid out by `layout`, and the number of subtitles. It takes the CAPT
/// document, not words like [`crate::srt::build`], for its title, speakers and language.
pub fn build(doc: &Doc, layout: &Layout) -> (String, usize) {
    write(doc, &cues(&doc.to_words(), layout), &[], layout, None, None)
}

/// TTML for cues and, for subtitles for the deaf and hard of hearing, sounds and voice labels in
/// `label_lang`; `doc` gives the title, the speakers and the language.
pub(crate) fn write(
    doc: &Doc,
    cues: &[Cue],
    sounds: &[Sound],
    layout: &Layout,
    voice: Option<&dyn Fn(&str) -> String>,
    label_lang: Option<&str>,
) -> (String, usize) {
    let grid = Grid::new(layout);
    let height = region_height(layout, &grid);
    // The named regions, then one per other place of the file, in order of first use.
    let mut regions: Vec<Region> = named_regions(layout, height).into();
    let mut places: Vec<Pos> = Vec::new();
    for p in cues.iter().filter_map(style::place) {
        let id = format!("r{}", places.len() + 1);
        let region = match p {
            Pos::Bottom | Pos::Top | Pos::Middle => continue,
            _ if places.contains(&p) => continue,
            Pos::Rect { x, y, w, h } => {
                Region { id, rect: (x, y, w, h), display: Display::Before, text_align: "center" }
            }
            Pos::Point { x, y } => point_region(id, x, y, height),
        };
        places.push(p);
        regions.push(region);
    }
    let region_of = |p: Option<Pos>| match p {
        None | Some(Pos::Bottom) => BOTTOM,
        Some(Pos::Top) => TOP,
        Some(Pos::Middle) => MIDDLE,
        Some(p) => NAMED + places.iter().position(|q| *q == p).expect("every place of the cues is listed"),
    };

    let agents = agent_ids(doc);
    let agent = |sp: Option<&str>| sp.and_then(|s| agents.iter().find(|(a, _)| a == s)).map(|(_, id)| id.clone());
    let markup = Markup {
        italic: Some(("<span tts:fontStyle=\"italic\">", "</span>")),
        lang: Some(&lang_span),
        voice,
        escape: &xml,
    };
    let lang = doc_lang(doc, cues);
    let mut items: Vec<Item> = cues
        .iter()
        .map(|c| {
            // Counted as shown: with a voice label, if SDH writes one.
            let lines = style::lines(c, &markup);
            Item {
                start: c.start,
                end: c.end,
                region: region_of(style::place(c)),
                agent: agent(c.speaker.as_deref()),
                widths: lines.iter().map(|l| shown_len(l)).collect(),
                text: lines.join("<br/>"),
            }
        })
        .collect();
    // A sound label in another language than the document's says so.
    let label_span = label_lang.filter(|l| *l != lang).map(lang_span);
    items.extend(sounds.iter().map(|s| {
        let text = match &label_span {
            Some((open, close)) => format!("{open}{}{close}", xml(&s.text)),
            None => xml(&s.text),
        };
        let widths = vec![s.text.chars().count()];
        Item { start: s.start, end: s.end, region: BOTTOM, agent: agent(s.speaker.as_deref()), text, widths }
    }));
    items.sort_by_key(|i| i.start);
    place_items(&mut items, &mut regions, &grid);

    let (w, h) = layout.frame;
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <tt xmlns=\"http://www.w3.org/ns/ttml\" xmlns:tts=\"http://www.w3.org/ns/ttml#styling\" \
         xmlns:ttm=\"http://www.w3.org/ns/ttml#metadata\" xmlns:ttp=\"http://www.w3.org/ns/ttml#parameter\" \
         ttp:timeBase=\"media\" ttp:contentProfiles=\"http://www.w3.org/ns/ttml/profile/imsc1.1/text\" \
         ttp:cellResolution=\"{} {}\" tts:extent=\"{w}px {h}px\" xml:lang=\"{}\">\n  <head>\n    <metadata>\n",
        grid.cols,
        grid.rows,
        xml(&lang)
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
        out.push_str(&r.element());
    }
    // White text with a black outline reads on any picture; the line height the regions are sized by.
    out.push_str(
        "    </layout>\n  </head>\n  <body tts:color=\"white\" tts:textOutline=\"black 0.05c\" \
         tts:fontFamily=\"proportionalSansSerif\" tts:lineHeight=\"125%\">\n    <div>\n",
    );
    for i in &items {
        let agent = i.agent.as_deref().map(|id| format!(" ttm:agent=\"{id}\"")).unwrap_or_default();
        out.push_str(&format!(
            "      <p begin=\"{}\" end=\"{}\" region=\"{}\"{agent}>{}</p>\n",
            timestamp(i.start),
            timestamp(i.end),
            regions[i.region].id,
            i.text
        ));
    }
    out.push_str("    </div>\n  </body>\n</tt>\n");
    (out, items.len())
}

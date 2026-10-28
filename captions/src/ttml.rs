//! A CAPT file -> TTML (the W3C Timed Text Markup Language, as IMSC 1.1 Text profiles it): what
//! broadcasters and streaming services ask for.
//!
//! Speakers are `ttm:agent`s and each subtitle names its speaker; a place is a `region` (the
//! bottom, the top, the middle, and one per point or rectangle of the file, exactly); italic and
//! other languages are `<span>`s. With SDH labels, sounds are subtitles and voices are labels
//! ([`crate::labels`]). On a vertical frame the bottom and top regions stay clear of a phone app's
//! buttons.

use crate::capt::{Doc, Pos};
use crate::cues::{Layout, cues};
use crate::labels;
use crate::style::{self, Markup, pct, speaker_name};

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn lang_span(l: &str) -> (String, String) {
    (format!("<span xml:lang=\"{}\">", xml(l)), "</span>".into())
}

/// The regions a frame has for its named places: (id, x, y, width, height, displayAlign).
fn named_regions(layout: &Layout) -> [(&'static str, f64, f64, f64, f64, &'static str); 3] {
    // The bottom region ends 5% above the frame's edge, or where a vertical frame's clear zone starts.
    let bottom = if layout.is_vertical() { 1.0 - style::bottom_clear(layout) } else { 0.95 };
    let top = if layout.is_vertical() { style::top_clear(layout) } else { 0.05 };
    [
        ("bottom", 0.1, bottom - 0.2, 0.8, 0.2, "after"),
        ("top", 0.1, top, 0.8, 0.2, "before"),
        ("middle", 0.1, 0.4, 0.8, 0.2, "center"),
    ]
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

/// TTML text for `doc` laid out by `layout`, and the number of subtitles. With `sdh` (the labels'
/// language, `en`, `ru`) it is subtitles for the deaf and hard of hearing: sounds and voices too.
pub fn build(doc: &Doc, layout: &Layout, sdh: Option<&str>) -> (String, usize) {
    let words = doc.to_words();
    let cues = cues(&words, layout);
    // Every other place of the file gets its own region, in order of first use.
    let mut places: Vec<Pos> = Vec::new();
    for p in cues.iter().filter_map(style::place) {
        if !matches!(p, Pos::Bottom | Pos::Top | Pos::Middle) && !places.contains(&p) {
            places.push(p);
        }
    }
    let region = |p: Option<Pos>| match p {
        None | Some(Pos::Bottom) => "bottom".to_string(),
        Some(Pos::Top) => "top".into(),
        Some(Pos::Middle) => "middle".into(),
        Some(p) => format!("r{}", places.iter().position(|q| *q == p).expect("listed") + 1),
    };

    let (w, h) = layout.frame;
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <tt xmlns=\"http://www.w3.org/ns/ttml\" xmlns:tts=\"http://www.w3.org/ns/ttml#styling\" \
         xmlns:ttm=\"http://www.w3.org/ns/ttml#metadata\" xmlns:ttp=\"http://www.w3.org/ns/ttml#parameter\" \
         ttp:timeBase=\"media\" ttp:contentProfiles=\"http://www.w3.org/ns/ttml/profile/imsc1.1/text\" \
         tts:extent=\"{w}px {h}px\" xml:lang=\"{}\">\n  <head>\n    <metadata>\n",
        xml(doc.lang.as_deref().unwrap_or(""))
    );
    if let Some(t) = &doc.title {
        out.push_str(&format!("      <ttm:title>{}</ttm:title>\n", xml(t)));
    }
    let agents = agent_ids(doc);
    for (sp, id) in &agents {
        out.push_str(&format!(
            "      <ttm:agent xml:id=\"{id}\" type=\"person\"><ttm:name type=\"full\">{}</ttm:name></ttm:agent>\n",
            xml(&speaker_name(doc, layout, sp))
        ));
    }
    out.push_str("    </metadata>\n    <layout>\n");
    let region_line = |id: &str, x: f64, y: f64, rw: f64, rh: f64, align: &str| {
        format!(
            "      <region xml:id=\"{id}\" tts:origin=\"{} {}\" tts:extent=\"{} {}\" tts:displayAlign=\"{align}\" tts:textAlign=\"center\"/>\n",
            pct(x),
            pct(y),
            pct(rw),
            pct(rh)
        )
    };
    for (id, x, y, rw, rh, align) in named_regions(layout) {
        out.push_str(&region_line(id, x, y, rw, rh, align));
    }
    for (i, p) in places.iter().enumerate() {
        let line = match *p {
            Pos::Rect { x, y, w, h } => region_line(&format!("r{}", i + 1), x, y, w, h, "before"),
            // A point gets a region centred on it, at most as big as the named ones and inside the
            // frame, so text centred in it is centred on the point.
            Pos::Point { x, y } => {
                let (rw, rh) = ((2.0 * x.min(1.0 - x)).min(0.8), (2.0 * y.min(1.0 - y)).min(0.2));
                region_line(&format!("r{}", i + 1), x - rw / 2.0, y - rh / 2.0, rw, rh, "center")
            }
            _ => unreachable!("named places have their own regions"),
        };
        out.push_str(&line);
    }
    out.push_str("    </layout>\n  </head>\n  <body>\n    <div>\n");

    let lang = sdh.unwrap_or_default();
    let voice = |v: &str| labels::voice(v, lang);
    let markup = Markup {
        italic: Some(("<span tts:fontStyle=\"italic\">", "</span>")),
        lang: Some(&lang_span),
        voice: sdh.map(|_| &voice as &dyn Fn(&str) -> String),
        escape: &xml,
    };
    let agent = |sp: Option<&str>| {
        sp.and_then(|s| agents.iter().find(|(a, _)| a == s))
            .map(|(_, id)| format!(" ttm:agent=\"{id}\""))
            .unwrap_or_default()
    };
    let mut items: Vec<(u64, String)> = cues
        .iter()
        .map(|c| {
            let p = format!(
                "      <p begin=\"{}\" end=\"{}\" region=\"{}\"{}>{}</p>\n",
                timestamp(c.start),
                timestamp(c.end),
                region(style::place(c)),
                agent(c.speaker.as_deref()),
                style::lines(c, &markup).join("<br/>")
            );
            (c.start, p)
        })
        .collect();
    if sdh.is_some() {
        for s in style::sounds(doc, layout, lang) {
            let p = format!(
                "      <p begin=\"{}\" end=\"{}\" region=\"bottom\"{}>{}</p>\n",
                timestamp(s.start),
                timestamp(s.end),
                agent(s.speaker.as_deref()),
                xml(&s.text)
            );
            items.push((s.start, p));
        }
    }
    items.sort_by_key(|(start, _)| *start);
    for (_, p) in &items {
        out.push_str(p);
    }
    out.push_str("    </div>\n  </body>\n</tt>\n");
    (out, items.len())
}

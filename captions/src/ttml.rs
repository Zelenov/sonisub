//! A CAPT file -> TTML (the W3C Timed Text Markup Language, as IMSC 1.1 Text profiles it): what
//! broadcasters and streaming services ask for.
//!
//! Speakers are `ttm:agent`s and each subtitle names its speaker; a place is a `region` (the
//! bottom, the top, the middle, and one per point or rectangle of the file, exactly); italic and
//! other languages are `<span>`s; sounds and voices are SDH labels ([`crate::labels`]). On a
//! vertical frame the bottom and top regions stay clear of a phone app's buttons.

use crate::capt::{Doc, Pos};
use crate::cues::{Layout, cues};
use crate::labels;
use crate::sdh::speaker_name;
use crate::style::{self, Markup, pct};

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
    let top = if layout.is_vertical() { 0.12 } else { 0.05 };
    [
        ("bottom", 0.1, bottom - 0.2, 0.8, 0.2, "after"),
        ("top", 0.1, top, 0.8, 0.2, "before"),
        ("middle", 0.1, 0.4, 0.8, 0.2, "center"),
    ]
}

/// TTML text for `doc` laid out by `layout`, labels in `lang` (`en`, `ru`), and the number of
/// subtitles.
pub fn build(doc: &Doc, layout: &Layout, lang: &str) -> (String, usize) {
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
         ttp:timeBase=\"media\" tts:extent=\"{w}px {h}px\" xml:lang=\"{}\">\n  <head>\n    <metadata>\n",
        xml(doc.lang.as_deref().unwrap_or(""))
    );
    if let Some(t) = &doc.title {
        out.push_str(&format!("      <ttm:title>{}</ttm:title>\n", xml(t)));
    }
    for s in &doc.speakers {
        out.push_str(&format!(
            "      <ttm:agent xml:id=\"speaker_{}\" type=\"person\"><ttm:name type=\"full\">{}</ttm:name></ttm:agent>\n",
            xml(&s.id),
            xml(&speaker_name(doc, layout, &s.id))
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
            // A point gets a region around it, as wide as the named ones and inside the frame.
            Pos::Point { x, y } => {
                let (rx, ry) = ((x - 0.4).clamp(0.0, 0.2), (y - 0.1).clamp(0.0, 0.8));
                region_line(&format!("r{}", i + 1), rx, ry, 0.8, 0.2, "center")
            }
            _ => unreachable!("named places have their own regions"),
        };
        out.push_str(&line);
    }
    out.push_str("    </layout>\n  </head>\n  <body>\n    <div>\n");

    let voice = |v: &str| labels::voice(v, lang);
    let markup = Markup {
        italic: Some(("<span tts:fontStyle=\"italic\">", "</span>")),
        lang: Some(&lang_span),
        voice: Some(&voice),
        escape: &xml,
    };
    let agent = |sp: Option<&str>| sp.map(|s| format!(" ttm:agent=\"speaker_{}\"", xml(s))).unwrap_or_default();
    let mut items: Vec<(u64, String)> = cues
        .iter()
        .map(|c| {
            let lines = if style::marked(c) { style::lines(c, &markup) } else { None };
            let lines = lines.unwrap_or_else(|| c.lines.iter().map(|l| xml(l)).collect());
            let p = format!(
                "      <p begin=\"{}\" end=\"{}\" region=\"{}\"{}>{}</p>\n",
                timestamp(c.start),
                timestamp(c.end),
                region(style::place(c)),
                agent(c.speaker.as_deref()),
                lines.join("<br/>")
            );
            (c.start, p)
        })
        .collect();
    for (start, end, speaker, code) in doc.sounds() {
        let p = format!(
            "      <p begin=\"{}\" end=\"{}\" region=\"bottom\"{}>{}</p>\n",
            timestamp(start),
            timestamp(end),
            agent(speaker),
            xml(&labels::sound(code, lang))
        );
        items.push((start, p));
    }
    items.sort_by_key(|(start, _)| *start);
    for (_, p) in &items {
        out.push_str(p);
    }
    out.push_str("    </div>\n  </body>\n</tt>\n");
    (out, items.len())
}

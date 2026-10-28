//! What the writers share for a CAPT file's marks: a cue's lines with markup for italic,
//! language and voice, and where on screen a cue goes in each format.

use crate::capt::Pos;
use crate::cues::{Cue, Layout, pieces};

/// The tags around words in a language: `en` -> (`<lang en>`, `</lang>`).
pub(crate) type LangTags<'a> = &'a dyn Fn(&str) -> (String, String);

/// How an output marks text; every field is optional except the escaping.
pub(crate) struct Markup<'a> {
    pub italic: Option<(&'a str, &'a str)>,
    pub lang: Option<LangTags<'a>>,
    pub voice: Option<&'a dyn Fn(&str) -> String>,
    pub escape: &'a dyn Fn(&str) -> String,
}

/// Whether any word of the cue has something to mark.
pub(crate) fn marked(cue: &Cue) -> bool {
    cue.words.iter().any(|w| w.marks.italic || w.marks.lang.is_some() || w.marks.voice.is_some())
}

/// The cue's lines with markup, or `None` if the lines do not split back into its words (the
/// caller then writes the lines as they are).
pub(crate) fn lines(cue: &Cue, m: &Markup) -> Option<Vec<String>> {
    let mut words = cue.words.iter().peekable();
    let mut prefix = if cue.prefix.is_empty() { 0 } else { pieces(cue.prefix.trim_end()).len() };
    let mut out = Vec::new();
    let mut voice: Option<&str> = None;
    for line in &cue.lines {
        let mut text = String::new();
        let (mut italic, mut lang): (bool, Option<&str>) = (false, None);
        let close = |text: &mut String, italic: &mut bool, lang: &mut Option<&str>| {
            if let (Some(l), Some(f)) = (lang.take(), m.lang) {
                text.push_str(&f(l).1);
            }
            if std::mem::take(italic)
                && let Some((_, end)) = m.italic
            {
                text.push_str(end);
            }
        };
        for (i, (piece, glued)) in pieces(line).into_iter().enumerate() {
            let space = i > 0 && !glued;
            if prefix > 0 {
                prefix -= 1;
                if space {
                    text.push(' ');
                }
                text.push_str(&(m.escape)(piece));
                continue;
            }
            let w = words.next().filter(|w| w.text == piece)?;
            let want_italic = m.italic.is_some() && w.marks.italic;
            let want_lang = m.lang.and(w.marks.lang.as_deref());
            let new_voice = m.voice.is_some() && !w.glued && w.marks.voice.as_deref().is_some_and(|v| Some(v) != voice);
            if !w.glued {
                voice = w.marks.voice.as_deref();
            }
            if want_italic != italic || want_lang != lang || new_voice {
                close(&mut text, &mut italic, &mut lang);
                if space {
                    text.push(' ');
                }
                if new_voice && let (Some(f), Some(v)) = (m.voice, voice) {
                    text.push_str(&(m.escape)(&f(v)));
                    text.push(' ');
                }
                if want_italic && let Some((start, _)) = m.italic {
                    text.push_str(start);
                    italic = true;
                }
                if let (Some(l), Some(f)) = (want_lang, m.lang) {
                    text.push_str(&f(l).0);
                    lang = Some(l);
                }
            } else if space {
                text.push(' ');
            }
            text.push_str(&(m.escape)(piece));
        }
        close(&mut text, &mut italic, &mut lang);
        out.push(text);
    }
    words.next().is_none().then_some(out)
}

/// The place of a cue: the first word's (a cue never mixes places).
pub(crate) fn place(cue: &Cue) -> Option<Pos> {
    cue.words.first().and_then(|w| w.marks.pos)
}

/// The share of the frame's height a vertical video keeps clear at the bottom (a phone app's
/// caption, buttons and progress bar); 0 for a horizontal one.
pub(crate) fn bottom_clear(layout: &Layout) -> f64 {
    if layout.is_vertical() { 0.28 } else { 0.0 }
}

/// The numpad alignment of SRT and ASS (7 8 9 / 4 5 6 / 1 2 3) nearest to a place.
pub(crate) fn numpad(pos: Option<Pos>) -> u8 {
    let (cx, cy) = match pos {
        None | Some(Pos::Bottom) => return 2,
        Some(Pos::Top) => return 8,
        Some(Pos::Middle) => return 5,
        Some(Pos::Point { x, y }) => (x, y),
        Some(Pos::Rect { x, y, w, h }) => (x + w / 2.0, y + h / 2.0),
    };
    let col = if cx < 1.0 / 3.0 {
        0
    } else if cx < 2.0 / 3.0 {
        1
    } else {
        2
    };
    let row = if cy < 1.0 / 3.0 {
        7
    } else if cy < 2.0 / 3.0 {
        4
    } else {
        1
    };
    row + col
}

/// A share as a WebVTT/TTML percentage: `0.725` -> `72.5%`.
pub(crate) fn pct(v: f64) -> String {
    format!("{}%", (v * 1000.0).round() / 10.0)
}

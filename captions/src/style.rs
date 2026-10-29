//! What the writers share for a CAPT file's marks: a cue's lines with markup for italic and
//! language, and where on screen a cue goes in each format.

use crate::capt::{Doc, Pos};
use crate::cues::{Cue, Layout, pieces, speaker_label};

/// The tags around words in a language: `en` -> (`<lang en>`, `</lang>`).
pub(crate) type LangTags<'a> = &'a dyn Fn(&str) -> (String, String);

/// How an output marks text; every field is optional except the escaping.
pub(crate) struct Markup<'a> {
    pub italic: Option<(&'a str, &'a str)>,
    pub lang: Option<LangTags<'a>>,
    pub escape: &'a dyn Fn(&str) -> String,
}

/// Whether any word of the cue has something to mark.
fn marked(cue: &Cue) -> bool {
    cue.words.iter().any(|w| w.marks.italic || w.marks.lang.is_some())
}

/// The cue's lines with markup, escaped; its lines as they are (escaped) when nothing is marked or
/// the lines do not split back into its words.
pub(crate) fn lines(cue: &Cue, m: &Markup) -> Vec<String> {
    marked(cue)
        .then(|| marked_lines(cue, m))
        .flatten()
        .unwrap_or_else(|| cue.lines.iter().map(|l| (m.escape)(l)).collect())
}

fn marked_lines(cue: &Cue, m: &Markup) -> Option<Vec<String>> {
    let mut words = cue.words.iter().peekable();
    let mut prefix = if cue.prefix.is_empty() { 0 } else { pieces(cue.prefix.trim_end()).len() };
    let mut out = Vec::new();
    // Pieces still to come of a word with a space in it (`New York`).
    let mut rest = 0;
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
            if rest > 0 {
                rest -= 1;
                if space {
                    text.push(' ');
                }
                text.push_str(&(m.escape)(piece));
                continue;
            }
            let w = words.next()?;
            rest = word_pieces(w, piece)?;
            let want_italic = m.italic.is_some() && w.marks.italic;
            let want_lang = m.lang.and(w.marks.lang.as_deref());
            if want_italic != italic || want_lang != lang {
                close(&mut text, &mut italic, &mut lang);
                if space {
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

/// How many more pieces of a line `w` takes after `piece`, its first; `None` if the line does not
/// go on with `w` there.
pub(crate) fn word_pieces(w: &crate::Word, piece: &str) -> Option<usize> {
    let own = pieces(&w.text);
    (own.first().map(|p| p.0) == Some(piece)).then(|| own.len() - 1)
}

/// The place of a cue: the first word's (a cue never mixes places).
pub(crate) fn place(cue: &Cue) -> Option<Pos> {
    cue.words.first().and_then(|w| w.marks.pos)
}

/// Text as it is, for formats that escape nothing.
pub(crate) fn plain(text: &str) -> String {
    text.to_string()
}

/// The name a speaker is shown by where no subtitle shows it next to speech (TTML agents): the
/// file's name, else the layout's, else "Speaker N".
pub(crate) fn speaker_name(doc: &Doc, layout: &Layout, speaker: &str) -> String {
    doc.speakers
        .iter()
        .find(|s| s.id == speaker)
        .and_then(|s| s.name.clone())
        .unwrap_or_else(|| speaker_label(speaker, &layout.speaker_names))
}

/// The numpad alignment of SRT's `{\anN}` (7 8 9 / 4 5 6 / 1 2 3) nearest to a place.
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

/// A share as a WebVTT percentage: `0.725` -> `72.5%`.
pub(crate) fn pct(v: f64) -> String {
    format!("{}%", (v * 1000.0).round() / 10.0)
}

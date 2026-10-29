//! What the writers share for a CAPT file's marks: a cue's lines with markup for italic,
//! language and voice, and where on screen a cue goes in each format.

use crate::capt::{Doc, Pos};
use crate::cues::{Cue, Layout, pieces, speaker_label};
use crate::labels;

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
fn marked(cue: &Cue) -> bool {
    cue.words.iter().any(|w| w.marks.italic || w.marks.lang.is_some() || w.marks.voice.is_some())
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
    let mut voice: Option<&str> = None;
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

/// How many more pieces of a line `w` takes after `piece`, its first; `None` if the line does not
/// go on with `w` there.
pub(crate) fn word_pieces(w: &crate::Word, piece: &str) -> Option<usize> {
    let own = pieces(&w.text);
    (own.first().map(|p| p.0) == Some(piece)).then(|| own.len() - 1)
}

/// What a piece of a cue's line is, for the writers that time every word (ASS karaoke, YTT).
pub(crate) enum StepKind<'a> {
    /// Part of the speaker label in front.
    Prefix,
    /// The first piece of `cue.words[index]`; `voice` when a voice starts with it.
    Word { index: usize, word: &'a crate::Word, voice: Option<&'a str> },
    /// A later piece of the same word (`York` of `New York`).
    More,
}

/// What comes before a piece of a cue's line.
#[derive(Clone, Copy)]
pub(crate) enum Gap {
    None,
    Space,
    Line,
}

/// A piece of a cue's line, in order: what comes before it, its text, and what it is.
pub(crate) struct Step<'a> {
    pub gap: Gap,
    pub text: &'a str,
    pub kind: StepKind<'a>,
}

/// The cue's lines as steps, every word found where it is shown; `None` if the lines do not split
/// back into its words.
pub(crate) fn steps(cue: &Cue) -> Option<Vec<Step<'_>>> {
    let mut words = cue.words.iter().enumerate();
    let mut prefix = if cue.prefix.is_empty() { 0 } else { pieces(cue.prefix.trim_end()).len() };
    let mut rest = 0;
    let mut said: Option<&str> = None;
    let mut out = Vec::new();
    for (l, line) in cue.lines.iter().enumerate() {
        for (i, (text, glued)) in pieces(line).into_iter().enumerate() {
            let gap = if i == 0 && l > 0 {
                Gap::Line
            } else if i > 0 && !glued {
                Gap::Space
            } else {
                Gap::None
            };
            let kind = if prefix > 0 {
                prefix -= 1;
                StepKind::Prefix
            } else if rest > 0 {
                rest -= 1;
                StepKind::More
            } else {
                let (index, word) = words.next()?;
                rest = word_pieces(word, text)?;
                let v = word.marks.voice.as_deref();
                let voice = (!word.glued && v != said).then_some(v).flatten();
                if !word.glued {
                    said = v;
                }
                StepKind::Word { index, word, voice }
            };
            out.push(Step { gap, text, kind });
        }
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

/// The share a vertical video keeps clear at the top (the app's tabs); 0 for a horizontal one.
pub(crate) fn top_clear(layout: &Layout) -> f64 {
    if layout.is_vertical() { 0.12 } else { 0.0 }
}

/// Text as it is, for formats that escape nothing.
pub(crate) fn plain(text: &str) -> String {
    text.to_string()
}

/// A sound as a subtitle of its own, for subtitles for the deaf and hard of hearing.
pub(crate) struct Sound {
    pub start: u64,
    pub end: u64,
    pub speaker: Option<String>,
    /// The label, after the speaker's name when the layout labels speakers: `Anna: [laughs]`.
    pub text: String,
}

/// The sound blocks of `doc` as subtitles, labels in `lang`; speakers named like the speech
/// around them ([`speaker_label`]), so one file never names a person two ways.
pub(crate) fn sounds(doc: &Doc, layout: &Layout, lang: &str) -> Vec<Sound> {
    // As speech is labelled: by speaker, when more than one is heard.
    let mut heard: Vec<&str> = doc.blocks.iter().filter_map(|b| b.speaker.as_deref()).collect();
    heard.sort_unstable();
    heard.dedup();
    let label = layout.speaker_labels && layout.by_speaker && heard.len() > 1;
    doc.sounds()
        .map(|b| {
            let (start, end, speaker, code) =
                (b.start, b.end, b.speaker.as_deref(), b.sound.as_deref().unwrap_or_default());
            let who = match speaker {
                Some(sp) if label => format!("{}: ", speaker_label(sp, &layout.speaker_names)),
                _ => String::new(),
            };
            Sound {
                start,
                end,
                speaker: speaker.map(str::to_string),
                text: format!("{who}{}", labels::sound(code, lang)),
            }
        })
        .collect()
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

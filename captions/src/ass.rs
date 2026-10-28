//! Cues -> ASS (Advanced SubStation Alpha, as Aegisub and libass read it).
//!
//! The same cues as SRT. Every speaker gets a style of their own (`Speaker 1`, or the speaker's name)
//! with a text colour from a fixed palette, so a person is restyled or renamed in one place; every
//! line names its speaker in `Name` and uses the speaker's style. Words without a speaker use
//! `Default`. With [`Options::karaoke`] each word lights up as it is spoken (`\k`). Subtitles for
//! the deaf and hard of hearing are [`crate::sdh::ass`].

use std::collections::BTreeMap;

use crate::Word;
use crate::capt::Pos;
use crate::cues::{Cue, Layout, cues, pieces, speaker_label};
use crate::style::{self, Markup, Sound};

/// What ASS adds to the layout.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Names by speaker label (`{"1": "Anna"}`). A speaker not listed is named by
    /// `layout.speaker_names`, else "Speaker N".
    pub names: BTreeMap<String, String>,
    /// Word-by-word highlight: a word is drawn dimmed until it is spoken (`\k`).
    pub karaoke: bool,
}

/// Text colours, RGB, in the order speakers appear; repeats after the last.
const PALETTE: [u32; 8] = [
    0xFFFFFF, // white
    0xFFE14D, // yellow
    0x5CE1FF, // cyan
    0x7CFC7C, // green
    0xFF8FD0, // pink
    0xFFAA4D, // orange
    0xC3A6FF, // violet
    0xFF7A7A, // red
];

const DEFAULT_STYLE: &str = "Default";

/// `&HAABBGGRR`, opaque.
fn colour(rgb: u32) -> String {
    let (r, g, b) = (rgb >> 16 & 0xff, rgb >> 8 & 0xff, rgb & 0xff);
    format!("&H00{b:02X}{g:02X}{r:02X}")
}

/// The colour a word has before it is spoken, with karaoke.
fn dimmed(rgb: u32) -> u32 {
    (rgb >> 1) & 0x7F7F7F
}

fn style_line(name: &str, rgb: u32) -> String {
    format!(
        "Style: {name},Arial,64,{},{},&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,3,1,2,80,80,60,1\n",
        colour(rgb),
        colour(dimmed(rgb))
    )
}

/// `H:MM:SS.cc`.
fn timestamp(ms: u64) -> String {
    let cs = ms / 10;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100)
}

/// A comma ends a field of a style or a dialogue line before `Text`.
fn field(text: &str) -> String {
    text.replace(',', " ").trim().to_string()
}

/// `{` `}` start an override block and `\` a tag; speech text never means either.
fn escape(text: &str) -> String {
    text.replace('\\', "/").replace('{', "(").replace('}', ")")
}

/// The cue's text with `\k` before every word: the time from its start to the next word's start,
/// in centiseconds; a voice label before words said in a new voice, with `voice`. `None` if the
/// lines do not split back into the cue's words.
fn karaoke(cue: &Cue, voice: Option<&dyn Fn(&str) -> String>) -> Option<String> {
    let mut words = cue.words.iter().peekable();
    let base = cue.start / 10;
    let mut prefix_pieces = if cue.prefix.is_empty() { 0 } else { pieces(cue.prefix.trim_end()).len() };
    let mut out = String::new();
    let mut italic = false;
    let mut said: Option<&str> = None;
    for (l, line) in cue.lines.iter().enumerate() {
        if l > 0 {
            out.push_str("\\N");
        }
        for (i, (piece, glued)) in pieces(line).into_iter().enumerate() {
            if i > 0 && !glued {
                out.push(' ');
            }
            if prefix_pieces > 0 {
                prefix_pieces -= 1;
                out.push_str(&escape(piece));
                continue;
            }
            let w = words.next().filter(|w| w.text == piece)?;
            if !w.glued && w.marks.voice.as_deref() != said {
                said = w.marks.voice.as_deref();
                if let (Some(f), Some(v)) = (voice, said) {
                    out.push_str(&escape(&f(v)));
                    out.push(' ');
                }
            }
            if w.marks.italic != italic {
                italic = w.marks.italic;
                out.push_str(if italic { "{\\i1}" } else { "{\\i0}" });
            }
            let until = words.peek().map_or(cue.end.max(w.end), |n| n.start) / 10;
            let from = (w.start / 10).max(base);
            out.push_str(&format!("{{\\k{}}}{}", until.saturating_sub(from), escape(piece)));
        }
    }
    if italic {
        out.push_str("{\\i0}");
    }
    words.next().is_none().then_some(out)
}

/// The override tag and the margins (left, right, vertical) that put a cue in its place on the
/// frame. A vertical frame keeps the bottom and the top clear for a phone app's buttons.
fn placement(cue: &Cue, layout: &Layout) -> (String, u32, u32, u32) {
    let (w, h) = (layout.frame.0 as f64, layout.frame.1 as f64);
    let share = |v: f64, of: f64| (v * of).round() as u32;
    match style::place(cue) {
        None | Some(Pos::Bottom) => (String::new(), 0, 0, share(style::bottom_clear(layout), h)),
        Some(Pos::Top) => ("{\\an8}".into(), 0, 0, share(style::top_clear(layout), h)),
        Some(Pos::Middle) => ("{\\an5}".into(), 0, 0, 0),
        Some(Pos::Point { x, y }) => (format!("{{\\an5\\pos({},{})}}", share(x, w), share(y, h)), 0, 0, 0),
        // The rectangle's width (left and right margins) and its top edge; ASS has no height.
        Some(Pos::Rect { x, y, w: rw, .. }) => ("{\\an8}".into(), share(x, w), share(1.0 - x - rw, w), share(y, h)),
    }
}

/// ASS text for `words` laid out by `layout`, and the number of cues.
pub fn build(words: &[Word], layout: &Layout, opts: &Options) -> (String, usize) {
    write(&cues(words, layout), &[], layout, opts, None)
}

/// ASS for cues and, for subtitles for the deaf and hard of hearing, sounds and voice labels.
pub(crate) fn write(
    cues: &[Cue],
    sounds: &[Sound],
    layout: &Layout,
    opts: &Options,
    voice: Option<&dyn Fn(&str) -> String>,
) -> (String, usize) {
    // A style per speaker, in order of appearance; a speaker heard only in sounds comes last.
    let mut styles: Vec<(String, String, String)> = Vec::new(); // speaker, style, name
    let speakers =
        cues.iter().filter_map(|c| c.speaker.as_deref()).chain(sounds.iter().filter_map(|s| s.speaker.as_deref()));
    for sp in speakers {
        if styles.iter().any(|(s, _, _)| s == sp) {
            continue;
        }
        let name = opts.names.get(sp).filter(|n| !n.trim().is_empty()).cloned();
        let name = field(&name.unwrap_or_else(|| speaker_label(sp, &layout.speaker_names)));
        let mut style = if name.is_empty() { format!("Speaker {}", field(sp)) } else { name.clone() };
        if style == DEFAULT_STYLE || styles.iter().any(|(_, s, _)| *s == style) {
            style = format!("{style} {}", field(sp));
        }
        styles.push((sp.to_string(), style, name));
    }
    let style_of = |speaker: Option<&str>| {
        speaker
            .and_then(|sp| styles.iter().find(|(s, _, _)| s == sp))
            .map_or((DEFAULT_STYLE, ""), |(_, style, name)| (style.as_str(), name.as_str()))
    };

    let mut out = format!(
        "[Script Info]\n\
         ScriptType: v4.00+\n\
         WrapStyle: 0\n\
         ScaledBorderAndShadow: yes\n\
         PlayResX: {}\n\
         PlayResY: {}\n\
         \n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, \
         Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, \
         Alignment, MarginL, MarginR, MarginV, Encoding\n",
        layout.frame.0, layout.frame.1
    );
    out.push_str(&style_line(DEFAULT_STYLE, PALETTE[0]));
    for (i, (_, style, _)) in styles.iter().enumerate() {
        out.push_str(&style_line(style, PALETTE[i % PALETTE.len()]));
    }
    out.push_str(
        "\n[Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );
    let markup = Markup { italic: Some(("{\\i1}", "{\\i0}")), lang: None, voice, escape: &escape };
    let mut events: Vec<(u64, String)> = Vec::new();
    for c in cues {
        let (style, name) = style_of(c.speaker.as_deref());
        let plain = || style::lines(c, &markup).join("\\N");
        let text = if opts.karaoke { karaoke(c, voice).unwrap_or_else(plain) } else { plain() };
        let (tag, ml, mr, mv) = placement(c, layout);
        let (start, end) = (timestamp(c.start), timestamp(c.end));
        events.push((c.start, format!("Dialogue: 0,{start},{end},{style},{name},{ml},{mr},{mv},,{tag}{text}\n")));
    }
    // A sound goes where speech goes by default: the bottom, clear of a phone app.
    let mv = (style::bottom_clear(layout) * layout.frame.1 as f64).round() as u32;
    for s in sounds {
        let (style, name) = style_of(s.speaker.as_deref());
        let (start, end, text) = (timestamp(s.start), timestamp(s.end), escape(&s.text));
        events.push((s.start, format!("Dialogue: 0,{start},{end},{style},{name},0,0,{mv},,{text}\n")));
    }
    events.sort_by_key(|(start, _)| *start);
    for (_, line) in &events {
        out.push_str(line);
    }
    (out, events.len())
}

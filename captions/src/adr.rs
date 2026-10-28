//! A CAPT file -> an ADR cue list for DaVinci Resolve's Fairlight page (ADR > List > Import Cue
//! List): one row per cue an actor records, `"Cue ID","In","Out","Character","Dialogue"`, quoted,
//! no header, UTF-8 with a byte order mark and CRLF line ends, timecodes `HH:MM:SS:FF`.
//!
//! A cue is a speaker's breath of speech: their cues as SRT cuts them, joined while the pause
//! between them is short and the cue stays short, never across another speaker. The speaker's
//! own sounds (a laugh) and voices (whispering) are in the dialogue as directions for the actor.

use crate::capt::Doc;
use crate::cues::{Layout, cues};
use crate::labels;
use crate::sdh::speaker_name;
use crate::style::{self, Markup};

/// How a cue list is cut and timed.
#[derive(Debug, Clone)]
pub struct Options {
    /// Frames per second of the timeline (24, 25, 30…); drop-frame timecode is not written.
    pub fps: u32,
    /// The timeline's start, milliseconds: Resolve starts a timeline at 01:00:00:00.
    pub start: u64,
    /// A pause up to this many seconds stays inside a cue.
    pub max_gap: f64,
    /// A cue is at most this many seconds…
    pub max_duration: f64,
    /// …and this many characters.
    pub max_chars: usize,
    /// The language of sound and voice labels (`en`, `ru`).
    pub labels: String,
}

impl Default for Options {
    fn default() -> Self {
        Options { fps: 25, start: 3_600_000, max_gap: 1.5, max_duration: 8.0, max_chars: 200, labels: "en".into() }
    }
}

/// `HH:MM:SS:FF` of `ms` after the timeline's start; `up` rounds to the next frame (an Out point
/// never cuts the end of a word), else down.
fn timecode(ms: u64, o: &Options, up: bool) -> String {
    let fps = o.fps.max(1) as u64;
    let t = (o.start + ms) as u128 * fps as u128;
    let frames = (if up { t.div_ceil(1000) } else { t / 1000 }) as u64;
    let secs = frames / fps;
    format!("{:02}:{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60, frames % fps)
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn plain(s: &str) -> String {
    s.to_string()
}

/// The cue list for `doc`, speaker names as `layout` gives them, and the number of cues.
pub fn build(doc: &Doc, layout: &Layout, o: &Options) -> (String, usize) {
    let voice = |v: &str| labels::voice(v, &o.labels);
    let markup = Markup { italic: None, lang: None, voice: Some(&voice), escape: &plain };
    let cut = Layout {
        max_line: o.max_chars,
        max_lines: 1,
        max_duration: o.max_duration,
        gap: o.max_gap,
        min_duration: 0.0,
        by_speaker: true,
        speaker_labels: false,
        wrap_lines: false,
        ..layout.clone()
    };
    // (start, end, speaker, text): the speaker's cues and their own sounds, in order of start.
    let mut units: Vec<(u64, u64, Option<String>, String)> = cues(&doc.to_words(), &cut)
        .iter()
        .map(|c| {
            let lines = if style::marked(c) { style::lines(c, &markup) } else { None };
            (c.start, c.end, c.speaker.clone(), lines.unwrap_or_else(|| c.lines.clone()).join(" "))
        })
        .collect();
    for (start, end, speaker, code) in doc.sounds() {
        if let Some(sp) = speaker {
            units.push((start, end, Some(sp.to_string()), labels::sound(code, &o.labels)));
        }
    }
    units.sort_by_key(|u| u.0);

    // Joined per speaker while the pause is short and the cue stays within its limits.
    let mut rows: Vec<(u64, u64, Option<String>, String)> = Vec::new();
    let mut open: Vec<usize> = Vec::new(); // the last row of each speaker, by position in `rows`
    let (gap_ms, max_ms) = ((o.max_gap * 1000.0) as u64, (o.max_duration * 1000.0) as u64);
    for (start, end, speaker, text) in units {
        let last = open.iter().copied().rev().find(|&i| rows[i].2 == speaker);
        match last {
            Some(i)
                if start.saturating_sub(rows[i].1) <= gap_ms
                    && end.max(rows[i].1) - rows[i].0 <= max_ms
                    && rows[i].3.chars().count() + 1 + text.chars().count() <= o.max_chars
                    // Another speaker's cue in between ends this one.
                    && rows.iter().skip(i + 1).all(|r| r.2 == speaker) =>
            {
                rows[i].1 = rows[i].1.max(end);
                rows[i].3 = format!("{} {text}", rows[i].3);
            }
            _ => {
                open.push(rows.len());
                rows.push((start, end, speaker, text));
            }
        }
    }

    let mut out = String::from("\u{feff}");
    for (n, (start, end, speaker, text)) in rows.iter().enumerate() {
        let who = speaker.as_deref().map(|s| speaker_name(doc, layout, s)).unwrap_or_default();
        let row = [(n + 1).to_string(), timecode(*start, o, false), timecode(*end, o, true), who, text.clone()];
        out.push_str(&row.iter().map(|f| quote(f)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    (out, rows.len())
}

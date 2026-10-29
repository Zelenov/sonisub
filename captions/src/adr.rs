//! A CAPT file -> an ADR cue list for DaVinci Resolve's Fairlight page (ADR > List > Import Cue
//! List): one row per cue an actor records, `"Cue ID","In","Out","Character","Dialogue"`, quoted,
//! no header, UTF-8 with a byte order mark and CRLF line ends, timecodes `HH:MM:SS:FF`.
//!
//! A cue is a speaker's breath of speech: their cues as SRT cuts them, joined while the pause
//! between them is short and the cue stays short; another speaker's speech ends it (their sounds
//! do not). The speaker's own sounds (a laugh) and voices (whispering) are in the dialogue as
//! directions for the actor, where they happen; a voice is named once where it starts.

use crate::Word;
use crate::capt::Doc;
use crate::cues::{Layout, cues, speaker_label};
use crate::labels;
use crate::style::speaker_name;

/// A timeline's frame rate, as Resolve offers them. Timecode counts [`Rate::frames`] a second;
/// a fractional rate (23.976, 29.97, 59.94) plays 1000/1001 as fast, so the same timecode is a
/// little later in the media. Drop-frame timecode is not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rate {
    Fps23_976,
    Fps24,
    #[default]
    Fps25,
    Fps29_97,
    Fps30,
    Fps50,
    Fps59_94,
    Fps60,
}

impl Rate {
    /// Every rate.
    pub const ALL: [Rate; 8] = [
        Rate::Fps23_976,
        Rate::Fps24,
        Rate::Fps25,
        Rate::Fps29_97,
        Rate::Fps30,
        Rate::Fps50,
        Rate::Fps59_94,
        Rate::Fps60,
    ];

    /// The rate as it is written: `23.976`, `25`, `29.97`…
    pub fn name(self) -> &'static str {
        match self {
            Rate::Fps23_976 => "23.976",
            Rate::Fps24 => "24",
            Rate::Fps25 => "25",
            Rate::Fps29_97 => "29.97",
            Rate::Fps30 => "30",
            Rate::Fps50 => "50",
            Rate::Fps59_94 => "59.94",
            Rate::Fps60 => "60",
        }
    }

    /// A rate as [`Rate::name`] writes it (`23.98` too).
    pub fn parse(s: &str) -> Result<Rate, String> {
        let s = s.trim();
        let s = if s == "23.98" { "23.976" } else { s };
        Rate::ALL.into_iter().find(|r| r.name() == s).ok_or_else(|| {
            let names: Vec<&str> = Rate::ALL.iter().map(|r| r.name()).collect();
            format!("frame rate {s:?}: use one of {}", names.join(", "))
        })
    }

    /// Frames a second the timecode counts: 24, 25, 30, 50 or 60.
    pub fn frames(self) -> u32 {
        match self {
            Rate::Fps23_976 | Rate::Fps24 => 24,
            Rate::Fps25 => 25,
            Rate::Fps29_97 | Rate::Fps30 => 30,
            Rate::Fps50 => 50,
            Rate::Fps59_94 | Rate::Fps60 => 60,
        }
    }

    /// Whether it plays at [`Rate::frames`] × 1000/1001.
    pub fn fractional(self) -> bool {
        matches!(self, Rate::Fps23_976 | Rate::Fps29_97 | Rate::Fps59_94)
    }

    /// The frame `ms` into the media falls in; `up` takes the next frame unless `ms` starts one.
    fn frame(self, ms: u64, up: bool) -> u64 {
        let den = if self.fractional() { 1001 } else { 1000 };
        let t = ms as u128 * self.frames() as u128;
        (if up { t.div_ceil(den) } else { t / den }) as u64
    }
}

/// A non-drop-frame timecode, `HH:MM:SS:FF`, within a day as Resolve's are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timecode {
    /// 0 to 23.
    pub hours: u32,
    /// 0 to 59.
    pub minutes: u32,
    /// 0 to 59.
    pub seconds: u32,
    /// Below the rate's frames a second; checked against a rate by [`Timecode::frames_at`].
    pub frames: u32,
}

impl Timecode {
    /// `HH:MM:SS:FF`, two digits each. Drop-frame timecode (`;` before the frames) is refused: it
    /// counts differently and the cues would land late.
    pub fn parse(s: &str) -> Result<Timecode, String> {
        if s.contains(';') {
            return Err(format!(
                "timecode {s:?}: drop-frame timecode is not supported; set the timeline to non-drop frame and use HH:MM:SS:FF"
            ));
        }
        let bad = || format!("timecode {s:?}: expected HH:MM:SS:FF");
        let parts: Vec<&str> = s.trim().split(':').collect();
        let [h, m, sec, f] = parts[..] else { return Err(bad()) };
        let num = |p: &str| {
            (p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse::<u32>().ok())
                .flatten()
                .ok_or_else(bad)
        };
        let tc = Timecode { hours: num(h)?, minutes: num(m)?, seconds: num(sec)?, frames: num(f)? };
        if tc.hours >= 24 || tc.minutes >= 60 || tc.seconds >= 60 {
            return Err(bad());
        }
        Ok(tc)
    }

    /// The frame count at `rate`; an error when a field is out of its range (a timecode built by
    /// hand) or the frames do not exist at that rate.
    pub fn frames_at(self, rate: Rate) -> Result<u64, String> {
        let fps = rate.frames();
        if self.hours >= 24 || self.minutes >= 60 || self.seconds >= 60 {
            return Err(format!("timecode {self}: hours run 00 to 23, minutes and seconds 00 to 59"));
        }
        if self.frames >= fps {
            return Err(format!("timecode {self}: frames run 00 to {:02} at {fps} frames a second", fps - 1));
        }
        Ok((self.hours as u64 * 3600 + self.minutes as u64 * 60 + self.seconds as u64) * fps as u64
            + self.frames as u64)
    }

    /// The timecode of frame `n` at `rate`; `None` past the day's last frame.
    fn from_frames(n: u64, rate: Rate) -> Option<Timecode> {
        let fps = rate.frames() as u64;
        let secs = n / fps;
        (secs < 24 * 3600).then(|| Timecode {
            hours: (secs / 3600) as u32,
            minutes: (secs / 60 % 60) as u32,
            seconds: (secs % 60) as u32,
            frames: (n % fps) as u32,
        })
    }
}

impl std::fmt::Display for Rate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::fmt::Display for Timecode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}:{:02}:{:02}", self.hours, self.minutes, self.seconds, self.frames)
    }
}

/// How a cue list is cut and timed.
#[derive(Debug, Clone)]
pub struct Options {
    /// The timeline's frame rate.
    pub rate: Rate,
    /// The timeline's start: Resolve starts a timeline at 01:00:00:00, whatever the rate. A start
    /// whose frames the rate does not have is an error of [`build`].
    pub start: Timecode,
    /// A pause up to this many seconds stays inside a cue.
    pub max_gap: f64,
    /// A cue is at most this many seconds…
    pub max_duration: f64,
    /// …and this many characters.
    pub max_chars: usize,
}

impl Default for Options {
    fn default() -> Self {
        let start = Timecode { hours: 1, minutes: 0, seconds: 0, frames: 0 };
        Options { rate: Rate::default(), start, max_gap: 1.5, max_duration: 8.0, max_chars: 200 }
    }
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// What is heard in a row: a word, or a sound of its speaker (and when it ends).
enum Heard<'a> {
    Word(&'a Word),
    Sound(&'a str, u64),
}

impl Heard<'_> {
    fn end(&self) -> u64 {
        match self {
            Heard::Word(w) => w.end,
            Heard::Sound(_, end) => *end,
        }
    }
}

/// A row of the list, or a piece to join into one: a speaker's cue with their sounds inside it in
/// time, or a sound of theirs on its own.
struct Piece<'a> {
    start: u64,
    end: u64,
    speaker: Option<&'a str>,
    heard: Vec<(u64, Heard<'a>)>,
    /// When it has speech, from its first word to its last: only another speaker's speech in
    /// between ends a row.
    spoken: Vec<(u64, u64)>,
}

impl<'a> Piece<'a> {
    /// A piece of what was heard, in order: from the first to the last thing's end.
    fn of(speaker: Option<&'a str>, heard: Vec<(u64, Heard<'a>)>) -> Piece<'a> {
        let start = heard.first().map_or(0, |(t, _)| *t);
        let end = heard.iter().map(|(_, h)| h.end()).max().unwrap_or(start);
        let words: Vec<&Word> =
            heard.iter().filter_map(|(_, h)| if let Heard::Word(w) = h { Some(*w) } else { None }).collect();
        let spoken = match (words.first(), words.last()) {
            (Some(a), Some(b)) => vec![(a.start, b.end)],
            _ => Vec::new(),
        };
        Piece { start, end, speaker, heard, spoken }
    }

    /// This piece as pieces of at most `max` characters of dialogue, cut between what is heard.
    fn split(self, max: usize, lang: &str) -> Vec<Piece<'a>> {
        if dialogue(&self.heard, lang).chars().count() <= max {
            return vec![self];
        }
        let speaker = self.speaker;
        let mut out = Vec::new();
        let mut part: Vec<(u64, Heard<'a>)> = Vec::new();
        for item in self.heard {
            part.push(item);
            if part.len() > 1 && dialogue(&part, lang).chars().count() > max {
                let last = part.pop().expect("just pushed");
                out.push(Piece::of(speaker, std::mem::take(&mut part)));
                part.push(last);
            }
        }
        if !part.is_empty() {
            out.push(Piece::of(speaker, part));
        }
        out
    }
}

/// The dialogue of a row: words, a sound's label where it is heard, a voice's where it starts.
fn dialogue(heard: &[(u64, Heard)], lang: &str) -> String {
    let mut text = String::new();
    let mut said: Option<&str> = None;
    for (_, h) in heard {
        match h {
            Heard::Word(w) => {
                if !text.is_empty() && !w.glued {
                    text.push(' ');
                }
                let v = w.marks.voice.as_deref();
                if !w.glued && v != said {
                    if let Some(v) = v {
                        text.push_str(&labels::voice(v, lang));
                        text.push(' ');
                    }
                    said = v;
                }
                text.push_str(&w.text);
            }
            Heard::Sound(code, _) => {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&labels::sound(code, lang));
            }
        }
    }
    text
}

/// A row of the cue list, as [`build`] writes it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Row {
    /// `Cue ID`, from 1.
    pub id: usize,
    /// When the cue is heard, milliseconds from the start of the media.
    pub start: u64,
    pub end: u64,
    /// `In` and `Out` on the timeline: the frames the cue starts and ends in, at least a frame long.
    pub tc_in: Timecode,
    pub tc_out: Timecode,
    /// `Character`: the speaker's name.
    pub character: String,
    /// `Dialogue`: the words with the speaker's sounds and voices as directions.
    pub dialogue: String,
}

/// The cue list for `doc`, speaker names as `layout` gives them, sound and voice labels in
/// `lang` (`en`, `ru`), and the number of cues. An error for a start the rate does not have, or a
/// list that would run past 23:59:59 of the timeline.
pub fn build(doc: &Doc, layout: &Layout, o: &Options, lang: &str) -> Result<(String, usize), String> {
    let rows = rows(doc, layout, o, lang)?;
    let mut out = String::from("\u{feff}");
    for r in &rows {
        let row =
            [r.id.to_string(), r.tc_in.to_string(), r.tc_out.to_string(), r.character.clone(), r.dialogue.clone()];
        out.push_str(&row.iter().map(|f| quote(f)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    Ok((out, rows.len()))
}

/// The rows of the cue list [`build`] writes, with the moments they are heard; the same errors.
pub fn rows(doc: &Doc, layout: &Layout, o: &Options, lang: &str) -> Result<Vec<Row>, String> {
    let start = o.start.frames_at(o.rate)?;
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
    let max_ms = (o.max_duration * 1000.0) as u64;
    let cues = cues(&doc.to_words(), &cut);
    // The speaker's own sounds go into the cue whose words they are heard among; a cue goes on as
    // long as such a sound, up to its limit and the speaker's next cue. Their other sounds alone.
    let mut heard: Vec<Vec<(u64, Heard)>> =
        cues.iter().map(|c| c.words.iter().map(|w| (w.start, Heard::Word(w))).collect()).collect();
    let mut alone: Vec<Piece> = Vec::new();
    for b in doc.sounds() {
        let (Some(sp), Some(code)) = (b.speaker.as_deref(), b.sound.as_deref()) else { continue };
        let inside =
            cues.iter().position(|c| c.speaker.as_deref() == Some(sp) && c.start <= b.start && b.start < c.end);
        match inside {
            Some(i) => {
                let next = cues[i + 1..].iter().find(|c| c.speaker == cues[i].speaker).map_or(u64::MAX, |c| c.start);
                let end = b.end.min(cues[i].start + max_ms).min(next).max(cues[i].end);
                heard[i].push((b.start, Heard::Sound(code, end)));
            }
            None => alone.push(Piece::of(Some(sp), vec![(b.start, Heard::Sound(code, b.end))])),
        }
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for (c, mut h) in cues.iter().zip(heard) {
        // A sound after the word it is heard during.
        h.sort_by_key(|(t, h)| (*t, matches!(h, Heard::Sound(..))));
        // A row is at most so many characters as written, labels and all.
        pieces.extend(Piece::of(c.speaker.as_deref(), h).split(o.max_chars, lang));
    }
    pieces.extend(alone);
    pieces.sort_by_key(|p| p.start);

    // Joined while the speaker goes on, the pause is short and the cue stays within its limits.
    let mut rows: Vec<Piece> = Vec::new();
    let gap_ms = (o.max_gap * 1000.0) as u64;
    for piece in pieces {
        // The speaker's last row, if no one else has spoken since it began, in time.
        let last = rows.iter().rposition(|r| r.speaker == piece.speaker);
        let open = last.filter(|&i| {
            let (from, to) = (rows[i].start, piece.start);
            rows.iter()
                .filter(|r| r.speaker != piece.speaker)
                .flat_map(|r| &r.spoken)
                .all(|&(s, e)| e <= from || s >= to)
        });
        if let Some(i) = open
            && piece.start.saturating_sub(rows[i].end) <= gap_ms
            && piece.end.max(rows[i].end) - rows[i].start <= max_ms
            && dialogue(&rows[i].heard, lang).chars().count() + 1 + dialogue(&piece.heard, lang).chars().count()
                <= o.max_chars
        {
            let row = &mut rows[i];
            row.end = row.end.max(piece.end);
            row.spoken.extend(piece.spoken);
            row.heard.extend(piece.heard);
            continue;
        }
        rows.push(piece);
    }

    let tc = |frame: u64| {
        Timecode::from_frames(start + frame, o.rate).ok_or_else(|| {
            format!("the cue list runs past 23:59:59 of the timeline; start it earlier than {}", o.start)
        })
    };
    let mut out = Vec::with_capacity(rows.len());
    for (n, r) in rows.iter().enumerate() {
        // A transcript without speakers still names the one voice, as the subtitles would; a
        // file with named speakers gives speech of no one a name of its own.
        let who = match r.speaker {
            Some(s) => speaker_name(doc, layout, s),
            None if doc.speakers.is_empty() => speaker_label("1", &layout.speaker_names),
            None => labels::unknown_speaker(lang),
        };
        // A cue is at least a frame long, so Resolve shows it.
        let from = o.rate.frame(r.start, false);
        let to = o.rate.frame(r.end, true).max(from + 1);
        out.push(Row {
            id: n + 1,
            start: r.start,
            end: r.end,
            tc_in: tc(from)?,
            tc_out: tc(to)?,
            character: who,
            dialogue: dialogue(&r.heard, lang),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timecode(ms: u64, o: &Options, up: bool) -> String {
        Timecode::from_frames(o.start.frames_at(o.rate).unwrap() + o.rate.frame(ms, up), o.rate).unwrap().to_string()
    }

    #[test]
    fn rates() {
        assert_eq!(Rate::parse("29.97"), Ok(Rate::Fps29_97));
        assert_eq!(Rate::parse("23.98"), Ok(Rate::Fps23_976));
        assert_eq!((Rate::Fps29_97.frames(), Rate::Fps29_97.fractional()), (30, true));
        assert_eq!(Rate::parse("25"), Ok(Rate::Fps25));
        assert!(Rate::parse("29").is_err());
        assert!(Rate::parse("0").is_err());
    }

    #[test]
    fn timecodes() {
        let r25 = Rate::default();
        let at = |s: &str, rate| Timecode::parse(s).and_then(|t| t.frames_at(rate));
        assert_eq!(at("01:00:00:00", r25), Ok(90_000));
        assert_eq!(at("00:00:01:24", r25), Ok(49));
        assert!(at("00:00:00:25", r25).unwrap_err().contains("frames run 00 to 24"));
        assert!(Timecode::parse("00:61:00:00").is_err());
        assert!(Timecode::parse("24:00:00:00").is_err());
        assert!(Timecode::parse("+1:00:00:00").is_err());
        assert!(Timecode::parse("1:00:00").is_err());
        assert!(Timecode::parse("01:00:00;00").unwrap_err().contains("drop-frame"));
        // The default start is 01:00:00:00 at any rate.
        let o = Options { rate: Rate::Fps24, ..Options::default() };
        assert_eq!(timecode(0, &o, false), "01:00:00:00");
    }

    #[test]
    fn a_fractional_rate_counts_frames_later() {
        let zero = Timecode { hours: 0, minutes: 0, seconds: 0, frames: 0 };
        let o = Options { rate: Rate::Fps29_97, start: zero, ..Options::default() };
        // 1001 ms is exactly 30 frames at 29.97; at 30 it would be 30.03.
        assert_eq!(timecode(1001, &o, false), "00:00:01:00");
        assert_eq!(timecode(1000, &o, false), "00:00:00:29");
        assert_eq!(timecode(1000, &o, true), "00:00:01:00");
    }
}

//! A CAPT file -> an ADR cue list for DaVinci Resolve's Fairlight page (ADR > List > Import Cue
//! List): one row per cue an actor records, `"Cue ID","In","Out","Character","Dialogue"`, quoted,
//! no header, UTF-8 with a byte order mark and CRLF line ends, timecodes `HH:MM:SS:FF`.
//!
//! A cue is a speaker's breath of speech: their cues as SRT cuts them, joined while the pause
//! between them is short and the cue stays short, never across another speaker. The speaker's
//! own sounds (a laugh) and voices (whispering) are in the dialogue as directions for the actor,
//! a voice once where it starts.

use crate::capt::Doc;
use crate::cues::{Layout, cues};
use crate::labels;
use crate::style::{self, Markup, speaker_name};

/// A timeline's frame rate. Timecode counts `frames` a second; a fractional rate (23.976, 29.97,
/// 59.94) plays 1000/1001 as fast, so the same timecode is a little later in the media.
/// Drop-frame timecode is not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    /// Frames a second the timecode counts: 24, 25, 30, 50, 60.
    pub frames: u32,
    /// Plays at `frames` × 1000/1001.
    pub fractional: bool,
}

impl Rate {
    /// The rates Resolve offers for a timeline, as they are written.
    pub const NAMES: [&str; 8] = ["23.976", "24", "25", "29.97", "30", "50", "59.94", "60"];

    /// `23.976`, `24`, `25`, `29.97`, `30`, `50`, `59.94` or `60`.
    pub fn parse(s: &str) -> Result<Rate, String> {
        let (frames, fractional) = match s.trim() {
            "23.976" | "23.98" => (24, true),
            "29.97" => (30, true),
            "59.94" => (60, true),
            other => match other.parse::<u32>() {
                Ok(n @ (24 | 25 | 30 | 50 | 60)) => (n, false),
                _ => return Err(format!("frame rate {s:?}: use one of {}", Rate::NAMES.join(", "))),
            },
        };
        Ok(Rate { frames, fractional })
    }

    /// The frame `ms` into the media falls in; `up` takes the next frame unless `ms` starts one.
    fn frame(self, ms: u64, up: bool) -> u64 {
        let den = if self.fractional { 1001 } else { 1000 };
        let t = ms as u128 * self.frames as u128;
        (if up { t.div_ceil(den) } else { t / den }) as u64
    }
}

impl Default for Rate {
    fn default() -> Self {
        Rate { frames: 25, fractional: false }
    }
}

/// `HH:MM:SS:FF` (or `;` before the frames) as a frame count at `rate`.
pub fn parse_timecode(s: &str, rate: Rate) -> Result<u64, String> {
    let bad = || format!("timecode {s:?}: expected HH:MM:SS:FF");
    let parts: Vec<&str> = s.trim().split([':', ';']).collect();
    let [h, m, sec, f] = parts[..] else { return Err(bad()) };
    let num = |p: &str| p.parse::<u64>().ok().filter(|_| p.len() <= 2).ok_or_else(bad);
    let (h, m, sec, f) = (num(h)?, num(m)?, num(sec)?, num(f)?);
    if m >= 60 || sec >= 60 {
        return Err(bad());
    }
    if f >= rate.frames as u64 {
        return Err(format!("timecode {s:?}: frame {f} at {} frames a second", rate.frames));
    }
    Ok((h * 3600 + m * 60 + sec) * rate.frames as u64 + f)
}

/// How a cue list is cut and timed.
#[derive(Debug, Clone)]
pub struct Options {
    /// The timeline's frame rate.
    pub rate: Rate,
    /// The timeline's start in frames: Resolve starts a timeline at 01:00:00:00.
    pub start: u64,
    /// A pause up to this many seconds stays inside a cue.
    pub max_gap: f64,
    /// A cue is at most this many seconds…
    pub max_duration: f64,
    /// …and this many characters.
    pub max_chars: usize,
}

impl Default for Options {
    fn default() -> Self {
        let rate = Rate::default();
        Options { rate, start: 3600 * rate.frames as u64, max_gap: 1.5, max_duration: 8.0, max_chars: 200 }
    }
}

/// `HH:MM:SS:FF` of `ms` into the media; `up` rounds to the next frame (an Out point never cuts
/// the end of a word), else down.
fn timecode(ms: u64, o: &Options, up: bool) -> String {
    let fps = o.rate.frames as u64;
    let frames = o.start + o.rate.frame(ms, up);
    let secs = frames / fps;
    format!("{:02}:{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60, frames % fps)
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// A row of the list, or a piece to join into one.
struct Row {
    start: u64,
    end: u64,
    speaker: Option<String>,
    text: String,
    /// The voice of the first and the last word; `None` for a sound.
    voices: Option<(Option<String>, Option<String>)>,
}

/// The cue list for `doc`, speaker names as `layout` gives them, sound and voice labels in
/// `lang` (`en`, `ru`), and the number of cues.
pub fn build(doc: &Doc, layout: &Layout, o: &Options, lang: &str) -> (String, usize) {
    let voice = |v: &str| labels::voice(v, lang);
    let markup = Markup { italic: None, lang: None, voice: Some(&voice), escape: &style::plain };
    let cut = Layout {
        max_line: o.max_chars,
        max_lines: 1,
        max_duration: o.max_duration,
        gap: o.max_gap,
        min_duration: 0.0,
        by_speaker: true,
        speaker_labels: false,
        wrap_lines: false,
        readable: false,
        ..layout.clone()
    };
    // The speaker's cues and their own sounds, in order of start.
    let mut units: Vec<Row> = cues(&doc.to_words(), &cut)
        .iter()
        .map(|c| Row {
            start: c.start,
            end: c.end,
            speaker: c.speaker.clone(),
            text: style::lines(c, &markup).join(" "),
            voices: Some((
                c.words.first().and_then(|w| w.marks.voice.clone()),
                c.words.last().and_then(|w| w.marks.voice.clone()),
            )),
        })
        .collect();
    for (start, end, speaker, code) in doc.sounds() {
        if let Some(sp) = speaker {
            let text = labels::sound(code, lang);
            units.push(Row { start, end, speaker: Some(sp.to_string()), text, voices: None });
        }
    }
    units.sort_by_key(|u| u.start);

    // Joined while the speaker goes on, the pause is short and the cue stays within its limits.
    let mut rows: Vec<Row> = Vec::new();
    // The voice the last row ends in, across the sounds inside it.
    let mut said: Option<String> = None;
    let (gap_ms, max_ms) = ((o.max_gap * 1000.0) as u64, (o.max_duration * 1000.0) as u64);
    for mut unit in units {
        let speech = unit.voices.is_some();
        let (opens, closes) = unit.voices.take().unwrap_or((None, None));
        if let Some(row) = rows.last_mut()
            && row.speaker == unit.speaker
            && unit.start.saturating_sub(row.end) <= gap_ms
            && unit.end.max(row.end) - row.start <= max_ms
            && row.text.chars().count() + 1 + unit.text.chars().count() <= o.max_chars
        {
            // A voice that goes on from the row is not named again.
            if let Some(v) = opens.as_deref().filter(|v| said.as_deref() == Some(*v))
                && let Some(rest) = unit.text.strip_prefix(&format!("{} ", voice(v)))
            {
                unit.text = rest.to_string();
            }
            row.end = row.end.max(unit.end);
            row.text = format!("{} {}", row.text, unit.text);
        } else {
            said = None;
            rows.push(unit);
        }
        // A sound leaves the voice as it was.
        if speech {
            said = closes;
        }
    }

    let mut out = String::from("\u{feff}");
    for (n, r) in rows.iter().enumerate() {
        let who = r.speaker.as_deref().map(|s| speaker_name(doc, layout, s)).unwrap_or_default();
        let row = [(n + 1).to_string(), timecode(r.start, o, false), timecode(r.end, o, true), who, r.text.clone()];
        out.push_str(&row.iter().map(|f| quote(f)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    (out, rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates() {
        assert_eq!(Rate::parse("29.97"), Ok(Rate { frames: 30, fractional: true }));
        assert_eq!(Rate::parse("25"), Ok(Rate { frames: 25, fractional: false }));
        assert!(Rate::parse("29").is_err());
        assert!(Rate::parse("0").is_err());
    }

    #[test]
    fn timecodes() {
        let r25 = Rate::default();
        assert_eq!(parse_timecode("01:00:00:00", r25), Ok(90_000));
        assert_eq!(parse_timecode("00:00:01;24", r25), Ok(49));
        assert!(parse_timecode("00:00:00:25", r25).is_err());
        assert!(parse_timecode("00:61:00:00", r25).is_err());
        assert!(parse_timecode("1:00:00", r25).is_err());
    }

    #[test]
    fn a_fractional_rate_counts_frames_later() {
        let o = Options { rate: Rate::parse("29.97").unwrap(), start: 0, ..Options::default() };
        // 1001 ms is exactly 30 frames at 29.97; at 30 it would be 30.03.
        assert_eq!(timecode(1001, &o, false), "00:00:01:00");
        assert_eq!(timecode(1000, &o, false), "00:00:00:29");
        assert_eq!(timecode(1000, &o, true), "00:00:01:00");
    }
}

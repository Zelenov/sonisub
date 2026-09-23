//! Soniox tokens -> subtitle cues -> SRT text.

use serde_json::Value;

/// Subtitle layout rules.
#[derive(Debug, Clone)]
pub struct Layout {
    /// Max characters per line.
    pub max_line: usize,
    /// Max lines per cue.
    pub max_lines: usize,
    /// Max cue duration, seconds.
    pub max_duration: f64,
    /// A pause longer than this (seconds) starts a new cue.
    pub gap: f64,
    /// Minimum cue duration, seconds (extended into following silence when possible).
    pub min_duration: f64,
    /// A speaker change starts a new cue.
    pub by_speaker: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self { max_line: 42, max_lines: 2, max_duration: 6.0, gap: 0.7, min_duration: 0.5, by_speaker: true }
    }
}

#[derive(Debug, Clone)]
struct Word {
    text: String,
    start: u64,
    end: u64,
    speaker: Option<String>,
}

type Cue = Vec<Word>;

/// Joins sub-word tokens (Soniox marks a new word with a leading space, sometimes as a separate " " token) into words.
fn words(transcript: &Value) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let mut space = false;
    for t in transcript["tokens"].as_array().into_iter().flatten() {
        if t["is_audio_event"].as_bool() == Some(true) || t["translation_status"].as_str() == Some("translation") {
            continue;
        }
        let text = t["text"].as_str().unwrap_or_default();
        if text.trim().is_empty() {
            space = true;
            continue;
        }
        let (start, end) = (t["start_ms"].as_u64().unwrap_or(0), t["end_ms"].as_u64().unwrap_or(0));
        let speaker = t["speaker"].as_str().map(str::to_string);
        let new_word = space || text.starts_with(char::is_whitespace);
        space = text.ends_with(char::is_whitespace);
        match out.last_mut() {
            Some(w) if !new_word && w.speaker == speaker => {
                w.text.push_str(text.trim_end());
                w.end = end;
            }
            _ => out.push(Word { text: text.trim().to_string(), start, end, speaker }),
        }
    }
    out
}

/// A real sentence end; "..." is a hesitation, not an end.
fn ends_sentence(w: &str) -> bool {
    let w = w.trim_end_matches(['»', '"', ')', '\'']);
    w.ends_with(['.', '?', '!']) && !w.ends_with("...") || w.ends_with("?..") || w.ends_with("!..")
}

fn text(c: &[Word]) -> String {
    c.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
}

fn segment(words: Vec<Word>, s: &Layout) -> Vec<Cue> {
    let by_speaker = s.by_speaker;
    let (max_ms, gap_ms) = ((s.max_duration * 1000.0) as u64, (s.gap * 1000.0) as u64);
    let mut cues: Vec<Cue> = Vec::new();
    let mut cur: Cue = Vec::new();
    for w in words {
        if let Some(prev) = cur.last() {
            if (by_speaker && w.speaker != prev.speaker)
                || w.start.saturating_sub(prev.end) > gap_ms
                || ends_sentence(&prev.text)
            {
                cues.push(std::mem::take(&mut cur));
            } else if !fits(&format!("{} {}", text(&cur), w.text), s) || w.end.saturating_sub(cur[0].start) > max_ms {
                // Full: cut after the last comma-like pause in the back two thirds, else right here.
                let cut = (cur.len() / 3..cur.len())
                    .rev()
                    .find(|&i| i > 0 && cur[i - 1].text.ends_with([',', ';', ':', '—', '–']))
                    .unwrap_or(cur.len());
                let rest = cur.split_off(cut);
                cues.push(std::mem::replace(&mut cur, rest));
            }
        }
        cur.push(w);
    }
    if !cur.is_empty() {
        cues.push(cur);
    }

    // A cue that starts with the last 1-2 words of the previous cue's sentence gets them moved back.
    let mut out: Vec<Cue> = Vec::new();
    for mut c in cues {
        if let Some(prev) = out.last_mut() {
            let k = c.iter().take(2).position(|w| ends_sentence(&w.text)).map(|i| i + 1);
            let last = prev.last().expect("cues are non-empty");
            if let Some(k) = k
                && !ends_sentence(&last.text)
                && (!by_speaker || last.speaker == c[0].speaker)
                && fits(&text(&[prev.as_slice(), &c[..k]].concat()), s)
            {
                prev.extend(c.drain(..k));
            }
        }
        if !c.is_empty() {
            out.push(c);
        }
    }
    out
}

/// Splits text into at most `lines` lines, using the narrowest width that fits so lines come out balanced.
fn wrap(t: &str, max_line: usize, lines: usize) -> String {
    let n = t.chars().count();
    if n <= max_line || lines <= 1 {
        return t.to_string();
    }
    let words: Vec<&str> = t.split(' ').collect();
    let fill = |width: usize| {
        let mut out: Vec<String> = vec![String::new()];
        for w in &words {
            let cur = out.last_mut().expect("never empty");
            if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > width {
                out.push(w.to_string());
            } else {
                if !cur.is_empty() {
                    cur.push(' ');
                }
                cur.push_str(w);
            }
        }
        out
    };
    let parts = lines.min(n.div_ceil(max_line));
    (n / parts..=n)
        .map(fill)
        .find(|l| l.len() <= parts)
        .unwrap_or_else(|| vec![t.to_string()])
        .join("
")
}

/// Whether the text wraps into the allowed number of lines without overflowing any of them.
fn fits(t: &str, s: &Layout) -> bool {
    let w = wrap(t, s.max_line, s.max_lines);
    w.lines().count() <= s.max_lines.max(1) && w.lines().all(|l| l.chars().count() <= s.max_line)
}

fn timestamp(ms: u64) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// Builds SRT text from a Soniox transcript; returns the text and the number of cues.
pub fn build(transcript: &Value, s: &Layout) -> (String, usize) {
    let cues = segment(words(transcript), s);
    let min_ms = (s.min_duration * 1000.0) as u64;
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        let start = c[0].start;
        let mut end = c.last().expect("cues are non-empty").end;
        // Too-short cues borrow time from the silence after them, never overlapping the next cue.
        if end < start + min_ms {
            let next = cues.get(i + 1).map_or(u64::MAX, |n| n[0].start);
            end = (start + min_ms).min(next.saturating_sub(1)).max(end);
        }
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            timestamp(start),
            timestamp(end),
            wrap(&text(c), s.max_line, s.max_lines)
        ));
    }
    (out, cues.len())
}

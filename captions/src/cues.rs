//! Words -> subtitle cues, the same for every subtitle format.
//!
//! A cue never mixes speakers and never runs past the end of a sentence. A sentence that doesn't fit
//! the layout is cut at punctuation (`, ; : — ...`) first, at pauses next, and between arbitrary words
//! only when nothing else fits. Lines inside a cue are broken by the same preference.

use std::ops::Range;

use crate::Word;

/// Subtitle layout rules.
#[derive(Debug, Clone)]
pub struct Layout {
    /// Max characters per line; 0 = no limit (one line per cue, cues are whole sentences).
    pub max_line: usize,
    /// Max lines per cue.
    pub max_lines: usize,
    /// Max cue duration, seconds; 0 = no limit.
    pub max_duration: f64,
    /// Silence longer than this (seconds) always ends a cue; 0 = never.
    pub gap: f64,
    /// Minimum cue duration, seconds (extended into following silence when possible).
    pub min_duration: f64,
    /// A speaker change ends a cue.
    pub by_speaker: bool,
    /// Prefix "Speaker 1: " when the speaker changes (only if the transcript has several speakers).
    pub speaker_labels: bool,
    /// Names for speakers 1, 2, ... used instead of "Speaker N".
    pub speaker_names: Vec<String>,
    /// Break a cue's text into up to `max_lines` lines of `max_line` chars. Off: one line per cue,
    /// up to `max_line × max_lines` chars.
    pub wrap_lines: bool,
    /// The video's frame, width and height in pixels. Writers that place text (ASS, WebVTT, TTML)
    /// use it; a vertical frame keeps text clear of a phone app's buttons ([`Layout::vertical`]).
    pub frame: (u32, u32),
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            max_line: 50,
            max_lines: 2,
            max_duration: 8.0,
            gap: 2.0,
            min_duration: 0.5,
            by_speaker: true,
            speaker_labels: false,
            speaker_names: Vec::new(),
            wrap_lines: false,
            frame: (1920, 1080),
        }
    }
}

impl Layout {
    /// No length or duration limit: every cue is one sentence of one speaker.
    pub fn unlimited() -> Self {
        Self { max_line: 0, max_duration: 0.0, ..Self::default() }
    }

    /// For a vertical video (Reels, Shorts, TikTok, 1080×1920): short lines broken in two, short
    /// cues, so the text stays narrow and quick to read on a phone.
    pub fn vertical() -> Self {
        Self { max_line: 22, max_lines: 2, max_duration: 4.0, wrap_lines: true, frame: (1080, 1920), ..Self::default() }
    }

    /// Whether the frame is taller than it is wide.
    pub fn is_vertical(&self) -> bool {
        self.frame.1 > self.frame.0
    }
}

const DASHES: [char; 2] = ['—', '–'];

/// Splits "finally—and" into "finally—" + glued "and", so a cue or a line may end at the dash.
/// Timing is shared out by length.
fn split_dashes(w: Word) -> Vec<Word> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = w.text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        cur.push(*c);
        if DASHES.contains(c) && i + 1 < chars.len() && !DASHES.contains(&chars[i + 1]) {
            parts.push(std::mem::take(&mut cur));
        }
    }
    parts.push(cur);
    if parts.len() == 1 {
        return vec![w];
    }
    let total = chars.len().max(1) as u64;
    let span = w.end.saturating_sub(w.start);
    let mut done = 0u64;
    let n = parts.len();
    parts
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let start = w.start + span * done / total;
            done += text.chars().count() as u64;
            let end = w.start + span * done / total;
            // Only the last part keeps what follows the word: its cut point and sentence end.
            let mut marks = w.marks.clone();
            if i + 1 < n {
                marks.cut = None;
                marks.sentence = None;
            }
            Word { text, start, end, glued: i > 0 || w.glued, marks, ..w.clone() }
        })
        .collect()
}

fn strip_closing(w: &str) -> &str {
    w.trim_end_matches(['»', '"', '”', ')', '\''])
}

/// Words whose final dot is not a sentence end.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "st", "sr", "jr", "vs", "approx", "dept", "fig", "vol", "т", "г", "гг", "др",
    "проф", "ул", "им", "напр", "см", "стр",
];

fn is_abbreviation(w: &str) -> bool {
    let core = w.trim_end_matches('.');
    let lower = core.to_lowercase();
    ABBREVIATIONS.contains(&lower.as_str())
        // Initials ("J.") and dotted forms ("e.g.", "U.S.", "т.е.").
        || (core.chars().count() == 1 && core.chars().all(char::is_uppercase))
        || (core.contains('.') && core.chars().filter(|c| c.is_alphabetic()).count() <= 4)
}

/// A real sentence end; "..." is a hesitation, "Dr." an abbreviation, not an end.
fn ends_sentence(w: &str) -> bool {
    let w = strip_closing(w);
    if w.ends_with("?..") || w.ends_with("!..") {
        return true;
    }
    w.ends_with(['?', '!']) || w.ends_with('.') && !w.ends_with("...") && !is_abbreviation(w)
}

/// A place inside a sentence where it can be cut cleanly.
fn ends_clause(w: &str) -> bool {
    let w = strip_closing(w);
    w.ends_with([',', ';', ':', '—', '–', '…']) || w.ends_with("...")
}

/// How clean a cut between two words is: 0 at punctuation, 1 at a pause, 2 anywhere else.
/// A CAPT file's cut points decide where a cut is clean: a cut point is as clean as its punctuation
/// or pause (at least a pause), and no cut point means "only if nothing else fits".
pub(crate) fn cut_level(prev: &Word, next: &Word) -> usize {
    const PAUSE_MS: u64 = 400;
    let natural = if ends_clause(&prev.text) || next.text.starts_with(['—', '–', '(', '«', '“', '"']) {
        0
    } else if next.start.saturating_sub(prev.end) >= PAUSE_MS {
        1
    } else {
        2
    };
    match prev.marks.cut {
        Some(true) => natural.min(1),
        Some(false) => 2,
        None => natural,
    }
}

/// A sentence ends after `w`: its punctuation, unless a CAPT file says otherwise.
fn ends(w: &Word) -> bool {
    w.marks.sentence.unwrap_or_else(|| ends_sentence(&w.text))
}

const CUE_COST: f64 = 100.0;

fn join(ws: &[Word]) -> String {
    let mut out = String::new();
    for (i, w) in ws.iter().enumerate() {
        if i > 0 && !w.glued {
            out.push(' ');
        }
        out.push_str(&w.text);
    }
    out
}

/// Text pieces a line may break between: words, and parts of words after a dash (`glued`).
pub(crate) fn pieces(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    for word in text.split(' ') {
        let mut rest = word;
        let mut glued = false;
        while let Some((i, c)) = rest.char_indices().find(|&(i, c)| {
            DASHES.contains(&c) && rest[i + c.len_utf8()..].chars().next().is_some_and(|n| !DASHES.contains(&n))
        }) {
            let cut = i + c.len_utf8();
            out.push((&rest[..cut], glued));
            rest = &rest[cut..];
            glued = true;
        }
        out.push((rest, glued));
    }
    out
}

/// Cuts at speaker changes, long silences and sentence ends. A sentence interrupted by a long pause
/// keeps its short tail: "…ближе всего <pause> описывается." stays one sentence.
pub(crate) fn sentences(words: Vec<Word>, s: &Layout) -> Vec<Vec<Word>> {
    let gap_ms = (s.gap * 1000.0) as u64;
    let mut blocks: Vec<Vec<Word>> = Vec::new();
    let mut cur: Vec<Word> = Vec::new();
    for w in words {
        if let Some(prev) = cur.last() {
            let hard = (s.by_speaker && w.speaker != prev.speaker)
                || (gap_ms > 0 && w.start.saturating_sub(prev.end) > gap_ms)
                || ends(prev);
            if hard {
                blocks.push(std::mem::take(&mut cur));
            }
        }
        cur.push(w);
    }
    if !cur.is_empty() {
        blocks.push(cur);
    }

    let mut out: Vec<Vec<Word>> = Vec::new();
    for mut b in blocks {
        if let Some(prev) = out.last_mut() {
            let last = prev.last().expect("blocks are non-empty");
            let tail = b.iter().take(2).position(ends).map(|i| i + 1);
            if let Some(k) = tail
                && !ends(last)
                && last.speaker == b[0].speaker
            {
                prev.extend(b.drain(..k));
            }
        }
        if !b.is_empty() {
            out.push(b);
        }
    }
    out
}

/// Splits `text` into at most `max_lines` lines of at most `max_line` chars, balanced, preferring breaks
/// after punctuation. `None` if it can't fit. `max_line == 0` means one line, any length.
fn wrap(text: &str, max_line: usize, max_lines: usize) -> Option<Vec<String>> {
    let total = text.chars().count();
    if max_line == 0 || total <= max_line {
        return Some(vec![text.to_string()]);
    }
    let words = pieces(text);
    let lens: Vec<usize> = words.iter().map(|w| w.0.chars().count()).collect();
    let line_len =
        |i: usize, j: usize| lens[i..j].iter().sum::<usize>() + words[i + 1..j].iter().filter(|w| !w.1).count();
    let n = words.len();
    // A line break away from punctuation costs as much as ~40% of a line of imbalance.
    let break_penalty = (max_line as f64 * 0.4).powi(2);

    for k in 2..=max_lines {
        let target = total as f64 / k as f64;
        // best[l][j]: cheapest way to put the first j words on l lines.
        let mut best = vec![vec![None::<(f64, usize)>; n + 1]; k + 1];
        best[0][0] = Some((0.0, 0));
        for l in 1..=k {
            for j in 1..=n {
                for i in (0..j).rev() {
                    let len = line_len(i, j);
                    if len > max_line {
                        break;
                    }
                    let Some((c, _)) = best[l - 1][i] else { continue };
                    let brk = if j < n && !ends_clause(words[j - 1].0) && !ends_sentence(words[j - 1].0) {
                        break_penalty
                    } else {
                        0.0
                    };
                    let cost = c + (len as f64 - target).powi(2) + brk;
                    if best[l][j].is_none_or(|(b, _)| cost < b) {
                        best[l][j] = Some((cost, i));
                    }
                }
            }
        }
        if best[k][n].is_some() {
            let mut lines = Vec::with_capacity(k);
            let mut j = n;
            for l in (1..=k).rev() {
                let i = best[l][j].expect("reachable").1;
                let mut line = String::new();
                for (x, (w, glued)) in words[i..j].iter().enumerate() {
                    if x > 0 && !glued {
                        line.push(' ');
                    }
                    line.push_str(w);
                }
                lines.push(line);
                j = i;
            }
            lines.reverse();
            return Some(lines);
        }
    }
    None
}

/// The lines of a cue's text under the layout, or `None` if it doesn't fit.
fn lay_out(text: &str, s: &Layout) -> Option<Vec<String>> {
    if s.wrap_lines {
        return wrap(text, s.max_line, s.max_lines.max(1));
    }
    let fits = s.max_line == 0 || text.chars().count() <= s.max_line * s.max_lines.max(1);
    fits.then(|| vec![text.to_string()])
}

/// Cost of words `ws` as one cue with `prefix` in front, or `None` if it breaks the layout.
fn cue_cost(ws: &[Word], prefix: &str, s: &Layout) -> Option<f64> {
    let text = format!("{prefix}{}", join(ws));
    lay_out(&text, s)?;
    let dur = ws.last().expect("non-empty").end.saturating_sub(ws[0].start) as f64 / 1000.0;
    if s.max_duration > 0.0 && dur > s.max_duration {
        return None;
    }
    // Prefer evenly filled cues: unused capacity is penalised quadratically.
    let fill = if s.max_line > 0 {
        text.chars().count() as f64 / (s.max_line * s.max_lines.max(1)) as f64
    } else if s.max_duration > 0.0 {
        dur / s.max_duration
    } else {
        1.0
    };
    Some(CUE_COST + 30.0 * (1.0 - fill.min(1.0)).powi(2))
}

/// Splits one sentence into cues. Cuts at punctuation first; only a piece that still doesn't fit is
/// cut at pauses, and only a piece that doesn't fit even then is cut between words.
fn split(ws: &[Word], prefix: &str, s: &Layout) -> Vec<Range<usize>> {
    refine(ws, 0..ws.len(), prefix, s, 0)
}

fn refine(ws: &[Word], range: Range<usize>, prefix: &str, s: &Layout, level: usize) -> Vec<Range<usize>> {
    let p = if range.start == 0 { prefix } else { "" };
    if level > 2 || cue_cost(&ws[range.clone()], p, s).is_some() {
        return vec![range];
    }
    partition(ws, range, prefix, s, level).into_iter().flat_map(|r| refine(ws, r, prefix, s, level + 1)).collect()
}

/// Best split of `range` using only cuts of `level` or cleaner. A piece with no such cut inside may
/// be too long (it is refined at the next level); any other piece must fit.
fn partition(ws: &[Word], range: Range<usize>, prefix: &str, s: &Layout, level: usize) -> Vec<Range<usize>> {
    let (a, b) = (range.start, range.end);
    let can_cut = |i: usize| i == a || i == b || cut_level(&ws[i - 1], &ws[i]) <= level;
    let mut best: Vec<Option<(f64, usize)>> = vec![None; b - a + 1];
    best[0] = Some((0.0, a));
    for j in a + 1..=b {
        if !can_cut(j) {
            continue;
        }
        for i in (a..j).rev() {
            if !can_cut(i) {
                continue;
            }
            let Some((c0, _)) = best[i - a] else { continue };
            let p = if i == 0 { prefix } else { "" };
            let cost = match cue_cost(&ws[i..j], p, s) {
                Some(c) => c,
                None if (i + 1..j).all(|k| !can_cut(k)) => CUE_COST * 10.0,
                None => continue,
            };
            if best[j - a].is_none_or(|(c, _)| c0 + cost < c) {
                best[j - a] = Some((c0 + cost, i));
            }
        }
    }
    let mut out = Vec::new();
    let mut j = b;
    while j > a {
        let i = best[j - a].expect("the whole range is always reachable").1;
        out.push(i..j);
        j = i;
    }
    out.reverse();
    out
}

pub(crate) fn speaker_label(speaker: &str, names: &[String]) -> String {
    speaker
        .parse::<usize>()
        .ok()
        .and_then(|n| names.get(n.checked_sub(1)?))
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("Speaker {speaker}"))
}

/// One subtitle: what is shown, when.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    /// Start and end, milliseconds. A cue shorter than [`Layout::min_duration`] is already extended
    /// into the silence after it.
    pub start: u64,
    pub end: u64,
    /// The text as shown, line by line (one line unless [`Layout::wrap_lines`]), with the speaker
    /// label in front when the layout asks for one.
    pub lines: Vec<String>,
    /// The speaker label (`"Speaker 1: "`) the first line starts with, or empty.
    pub prefix: String,
    /// The speaker of every word in the cue; `None` without diarization, or when the layout lets a
    /// cue mix speakers (`by_speaker` off).
    pub speaker: Option<String>,
    /// The cue's words in order; a word with a dash inside ("finally—and") is two words here, the
    /// second `glued`.
    pub words: Vec<Word>,
}

impl Cue {
    /// The lines joined with `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// Cuts words into cues by the layout rules (see the module docs).
pub fn cues(words: &[Word], s: &Layout) -> Vec<Cue> {
    let words: Vec<Word> = words.iter().cloned().flat_map(split_dashes).collect();
    let mut speakers: Vec<&str> = words.iter().filter_map(|w| w.speaker.as_deref()).collect();
    speakers.sort_unstable();
    speakers.dedup();
    let labels = s.speaker_labels && s.by_speaker && speakers.len() > 1;

    let mut cues: Vec<Cue> = Vec::new();
    let mut prev_speaker: Option<Option<String>> = None;
    for sentence in sentences(words.clone(), s) {
        let speaker = sentence[0].speaker.clone();
        let prefix = match &speaker {
            Some(sp) if labels && prev_speaker.as_ref() != Some(&speaker) => {
                format!("{}: ", speaker_label(sp, &s.speaker_names))
            }
            _ => String::new(),
        };
        prev_speaker = Some(speaker.clone());
        // A subtitle sits in one place: a sentence whose words change place is cut there first.
        let mut runs: Vec<&[Word]> = Vec::new();
        let mut from = 0;
        for i in 1..=sentence.len() {
            if i == sentence.len() || sentence[i].marks.pos != sentence[i - 1].marks.pos {
                runs.push(&sentence[from..i]);
                from = i;
            }
        }
        for (ri, run) in runs.into_iter().enumerate() {
            let pre = if ri == 0 { prefix.as_str() } else { "" };
            for (k, r) in split(run, pre, s).into_iter().enumerate() {
                let ws = &run[r];
                let p = if k == 0 { pre } else { "" };
                let text = format!("{p}{}", join(ws));
                cues.push(Cue {
                    start: ws[0].start,
                    end: ws.last().expect("non-empty").end,
                    lines: lay_out(&text, s).unwrap_or_else(|| vec![text.clone()]),
                    prefix: p.to_string(),
                    speaker: if s.by_speaker { speaker.clone() } else { None },
                    words: ws.to_vec(),
                });
            }
        }
    }

    let min_ms = (s.min_duration * 1000.0) as u64;
    for i in 0..cues.len() {
        let (start, end) = (cues[i].start, cues[i].end);
        // Too-short cues borrow time from the silence after them, never overlapping the next cue.
        if end < start + min_ms {
            let next = cues.get(i + 1).map_or(u64::MAX, |n| n.start);
            cues[i].end = (start + min_ms).min(next.saturating_sub(1)).max(end);
        }
    }
    cues
}

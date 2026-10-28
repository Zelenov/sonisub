//! CAPT: a transcript as a small text file, between a recogniser and every subtitle format.
//!
//! A file is a header and blocks. A block is one source of sound over a stretch of time: a speaker
//! talking (a text line of words with `{hundredths}` marks and ` / ` cut points), or a sound
//! (`sound:phone`, no text). Span lines `{from..to} key:value` give options to part of a block's
//! text. The format is specified in `docs/capt.md` of the sonisub repository.
//!
//! - [`Doc::parse`] reads a file, [`Doc::write`] writes one; a `Doc` made by `parse` or
//!   [`Doc::from_spoken`] reads back the same: `Doc::parse(&doc.write()) == Ok(doc)`.
//! - [`Doc::from_spoken`] makes one from a recogniser's words with their pieces;
//!   [`Doc::from_words`] from plain [`crate::Word`]s (one piece per word).
//! - [`Doc::to_words`] gives the words back for the writers ([`crate::srt`], [`crate::premiere`], …).
//!
//! Times are kept to 10 ms (the marks are hundredths) and confidence to two decimals.

use std::fmt;

use crate::cues::{self, Layout};

/// Options as written, in order: a key and the parts of its value
/// (`alt:con,0.20` -> `("alt", ["con", "0.20"])`).
pub type KeyValues = Vec<(String, Vec<String>)>;

/// A CAPT file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc {
    /// The file's id, made once and never changed; the seed of Premiere's speaker ids.
    pub id: Option<String>,
    /// The name of the work.
    pub title: Option<String>,
    /// The main language; a block's words are in it unless the block or a span says otherwise.
    pub lang: Option<String>,
    /// The source file's name.
    pub media: Option<String>,
    /// The source's length, milliseconds.
    pub duration: Option<u64>,
    /// The video's frame, width and height in pixels (`frame:1080,1920` for a vertical video):
    /// outputs that place text (ASS, WebVTT, TTML) keep it clear of the edges and a phone app's
    /// buttons.
    pub frame: Option<(u32, u32)>,
    /// Every speaker the writer knows, in order of appearance.
    pub speakers: Vec<Speaker>,
    /// Header options this version does not know, kept as written.
    pub other: KeyValues,
    /// The blocks, in order of start.
    pub blocks: Vec<Block>,
}

/// A header line `speaker:1 name:Anna`.
#[derive(Debug, Clone, PartialEq)]
pub struct Speaker {
    /// What blocks name the speaker by (`speaker:1`).
    pub id: String,
    /// The name to show; without one the speaker is "Speaker N".
    pub name: Option<String>,
    /// The line's other options, kept as written.
    pub other: KeyValues,
}

/// Where on screen.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pos {
    /// Bottom centre, the default.
    Bottom,
    /// Top centre.
    Top,
    /// The centre of the frame.
    Middle,
    /// The text is centred on a point.
    Point {
        /// Share of the frame's width from its left edge, 0 to 1.
        x: f64,
        /// Share of the frame's height from its top edge, 0 to 1.
        y: f64,
    },
    /// The text wraps inside a rectangle and starts at its top.
    Rect {
        /// The left edge, as a share of the frame's width.
        x: f64,
        /// The top edge, as a share of the frame's height.
        y: f64,
        /// The width, as a share of the frame's width.
        w: f64,
        /// The height, as a share of the frame's height.
        h: f64,
    },
}

/// One source of sound over a stretch of time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    /// Start, milliseconds from the start of the media.
    pub start: u64,
    /// End, milliseconds from the start of the media.
    pub end: u64,
    /// Six characters of Crockford base32, unique in the file.
    pub id: Option<String>,
    /// Who speaks or makes the sound.
    pub speaker: Option<String>,
    /// A sound, not speech (`phone`, `laughter`): such a block has no text.
    pub sound: Option<String>,
    /// The language of the words; the file's when `None`.
    pub lang: Option<String>,
    /// Where on screen; bottom when `None`.
    pub pos: Option<Pos>,
    /// How the words are said (`whisper`).
    pub voice: Option<String>,
    /// Block options this version does not know, kept as written.
    pub other: KeyValues,
    /// The text: phrases (the words between cut points), each a list of words.
    pub phrases: Vec<Vec<Word>>,
    /// The text line had no marks (a hand-written line): its words got times spread over the block
    /// by length, and it is written back without marks. `false` writes every time.
    pub untimed: bool,
    /// Options for parts of the text.
    pub spans: Vec<Span>,
}

/// A word of a block's text with its pieces; [`Doc::to_words`] turns a file's into [`crate::Word`]s.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    /// The pieces in order; at least one.
    pub pieces: Vec<Piece>,
    /// End, milliseconds from the start of the media.
    pub end: u64,
}

/// A piece of a word with the moment it starts (a token of the recogniser).
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// The piece as written, without the escapes of the file.
    pub text: String,
    /// Start, milliseconds from the start of the media.
    pub start: u64,
}

/// Options for the text from one moment up to another: `from` included, `to` excluded.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// A moment a piece starts, milliseconds from the start of the media.
    pub from: u64,
    /// A later moment a piece starts or a word ends.
    pub to: u64,
    /// The options, as written.
    pub options: KeyValues,
}

/// What is wrong at a line of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    /// The line, from 1.
    pub line: usize,
    /// What is wrong, and what to write instead.
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for Error {}

/// Every problem [`Doc::parse`] found, in order of line.
#[derive(Debug, Clone, PartialEq)]
pub struct Errors(pub Vec<Error>);

impl fmt::Display for Errors {
    /// One problem per line: `line 4: a mark holds a time in hundredths, not “x”`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<String> = self.0.iter().map(ToString::to_string).collect();
        f.write_str(&lines.join("\n"))
    }
}

impl std::error::Error for Errors {}

/// A word as a recogniser hears it, for [`Doc::from_spoken`]: [`crate::Word`] with its pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Spoken {
    /// The pieces in order; at least one.
    pub pieces: Vec<SpokenPiece>,
    /// End, milliseconds from the start of the audio.
    pub end: u64,
    /// Speaker label ("1"); `None` without diarization.
    pub speaker: Option<String>,
    /// Language code ("en").
    pub language: Option<String>,
}

/// A piece of a [`Spoken`] word: a token of the recogniser.
#[derive(Debug, Clone, PartialEq)]
pub struct SpokenPiece {
    /// The token's text, without the spaces around it.
    pub text: String,
    /// Start, milliseconds from the start of the audio.
    pub start: u64,
    /// How sure the recogniser is, 0 to 1.
    pub confidence: f64,
}

impl From<&Spoken> for crate::Word {
    /// The pieces joined; the first one's start; the running mean of their confidence.
    fn from(s: &Spoken) -> Self {
        let mut confidence = 0.0;
        for (n, p) in s.pieces.iter().enumerate() {
            confidence = if n == 0 { p.confidence } else { (confidence * n as f64 + p.confidence) / (n + 1) as f64 };
        }
        crate::Word {
            text: s.pieces.iter().map(|p| p.text.as_str()).collect(),
            start: s.pieces.first().map_or(s.end, |p| p.start),
            end: s.end,
            speaker: s.speaker.clone(),
            language: s.language.clone(),
            confidence,
            glued: false,
            marks: Default::default(),
        }
    }
}

impl Word {
    /// The word as written: its pieces joined.
    pub fn text(&self) -> String {
        self.pieces.iter().map(|p| p.text.as_str()).collect()
    }

    /// The start of its first piece.
    pub fn start(&self) -> u64 {
        self.pieces.first().map_or(self.end, |p| p.start)
    }
}

impl Block {
    /// The words of the text, in order.
    pub fn words(&self) -> impl Iterator<Item = &Word> {
        self.phrases.iter().flatten()
    }

    /// The value (its parts) of option `key` in the first span that covers the piece starting at
    /// `at` and has that option.
    pub fn span_option(&self, at: u64, key: &str) -> Option<&[String]> {
        self.spans
            .iter()
            .filter(|s| s.from <= at && at < s.to)
            .find_map(|s| s.options.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_slice()))
    }
}

// ---------- reading ----------

/// Which span options cover whole words only; the others may cover any pieces.
const WORD_SPAN_OPTIONS: [&str; 5] = ["lang", "pos", "voice", "style", "sentence"];
const POS_NAMES: [(&str, Pos); 3] = [("bottom", Pos::Bottom), ("top", Pos::Top), ("middle", Pos::Middle)];

/// `HH:MM:SS.mmm` -> milliseconds; `None` if it is not one, or too large.
fn parse_time(s: &str) -> Option<u64> {
    let (hms, ms) = s.split_once('.')?;
    let mut parts = hms.split(':');
    let (h, m, sec) = (parts.next()?, parts.next()?, parts.next()?);
    let digits = |p: &str, n: Option<usize>| {
        !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && n.is_none_or(|n| p.len() == n)
    };
    if parts.next().is_some()
        || !digits(h, None)
        || !digits(m, Some(2))
        || !digits(sec, Some(2))
        || !digits(ms, Some(3))
    {
        return None;
    }
    let (h, m, sec, ms): (u64, u64, u64, u64) = (h.parse().ok()?, m.parse().ok()?, sec.parse().ok()?, ms.parse().ok()?);
    if m >= 60 || sec >= 60 {
        return None;
    }
    h.checked_mul(3_600_000)?.checked_add((m * 60 + sec) * 1000 + ms)
}

/// `base` + `cs` hundredths, in milliseconds; `None` if too large.
fn offset(base: u64, cs: u64) -> Option<u64> {
    base.checked_add(cs.checked_mul(10)?)
}

/// A line meant as a block line: a first word that starts with a digit, then ` --> ` (a mistyped
/// time is still one, so it gets the block line's error). A header key starts with a letter, a
/// marked text line with `{`.
fn is_block_line(line: &str) -> bool {
    let mut it = line.split(' ');
    it.next().is_some_and(|t| t.starts_with(|c: char| c.is_ascii_digit())) && it.next() == Some("-->")
}

/// `key:value key:"a b",c` -> options. A part is bare (no space, comma or quote) or quoted, with
/// `\"` and `\\` inside quotes.
fn parse_options(s: &str) -> Result<KeyValues, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: KeyValues = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' {
            i += 1;
            continue;
        }
        let k = i;
        while i < chars.len() && chars[i] != ':' && chars[i] != ' ' {
            i += 1;
        }
        if i >= chars.len() || chars[i] != ':' || i == k {
            return Err(format!("expected key:value, found “{}”", chars[k..i].iter().collect::<String>()));
        }
        let key: String = chars[k..i].iter().collect();
        i += 1;
        let mut parts = Vec::new();
        loop {
            let mut part = String::new();
            if chars.get(i) == Some(&'"') {
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err(format!("the quoted value of “{key}:” is never closed")),
                        Some('"') => break,
                        Some('\\') if matches!(chars.get(i + 1), Some('"' | '\\')) => {
                            part.push(chars[i + 1]);
                            i += 2;
                        }
                        Some(&c) => {
                            part.push(c);
                            i += 1;
                        }
                    }
                }
                i += 1;
            } else {
                while let Some(&c) = chars.get(i).filter(|&&c| c != ',' && c != ' ') {
                    if c == '"' {
                        return Err(format!("a quote inside the bare value of “{key}:”: quote the whole value"));
                    }
                    part.push(c);
                    i += 1;
                }
            }
            parts.push(part);
            if chars.get(i) == Some(&',') {
                i += 1;
                continue;
            }
            break;
        }
        if chars.get(i).is_some_and(|&c| c != ' ') {
            return Err(format!("“{key}:” is followed by “{}” without a space", chars[i]));
        }
        out.push((key, parts));
    }
    Ok(out)
}

fn parse_pos(parts: &[String]) -> Result<Pos, String> {
    let bad = || {
        format!(
            "pos is bottom, top, middle, a point x,y or a rectangle x,y,w,h, as parts of the frame from 0 to 1; not “{}”",
            parts.join(",")
        )
    };
    if let [name] = parts {
        return POS_NAMES.iter().find(|(n, _)| n == name).map(|(_, p)| *p).ok_or_else(bad);
    }
    let n: Vec<f64> = parts.iter().map(|p| p.parse::<f64>()).collect::<Result<_, _>>().map_err(|_| bad())?;
    if n.iter().any(|v| !(0.0..=1.0).contains(v)) {
        return Err(bad());
    }
    match n[..] {
        [x, y] => Ok(Pos::Point { x, y }),
        [x, y, w, h] if w > 0.0 && h > 0.0 && x + w <= 1.0 + 1e-9 && y + h <= 1.0 + 1e-9 => {
            Ok(Pos::Rect { x, y, w, h })
        }
        [_, _, _, _] => Err(format!("the rectangle {} runs out of the frame or has no size", parts.join(","))),
        _ => Err(bad()),
    }
}

enum Token {
    Text(String),
    Mark(u64),
    Space,
    Cut,
}

/// One text line -> tokens. A backslash makes the next character text; a bare `{` opens a mark.
fn scan(line: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Token>| {
        if !text.is_empty() {
            out.push(Token::Text(std::mem::take(text)));
        }
    };
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                text.push(chars[i + 1]);
                i += 2;
            }
            '{' => {
                let close = chars[i..].iter().position(|&c| c == '}').ok_or_else(|| {
                    format!("a “{{” at column {} is never closed: write \\{{ for a brace in the text", i + 1)
                })?;
                let body: String = chars[i + 1..i + close].iter().collect();
                if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(format!("a mark holds a time in hundredths, not “{body}”"));
                }
                flush(&mut text, &mut out);
                out.push(Token::Mark(body.parse().map_err(|_| format!("the mark {{{body}}} is too large"))?));
                i += close + 1;
            }
            '}' => {
                return Err(format!("a “}}” at column {} closes nothing: write \\}} for a brace in the text", i + 1));
            }
            ' ' if chars.get(i + 1) == Some(&'/') && chars.get(i + 2) == Some(&' ') => {
                flush(&mut text, &mut out);
                out.push(Token::Cut);
                i += 3;
            }
            ' ' => {
                flush(&mut text, &mut out);
                out.push(Token::Space);
                i += 1;
            }
            c => {
                text.push(c);
                i += 1;
            }
        }
    }
    flush(&mut text, &mut out);
    Ok(out)
}

/// A piece or word end without a mark before it; [`spread_times`] gives it one.
const UNTIMED: u64 = u64::MAX;

fn end_word(word: &mut Option<Word>, phrases: &mut [Vec<Word>], last: &mut Option<u64>) {
    if let Some(w) = word.take().filter(|w| !w.pieces.is_empty()) {
        phrases.last_mut().expect("never empty").push(w);
    }
    *last = None;
}

/// Tokens -> phrases of words, and whether the line has marks. A line has marks on every word or
/// on none; marks never go back, and piece starts rise strictly.
fn words_of(tokens: Vec<Token>, base: u64) -> Result<(Vec<Vec<Word>>, bool), String> {
    let mut phrases: Vec<Vec<Word>> = vec![Vec::new()];
    let mut word: Option<Word> = None;
    let mut last: Option<u64> = None;
    let mut marked = false;
    let (mut prev_mark, mut prev_piece) = (None::<u64>, None::<(u64, u64)>);
    for t in tokens {
        match t {
            Token::Space => end_word(&mut word, &mut phrases, &mut last),
            Token::Cut => {
                end_word(&mut word, &mut phrases, &mut last);
                phrases.push(Vec::new());
            }
            Token::Mark(cs) => {
                marked = true;
                let t = offset(base, cs).ok_or_else(|| format!("the mark {{{cs}}} is too large"))?;
                if let Some(p) = prev_mark.filter(|&p| t < p) {
                    let rel = (p - base) / 10;
                    return Err(format!("the mark {{{cs}}} is earlier than the mark {{{rel}}} before it"));
                }
                prev_mark = Some(t);
                last = Some(t);
                word.get_or_insert_with(|| Word { pieces: Vec::new(), end: UNTIMED }).end = t;
            }
            Token::Text(text) => {
                let start = last.unwrap_or(UNTIMED);
                if let Some((_, cs)) = prev_piece.filter(|&(p, _)| start != UNTIMED && start <= p) {
                    return Err(format!("two pieces start at {{{cs}}}: a piece starts after the one before it"));
                }
                if start != UNTIMED {
                    prev_piece = Some((start, (start - base) / 10));
                }
                let w = word.get_or_insert_with(|| Word { pieces: Vec::new(), end: UNTIMED });
                w.pieces.push(Piece { text, start });
                w.end = UNTIMED;
            }
        }
    }
    end_word(&mut word, &mut phrases, &mut last);
    phrases.retain(|p| !p.is_empty());
    let untimed = phrases.iter().flatten().any(|w| w.end == UNTIMED || w.pieces.iter().any(|p| p.start == UNTIMED));
    if marked && untimed {
        return Err("every word of a line has marks, or none does: {start}word{end}".into());
    }
    Ok((phrases, marked))
}

/// A line without marks: its words get times spread over the block by length.
fn spread_times(b: &mut Block) {
    let total: usize = b.words().map(|w| w.text().chars().count() + 1).sum();
    if total == 0 {
        return;
    }
    let (start, len) = (b.start, b.end.saturating_sub(b.start));
    let n = b.words().count();
    let share = |chars: usize| start + (len as u128 * chars as u128 / total as u128) as u64;
    let mut at = 0usize;
    for (k, w) in b.phrases.iter_mut().flatten().enumerate() {
        let s = share(at);
        at += w.text().chars().count() + 1;
        let e = if k + 1 == n { b.end } else { share(at - 1) };
        if w.end == UNTIMED {
            w.end = e;
        }
        for (i, p) in w.pieces.iter_mut().enumerate() {
            if p.start == UNTIMED {
                p.start = if i == 0 { s } else { w.end };
            }
        }
    }
}

fn check_spans(b: &Block, lines: &[(usize, Span)], errors: &mut Vec<Error>) {
    let pieces: Vec<u64> = b.words().flat_map(|w| w.pieces.iter().map(|p| p.start)).collect();
    let words: Vec<(u64, u64)> = b.words().map(|w| (w.start(), w.end)).collect();
    let rel = |ms: u64| (ms - b.start) / 10;
    for (line, s) in lines {
        let name = format!("{{{}..{}}}", rel(s.from), rel(s.to));
        let mut err = |message: String| errors.push(Error { line: *line, message: format!("{name}: {message}") });
        if !pieces.contains(&s.from) {
            err(format!("no piece starts at {{{}}}", rel(s.from)));
            continue;
        }
        if s.to <= s.from || !(pieces.contains(&s.to) || words.iter().any(|w| w.1 == s.to)) {
            err(format!("{{{}}} is not a later mark of this block", rel(s.to)));
            continue;
        }
        let covered: Vec<&(u64, u64)> = words.iter().filter(|w| w.0 >= s.from && w.0 < s.to).collect();
        let whole = covered.first().is_some_and(|w| w.0 == s.from) && covered.last().is_some_and(|w| w.1 == s.to);
        for (key, value) in &s.options {
            if WORD_SPAN_OPTIONS.contains(&key.as_str()) && !whole {
                err(format!("“{key}:” covers whole words: from a word start to a word end"));
            } else if key == "pos"
                && let Err(e) = parse_pos(value)
            {
                err(e);
            }
        }
    }
}

impl Doc {
    /// Reads a CAPT file. Every problem found is returned, each with its line.
    pub fn parse(text: &str) -> Result<Doc, Errors> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
        let mut errors = Vec::new();
        let mut doc = Doc::default();
        if lines.first().map(|l| l.trim_end()) != Some("CAPT 1") {
            errors.push(Error { line: 1, message: "a CAPT file starts with the line “CAPT 1”".into() });
        }
        let mut i = 1;
        while i < lines.len() && !is_block_line(lines[i]) {
            let line = lines[i].trim();
            i += 1;
            if !line.is_empty() {
                doc.read_header(line, i, &mut errors);
            }
        }
        // The start and line of the block before, to check the order.
        let mut prev: Option<(u64, usize)> = None;
        while i < lines.len() {
            if lines[i].trim().is_empty() {
                i += 1;
                continue;
            }
            let at = i + 1;
            let mut block_lines = vec![lines[i]];
            while i + 1 < lines.len() && !lines[i + 1].trim().is_empty() {
                if is_block_line(lines[i + 1]) {
                    // The next block starts here; it is read as one, and the missing blank line named.
                    errors.push(Error { line: i + 2, message: "put a blank line before this block line".into() });
                    break;
                }
                i += 1;
                block_lines.push(lines[i]);
            }
            i += 1;
            if let Some(b) = parse_block(&block_lines, at, &mut errors) {
                if let Some((prev_start, prev_at)) = prev
                    && b.start < prev_start
                {
                    errors.push(Error {
                        line: at,
                        message: format!(
                            "this block starts before the block on line {prev_at}: blocks are in order of start"
                        ),
                    });
                }
                prev = Some((b.start, at));
                doc.blocks.push(b);
            }
        }
        errors.sort_by_key(|e| e.line);
        if errors.is_empty() { Ok(doc) } else { Err(Errors(errors)) }
    }

    /// One header line: a speaker when it has `speaker:`, else file options.
    fn read_header(&mut self, line: &str, line_no: usize, errors: &mut Vec<Error>) {
        let opts = match parse_options(line) {
            Ok(o) => o,
            Err(message) => return errors.push(Error { line: line_no, message }),
        };
        if opts.iter().any(|(k, _)| k == "speaker") {
            let mut s = Speaker { id: String::new(), name: None, other: Vec::new() };
            for (key, value) in opts {
                match key.as_str() {
                    "speaker" => s.id = value.join(","),
                    "name" => s.name = Some(value.join(",")),
                    _ => s.other.push((key, value)),
                }
            }
            return self.speakers.push(s);
        }
        for (key, value) in opts {
            let joined = value.join(",");
            match key.as_str() {
                "id" => self.id = Some(joined),
                "title" => self.title = Some(joined),
                "lang" => self.lang = Some(joined),
                "media" => self.media = Some(joined),
                "frame" => match value.iter().map(|v| v.parse::<u32>()).collect::<Result<Vec<_>, _>>().as_deref() {
                    Ok([w, h]) if *w > 0 && *h > 0 => self.frame = Some((*w, *h)),
                    _ => errors.push(Error {
                        line: line_no,
                        message: format!("frame is width,height in pixels, not “{joined}”"),
                    }),
                },
                "duration" => match parse_time(&joined) {
                    Some(ms) => self.duration = Some(ms),
                    None => errors
                        .push(Error {
                            line: line_no, message: format!("duration is HH:MM:SS.mmm, not “{joined}”")
                        }),
                },
                _ => self.other.push((key, value)),
            }
        }
    }
}

fn parse_block(lines: &[&str], at: usize, errors: &mut Vec<Error>) -> Option<Block> {
    let mut it = lines[0].splitn(4, ' ');
    let (start, arrow, end, rest) = (it.next(), it.next(), it.next(), it.next().unwrap_or(""));
    let (Some(start), Some("-->"), Some(end)) = (start.and_then(parse_time), arrow, end.and_then(parse_time)) else {
        errors.push(Error {
            line: at, message: "a block line is “HH:MM:SS.mmm --> HH:MM:SS.mmm key:value…”".into()
        });
        return None;
    };
    let mut b = Block { start, end, ..Block::default() };
    if end < start {
        errors.push(Error { line: at, message: "the block ends before it starts".into() });
    }
    match parse_options(rest) {
        Err(message) => errors.push(Error { line: at, message }),
        Ok(opts) => {
            for (key, value) in opts {
                let joined = value.join(",");
                match key.as_str() {
                    "id" => b.id = Some(joined),
                    "speaker" => b.speaker = Some(joined),
                    "sound" => b.sound = Some(joined),
                    "lang" => b.lang = Some(joined),
                    "voice" => b.voice = Some(joined),
                    "pos" => match parse_pos(&value) {
                        Ok(p) => b.pos = Some(p),
                        Err(message) => errors.push(Error { line: at, message }),
                    },
                    _ => b.other.push((key, value)),
                }
            }
        }
    }
    if b.sound.is_some() {
        if lines.len() > 1 {
            errors.push(Error { line: at + 1, message: "a sound block has no text: the sound is its code".into() });
        }
        return Some(b);
    }
    let mut text_ok = true;
    if let Some(line) = lines.get(1) {
        match scan(line).and_then(|t| words_of(t, start)) {
            Err(message) => {
                errors.push(Error { line: at + 1, message });
                text_ok = false;
            }
            Ok((phrases, marked)) => {
                b.phrases = phrases;
                b.untimed = !marked;
                // Marks never go back, so a word's end is its latest mark. A block that ends before it
                // starts is reported already.
                if marked
                    && end >= start
                    && let Some(w) = b.words().find(|w| w.end > end)
                {
                    errors.push(Error {
                        line: at + 1,
                        message: format!("the mark {{{}}} is after the block's end", (w.end - start) / 10),
                    });
                    text_ok = false;
                }
                if b.untimed {
                    spread_times(&mut b);
                }
            }
        }
    }
    let mut spans = Vec::new();
    for (k, line) in lines.iter().enumerate().skip(2) {
        let line_no = at + k;
        let parsed = line.strip_prefix('{').and_then(|r| r.split_once('}')).and_then(|(range, opts)| {
            let (a, z) = range.split_once("..")?;
            let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
            (all_digits(a) && all_digits(z)).then_some((a, z, opts))
        });
        let Some((a, z, opts)) = parsed else {
            errors.push(Error {
                line: line_no,
                message: "after the text a block has only spans, “{from..to} key:value”".into(),
            });
            continue;
        };
        let from = a.parse().ok().and_then(|a| offset(start, a));
        let to = z.parse().ok().and_then(|z| offset(start, z));
        let (Some(from), Some(to)) = (from, to) else {
            errors.push(Error { line: line_no, message: format!("the span {{{a}..{z}}} is too large") });
            continue;
        };
        match parse_options(opts.trim()) {
            Err(message) => errors.push(Error { line: line_no, message }),
            Ok(options) => spans.push((line_no, Span { from, to, options })),
        }
    }
    // Spans are checked against the words, so only when the text line could be read.
    if text_ok {
        check_spans(&b, &spans, errors);
    }
    b.spans = spans.into_iter().map(|(_, s)| s).collect();
    Some(b)
}

// ---------- writing ----------

fn time(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// A control character (a line break, a tab) as a space: a file's lines must stay lines.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn part(p: &str) -> String {
    let p = one_line(p);
    if !p.is_empty() && !p.chars().any(|c| c == ',' || c == '"' || c.is_whitespace()) {
        return p;
    }
    format!("\"{}\"", p.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Options as one line; `None` values are left out.
fn options<'a>(opts: impl IntoIterator<Item = (&'a str, Option<Vec<String>>)>) -> String {
    opts.into_iter()
        .filter_map(|(k, v)| Some(format!("{k}:{}", v?.iter().map(|p| part(p)).collect::<Vec<_>>().join(","))))
        .collect::<Vec<_>>()
        .join(" ")
}

fn kept(other: &KeyValues) -> impl Iterator<Item = (&str, Option<Vec<String>>)> {
    other.iter().map(|(k, v)| (k.as_str(), Some(v.clone())))
}

fn one(v: &Option<String>) -> Option<Vec<String>> {
    v.as_ref().map(|v| vec![v.clone()])
}

fn pos_parts(p: Pos) -> Vec<String> {
    let numbers = |n: &[f64]| n.iter().map(|v| v.to_string()).collect();
    match p {
        Pos::Bottom => vec!["bottom".into()],
        Pos::Top => vec!["top".into()],
        Pos::Middle => vec!["middle".into()],
        Pos::Point { x, y } => numbers(&[x, y]),
        Pos::Rect { x, y, w, h } => numbers(&[x, y, w, h]),
    }
}

/// A piece of text with what would mean something else escaped: braces, backslashes, a space
/// inside a word, and a slash that is a whole word.
fn escape(text: &str, whole_word: bool) -> String {
    if whole_word && text == "/" {
        return "\\/".into();
    }
    one_line(text).replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}").replace(' ', "\\ ")
}

impl Doc {
    /// The file as text. A `Doc` made by [`Doc::parse`] or [`Doc::from_spoken`] reads back the same.
    pub fn write(&self) -> String {
        let mut out = String::from("CAPT 1\n");
        let mut line = |text: String| {
            if !text.is_empty() {
                out.push_str(&text);
                out.push('\n');
            }
        };
        line(options([("id", one(&self.id))]));
        line(options([("title", one(&self.title))]));
        line(options([("lang", one(&self.lang))]));
        line(options([
            ("media", one(&self.media)),
            ("duration", self.duration.map(|d| vec![time(d)])),
            ("frame", self.frame.map(|(w, h)| vec![w.to_string(), h.to_string()])),
        ]));
        for (k, v) in &self.other {
            line(options([(k.as_str(), Some(v.clone()))]));
        }
        for s in &self.speakers {
            line(options(
                [("speaker", Some(vec![s.id.clone()])), ("name", one(&s.name))].into_iter().chain(kept(&s.other)),
            ));
        }
        for b in &self.blocks {
            out.push('\n');
            out.push_str(&b.write());
        }
        out
    }
}

impl Block {
    fn write(&self) -> String {
        let opts = options(
            [
                ("id", one(&self.id)),
                ("speaker", one(&self.speaker)),
                ("sound", one(&self.sound)),
                ("lang", one(&self.lang)),
                ("pos", self.pos.map(pos_parts)),
                ("voice", one(&self.voice)),
            ]
            .into_iter()
            .chain(kept(&self.other)),
        );
        let mut out = format!("{} --> {}", time(self.start), time(self.end));
        if !opts.is_empty() {
            out.push(' ');
            out.push_str(&opts);
        }
        out.push('\n');
        if self.sound.is_some() || self.phrases.is_empty() {
            return out;
        }
        let cs = |ms: u64| ms.saturating_sub(self.start).saturating_add(5) / 10;
        let word = |w: &Word| {
            let whole = w.pieces.len() == 1;
            if self.untimed {
                return w.pieces.iter().map(|x| escape(&x.text, whole)).collect::<String>();
            }
            let mut s: String =
                w.pieces.iter().map(|x| format!("{{{}}}{}", cs(x.start), escape(&x.text, whole))).collect();
            s.push_str(&format!("{{{}}}", cs(w.end)));
            s
        };
        let phrases: Vec<String> =
            self.phrases.iter().map(|p| p.iter().map(word).collect::<Vec<_>>().join(" ")).collect();
        let text = phrases.join(" / ");
        // A line without marks that would read as a block line starts with an escape.
        if is_block_line(&text) {
            out.push('\\');
        }
        out.push_str(&text);
        out.push('\n');
        for s in &self.spans {
            out.push_str(&format!("{{{}..{}}}", cs(s.from), cs(s.to)));
            let opts = options(kept(&s.options));
            if !opts.is_empty() {
                out.push(' ');
                out.push_str(&opts);
            }
            out.push('\n');
        }
        out
    }
}

// ---------- from words ----------

/// Crockford base32, without `i l o u`.
const BASE32: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Six characters of a hash of `seed`: the same transcript always gives the same ids.
fn block_id(seed: &str) -> String {
    let h = crate::hash64(seed, 0);
    (0..6).map(|i| BASE32[(h >> (i * 5) & 31) as usize] as char).collect()
}

/// The language most of `langs` are in; first seen wins a tie.
fn majority<'a>(langs: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for l in langs {
        match counts.iter_mut().find(|(c, _)| *c == l) {
            Some((_, n)) => *n += 1,
            None => counts.push((l, 1)),
        }
    }
    counts.iter().rev().max_by_key(|(_, n)| *n).map(|(l, _)| l.to_string())
}

/// The latest time a writer keeps, so a written file reads back: a block's start plus a mark
/// times 10 always fits a `u64`.
const LATEST: u64 = u64::MAX / 100 * 10;

/// Times on the 10 ms grid the marks are written on.
fn grid(ms: u64) -> u64 {
    (ms.min(LATEST) + 5) / 10 * 10
}

fn span(from: u64, to: u64, key: &str, value: String) -> Span {
    Span { from, to, options: vec![(key.into(), vec![value])] }
}

impl Doc {
    /// A file from a recogniser's words: one block per sentence of one speaker (as [`cues`] finds
    /// sentences: a sentence end, a speaker change, a silence over 2 s), a cut point wherever
    /// [`cues`] cuts cleanly (punctuation or a pause of 0.4 s), the confidence of every piece below
    /// 1 and the language of every word that differs from its block's as spans. Times move to the
    /// 10 ms grid; a piece starts at least 10 ms after the one before it and a word ends at least
    /// 10 ms after its last piece starts. `id` is the file's id (the transcription id): block ids
    /// and Premiere ids are made from it.
    pub fn from_spoken(spoken: &[Spoken], id: Option<String>) -> Doc {
        // What the file can hold, so the Doc is what reads back: a control character as a space,
        // no empty piece, no word without pieces.
        let spoken: Vec<Spoken> = spoken
            .iter()
            .map(|s| Spoken {
                pieces: s
                    .pieces
                    .iter()
                    .filter(|p| !p.text.is_empty())
                    .map(|p| SpokenPiece { text: one_line(&p.text), ..p.clone() })
                    .collect(),
                end: s.end,
                speaker: s.speaker.as_deref().map(one_line),
                language: s.language.as_deref().map(one_line),
            })
            .filter(|s| !s.pieces.is_empty())
            .collect();
        let id = id.as_deref().map(one_line);
        let plain: Vec<crate::Word> = spoken
            .iter()
            .map(|s| {
                let w = crate::Word::from(s);
                crate::Word { start: grid(w.start), end: grid(w.end), ..w }
            })
            .collect();
        let lang = majority(plain.iter().filter_map(|w| w.language.as_deref()));
        let mut doc = Doc { id, lang: lang.clone(), ..Doc::default() };
        for w in &plain {
            if let Some(sp) = &w.speaker
                && !doc.speakers.iter().any(|s| &s.id == sp)
            {
                doc.speakers.push(Speaker { id: sp.clone(), name: None, other: Vec::new() });
            }
        }
        let seed = doc.id.clone().unwrap_or_default();
        let mut at = 0;
        // The format's own block rule, fixed here: not the subtitle layout's defaults.
        let blocks = Layout { gap: 2.0, by_speaker: true, ..Layout::default() };
        for sentence in cues::sentences(plain.clone(), &blocks) {
            let range = at..at + sentence.len();
            at = range.end;
            let mut b = block(&plain[range.clone()], &spoken[range], lang.as_ref());
            let mut salt = 0;
            b.id = Some(loop {
                let id = block_id(&format!("{seed}/block/{}/{salt}", b.start));
                if !doc.blocks.iter().any(|o| o.id.as_ref() == Some(&id)) {
                    break id;
                }
                salt += 1;
            });
            doc.blocks.push(b);
        }
        // Blocks are in order of start, whatever order the words came in; overlaps stay.
        doc.blocks.sort_by_key(|b| b.start);
        doc
    }

    /// A file from plain words: each word is one piece with its confidence.
    pub fn from_words(words: &[crate::Word], id: Option<String>) -> Doc {
        let spoken: Vec<Spoken> = words
            .iter()
            .map(|w| Spoken {
                pieces: vec![SpokenPiece { text: w.text.clone(), start: w.start, confidence: w.confidence }],
                end: w.end,
                speaker: w.speaker.clone(),
                language: w.language.clone(),
            })
            .collect();
        Doc::from_spoken(&spoken, id)
    }

    /// The speech as [`crate::Word`]s, for the writers: a word's language is its span's, else its
    /// block's, else the file's; its confidence the mean of its pieces'. Sound blocks are left out.
    pub fn to_words(&self) -> Vec<crate::Word> {
        let mut out = Vec::new();
        for b in self.blocks.iter().filter(|b| b.sound.is_none()) {
            for w in b.words() {
                let language = b
                    .span_option(w.start(), "lang")
                    .and_then(|v| v.first().cloned())
                    .or_else(|| b.lang.clone())
                    .or_else(|| self.lang.clone());
                let confs: Vec<f64> = w
                    .pieces
                    .iter()
                    .map(|p| b.span_option(p.start, "conf").and_then(|v| v.first()?.parse().ok()).unwrap_or(1.0))
                    .collect();
                out.push(crate::Word {
                    text: w.text(),
                    start: w.start(),
                    end: w.end,
                    speaker: b.speaker.clone(),
                    language,
                    confidence: confs.iter().sum::<f64>() / confs.len().max(1) as f64,
                    glued: false,
                    marks: Default::default(),
                });
            }
            // What the file says about each word, for the layout and the writers.
            let words: Vec<&Word> = b.words().collect();
            let first = out.len() - words.len();
            let ends: Vec<usize> = b
                .phrases
                .iter()
                .scan(0, |n, p| {
                    *n += p.len();
                    Some(*n - 1)
                })
                .collect();
            for (k, w) in words.iter().enumerate() {
                let m = &mut out[first + k].marks;
                // A cut point follows the last word of a phrase; a block ends at one too.
                m.cut = Some(ends.contains(&k));
                m.sentence = b
                    .spans
                    .iter()
                    .filter(|s| s.to == w.end && s.from <= w.start())
                    .find_map(|s| s.options.iter().find(|(key, _)| key == "sentence"))
                    .and_then(|(_, v)| match v.first().map(String::as_str) {
                        Some("end") => Some(true),
                        Some("continues") => Some(false),
                        _ => None,
                    });
                m.italic = b.span_option(w.start(), "style").is_some_and(|v| v.iter().any(|s| s == "italic"));
                m.pos = b
                    .span_option(w.start(), "pos")
                    .and_then(|v| parse_pos(v).ok())
                    .or(b.pos)
                    .filter(|p| *p != Pos::Bottom);
                m.voice =
                    b.span_option(w.start(), "voice").and_then(|v| v.first().cloned()).or_else(|| b.voice.clone());
                // Marked against the file's language: a Russian block in an English file is Russian
                // all through, an English word inside it is not marked.
                let language = out[first + k].language.clone();
                out[first + k].marks.lang = language.filter(|l| Some(l) != self.lang.as_ref());
            }
        }
        out
    }

    /// The sound blocks (`sound` is set), in order.
    pub fn sounds(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(|b| b.sound.is_some())
    }
}

/// One sentence's block: words on the grid (`plain`) with their pieces (`spoken`).
fn block(plain: &[crate::Word], spoken: &[Spoken], file_lang: Option<&String>) -> Block {
    let block_lang = majority(plain.iter().filter_map(|w| w.language.as_deref()));
    let mut b = Block {
        start: plain[0].start,
        end: plain.last().expect("a sentence has words").end,
        speaker: plain[0].speaker.clone(),
        lang: block_lang.clone().filter(|l| Some(l) != file_lang),
        ..Block::default()
    };
    // Piece starts rise strictly through the block: a tie moves by 10 ms.
    let mut prev: Option<u64> = None;
    let all: Vec<Vec<Piece>> = plain
        .iter()
        .zip(spoken)
        .map(|(w, s)| {
            s.pieces
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let t = if i == 0 { w.start } else { grid(p.start) };
                    let t = prev.map_or(t, |prev| t.max(prev.saturating_add(10)));
                    prev = Some(t);
                    Piece { text: p.text.clone(), start: t }
                })
                .collect()
        })
        .collect();
    let mut conf_spans = Vec::new();
    let mut lang_spans: Vec<Span> = Vec::new();
    let mut phrase: Vec<Word> = Vec::new();
    // Whether the word before is in the last language span: only then does a word join it.
    let mut in_span = false;
    for (k, ((w, s), pieces)) in plain.iter().zip(spoken).zip(all.iter().cloned()).enumerate() {
        if k > 0 && cues::cut_level(&plain[k - 1], w) <= 1 {
            b.phrases.push(std::mem::take(&mut phrase));
        }
        let confs: Vec<f64> = s.pieces.iter().map(|p| (p.confidence * 100.0).round() / 100.0).collect();
        let other_lang = w.language.as_ref().filter(|l| block_lang.as_ref().is_some_and(|bl| bl != *l));
        // A span is a stretch of time, so a word with a span on its last piece ends at least 10 ms
        // after that piece starts; any other word keeps its own end (Soniox gives some no length).
        // No word ends after the next one starts, so a span never covers part of the next word.
        let last = pieces.last().expect("a word has pieces").start;
        let floor =
            if other_lang.is_some() || confs.last().is_some_and(|c| *c < 1.0) { last.saturating_add(10) } else { last };
        // The next word starts at least 10 ms after this one's last piece, so `floor` always fits.
        let next = all.get(k + 1).map_or(u64::MAX, |n| n[0].start);
        let end = w.end.max(floor).min(next);
        for (i, conf) in confs.iter().enumerate() {
            if *conf < 1.0 {
                let to = pieces.get(i + 1).map_or(end, |n| n.start);
                conf_spans.push(span(pieces[i].start, to, "conf", format!("{conf:.2}")));
            }
        }
        if let Some(l) = other_lang {
            // Neighbouring words in the same other language share one span.
            match lang_spans.last_mut() {
                Some(prev) if in_span && prev.options[0].1[0] == *l => prev.to = end,
                _ => lang_spans.push(span(pieces[0].start, end, "lang", l.clone())),
            }
        }
        in_span = other_lang.is_some();
        phrase.push(Word { pieces, end });
    }
    b.phrases.push(phrase);
    b.end = b.end.max(b.words().last().map_or(b.end, |w| w.end));
    b.spans = conf_spans.into_iter().chain(lang_spans).collect();
    b
}

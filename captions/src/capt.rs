//! CAPT: a transcript as a small text file, between a recogniser and every subtitle format.
//!
//! A file is a header and blocks. A block is one source of sound over a stretch of time: a speaker
//! talking (a text line of words with `{hundredths}` marks and ` / ` cut points), or a sound
//! (`sound:phone`, no text). Span lines `{from..to} key:value` give options to part of a block's
//! text. The format is specified in `docs/capt.md` of the sonisub repository.
//!
//! - [`Doc::parse`] reads a file, [`Doc::write`] writes one; for a file this crate wrote,
//!   `Doc::parse(&doc.write()) == Ok(doc)`.
//! - [`Doc::from_spoken`] makes one from a recogniser's words with their pieces;
//!   [`Doc::from_words`] from plain [`crate::Word`]s (one piece per word).
//! - [`Doc::words`] gives the words back for the writers ([`crate::srt`], [`crate::premiere`], …).

use std::fmt;

use crate::cues::{self, Layout};

/// Options as written, in order: a key and the parts of its value (`alt:con,0.20` -> `("alt", ["con", "0.20"])`).
pub type Options = Vec<(String, Vec<String>)>;

/// A CAPT file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc {
    /// The file's id, made once and never changed; the seed of Premiere's speaker ids.
    pub id: Option<String>,
    /// The name of the work.
    pub title: Option<String>,
    /// The main language; a block's words are in it unless the block or a span says otherwise.
    pub lang: Option<String>,
    /// The source file and its length, milliseconds.
    pub media: Option<String>,
    pub duration: Option<u64>,
    /// Every speaker the writer knows, in order of appearance.
    pub speakers: Vec<Speaker>,
    /// Header options this version does not know, kept as written.
    pub other: Options,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Speaker {
    /// What blocks name it by (`speaker:1`).
    pub id: String,
    pub name: Option<String>,
}

/// Where on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pos {
    Bottom,
    Top,
    Middle,
    /// The text is centred on a point; parts of the frame from its top left corner.
    Point {
        x: f64,
        y: f64,
    },
    /// The text wraps inside a rectangle and starts at its top.
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
}

/// One source of sound over a stretch of time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    /// Milliseconds from the start of the media.
    pub start: u64,
    pub end: u64,
    pub id: Option<String>,
    pub speaker: Option<String>,
    /// A sound, not speech (`phone`, `laughter`): such a block has no text.
    pub sound: Option<String>,
    pub lang: Option<String>,
    pub pos: Option<Pos>,
    pub voice: Option<String>,
    /// Block options this version does not know, kept as written.
    pub other: Options,
    /// The text: phrases (the words between cut points), each a list of words.
    pub phrases: Vec<Vec<Word>>,
    /// Whether the text line has marks; a hand-written line may have none, and its words get times
    /// spread over the block by length.
    pub marked: bool,
    pub spans: Vec<Span>,
}

/// A word of a block's text.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub pieces: Vec<Piece>,
    /// Milliseconds from the start of the media.
    pub end: u64,
}

/// A piece of a word with the moment it starts (a token of the recogniser).
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub text: String,
    pub start: u64,
}

/// Options for the text from one moment up to another: `from` included, `to` excluded.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub from: u64,
    pub to: u64,
    pub options: Options,
}

/// What is wrong at a line of a file (1-based).
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for Error {}

/// A word as a recogniser hears it, for [`Doc::from_spoken`].
#[derive(Debug, Clone, PartialEq)]
pub struct Spoken {
    pub pieces: Vec<SpokenPiece>,
    pub end: u64,
    pub speaker: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpokenPiece {
    pub text: String,
    pub start: u64,
    /// 0 to 1.
    pub confidence: f64,
}

impl Word {
    /// The word as written: its pieces joined.
    pub fn text(&self) -> String {
        self.pieces.iter().map(|p| p.text.as_str()).collect()
    }

    pub fn start(&self) -> u64 {
        self.pieces.first().map_or(self.end, |p| p.start)
    }
}

impl Block {
    /// The words of the text, in order.
    pub fn words(&self) -> impl Iterator<Item = &Word> {
        self.phrases.iter().flatten()
    }

    /// The first value of a span option covering the piece that starts at `at`.
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

/// `HH:MM:SS.mmm` -> milliseconds.
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
    (m < 60 && sec < 60).then_some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

/// `key:value key:"a b",c` -> options. A part is bare (no space, comma or quote) or quoted, with
/// `\"` and `\\` inside quotes.
fn parse_options(s: &str) -> Result<Options, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Options = Vec::new();
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

/// Tokens -> phrases of words; pieces without a mark before them get `u64::MAX` (spread later).
fn words_of(tokens: Vec<Token>, base: u64) -> (Vec<Vec<Word>>, bool) {
    let mut phrases: Vec<Vec<Word>> = vec![Vec::new()];
    let mut word: Option<Word> = None;
    let mut last: Option<u64> = None;
    let mut marked = false;
    fn end_word(word: &mut Option<Word>, phrases: &mut [Vec<Word>], last: &mut Option<u64>) {
        if let Some(w) = word.take().filter(|w| !w.pieces.is_empty()) {
            phrases.last_mut().expect("never empty").push(w);
        }
        *last = None;
    }
    for t in tokens {
        match t {
            Token::Space => end_word(&mut word, &mut phrases, &mut last),
            Token::Cut => {
                end_word(&mut word, &mut phrases, &mut last);
                phrases.push(Vec::new());
            }
            Token::Mark(cs) => {
                marked = true;
                let at = base + cs * 10;
                last = Some(at);
                word.get_or_insert_with(|| Word { pieces: Vec::new(), end: u64::MAX }).end = at;
            }
            Token::Text(text) => {
                let w = word.get_or_insert_with(|| Word { pieces: Vec::new(), end: u64::MAX });
                w.pieces.push(Piece { text, start: last.unwrap_or(u64::MAX) });
                w.end = u64::MAX;
            }
        }
    }
    end_word(&mut word, &mut phrases, &mut last);
    phrases.retain(|p| !p.is_empty());
    (phrases, marked)
}

/// Words with no time get times spread over the block by length; a marked word keeps its own.
fn spread_times(b: &mut Block) {
    let total: usize = b.words().map(|w| w.text().chars().count() + 1).sum();
    if total == 0 {
        return;
    }
    let (start, len) = (b.start, b.end.saturating_sub(b.start));
    let n = b.words().count();
    let mut at = 0usize;
    for (k, w) in b.phrases.iter_mut().flatten().enumerate() {
        let s = start + (len as u128 * at as u128 / total as u128) as u64;
        at += w.text().chars().count() + 1;
        let e = if k + 1 == n { b.end } else { start + (len as u128 * (at - 1) as u128 / total as u128) as u64 };
        if w.end == u64::MAX {
            w.end = e;
        }
        for (i, p) in w.pieces.iter_mut().enumerate() {
            if p.start == u64::MAX {
                p.start = if i == 0 { s } else { w.end };
            }
        }
    }
}

fn check_spans(b: &Block, lines: &[(usize, Span)], errors: &mut Vec<Error>) {
    let pieces: Vec<u64> = b.words().flat_map(|w| w.pieces.iter().map(|p| p.start)).collect();
    let words: Vec<(u64, u64)> = b.words().map(|w| (w.start(), w.end)).collect();
    for (line, s) in lines {
        let rel = |ms: u64| (ms - b.start) / 10;
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
    pub fn parse(text: &str) -> Result<Doc, Vec<Error>> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
        let mut errors = Vec::new();
        let mut doc = Doc::default();
        if lines.first().map(|l| l.trim_end()) != Some("CAPT 1") {
            errors.push(Error { line: 1, message: "a CAPT file starts with the line “CAPT 1”".into() });
        }
        let mut i = 1;
        while i < lines.len() && !lines[i].contains(" --> ") {
            let line = lines[i].trim();
            i += 1;
            if line.is_empty() {
                continue;
            }
            match parse_options(line) {
                Err(message) => errors.push(Error { line: i, message }),
                Ok(opts) if opts.first().is_some_and(|(k, _)| k == "speaker") => {
                    let id = opts[0].1.join(",");
                    let name = opts.iter().find(|(k, _)| k == "name").map(|(_, v)| v.join(","));
                    doc.speakers.push(Speaker { id, name });
                }
                Ok(opts) => {
                    for (key, value) in opts {
                        let joined = value.join(",");
                        match key.as_str() {
                            "id" => doc.id = Some(joined),
                            "title" => doc.title = Some(joined),
                            "lang" => doc.lang = Some(joined),
                            "media" => doc.media = Some(joined),
                            "duration" => match parse_time(&joined) {
                                Some(ms) => doc.duration = Some(ms),
                                None => errors.push(Error {
                                    line: i,
                                    message: format!("duration is HH:MM:SS.mmm, not “{joined}”"),
                                }),
                            },
                            _ => doc.other.push((key, value)),
                        }
                    }
                }
            }
        }
        while i < lines.len() {
            if lines[i].trim().is_empty() {
                i += 1;
                continue;
            }
            let at = i + 1;
            let mut block_lines = vec![lines[i]];
            while i + 1 < lines.len() && !lines[i + 1].trim().is_empty() {
                i += 1;
                block_lines.push(lines[i]);
            }
            i += 1;
            if let Some(b) = Self::parse_block(&block_lines, at, &mut errors) {
                doc.blocks.push(b);
            }
        }
        if errors.is_empty() { Ok(doc) } else { Err(errors) }
    }

    fn parse_block(lines: &[&str], at: usize, errors: &mut Vec<Error>) -> Option<Block> {
        let head = lines[0];
        let mut it = head.splitn(4, ' ');
        let (start, arrow, end, rest) = (it.next(), it.next(), it.next(), it.next().unwrap_or(""));
        let (Some(start), Some("-->"), Some(end)) = (start.and_then(parse_time), arrow, end.and_then(parse_time))
        else {
            errors
                .push(Error {
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
        match lines.get(1).map(|l| scan(l)) {
            None => {}
            Some(Err(message)) => errors.push(Error { line: at + 1, message }),
            Some(Ok(tokens)) => {
                let (phrases, marked) = words_of(tokens, start);
                b.phrases = phrases;
                b.marked = marked;
                spread_times(&mut b);
            }
        }
        let mut spans = Vec::new();
        for (k, line) in lines.iter().enumerate().skip(2) {
            let line_no = at + k;
            let parsed = line.strip_prefix('{').and_then(|r| r.split_once('}')).and_then(|(range, opts)| {
                let (a, z) = range.split_once("..")?;
                let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
                (all_digits(a) && all_digits(z)).then(|| (a.parse::<u64>().ok(), z.parse::<u64>().ok(), opts))
            });
            let Some((Some(a), Some(z), opts)) = parsed else {
                errors.push(Error {
                    line: line_no,
                    message: "after the text a block has only spans, “{from..to} key:value”".into(),
                });
                continue;
            };
            match parse_options(opts.trim()) {
                Err(message) => errors.push(Error { line: line_no, message }),
                Ok(options) => spans.push((line_no, Span { from: start + a * 10, to: start + z * 10, options })),
            }
        }
        check_spans(&b, &spans, errors);
        b.spans = spans.into_iter().map(|(_, s)| s).collect();
        Some(b)
    }
}

// ---------- writing ----------

fn time(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

fn part(p: &str) -> String {
    if !p.is_empty() && !p.contains([' ', ',', '"']) {
        return p.to_string();
    }
    format!("\"{}\"", p.replace('\\', "\\\\").replace('"', "\\\""))
}

fn option(key: &str, parts: &[String]) -> String {
    format!("{key}:{}", parts.iter().map(|p| part(p)).collect::<Vec<_>>().join(","))
}

fn options(opts: &Options) -> String {
    opts.iter().map(|(k, v)| option(k, v)).collect::<Vec<_>>().join(" ")
}

fn number(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('e') { format!("{v:.6}") } else { s }
}

fn pos_value(p: Pos) -> String {
    match p {
        Pos::Bottom => "bottom".into(),
        Pos::Top => "top".into(),
        Pos::Middle => "middle".into(),
        Pos::Point { x, y } => format!("{},{}", number(x), number(y)),
        Pos::Rect { x, y, w, h } => format!("{},{},{},{}", number(x), number(y), number(w), number(h)),
    }
}

/// A piece of text with the four characters that would mean something else escaped.
fn escape(text: &str, whole_word: bool) -> String {
    if whole_word && text == "/" {
        return "\\/".into();
    }
    text.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}")
}

impl Doc {
    /// The file as text; a file `Doc::parse` reads back as this same `Doc`.
    pub fn write(&self) -> String {
        let mut out = String::from("CAPT 1\n");
        let mut head = |key: &str, value: Option<String>| {
            if let Some(v) = value {
                out.push_str(&option(key, &[v]));
                out.push('\n');
            }
        };
        head("id", self.id.clone());
        head("title", self.title.clone());
        head("lang", self.lang.clone());
        match (&self.media, self.duration) {
            (Some(m), Some(d)) => out.push_str(&format!(
                "{} {}\n",
                option("media", std::slice::from_ref(m)),
                option("duration", &[time(d)])
            )),
            (Some(m), None) => out.push_str(&format!("{}\n", option("media", std::slice::from_ref(m)))),
            (None, Some(d)) => out.push_str(&format!("{}\n", option("duration", &[time(d)]))),
            (None, None) => {}
        }
        for (k, v) in &self.other {
            out.push_str(&option(k, v));
            out.push('\n');
        }
        for s in &self.speakers {
            out.push_str(&option("speaker", std::slice::from_ref(&s.id)));
            if let Some(n) = &s.name {
                out.push(' ');
                out.push_str(&option("name", std::slice::from_ref(n)));
            }
            out.push('\n');
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
        let pos = self.pos.map(|p| pos_value(p).split(',').map(str::to_string).collect::<Vec<_>>());
        let one = |v: &Option<String>| v.as_ref().map(|v| vec![v.clone()]);
        let known = [
            ("id", one(&self.id)),
            ("speaker", one(&self.speaker)),
            ("sound", one(&self.sound)),
            ("lang", one(&self.lang)),
            ("pos", pos),
            ("voice", one(&self.voice)),
        ];
        let mut opts: Options = known.into_iter().filter_map(|(k, v)| Some((k.to_string(), v?))).collect();
        opts.extend(self.other.iter().cloned());
        let mut out = format!("{} --> {}", time(self.start), time(self.end));
        if !opts.is_empty() {
            out.push(' ');
            out.push_str(&options(&opts));
        }
        out.push('\n');
        if self.sound.is_some() || self.phrases.is_empty() {
            return out;
        }
        let cs = |ms: u64| (ms.saturating_sub(self.start) + 5) / 10;
        let phrases: Vec<String> = self
            .phrases
            .iter()
            .map(|p| {
                p.iter()
                    .map(|w| {
                        let whole = w.pieces.len() == 1;
                        if !self.marked {
                            return w.pieces.iter().map(|x| escape(&x.text, whole)).collect::<String>();
                        }
                        let mut s: String =
                            w.pieces.iter().map(|x| format!("{{{}}}{}", cs(x.start), escape(&x.text, whole))).collect();
                        s.push_str(&format!("{{{}}}", cs(w.end)));
                        s
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        out.push_str(&phrases.join(" / "));
        out.push('\n');
        for s in &self.spans {
            out.push_str(&format!("{{{}..{}}}", cs(s.from), cs(s.to)));
            if !s.options.is_empty() {
                out.push(' ');
                out.push_str(&options(&s.options));
            }
            out.push('\n');
        }
        out
    }
}

// ---------- from words ----------

/// Crockford base32, without `i l o u`.
const BASE32: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Six characters of an FNV-1a hash: the same transcript always gives the same ids.
fn block_id(seed: &str) -> String {
    let h = seed.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    let h = (h ^ (h >> 31)).wrapping_mul(0x94d0_49bb_1331_11eb);
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

/// Times on the 10 ms grid the marks are written on.
fn grid(ms: u64) -> u64 {
    (ms + 5) / 10 * 10
}

impl Doc {
    /// A file from a recogniser's words: one block per sentence (as [`cues`] finds sentences), a cut
    /// point wherever [`cues`] cuts cleanly (punctuation or a pause), the confidence of every piece
    /// below 1 and the language of every word that differs from its block's as spans. `id` is the
    /// file's id (the transcription id): block ids and Premiere ids are made from it.
    pub fn from_spoken(spoken: &[Spoken], id: Option<String>) -> Doc {
        let plain: Vec<crate::Word> = spoken
            .iter()
            .map(|s| crate::Word {
                text: s.pieces.iter().map(|p| p.text.as_str()).collect(),
                start: grid(s.pieces.first().map_or(s.end, |p| p.start)),
                end: grid(s.end),
                speaker: s.speaker.clone(),
                language: s.language.clone(),
                confidence: s.pieces.iter().map(|p| p.confidence).sum::<f64>() / s.pieces.len().max(1) as f64,
                glued: false,
            })
            .collect();
        let lang = majority(plain.iter().filter_map(|w| w.language.as_deref()));
        let mut doc = Doc { id, lang: lang.clone(), ..Doc::default() };
        for w in &plain {
            if let Some(sp) = &w.speaker
                && !doc.speakers.iter().any(|s| &s.id == sp)
            {
                doc.speakers.push(Speaker { id: sp.clone(), name: None });
            }
        }
        let seed = doc.id.clone().unwrap_or_default();
        let mut used: Vec<String> = Vec::new();
        let mut at = 0;
        for sentence in cues::sentences(plain.clone(), &Layout::default()) {
            let range = at..at + sentence.len();
            at = range.end;
            let ws = &plain[range.clone()];
            let block_lang = majority(ws.iter().filter_map(|w| w.language.as_deref()));
            let mut salt = 0;
            let bid = loop {
                let id = block_id(&format!("{seed}/block/{}/{salt}", ws[0].start));
                if !used.contains(&id) {
                    break id;
                }
                salt += 1;
            };
            used.push(bid.clone());
            let mut b = Block {
                start: ws[0].start,
                end: ws.last().expect("a sentence has words").end,
                id: Some(bid),
                speaker: ws[0].speaker.clone(),
                lang: block_lang.clone().filter(|l| Some(l) != lang.as_ref()),
                marked: true,
                ..Block::default()
            };
            let mut phrase: Vec<Word> = Vec::new();
            let mut prev_start = None::<u64>;
            for (k, (w, s)) in ws.iter().zip(&spoken[range]).enumerate() {
                if k > 0 && cues::cut_level(&ws[k - 1], w) <= 1 {
                    b.phrases.push(std::mem::take(&mut phrase));
                }
                let mut pieces: Vec<Piece> = Vec::new();
                for (i, p) in s.pieces.iter().enumerate() {
                    // Piece starts rise strictly within a block: a tie moves by 10 ms.
                    let mut t = if i == 0 { w.start } else { grid(p.start) };
                    if let Some(prev) = prev_start
                        && t <= prev
                    {
                        t = prev + 10;
                    }
                    prev_start = Some(t);
                    pieces.push(Piece { text: p.text.clone(), start: t });
                }
                let end = w.end.max(prev_start.unwrap_or(w.end));
                for (i, p) in s.pieces.iter().enumerate() {
                    let conf = (p.confidence * 100.0).round() / 100.0;
                    if conf < 1.0 {
                        let to = pieces.get(i + 1).map_or(end, |n| n.start);
                        b.spans.push(Span {
                            from: pieces[i].start,
                            to,
                            options: vec![("conf".into(), vec![format!("{conf:.2}")])],
                        });
                    }
                }
                if let (Some(l), Some(bl)) = (&w.language, &block_lang)
                    && l != bl
                {
                    b.spans.push(Span {
                        from: pieces[0].start,
                        to: end,
                        options: vec![("lang".into(), vec![l.clone()])],
                    });
                }
                phrase.push(Word { pieces, end });
            }
            b.phrases.push(phrase);
            b.end = b.end.max(b.words().last().map_or(b.end, |w| w.end));
            merge_lang_spans(&mut b);
            doc.blocks.push(b);
        }
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
    pub fn words(&self) -> Vec<crate::Word> {
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
                });
            }
        }
        out
    }
}

/// Neighbouring words in the same other language get one span.
fn merge_lang_spans(b: &mut Block) {
    let words: Vec<(u64, u64)> = b.words().map(|w| (w.start(), w.end)).collect();
    let mut merged: Vec<Span> = Vec::new();
    for s in std::mem::take(&mut b.spans) {
        let is_lang = s.options.first().is_some_and(|(k, _)| k == "lang");
        if is_lang
            && let Some(prev) = merged.iter_mut().rev().find(|p| p.options.first().is_some_and(|(k, _)| k == "lang"))
            && prev.options == s.options
            && words
                .iter()
                .position(|w| w.1 == prev.to)
                .is_some_and(|i| words.get(i + 1).is_some_and(|n| n.0 == s.from))
        {
            prev.to = s.to;
            continue;
        }
        merged.push(s);
    }
    // conf spans first, in order of time, then lang spans: the order a reader expects.
    merged.sort_by_key(|s| (s.options.first().is_some_and(|(k, _)| k != "conf"), s.from));
    b.spans = merged;
}

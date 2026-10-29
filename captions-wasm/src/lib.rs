//! `captions` in the browser: a CAPT file -> every output format of `captions`, and what a preview
//! of each needs, with no network and no speech provider.
//!
//! - [`parse`]: the file's summary, or every problem with its line.
//! - [`formats`]: each format with its options as `captions` takes them: types, defaults, limits.
//! - [`render`]: the file's text, as `sonisub render clip.capt -t <format>` writes it.
//! - [`layout`]: the cues (or ADR rows, or the Premiere transcript) with per-word and per-piece times.
//! - [`merge_speakers`] (`mergeSpeakers`): the file with speakers made one, as CAPT text.
//!
//! Values go in and out as plain JavaScript objects (through JSON); the TypeScript types are below.

use captions::capt::{Doc, Pos};
use captions::{Layout, adr, ass, cues, labels};
use serde_json::{Map, Value, json};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TS_TYPES: &str = r#"
/** A CAPT file's summary. Times in milliseconds. */
export interface Summary {
  id: string | null;
  title: string | null;
  /** The main language. */
  lang: string | null;
  /** The source file's name. */
  media: string | null;
  /** The header's duration, else the end of the last block. */
  duration: number;
  speakers: { id: string; name: string | null }[];
  /** Languages of the words, the file's first. */
  languages: string[];
  words: number;
  /** Sound blocks (`sound:laughter`). */
  sounds: number;
}

/** One problem of a CAPT file. `parse`, `render` and `layout` throw an `Error` with `errors`. */
export interface CaptError {
  line: number;
  message: string;
}

/** An option of a format, as `captions` takes it. */
export interface FormatOption {
  /** The key in `options`. */
  name: string;
  /** `map`: an object of strings by speaker id. `enum`: one of `values`. */
  type: "integer" | "number" | "boolean" | "string" | "string[]" | "map" | "enum";
  default: unknown;
  min?: number;
  max?: number;
  /** For `enum`: what it takes (`null`: decided by the file). */
  values?: (string | null)[];
  /** For `string`: the form it takes (`HH:MM:SS:FF`). */
  pattern?: string;
  description: string;
}

export interface Format {
  /** What `render` and `layout` take. */
  name: string;
  /** Other names `render` and `layout` take for it. */
  aliases: string[];
  /** The file's extension, without the dot (`srt`, `premiere.json`, `adr.csv`). */
  extension: string;
  contentType: string;
  description: string;
  /** What `layout` gives for it. */
  layout: "cues" | "adr" | "premiere" | "text";
  options: FormatOption[];
}

/** Option values by name; a missing or `null` value is the default. */
export type Options = Record<string, unknown>;

export interface Rendered {
  text: string;
  /** `<media name>.<extension>`, `captions.<extension>` when the file names no media. */
  fileName: string;
  contentType: string;
  /** Cues or rows written; 0 for formats without cues (transcript, Premiere, CAPT). */
  count: number;
}

/** A piece of a word (a syllable, a token): `end` is the next piece's start, the last one's the word's end. */
export interface Piece {
  text: string;
  start: number;
  end: number;
}

export interface Word {
  /** As shown, with its punctuation. */
  text: string;
  start: number;
  end: number;
  /** Written right after the word before, without a space (the part after a dash). */
  glued: boolean;
  /** The line of the cue it starts on, from 0. */
  line: number;
  italic: boolean;
  /** Its language when it differs from the file's. */
  lang: string | null;
  /** How it is said (`whisper`). */
  voice: string | null;
  /** The language it is in. */
  language: string | null;
  confidence: number;
  pieces: Piece[];
}

/** A piece of a line as shown: a word (`word` is its index in `words`), the rest of a word after a space or a dash, or the speaker label (`word` null). */
export interface Shown {
  line: number;
  /** A space comes before it on its line. */
  space: boolean;
  text: string;
  word: number | null;
}

/** Where on screen: `null` is the bottom. `x`, `y`, `w`, `h` are shares of the frame, from its top left. */
export type Position =
  | "top"
  | "middle"
  | { point: { x: number; y: number } }
  | { rect: { x: number; y: number; w: number; h: number } }
  | null;

export interface Cue {
  start: number;
  end: number;
  /** The speaker's id; `null` without speakers or with `bySpeaker` off. */
  speaker: string | null;
  /** The speaker label the first line starts with (`"Anna: "`), or "". */
  prefix: string;
  /** The lines as shown, label included, without markup. */
  lines: string[];
  position: Position;
  /** ASS only: the speaker's style. Colours `#rrggbb`; `dimmed` is a word's before it is spoken, with karaoke. */
  style?: { name: string; colour: string; dimmed: string };
  words: Word[];
  /** The lines piece by piece; `null` if they do not split back into the words. */
  shown: Shown[] | null;
}

export interface AdrRow {
  id: number;
  /** When the cue is heard, milliseconds of the media. */
  start: number;
  end: number;
  /** Timeline timecodes, `HH:MM:SS:FF`. */
  in: string;
  out: string;
  character: string;
  dialogue: string;
}

export type Layout = { speakers: { id: string; name: string | null }[] } & (
  | { kind: "cues"; cues: Cue[] }
  | { kind: "adr"; fps: string; timelineStart: string; rows: AdrRow[] }
  /** Adobe's transcript JSON, as the `.premiere.json` file holds it (times in seconds). */
  | { kind: "premiere"; transcript: any }
  | { kind: "text"; text: string }
);
"#;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Srt,
    Vtt,
    Ass,
    Premiere,
    Capt,
    Sbv,
    Lrc,
    Transcript,
    Ttml,
    Adr,
    Ytt,
}

/// A format as `formats()` describes it; extensions and names as the `sonisub` command line has them.
struct Info {
    format: Format,
    name: &'static str,
    aliases: &'static [&'static str],
    extension: &'static str,
    content_type: &'static str,
    layout: &'static str,
    description: &'static str,
}

const FORMATS: [Info; 11] = [
    Info {
        format: Format::Srt,
        name: "srt",
        aliases: &[],
        extension: "srt",
        content_type: "application/x-subrip",
        layout: "cues",
        description: "SubRip subtitles: italic as <i>, a place other than the bottom as {\\anN}.",
    },
    Info {
        format: Format::Vtt,
        name: "vtt",
        aliases: &[],
        extension: "vtt",
        content_type: "text/vtt",
        layout: "cues",
        description: "WebVTT subtitles, the same cues: italic, languages and places as cue settings.",
    },
    Info {
        format: Format::Ass,
        name: "ass",
        aliases: &[],
        extension: "ass",
        content_type: "text/x-ssa",
        layout: "cues",
        description: "ASS subtitles, the same cues, a style and a colour per speaker; karaoke lights up each word as it is spoken.",
    },
    Info {
        format: Format::Premiere,
        name: "premiere",
        aliases: &[],
        extension: "premiere.json",
        content_type: "application/json",
        layout: "premiere",
        description: "Premiere Pro transcript (Text panel > Transcript > Import Static Transcript): segments of one speaker and language, every word timed.",
    },
    Info {
        format: Format::Capt,
        name: "capt",
        aliases: &[],
        extension: "capt",
        content_type: "text/plain",
        layout: "text",
        description: "CAPT, the whole transcript as small text: every other format is made from it.",
    },
    Info {
        format: Format::Sbv,
        name: "sbv",
        aliases: &[],
        extension: "sbv",
        content_type: "text/plain",
        layout: "cues",
        description: "YouTube's SBV, the same cues as SRT, plain text.",
    },
    Info {
        format: Format::Lrc,
        name: "lrc",
        aliases: &[],
        extension: "lrc",
        content_type: "text/plain",
        layout: "cues",
        description: "Enhanced LRC, a time before every word, for karaoke players and music apps.",
    },
    Info {
        format: Format::Transcript,
        name: "transcript",
        aliases: &["text", "txt"],
        extension: "txt",
        content_type: "text/plain",
        layout: "text",
        description: "A plain text transcript: a paragraph per speaker turn with the name and the time, sounds and voices as labels.",
    },
    Info {
        format: Format::Ttml,
        name: "ttml",
        aliases: &[],
        extension: "ttml",
        content_type: "application/ttml+xml",
        layout: "cues",
        description: "TTML / IMSC 1.1 Text: speakers, places, italic, languages.",
    },
    Info {
        format: Format::Adr,
        name: "adr",
        aliases: &[],
        extension: "adr.csv",
        content_type: "text/csv",
        layout: "adr",
        description: "A DaVinci Resolve Fairlight ADR cue list: rows per speaker with timeline timecodes, sounds and voices as directions.",
    },
    Info {
        format: Format::Ytt,
        name: "ytt",
        aliases: &[],
        extension: "ytt",
        content_type: "application/xml",
        layout: "cues",
        description: "YTT, YouTube's own timed text (srv3): a time for every word, places, italic.",
    },
];

fn info(name: &str) -> Result<&'static Info, JsValue> {
    let name = name.trim().to_ascii_lowercase();
    FORMATS.iter().find(|f| f.name == name || f.aliases.contains(&name.as_str())).ok_or_else(|| {
        let names: Vec<&str> = FORMATS.iter().map(|f| f.name).collect();
        error(&format!("no format {name:?}: use one of {}", names.join(", ")))
    })
}

/// `captions::Layout`'s fields, by their names here.
const LAYOUT: [&str; 9] = [
    "maxLine",
    "maxLines",
    "maxDuration",
    "gap",
    "minDuration",
    "bySpeaker",
    "speakerLabels",
    "speakerNames",
    "wrapLines",
];

/// The options a format takes: what its `captions` writer reads, nothing more.
fn option_names(f: Format) -> Vec<&'static str> {
    match f {
        Format::Srt | Format::Vtt | Format::Sbv | Format::Lrc | Format::Ytt | Format::Ttml => LAYOUT.to_vec(),
        Format::Ass => [&LAYOUT[..], &["karaoke", "names"]].concat(),
        // `transcript::build` reads only the layout's speaker names.
        Format::Transcript => vec!["speakerNames", "labels"],
        // `adr::build` cuts by its own options; of the layout only the speaker names stay.
        Format::Adr => vec!["fps", "timelineStart", "maxGap", "maxDuration", "maxChars", "speakerNames", "labels"],
        // `premiere::build` reads the layout's gap, speaker cut and speaker names.
        Format::Premiere => vec!["bySpeaker", "gap", "speakerNames"],
        // Names given go into the file's header, as `sonisub render --speaker-names` does.
        Format::Capt => vec!["speakerNames"],
    }
}

const U32_MAX: u64 = u32::MAX as u64;

/// What an option is and its default, from `captions`' own defaults.
fn spec(f: Format, name: &str) -> Value {
    let l = Layout::default();
    let a = adr::Options::default();
    let s = ass::Options::default();
    let (kind, default, extra, description): (&str, Value, Value, &str) = match name {
        "maxLine" => (
            "integer",
            json!(l.max_line),
            json!({"min": 0, "max": U32_MAX}),
            "Max characters per line (with wrapLines); a cue holds up to maxLine × maxLines characters. 0 = no limit: one line per cue, cues are whole sentences.",
        ),
        "maxLines" => {
            ("integer", json!(l.max_lines), json!({"min": 0, "max": U32_MAX}), "Max lines per cue (0 counts as 1).")
        }
        "maxDuration" if f == Format::Adr => {
            ("number", json!(a.max_duration), json!({"min": 0}), "A cue is at most this many seconds.")
        }
        "maxDuration" => {
            ("number", json!(l.max_duration), json!({"min": 0}), "Max cue duration, seconds; 0 = no limit.")
        }
        "gap" => (
            "number",
            json!(l.gap),
            json!({"min": 0}),
            "Silence longer than this (seconds) always ends a cue; 0 = never.",
        ),
        "minDuration" => (
            "number",
            json!(l.min_duration),
            json!({"min": 0}),
            "Minimum cue duration, seconds (extended into the following silence when possible).",
        ),
        "bySpeaker" => ("boolean", json!(l.by_speaker), json!({}), "A speaker change ends a cue."),
        "speakerLabels" => (
            "boolean",
            json!(l.speaker_labels),
            json!({}),
            "Prefix \"Speaker 1: \" (or the name) when the speaker changes; only with several speakers.",
        ),
        "speakerNames" => (
            "string[]",
            json!(l.speaker_names),
            json!({}),
            "Names for speakers 1, 2, … instead of \"Speaker N\" (the file's names when empty); they go into the file's header too.",
        ),
        "wrapLines" => (
            "boolean",
            json!(l.wrap_lines),
            json!({}),
            "Break a cue's text into up to maxLines lines of maxLine characters. Off: one line per cue.",
        ),
        "karaoke" => (
            "boolean",
            json!(s.karaoke),
            json!({}),
            "Word-by-word highlight: a word is drawn dimmed until it is spoken (\\k).",
        ),
        "names" => (
            "map",
            json!(s.names),
            json!({}),
            "Names by speaker id ({\"1\": \"Anna\"}): the speakers' styles and the Name of their lines. A speaker not listed is named by speakerNames, else the file, else \"Speaker N\".",
        ),
        "labels" => {
            let mut values: Vec<Value> = vec![Value::Null];
            values.extend(labels::LANGUAGES.iter().map(|l| json!(l)));
            (
                "enum",
                Value::Null,
                json!({ "values": values }),
                "The language of sound and voice labels: en ([laughs]) or ru ((смеётся)); null: ru for a Russian file, else en.",
            )
        }
        "fps" => {
            let values: Vec<&str> = adr::Rate::ALL.iter().map(|r| r.name()).collect();
            (
                "enum",
                json!(a.rate.name()),
                json!({ "values": values }),
                "The frame rate of the Fairlight timeline (Project Settings > Master Settings > Timeline frame rate).",
            )
        }
        "timelineStart" => (
            "string",
            json!(a.start.to_string()),
            json!({"pattern": "HH:MM:SS:FF"}),
            "Where the clip starts on the timeline, non-drop frame; usually the timeline's own start (Timeline > Starting Timecode).",
        ),
        "maxGap" => {
            ("number", json!(a.max_gap), json!({"min": 0}), "A pause up to this many seconds stays inside a cue.")
        }
        "maxChars" => {
            ("integer", json!(a.max_chars), json!({"min": 0, "max": U32_MAX}), "A cue is at most this many characters.")
        }
        _ => unreachable!("an option without a spec: {name}"),
    };
    let mut out = Map::new();
    out.insert("name".into(), json!(name));
    out.insert("type".into(), json!(kind));
    out.insert("default".into(), default);
    if let Value::Object(extra) = extra {
        out.extend(extra);
    }
    out.insert("description".into(), json!(description));
    Value::Object(out)
}

/// Everything a writer takes, from the defaults and the given options.
struct Settings {
    layout: Layout,
    ass: ass::Options,
    adr: adr::Options,
    labels: Option<String>,
}

fn settings(f: Format, options: &JsValue) -> Result<Settings, JsValue> {
    let mut s = Settings { layout: Layout::default(), ass: Default::default(), adr: Default::default(), labels: None };
    let given = if options.is_undefined() || options.is_null() {
        Map::new()
    } else {
        match from_js(options)? {
            Value::Object(m) => m,
            _ => return Err(error("options must be an object")),
        }
    };
    let names = option_names(f);
    for (key, value) in given {
        if !names.contains(&key.as_str()) {
            return Err(error(&format!("{} takes no option {key:?}; it takes: {}", info_of(f).name, names.join(", "))));
        }
        if value.is_null() {
            continue;
        }
        let bad = |what: &str| error(&format!("option {key}: {what}, not {value}"));
        let int =
            || value.as_u64().filter(|n| *n <= U32_MAX).map(|n| n as usize).ok_or_else(|| bad("a whole number ≥ 0"));
        let num = || value.as_f64().filter(|n| n.is_finite() && *n >= 0.0).ok_or_else(|| bad("a number ≥ 0"));
        let flag = || value.as_bool().ok_or_else(|| bad("true or false"));
        let text = || value.as_str().ok_or_else(|| bad("a string"));
        match key.as_str() {
            "maxLine" => s.layout.max_line = int()?,
            "maxLines" => s.layout.max_lines = int()?,
            "maxDuration" if f == Format::Adr => s.adr.max_duration = num()?,
            "maxDuration" => s.layout.max_duration = num()?,
            "gap" => s.layout.gap = num()?,
            "minDuration" => s.layout.min_duration = num()?,
            "bySpeaker" => s.layout.by_speaker = flag()?,
            "speakerLabels" => s.layout.speaker_labels = flag()?,
            "wrapLines" => s.layout.wrap_lines = flag()?,
            "speakerNames" => {
                s.layout.speaker_names = value
                    .as_array()
                    .and_then(|a| a.iter().map(|n| n.as_str().map(String::from)).collect())
                    .ok_or_else(|| bad("a list of strings"))?
            }
            "karaoke" => s.ass.karaoke = flag()?,
            "names" => {
                s.ass.names = value
                    .as_object()
                    .and_then(|m| m.iter().map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
                    .ok_or_else(|| bad("an object of names by speaker id"))?
            }
            "labels" => {
                let l = text()?;
                if !labels::LANGUAGES.contains(&l) {
                    return Err(bad(&format!("one of {}", labels::LANGUAGES.join(", "))));
                }
                s.labels = Some(l.to_string());
            }
            "fps" => s.adr.rate = adr::Rate::parse(text()?).map_err(|e| error(&format!("option fps: {e}")))?,
            "timelineStart" => {
                s.adr.start = adr::Timecode::parse(text()?).map_err(|e| error(&format!("option timelineStart: {e}")))?
            }
            "maxGap" => s.adr.max_gap = num()?,
            "maxChars" => s.adr.max_chars = int()?,
            _ => unreachable!("an option without a setter: {key}"),
        }
    }
    Ok(s)
}

fn info_of(f: Format) -> &'static Info {
    FORMATS.iter().find(|i| i.format == f).expect("every format is listed")
}

/// A file read for a format, named as `sonisub render` names it: names given by the options go
/// into its header and win over its own; without them its names are the layout's.
struct Prepared {
    doc: Doc,
    s: Settings,
    labels: String,
}

fn prepare(capt: &str, f: Format, options: &JsValue) -> Result<Prepared, JsValue> {
    let mut doc = read(capt)?;
    let mut s = settings(f, options)?;
    if !s.layout.speaker_names.is_empty() || !s.ass.names.is_empty() {
        doc.name_speakers(&s.layout.speaker_names, &s.ass.names);
    }
    if s.layout.speaker_names.is_empty() {
        s.layout.speaker_names = doc.speaker_names();
    }
    s.ass.names = doc.speakers.iter().filter_map(|sp| Some((sp.id.clone(), sp.name.clone()?))).collect();
    let labels = s.labels.clone().unwrap_or_else(|| labels::for_language(doc.lang.as_deref()).into());
    Ok(Prepared { doc, s, labels })
}

fn read(capt: &str) -> Result<Doc, JsValue> {
    let text = capt.strip_prefix('\u{feff}').unwrap_or(capt);
    Doc::parse(text).map_err(|errors| {
        let list: Vec<String> = errors.0.iter().map(ToString::to_string).collect();
        let e = js_sys::Error::new(&format!("not a valid CAPT file:\n{}", list.join("\n")));
        let items: Vec<Value> = errors.0.iter().map(|e| json!({"line": e.line, "message": e.message})).collect();
        let _ = js_sys::Reflect::set(&e, &"errors".into(), &to_js(&json!(items)));
        e.into()
    })
}

fn error(message: &str) -> JsValue {
    js_sys::Error::new(message).into()
}

fn to_js(v: &Value) -> JsValue {
    js_sys::JSON::parse(&v.to_string()).expect("serde_json writes JSON")
}

fn from_js(v: &JsValue) -> Result<Value, JsValue> {
    let text: String = js_sys::JSON::stringify(v).map_err(|_| error("options must be JSON"))?.into();
    serde_json::from_str(&text).map_err(|e| error(&format!("options: {e}")))
}

/// The file's summary, or an `Error` with `errors` (`CaptError[]`): every problem with its line.
#[wasm_bindgen(unchecked_return_type = "Summary")]
pub fn parse(capt: &str) -> Result<JsValue, JsValue> {
    let doc = read(capt)?;
    let words = doc.to_words();
    let mut languages: Vec<String> = doc.lang.iter().cloned().collect();
    for l in words.iter().filter_map(|w| w.language.as_ref()) {
        if !languages.contains(l) {
            languages.push(l.clone());
        }
    }
    let duration = doc.duration.unwrap_or_else(|| doc.blocks.iter().map(|b| b.end).max().unwrap_or(0));
    Ok(to_js(&json!({
        "id": doc.id,
        "title": doc.title,
        "lang": doc.lang,
        "media": doc.media,
        "duration": duration,
        "speakers": speakers(&doc),
        "languages": languages,
        "words": words.len(),
        "sounds": doc.sounds().count(),
    })))
}

/// The file with speakers made one, as CAPT text: `{3: 1}` makes every block and header line of
/// speaker 3 speaker 1 (`captions::capt::Doc::merge_speakers`). Chains resolve (`{3: 2, 2: 1}`), the
/// other speakers keep their numbers, the merged speaker keeps its name, else takes the source's.
/// Throws for a broken file (with `errors`, as `parse`), a speaker mapped onto itself, or a number
/// the file has no speaker for.
#[wasm_bindgen(js_name = mergeSpeakers)]
pub fn merge_speakers(
    capt: &str,
    #[wasm_bindgen(unchecked_param_type = "Record<number, number>")] map: JsValue,
) -> Result<String, JsValue> {
    let mut doc = read(capt)?;
    let entries = if map.is_object() { from_js(&map)? } else { Value::Null };
    let Value::Object(entries) = entries else {
        return Err(error("the map must be an object of speaker numbers: { 3: 1 }"));
    };
    let mut pairs = std::collections::BTreeMap::new();
    for (from, to) in entries {
        let to = to.as_u64().and_then(|n| u32::try_from(n).ok());
        match (from.parse::<u32>(), to) {
            (Ok(from), Some(to)) => pairs.insert(from, to),
            _ => return Err(error(&format!("speaker {from}: speakers are numbers (`{{ 3: 1 }}`)"))),
        };
    }
    doc.merge_speakers(&pairs).map_err(|e| error(&e))?;
    Ok(doc.write())
}

fn speakers(doc: &Doc) -> Value {
    json!(doc.speakers.iter().map(|s| json!({"id": s.id, "name": s.name})).collect::<Vec<_>>())
}

/// Every output format with the options `captions` takes for it: types, defaults, limits.
#[wasm_bindgen(unchecked_return_type = "Format[]")]
pub fn formats() -> JsValue {
    let list: Vec<Value> = FORMATS
        .iter()
        .map(|i| {
            json!({
                "name": i.name,
                "aliases": i.aliases,
                "extension": i.extension,
                "contentType": i.content_type,
                "description": i.description,
                "layout": i.layout,
                "options": option_names(i.format).iter().map(|n| spec(i.format, n)).collect::<Vec<_>>(),
            })
        })
        .collect();
    to_js(&json!(list))
}

/// The file in `format` with `options` (missing ones are the defaults), as
/// `sonisub render clip.capt -t <format>` writes it, and a name for it.
#[wasm_bindgen(unchecked_return_type = "Rendered")]
pub fn render(
    capt: &str,
    format: &str,
    #[wasm_bindgen(unchecked_param_type = "Options")] options: Option<JsValue>,
) -> Result<JsValue, JsValue> {
    let i = info(format)?;
    let p = prepare(capt, i.format, &options.unwrap_or(JsValue::UNDEFINED))?;
    let (text, count) = write(&p, i.format)?;
    let base = p
        .doc
        .media
        .as_deref()
        .map(|m| m.rsplit(['/', '\\']).next().unwrap_or(m))
        .map(|m| m.rsplit_once('.').map_or(m, |(stem, _)| stem))
        .filter(|m| !m.trim().is_empty())
        .unwrap_or("captions");
    Ok(to_js(&json!({
        "text": text,
        "fileName": format!("{base}.{}", i.extension),
        "contentType": i.content_type,
        "count": count,
    })))
}

/// What `sonisub render` writes (`job::write_outputs`), with its errors for nothing to write.
fn write(p: &Prepared, f: Format) -> Result<(String, usize), JsValue> {
    let (doc, l) = (&p.doc, &p.s.layout);
    let words = doc.to_words();
    let sounds = doc.sounds().next().is_some();
    if words.is_empty() && !(f == Format::Transcript && sounds) {
        return Err(error("the file has no words"));
    }
    let (text, n) = match f {
        Format::Srt => captions::srt::build(&words, l),
        Format::Vtt => captions::vtt::build(&words, l),
        Format::Ass => ass::build(&words, l, &p.s.ass),
        Format::Sbv => captions::sbv::build(&words, l),
        Format::Lrc => captions::lrc::build(&words, l),
        Format::Ytt => captions::ytt::build(&words, l),
        Format::Transcript => (captions::transcript::build(doc, l, &p.labels), 0),
        Format::Ttml => captions::ttml::build(doc, l),
        Format::Adr => adr::build(doc, l, &p.s.adr, &p.labels).map_err(|e| error(&e))?,
        Format::Premiere => {
            // The command line seeds Premiere's ids with the file's id, else its file name.
            let seed = doc.id.clone().unwrap_or_else(|| "captions.capt".into());
            let t = captions::premiere::build(&words, l, doc.lang.as_deref(), &seed)
                .ok_or_else(|| error("no words to write"))?;
            (serde_json::to_string_pretty(&t).expect("JSON"), 0)
        }
        Format::Capt => (doc.write(), 0),
    };
    let has_cues = !matches!(f, Format::Premiere | Format::Capt | Format::Transcript);
    if n == 0 && has_cues {
        let why = if f == Format::Adr { "no speech and no sound of a speaker for an ADR cue list" } else { "no words" };
        return Err(error(&format!("nothing to write: {why}")));
    }
    Ok((text, n))
}

fn position(pos: Option<Pos>) -> Value {
    match pos {
        None | Some(Pos::Bottom) => Value::Null,
        Some(Pos::Top) => json!("top"),
        Some(Pos::Middle) => json!("middle"),
        Some(Pos::Point { x, y }) => json!({"point": {"x": x, "y": y}}),
        Some(Pos::Rect { x, y, w, h }) => json!({"rect": {"x": x, "y": y, "w": w, "h": h}}),
    }
}

fn rgb(c: u32) -> String {
    format!("#{c:06x}")
}

/// What a preview of `format` needs, laid out with `options` as `render` lays it out: the cues with
/// every word and piece timed and placed (subtitle formats), the ADR rows, Premiere's transcript,
/// or the text (transcript, CAPT).
#[wasm_bindgen(unchecked_return_type = "Layout")]
pub fn layout(
    capt: &str,
    format: &str,
    #[wasm_bindgen(unchecked_param_type = "Options")] options: Option<JsValue>,
) -> Result<JsValue, JsValue> {
    let i = info(format)?;
    let p = prepare(capt, i.format, &options.unwrap_or(JsValue::UNDEFINED))?;
    let mut out = match i.layout {
        "cues" => cue_layout(&p, i.format)?,
        "adr" => {
            let rows = adr::rows(&p.doc, &p.s.layout, &p.s.adr, &p.labels).map_err(|e| error(&e))?;
            let rows: Vec<Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "id": r.id, "start": r.start, "end": r.end,
                        "in": r.tc_in.to_string(), "out": r.tc_out.to_string(),
                        "character": r.character, "dialogue": r.dialogue,
                    })
                })
                .collect();
            json!({"kind": "adr", "fps": p.s.adr.rate.name(), "timelineStart": p.s.adr.start.to_string(), "rows": rows})
        }
        "premiere" => {
            let (text, _) = write(&p, i.format)?;
            json!({"kind": "premiere", "transcript": serde_json::from_str::<Value>(&text).expect("JSON")})
        }
        _ => json!({"kind": "text", "text": write(&p, i.format)?.0}),
    };
    out["speakers"] = speakers(&p.doc);
    Ok(to_js(&out))
}

fn cue_layout(p: &Prepared, f: Format) -> Result<Value, JsValue> {
    let words = p.doc.to_words();
    if words.is_empty() {
        return Err(error("the file has no words"));
    }
    let list = cues::cues(&words, &p.s.layout);
    let pieces = p.doc.pieces(&list).ok_or_else(|| error("the cues do not match the file's words"))?;
    let styles = (f == Format::Ass).then(|| ass::styles(&list, &p.s.layout, &p.s.ass));
    let cues: Vec<Value> = list
        .iter()
        .zip(pieces)
        .map(|(c, pieces)| {
            let shown = c.shown();
            let words: Vec<Value> = c
                .words
                .iter()
                .zip(pieces)
                .enumerate()
                .map(|(k, (w, pieces))| {
                    let line = shown.as_ref().and_then(|s| s.iter().find(|s| s.word == Some(k))).map_or(0, |s| s.line);
                    let pieces: Vec<Value> = pieces
                        .iter()
                        .enumerate()
                        .map(|(j, piece)| {
                            let end = pieces.get(j + 1).map_or(w.end, |n| n.start);
                            json!({"text": piece.text, "start": piece.start, "end": end})
                        })
                        .collect();
                    json!({
                        "text": w.text, "start": w.start, "end": w.end, "glued": w.glued, "line": line,
                        "italic": w.marks.italic, "lang": w.marks.lang, "voice": w.marks.voice,
                        "language": w.language, "confidence": w.confidence, "pieces": pieces,
                    })
                })
                .collect();
            let mut cue = json!({
                "start": c.start, "end": c.end, "speaker": c.speaker, "prefix": c.prefix, "lines": c.lines,
                "position": position(c.words.first().and_then(|w| w.marks.pos)),
                "words": words,
                "shown": shown.map(|s| s.iter().map(|s| json!({
                    "line": s.line, "space": s.space, "text": s.text, "word": s.word,
                })).collect::<Vec<_>>()),
            });
            if let Some(styles) = &styles {
                let s = c
                    .speaker
                    .as_deref()
                    .and_then(|sp| styles.iter().find(|s| s.speaker.as_deref() == Some(sp)))
                    .unwrap_or(&styles[0]);
                cue["style"] = json!({"name": s.name, "colour": rgb(s.colour), "dimmed": rgb(s.dimmed)});
            }
            cue
        })
        .collect();
    Ok(json!({"kind": "cues", "cues": cues}))
}

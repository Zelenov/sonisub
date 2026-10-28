//! Words -> Premiere Pro transcript JSON (Adobe's "Import Static Transcript" format).
//!
//! Spec: `transcript_format_spec.json` in github.com/AdobeDocs/uxp-premiere-pro-samples
//! (sample-panels/premiere-api/assets). Word-level timing in seconds; words grouped into segments of
//! one speaker and one language; speakers referenced by UUID. Every field is required and no other
//! field is allowed.

use serde_json::{Value, json};

use crate::Word;
use crate::cues::{self, Layout};

/// Language code (as Soniox gives it) -> Premiere Pro language code. Premiere knows only these.
const LANGUAGES: &[(&str, &str)] = &[
    ("en", "en-us"),
    ("zh", "cmn-hans"),
    ("es", "es-es"),
    ("de", "de-de"),
    ("fr", "fr-fr"),
    ("ja", "ja-jp"),
    ("pt", "pt-br"),
    ("ko", "ko-kr"),
    ("it", "it-it"),
    ("ru", "ru-ru"),
    ("hi", "hi-in"),
    ("no", "nb-no"),
    ("nb", "nb-no"),
    ("sv", "sv-se"),
    ("nl", "nl-nl"),
    ("da", "da-dk"),
    ("id", "id-id"),
    ("th", "th-th"),
    ("vi", "vi-vn"),
    ("ms", "ms-my"),
    ("tr", "tr-tr"),
    ("pl", "pl-pl"),
    ("tl", "fil-ph"),
    ("te", "te-in"),
    ("ml", "ml-in"),
    ("pa", "pa-in"),
];

const UNKNOWN_LANGUAGE: &str = "??-??";

/// Words Premiere may tag as fillers (and offer to remove).
const FILLERS: &[&str] = &["um", "umm", "uh", "uhm", "hmm", "mm", "er", "erm", "ah", "эм", "ээ", "э", "мм", "хм"];

fn premiere_language(soniox: Option<&str>) -> &'static str {
    soniox.and_then(|l| LANGUAGES.iter().find(|(s, _)| s.eq_ignore_ascii_case(l))).map_or(UNKNOWN_LANGUAGE, |(_, p)| p)
}

/// The language most words are in; words without one count as `fallback`.
fn majority<'a>(words: impl Iterator<Item = &'a Word>, fallback: Option<&str>) -> &'static str {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for w in words {
        let l = premiere_language(w.language.as_deref().or(fallback));
        match counts.iter_mut().find(|(c, _)| *c == l) {
            Some((_, n)) => *n += 1,
            None => counts.push((l, 1)),
        }
    }
    // First seen wins a tie; a known language beats "??-??".
    counts.iter().rev().max_by_key(|(l, n)| (*n, *l != UNKNOWN_LANGUAGE)).map_or(UNKNOWN_LANGUAGE, |(l, _)| l)
}

fn secs(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

/// UUID v4 layout derived from `seed`, so the same words always give the same file.
fn uuid(seed: &str) -> String {
    let (a, b) = (crate::hash64(seed, 0), crate::hash64(seed, 0x9e37_79b9_7f4a_7c15));
    let a = (a & !0xf000) | 0x4000; // version 4
    let b = (b & !(0b11 << 62)) | (0b10 << 62); // RFC 4122 variant
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0xffff,
        b >> 48,
        b & 0xffff_ffff_ffff
    )
}

fn word_json(w: &Word, eos: bool) -> Value {
    let bare: String = w.text.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let kind = if bare.is_empty() { "punctuation" } else { "word" };
    let tags: Vec<&str> = if FILLERS.contains(&bare.as_str()) { vec!["filler"] } else { vec![] };
    json!({
        "confidence": (w.confidence.clamp(0.0, 1.0) * 1000.0).round() / 1000.0,
        "duration": secs(w.end.saturating_sub(w.start)),
        "eos": eos,
        "start": secs(w.start),
        "tags": tags,
        "text": w.text,
        "type": kind,
    })
}

/// Builds a Premiere Pro transcript from words; `None` if there are none.
///
/// A segment is one speaker and one language; a new one starts at a speaker or language change and
/// at silence longer than `layout.gap`. `eos` marks where a subtitle sentence would end (sentence end,
/// long pause, speaker change). Speakers are named like the
/// subtitle labels: `layout.speaker_names`, else "Speaker N". Words Soniox gave no language (language
/// identification off) are taken to be in `default_language` ("en"), e.g. the first language hint.
/// Speaker ids are derived from `seed` (e.g. the transcription's id): the same seed and words give
/// the same file.
pub fn build(words: &[Word], layout: &Layout, default_language: Option<&str>, seed: &str) -> Option<Value> {
    if words.is_empty() {
        return None;
    }
    let speaker_key = |w: &Word| if layout.by_speaker { w.speaker.clone().unwrap_or_default() } else { String::new() };

    // Speakers in order of appearance.
    let mut speakers: Vec<(String, String)> = Vec::new();
    for w in words {
        let key = speaker_key(w);
        if !speakers.iter().any(|(k, _)| *k == key) {
            speakers.push((key.clone(), uuid(&format!("{seed}/speaker/{key}"))));
        }
    }
    let root_language = majority(words.iter(), default_language);

    let gap_ms = (layout.gap * 1000.0) as u64;
    let cut = Layout { gap: layout.gap, by_speaker: layout.by_speaker, ..Layout::unlimited() };
    let mut segments: Vec<(String, &'static str, Vec<Value>, u64, u64)> = Vec::new();
    for sentence in cues::sentences(words.to_vec(), &cut) {
        let speaker = speaker_key(&sentence[0]);
        let language = majority(sentence.iter(), default_language);
        let (start, end) = (sentence[0].start, sentence.last().expect("non-empty").end);
        let n = sentence.len();
        let items = sentence.iter().enumerate().map(|(i, w)| word_json(w, i + 1 == n));
        match segments.last_mut() {
            Some((sp, lang, ws, _, e))
                if *sp == speaker && *lang == language && (gap_ms == 0 || start.saturating_sub(*e) <= gap_ms) =>
            {
                ws.extend(items);
                *e = end;
            }
            _ => segments.push((speaker, language, items.collect(), start, end)),
        }
    }

    let id_of = |key: &str| speakers.iter().find(|(k, _)| k == key).map(|(_, id)| id.clone()).unwrap_or_default();
    let segments: Vec<Value> = segments
        .into_iter()
        .map(|(speaker, language, words, start, end)| {
            json!({
                "duration": secs(end.saturating_sub(start)),
                "language": language,
                "speaker": id_of(&speaker),
                "start": secs(start),
                "words": words,
            })
        })
        .collect();
    let speakers: Vec<Value> = speakers
        .iter()
        .enumerate()
        .map(|(i, (key, id))| {
            let n = if key.is_empty() { (i + 1).to_string() } else { key.clone() };
            json!({ "id": id, "name": cues::speaker_label(&n, &layout.speaker_names) })
        })
        .collect();
    Some(json!({ "language": root_language, "segments": segments, "speakers": speakers }))
}

/// Subtitles (SRT text and the number of cues) from a Premiere Pro transcript, laid out by `layout`
/// like the SRT made from the same speech: `None` when `source` is not a Premiere transcript or has
/// no words. Speakers are named as the file names them (a speaker without a name is "Speaker N");
/// `layout.speaker_names` is not used. A segment whose speaker the file does not list is read without
/// a speaker (never labelled). Premiere's own sentence ends (`eos`) are not used either:
/// the layout decides where a cue ends. Tested on transcripts sonisub wrote; others are read on a
/// best-effort basis.
pub fn to_srt(source: &str, layout: &Layout) -> Option<(String, usize)> {
    let transcript: Value = serde_json::from_str(source.strip_prefix('\u{feff}').unwrap_or(source)).ok()?;
    let speakers = transcript["speakers"].as_array()?;
    let segments = transcript["segments"].as_array()?;
    // Speaker ids in order, as the numbers `srt` labels by.
    let ids: Vec<&str> = speakers.iter().map(|s| s["id"].as_str().unwrap_or_default()).collect();
    let layout = Layout {
        speaker_names: speakers.iter().map(|s| s["name"].as_str().unwrap_or_default().to_string()).collect(),
        ..layout.clone()
    };

    let mut words: Vec<Word> = Vec::new();
    for segment in segments {
        let speaker =
            segment["speaker"].as_str().and_then(|id| ids.iter().position(|i| *i == id)).map(|n| (n + 1).to_string());
        let language = segment["language"].as_str().map(str::to_string);
        for w in segment["words"].as_array().into_iter().flatten() {
            let (Some(text), Some(start)) = (w["text"].as_str(), w["start"].as_f64()) else { continue };
            let text = text.trim();
            if text.is_empty() || !(0.0..=MAX_SECONDS).contains(&start) {
                continue;
            }
            let start_ms = ms(start);
            let duration = w["duration"].as_f64().filter(|d| d.is_finite() && *d >= 0.0).unwrap_or(0.0);
            let end_ms = start_ms + ms(duration.min(MAX_WORD_SECONDS));
            match words.last_mut() {
                // Marks that belong to the word before them (even across a speaker change: a mark
                // never starts a cue) are written right after it. Dashes, `+`, `&` and emoji are
                // words of their own, as Soniox has them.
                Some(prev) if text.chars().all(|c| ATTACHED.contains(c)) => {
                    prev.text.push_str(text);
                    // A mark after a pause (the first word of another speaker's segment) does not
                    // stretch the word it joins over the silence.
                    if start_ms <= prev.end + 500 {
                        prev.end = prev.end.max(end_ms);
                    }
                }
                _ => words.push(Word {
                    text: text.to_string(),
                    start: start_ms,
                    end: end_ms,
                    speaker: speaker.clone(),
                    language: language.clone(),
                    confidence: w["confidence"].as_f64().unwrap_or(1.0),
                    glued: false,
                    marks: Default::default(),
                }),
            }
        }
    }
    if words.is_empty() {
        return None;
    }
    words.sort_by_key(|w| w.start);
    Some(crate::srt::build(&words, &layout))
}

/// Words later than this (100 hours) are junk, not speech.
const MAX_SECONDS: f64 = 360_000.0;

/// A word is never longer than this; a junk duration must not stretch a cue over hours.
const MAX_WORD_SECONDS: f64 = 60.0;

/// Marks that are written right after the word before them.
const ATTACHED: &str = ".,!?;:…)]}»”’%";

fn ms(seconds: f64) -> u64 {
    (seconds * 1000.0).round().min(MAX_SECONDS * 1000.0) as u64
}

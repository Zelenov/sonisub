//! Soniox tokens -> Premiere Pro transcript JSON (Adobe's "Import Static Transcript" format).
//!
//! Spec: `transcript_format_spec.json` in github.com/AdobeDocs/uxp-premiere-pro-samples
//! (sample-panels/premiere-api/assets). Word-level timing in seconds; words grouped into segments of
//! one speaker and one language; speakers referenced by UUID. Every field is required and no other
//! field is allowed.

use serde_json::{Value, json};

use crate::srt::{self, Layout, Word};

/// Soniox language code -> Premiere Pro language code. Premiere knows only these.
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

/// UUID v4 layout derived from `seed`, so the same transcript always gives the same file.
fn uuid(seed: &str) -> String {
    let hash = |salt: u64| {
        let h = seed.bytes().fold(0xcbf2_9ce4_8422_2325 ^ salt, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
        // splitmix64 finaliser: similar seeds ("…/speaker/1", "…/speaker/2") give unrelated ids.
        let h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        let h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^ (h >> 31)
    };
    let (a, b) = (hash(0), hash(0x9e37_79b9_7f4a_7c15));
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

/// Builds a Premiere Pro transcript from a Soniox transcript; `None` if there are no words.
///
/// A segment is one speaker and one language; a new one starts at a speaker or language change and
/// at silence longer than `layout.gap`. `eos` marks where a subtitle sentence would end (sentence end,
/// long pause, speaker change). Speakers are named like the
/// subtitle labels: `layout.speaker_names`, else "Speaker N". Words Soniox gave no language (language
/// identification off) are taken to be in `default_language` ("en"), e.g. the first language hint.
pub fn build(transcript: &Value, layout: &Layout, default_language: Option<&str>) -> Option<Value> {
    let words = srt::words(transcript);
    if words.is_empty() {
        return None;
    }
    let seed = transcript["id"].as_str().unwrap_or_default().to_string();
    let speaker_key = |w: &Word| if layout.by_speaker { w.speaker.clone().unwrap_or_default() } else { String::new() };

    // Speakers in order of appearance.
    let mut speakers: Vec<(String, String)> = Vec::new();
    for w in &words {
        let key = speaker_key(w);
        if !speakers.iter().any(|(k, _)| *k == key) {
            speakers.push((key.clone(), uuid(&format!("{seed}/speaker/{key}"))));
        }
    }
    let root_language = majority(words.iter(), default_language);

    let gap_ms = (layout.gap * 1000.0) as u64;
    let cut = Layout { gap: layout.gap, by_speaker: layout.by_speaker, ..Layout::unlimited() };
    let mut segments: Vec<(String, &'static str, Vec<Value>, u64, u64)> = Vec::new();
    for sentence in srt::sentences(words, &cut) {
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
            json!({ "id": id, "name": srt::speaker_label(&n, &layout.speaker_names) })
        })
        .collect();
    Some(json!({ "language": root_language, "segments": segments, "speakers": speakers }))
}

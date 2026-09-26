//! Premiere Pro transcript: expected output for the fixtures, and the rules of Adobe's schema
//! (github.com/AdobeDocs/uxp-premiere-pro-samples, transcript_format_spec.json).

mod common;

use serde_json::{Value, json};
use sonisub::premiere::build;
use sonisub::srt::Layout;

const LANGUAGES: &[&str] = &[
    "en-us", "en-gb", "zh-hk", "cmn-hans", "cmn-hant", "es-es", "de-de", "fr-fr", "ja-jp", "pt-pt", "pt-br", "ko-kr",
    "it-it", "ru-ru", "hi-in", "nb-no", "sv-se", "nl-nl", "da-dk", "id-id", "th-th", "vi-vn", "ms-my", "tr-tr", "pl-pl",
    "fil-ph", "te-in", "ml-in", "pa-in", "??-??",
];

fn keys(v: &Value) -> Vec<&str> {
    v.as_object().unwrap().keys().map(String::as_str).collect()
}

fn is_uuid_v4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12])
        && s.chars().all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && parts[2].starts_with('4')
        && parts[3].starts_with(['8', '9', 'a', 'b'])
}

/// Exactly the fields the schema requires (it allows no others), valid values, words in time order.
fn assert_valid(t: &Value) {
    assert_eq!(keys(t), ["language", "segments", "speakers"]);
    assert!(LANGUAGES.contains(&t["language"].as_str().unwrap()));
    let speakers = t["speakers"].as_array().unwrap();
    assert!(!speakers.is_empty());
    for s in speakers {
        assert_eq!(keys(s), ["id", "name"]);
        assert!(is_uuid_v4(s["id"].as_str().unwrap()), "{s}");
        assert!(!s["name"].as_str().unwrap().is_empty());
    }
    let segments = t["segments"].as_array().unwrap();
    assert!(!segments.is_empty());
    let mut last = 0.0;
    for seg in segments {
        assert_eq!(keys(seg), ["duration", "language", "speaker", "start", "words"]);
        assert!(LANGUAGES.contains(&seg["language"].as_str().unwrap()));
        assert!(speakers.iter().any(|s| s["id"] == seg["speaker"]), "unknown speaker in {seg}");
        let words = seg["words"].as_array().unwrap();
        assert!(!words.is_empty());
        assert_eq!(seg["start"], words[0]["start"]);
        for w in words {
            assert_eq!(keys(w), ["confidence", "duration", "eos", "start", "tags", "text", "type"]);
            let c = w["confidence"].as_f64().unwrap();
            assert!((0.0..=1.0).contains(&c));
            assert!(w["duration"].as_f64().unwrap() >= 0.0);
            let start = w["start"].as_f64().unwrap();
            assert!(start >= last, "words out of order at {w}");
            last = start;
            assert!(["word", "punctuation"].contains(&w["type"].as_str().unwrap()));
            assert!(w["tags"].as_array().unwrap().iter().all(|t| t == "filler" || t == "profanity"));
        }
    }
}

fn text(t: &Value) -> Vec<String> {
    t["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["words"].as_array().unwrap().iter().map(|w| w["text"].as_str().unwrap()).collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn fixtures_match_expected_and_follow_the_schema() {
    for name in ["dialog", "punctuation"] {
        let t = build(&common::transcript_of(name), &Layout::default(), Some("en")).unwrap();
        assert_valid(&t);
        assert_eq!(t, common::golden_json(&format!("{name}.premiere.json")), "{name}");
    }
}

#[test]
fn a_segment_is_one_speaker_and_one_language() {
    let t = build(&common::transcript(), &Layout::default(), None).unwrap();
    assert_eq!(t["language"], "en-us");
    let langs: Vec<&str> = t["segments"].as_array().unwrap().iter().map(|s| s["language"].as_str().unwrap()).collect();
    assert_eq!(langs, ["en-us", "en-us", "en-us", "ru-ru", "es-es", "en-us", "en-us"]);
    assert_eq!(text(&t)[3], "А теперь короткая фраза по-русски.");
    assert_eq!(t["speakers"].as_array().unwrap().len(), 4);
}

#[test]
fn eos_ends_every_sentence_and_dashes_stay_inside_words() {
    let t = build(&common::transcript_of("punctuation"), &Layout::default(), Some("en")).unwrap();
    let first = &t["segments"][0]["words"];
    let eos: Vec<&str> = first.as_array().unwrap().iter().filter(|w| w["eos"] == true).map(|w| w["text"].as_str().unwrap()).collect();
    assert_eq!(eos, ["subtitles."]);
    assert!(text(&t)[0].contains("finally—and this is the tricky part—we"));
    // "Dr." is not a sentence end.
    assert!(t["segments"].as_array().unwrap().iter().flat_map(|s| s["words"].as_array().unwrap()).any(|w| w["text"] == "Dr." && w["eos"] == false));
}

#[test]
fn speaker_names_and_no_diarization() {
    let named = Layout { speaker_names: vec!["Eugene".into(), "Sasha".into()], ..Layout::default() };
    let t = build(&common::transcript(), &named, None).unwrap();
    let names: Vec<&str> = t["speakers"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Eugene", "Sasha", "Speaker 3", "Speaker 4"]);

    let one = build(&common::transcript(), &Layout { by_speaker: false, ..Layout::default() }, None).unwrap();
    assert_eq!(one["speakers"].as_array().unwrap().len(), 1);
    assert_valid(&one);
}

#[test]
fn language_without_identification_comes_from_the_hint() {
    let p = common::transcript_of("punctuation");
    assert_eq!(build(&p, &Layout::default(), Some("ru")).unwrap()["language"], "ru-ru");
    assert_eq!(build(&p, &Layout::default(), None).unwrap()["language"], "??-??");
    assert_eq!(build(&p, &Layout::default(), Some("xx")).unwrap()["language"], "??-??");
}

#[test]
fn fillers_are_tagged_and_bare_punctuation_is_typed() {
    let tok = |text: &str, start: u64| json!({"text": text, "start_ms": start, "end_ms": start + 100, "confidence": 0.9, "language": "en", "speaker": "1"});
    let t = json!({"tokens": [tok("Um,", 0), tok(" so", 200), tok(" —", 400), tok(" yes.", 600)]});
    let t = build(&t, &Layout::default(), None).unwrap();
    let words = t["segments"][0]["words"].as_array().unwrap();
    assert_eq!(words[0]["tags"], json!(["filler"]));
    assert_eq!(words[1]["tags"], json!([]));
    assert_eq!(words[2]["type"], "punctuation");
    assert_eq!(words[3]["type"], "word");
    assert_valid(&t);
}

#[test]
fn nothing_for_a_transcript_without_words() {
    assert!(build(&common::transcript_of("nospeech"), &Layout::default(), Some("en")).is_none());
}

#[test]
fn same_transcript_same_ids() {
    let a = build(&common::transcript(), &Layout::default(), None).unwrap();
    let b = build(&common::transcript(), &Layout::default(), None).unwrap();
    assert_eq!(a["speakers"], b["speakers"]);
    let other = build(&common::transcript_of("punctuation"), &Layout::default(), None).unwrap();
    assert_ne!(a["speakers"][0]["id"], other["speakers"][0]["id"]);
}

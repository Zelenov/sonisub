//! Premiere Pro transcript: expected output for the fixtures, and the rules of Adobe's schema
//! (github.com/AdobeDocs/uxp-premiere-pro-samples, transcript_format_spec.json).

mod common;

use serde_json::{Value, json};
use sonisub::premiere::{build, to_srt};
use sonisub::srt::Layout;

const LANGUAGES: &[&str] = &[
    "en-us", "en-gb", "zh-hk", "cmn-hans", "cmn-hant", "es-es", "de-de", "fr-fr", "ja-jp", "pt-pt", "pt-br", "ko-kr",
    "it-it", "ru-ru", "hi-in", "nb-no", "sv-se", "nl-nl", "da-dk", "id-id", "th-th", "vi-vn", "ms-my", "tr-tr",
    "pl-pl", "fil-ph", "te-in", "ml-in", "pa-in", "??-??",
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
        .map(|s| {
            s["words"].as_array().unwrap().iter().map(|w| w["text"].as_str().unwrap()).collect::<Vec<_>>().join(" ")
        })
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
    let eos: Vec<&str> =
        first.as_array().unwrap().iter().filter(|w| w["eos"] == true).map(|w| w["text"].as_str().unwrap()).collect();
    assert_eq!(eos, ["subtitles."]);
    assert!(text(&t)[0].contains("finally—and this is the tricky part—we"));
    // "Dr." is not a sentence end.
    assert!(
        t["segments"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["words"].as_array().unwrap())
            .any(|w| w["text"] == "Dr." && w["eos"] == false)
    );
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

// ------------------------------------------------------------ reading a transcript back

fn written(t: &Value) -> String {
    serde_json::to_string_pretty(t).unwrap()
}

/// The subtitles of a transcript that sonisub wrote are the ones sonisub writes as an `.srt`
/// from the same speech, for both layouts (the "short line" and the "whole sentence" setting).
#[test]
fn a_written_transcript_reads_back_as_the_srt_made_from_the_same_speech() {
    for name in ["dialog", "punctuation"] {
        let soniox = common::transcript_of(name);
        let premiere = written(&build(&soniox, &Layout::default(), Some("en")).unwrap());
        for layout in [
            Layout::default(),
            Layout::unlimited(),
            Layout { speaker_labels: true, ..Layout::default() },
            Layout { by_speaker: false, ..Layout::default() },
            Layout { wrap_lines: true, ..Layout::default() },
            Layout { gap: 0.0, ..Layout::default() },
        ] {
            let (expected, cues) = sonisub::srt::build(&soniox, &layout);
            let (srt, n) = to_srt(&premiere, &layout).expect("a transcript");
            assert_eq!((srt, n), (expected, cues), "{name} {layout:?}");
        }
    }
}

#[test]
fn speakers_are_named_as_the_file_names_them() {
    let named = Layout { speaker_names: vec!["Eugene".into(), "Sasha".into()], ..Layout::default() };
    let premiere = written(&build(&common::transcript(), &named, None).unwrap());
    let labelled = Layout { speaker_labels: true, ..Layout::default() };
    let (srt, _) = to_srt(&premiere, &labelled).unwrap();
    assert!(srt.contains("Eugene: "), "{srt}");
    assert!(srt.contains("Sasha: "), "{srt}");
    assert!(srt.contains("Speaker 3: "), "{srt}");
    // Without labels the text carries no names.
    let (plain, _) = to_srt(&premiere, &Layout::default()).unwrap();
    assert!(!plain.contains("Eugene"));
}

#[test]
fn something_that_is_not_a_premiere_transcript_is_none() {
    let layout = Layout::default();
    assert!(to_srt("", &layout).is_none());
    assert!(to_srt("not json", &layout).is_none());
    assert!(to_srt("[1, 2]", &layout).is_none());
    assert!(to_srt(r#"{"tokens": []}"#, &layout).is_none(), "a Soniox transcript is not one");
    assert!(to_srt(r#"{"language": "en-us", "segments": [], "speakers": []}"#, &layout).is_none());
    assert!(to_srt(r#"{"segments": [{"words": [{"text": 1}]}], "speakers": []}"#, &layout).is_none());
}

#[test]
fn damaged_words_are_skipped_and_a_bom_is_fine() {
    let json = format!(
        "\u{feff}{}",
        r#"{"language": "en-us", "speakers": [{"id": "a", "name": "Speaker 1"}], "segments": [
            {"language": "en-us", "speaker": "a", "start": 0, "duration": 2, "words": [
                {"text": "Hello", "start": 0.0, "duration": 0.5, "confidence": 1, "eos": false, "tags": [], "type": "word"},
                {"text": "", "start": 0.5, "duration": 0.1},
                {"start": 0.6},
                {"text": "bad", "start": -1.0, "duration": 1},
                {"text": "huge", "start": 1e300, "duration": 1e300},
                {"text": "late", "start": 99999999, "duration": 1},
                {"text": "ok", "start": 0.6, "duration": -1},
                {"text": "!", "start": 0.5, "duration": 0.1, "type": "punctuation"}
            ]}]}"#
    );
    let (srt, n) = to_srt(&json, &Layout::default()).expect("the good words are kept");
    assert_eq!(n, 1);
    assert!(srt.contains("Hello ok!"), "{srt}");
    for junk in ["bad", "huge", "late"] {
        assert!(!srt.contains(junk), "{srt}");
    }
}

#[test]
fn a_speaker_the_file_does_not_list_is_read_without_a_label() {
    let json = r#"{"language": "en-us", "speakers": [{"id": "a", "name": "Anna"}, {"id": "b", "name": "Boris"}], "segments": [
        {"language": "en-us", "speaker": "zzz", "start": 0, "duration": 1, "words": [
            {"text": "Mystery", "start": 0.0, "duration": 0.5, "type": "word"}]},
        {"language": "en-us", "speaker": "a", "start": 3, "duration": 1, "words": [
            {"text": "Hello", "start": 3.0, "duration": 0.5, "type": "word"}]},
        {"language": "en-us", "speaker": "b", "start": 6, "duration": 1, "words": [
            {"text": "Hi", "start": 6.0, "duration": 0.5, "type": "word"}]}]}"#;
    let labelled = Layout { speaker_labels: true, ..Layout::default() };
    let (srt, _) = to_srt(json, &labelled).expect("read");
    assert!(srt.contains("Mystery"), "{srt}");
    assert!(!srt.contains(": Mystery"), "{srt}");
    assert!(srt.contains("Anna: Hello"), "{srt}");
    assert!(srt.contains("Boris: Hi"), "{srt}");
}

#[test]
fn standalone_dashes_and_symbols_survive_the_round_trip_but_marks_attach() {
    let tok = |text: &str, start: u64| json!({"text": text, "start_ms": start, "end_ms": start + 100, "confidence": 0.9, "language": "en", "speaker": "1"});
    let soniox = json!({"tokens": [
        tok("5", 0), tok(" +", 200), tok(" 3", 400), tok(" —", 600), tok(" ok.", 800), tok(" &", 1000), tok(" fine", 1200)
    ]});
    let premiere = written(&build(&soniox, &Layout::default(), Some("en")).unwrap());
    let layout = Layout::default();
    assert_eq!(to_srt(&premiere, &layout).unwrap(), sonisub::srt::build(&soniox, &layout));
    assert!(to_srt(&premiere, &layout).unwrap().0.contains("5 + 3 — ok."));
}

#[test]
fn a_label_only_where_the_speaker_changes_and_a_leading_mark_attaches() {
    let json = r#"{"language": "en-us", "speakers": [{"id": "a", "name": "Anna"}, {"id": "b", "name": "Boris"}], "segments": [
        {"language": "en-us", "speaker": "a", "start": 0, "duration": 1, "words": [
            {"text": "One", "start": 0.0, "duration": 0.4, "type": "word"},
            {"text": "two", "start": 0.5, "duration": 0.4, "type": "word"}]},
        {"language": "en-us", "speaker": "a", "start": 1, "duration": 1, "words": [
            {"text": "three", "start": 1.0, "duration": 0.4, "type": "word"}]},
        {"language": "en-us", "speaker": "b", "start": 5, "duration": 1, "words": [
            {"text": ".", "start": 5.0, "duration": 0.1, "type": "punctuation"},
            {"text": "Four", "start": 5.2, "duration": 0.4, "type": "word"}]}]}"#;
    let labelled = Layout { speaker_labels: true, ..Layout::default() };
    let (srt, n) = to_srt(json, &labelled).unwrap();
    assert_eq!(n, 2, "{srt}");
    assert_eq!(
        srt,
        "1\n00:00:00,000 --> 00:00:01,400\nAnna: One two three.\n\n2\n00:00:05,200 --> 00:00:05,700\nBoris: Four\n\n"
    );
}

#[test]
fn a_junk_duration_does_not_stretch_a_cue() {
    let json = r#"{"language": "en-us", "speakers": [{"id": "a", "name": "Speaker 1"}], "segments": [
        {"language": "en-us", "speaker": "a", "start": 0, "duration": 1, "words": [
            {"text": "Hello", "start": 0.5, "duration": 1e300, "type": "word"}]}]}"#;
    let (srt, _) = to_srt(json, &Layout::default()).unwrap();
    assert!(srt.contains("00:00:00,500 --> 00:01:00,500"), "{srt}");
}

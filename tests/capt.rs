//! CAPT made from real Soniox transcripts: the expected file, reading it back, and the same SRT,
//! WebVTT, ASS and Premiere transcript as the ones made from the transcript itself.

mod common;

use serde_json::Value;
use sonisub::captions::capt::Doc;
use sonisub::captions::{Layout, ass, premiere, srt, vtt};
use sonisub::soniox;

const NAMES: [&str; 2] = ["dialog", "punctuation"];

fn doc(name: &str) -> Doc {
    soniox::capt(&common::transcript_of(name))
}

#[test]
fn golden_capt_for_real_transcripts() {
    for name in NAMES {
        assert_eq!(doc(name).write(), common::golden(&format!("{name}.capt")), "{name}");
    }
}

#[test]
fn a_written_file_reads_back_as_the_same_doc() {
    for name in NAMES {
        let file = common::golden(&format!("{name}.capt"));
        let read = Doc::parse(&file).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(read, doc(name), "{name}");
        assert_eq!(read.write(), file, "{name}");
    }
}

#[test]
fn the_words_come_back_as_soniox_gave_them() {
    for name in NAMES {
        let t = common::transcript_of(name);
        let from_capt = Doc::parse(&common::golden(&format!("{name}.capt"))).unwrap().words();
        let from_soniox = soniox::words(&t);
        assert_eq!(from_capt.len(), from_soniox.len(), "{name}");
        for (a, b) in from_capt.iter().zip(&from_soniox) {
            assert_eq!(
                (&a.text, a.start, a.end, &a.speaker, &a.language),
                (&b.text, b.start, b.end, &b.speaker, &b.language),
                "{name}"
            );
            // Confidence is kept to two decimals per piece.
            assert!(
                (a.confidence - b.confidence).abs() <= 0.005,
                "{name}: {} {} {}",
                a.text,
                a.confidence,
                b.confidence
            );
        }
    }
}

#[test]
fn subtitles_from_capt_are_the_subtitles_from_soniox() {
    let speakers = Layout { speaker_labels: true, ..Layout::default() };
    let wrapped = Layout { wrap_lines: true, ..Layout::default() };
    for name in NAMES {
        let words = Doc::parse(&common::golden(&format!("{name}.capt"))).unwrap().words();
        assert_eq!(srt::build(&words, &Layout::default()).0, common::golden(&format!("{name}.srt")), "{name}.srt");
        assert_eq!(
            srt::build(&words, &Layout::unlimited()).0,
            common::golden(&format!("{name}.unlimited.srt")),
            "{name}"
        );
        assert_eq!(srt::build(&words, &speakers).0, common::golden(&format!("{name}.speakers.srt")), "{name}");
        assert_eq!(srt::build(&words, &wrapped).0, common::golden(&format!("{name}.wrap.srt")), "{name}");
    }
    let words = Doc::parse(&common::golden("dialog.capt")).unwrap().words();
    let layout = Layout::default();
    assert_eq!(vtt::build(&words, &layout).0, common::golden("dialog.vtt"));
    assert_eq!(ass::build(&words, &layout, &ass::Options::default()).0, common::golden("dialog.ass"));
    let karaoke = ass::Options { karaoke: true, ..Default::default() };
    assert_eq!(ass::build(&words, &layout, &karaoke).0, common::golden("dialog.karaoke.ass"));
}

/// The transcript without confidence, which CAPT keeps to two decimals.
fn without_confidence(mut t: Value) -> Value {
    for s in t["segments"].as_array_mut().unwrap() {
        for w in s["words"].as_array_mut().unwrap() {
            w.as_object_mut().unwrap().remove("confidence");
        }
    }
    t
}

#[test]
fn the_premiere_transcript_from_capt_has_the_same_speakers_words_and_times() {
    for name in NAMES {
        let d = Doc::parse(&common::golden(&format!("{name}.capt"))).unwrap();
        let t = premiere::build(&d.words(), &Layout::default(), Some("en"), d.id.as_deref().unwrap()).unwrap();
        let golden = common::golden_json(&format!("{name}.premiere.json"));
        assert_eq!(without_confidence(t), without_confidence(golden), "{name}");
    }
}

#[test]
fn the_example_of_the_spec_reads_and_writes_back() {
    let spec = std::fs::read_to_string(common::fixture("../../docs/capt.md")).unwrap().replace("\r\n", "\n");
    let example = spec.split("```\n").nth(1).unwrap();
    let doc = Doc::parse(example).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(doc.write(), example);
    assert_eq!(doc.blocks.len(), 3);
}

#[test]
fn every_token_unsure_still_writes_a_readable_file() {
    for name in NAMES {
        let mut t = common::transcript_of(name);
        for tok in t["tokens"].as_array_mut().unwrap() {
            tok["confidence"] = 0.5.into();
        }
        let doc = soniox::capt(&t);
        assert_eq!(Doc::parse(&doc.write()), Ok(doc), "{name}");
    }
}

//! Every output made from a CAPT file, against expected files: `dialog.capt` (made from a real
//! Soniox transcript, no marks) and `marks.capt` (written by hand with every mark: italic, a word
//! in another language, cut points moved, a sentence end, a voice, the top, a rectangle, sounds).
//!
//! `BLESS=1 cargo test --test outputs` rewrites the expected files from what the writers make now;
//! read the diff before committing it.

mod common;

use sonisub::captions::capt::Doc;
use sonisub::captions::{Layout, ass, srt, vtt};

fn doc(name: &str) -> Doc {
    Doc::parse(&common::golden(&format!("{name}.capt"))).unwrap()
}

/// The layout a user gets with `-s` and the file's speaker names.
fn layout(d: &Doc) -> Layout {
    let names: Vec<String> = d.speakers.iter().map(|s| s.name.clone().unwrap_or_default()).collect();
    Layout { speaker_labels: true, speaker_names: names, ..Layout::default() }
}

fn bless() -> bool {
    std::env::var_os("BLESS").is_some()
}

fn check(name: &str, ext: &str, text: &str) {
    let file = format!("{name}.{ext}");
    if bless() {
        std::fs::write(common::fixture(&file), text).unwrap();
    }
    assert_eq!(common::normalize(text), common::golden(&file), "{file}");
}

#[test]
fn marks_in_srt_webvtt_and_ass() {
    let d = doc("marks");
    let l = layout(&d);
    let words = d.to_words();
    check("marks", "srt", &srt::build(&words, &l).0);
    check("marks", "vtt", &vtt::build(&words, &l).0);
    check("marks", "ass", &ass::build(&words, &l, &ass::Options::default()).0);
}

#[test]
fn cut_points_moved_by_hand_and_a_sentence_end_are_followed() {
    let d = doc("marks");
    // Short wrapped lines, so the sentence has to be cut.
    let v = Layout { max_line: 22, max_duration: 4.0, wrap_lines: true, ..layout(&d) };
    let srt = srt::build(&d.to_words(), &v).0;
    // The file has cut points after "этот" and "упал", not after "Короче,": no cue ends at that
    // comma (a line may). And it ends a sentence after "Dr.".
    assert!(srt.contains("Boris: Короче,\nэтот deployment упал\n\n"), "{srt}");
    assert!(srt.contains("please Dr.\n\n"), "{srt}");
}

#[test]
fn italic_and_places_survive_karaoke() {
    let d = doc("marks");
    let l = layout(&d);
    let ass = ass::build(&d.to_words(), &l, &ass::Options { karaoke: true, ..Default::default() }).0;
    assert!(ass.contains("{\\i1}{\\k26}War"), "{ass}");
    assert!(ass.contains("{\\an8}"), "{ass}");
}

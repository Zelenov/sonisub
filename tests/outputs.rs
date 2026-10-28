//! Every output made from a CAPT file, against expected files: `dialog.capt` (made from a real
//! Soniox transcript, no marks) and `marks.capt` (written by hand with every mark: italic, a word
//! in another language, cut points moved, a sentence end, a voice, the top, a rectangle, sounds).

mod common;

use sonisub::captions::capt::Doc;
use sonisub::captions::{Layout, adr, ass, lrc, sbv, sdh, srt, transcript, ttml, vtt};

fn doc(name: &str) -> Doc {
    Doc::parse(&common::golden(&format!("{name}.capt"))).unwrap()
}

/// The layout a user gets with `-s`, the file's speaker names, and its vertical variant.
fn layouts(d: &Doc) -> (Layout, Layout) {
    let names: Vec<String> = d.speakers.iter().map(|s| s.name.clone().unwrap_or_default()).collect();
    (
        Layout { speaker_labels: true, speaker_names: names.clone(), ..Layout::default() },
        Layout { speaker_names: names, ..Layout::vertical() },
    )
}

fn check(name: &str, ext: &str, text: &str) {
    assert_eq!(common::normalize(text), common::golden(&format!("{name}.{ext}")), "{name}.{ext}");
}

#[test]
fn outputs_of_both_fixtures() {
    for name in ["dialog", "marks"] {
        let d = doc(name);
        let (l, _) = layouts(&d);
        let words = d.to_words();
        check(name, "sdh.srt", &sdh::build(&d, &l, "en").0);
        check(name, "ttml", &ttml::build(&d, &l, "en").0);
        check(name, "lrc", &lrc::build(&words, &l).0);
        check(name, "sbv", &sbv::build(&words, &l).0);
        check(name, "txt", &transcript::build(&d, &l, "en"));
        // CRLF is what Fairlight reads: compared as written, not normalised.
        assert_eq!(
            adr::build(&d, &l, &adr::Options::default()).0,
            read_raw(&format!("{name}.adr.csv")),
            "{name}.adr.csv"
        );
    }
}

fn read_raw(file: &str) -> String {
    String::from_utf8(std::fs::read(common::fixture(file)).unwrap()).unwrap()
}

#[test]
fn marks_in_srt_webvtt_and_ass() {
    let d = doc("marks");
    let (l, _) = layouts(&d);
    let words = d.to_words();
    check("marks", "srt", &srt::build(&words, &l).0);
    check("marks", "vtt", &vtt::build(&words, &l).0);
    check("marks", "ass", &ass::build(&words, &l, &ass::Options::default()).0);
    check("marks", "sdh.ru.srt", &sdh::build(&d, &l, "ru").0);
}

#[test]
fn a_vertical_frame_keeps_text_clear_of_the_phone_app() {
    let d = doc("marks");
    let (_, v) = layouts(&d);
    let words = d.to_words();
    let karaoke = ass::Options { karaoke: true, ..Default::default() };
    check("marks", "vertical.ass", &ass::build(&words, &v, &karaoke).0);
    check("marks", "vertical.vtt", &vtt::build(&words, &v).0);
    check("marks", "vertical.ttml", &ttml::build(&d, &v, "en").0);
    let ass = ass::build(&words, &v, &karaoke).0;
    assert!(ass.contains("PlayResX: 1080\nPlayResY: 1920\n"));
    // The bottom stays clear: 28% of 1920 px.
    assert!(ass.contains(",Anna,Anna,0,0,538,,"), "{ass}");
}

#[test]
fn cut_points_moved_by_hand_and_a_sentence_end_are_followed() {
    let d = doc("marks");
    let (_, v) = layouts(&d);
    let srt = srt::build(&d.to_words(), &v).0;
    // The file cuts after "этот", not after "Короче,", and ends a sentence after "Dr.".
    assert!(srt.contains("Короче, этот\ndeployment упал\n"), "{srt}");
    assert!(srt.contains("please Dr.\n\n"), "{srt}");
}

#[test]
fn italic_and_places_survive_karaoke() {
    let d = doc("marks");
    let (l, _) = layouts(&d);
    let ass = ass::build(&d.to_words(), &l, &ass::Options { karaoke: true, ..Default::default() }).0;
    assert!(ass.contains("{\\i1}{\\k26}War"), "{ass}");
    assert!(ass.contains("{\\an8}"), "{ass}");
}

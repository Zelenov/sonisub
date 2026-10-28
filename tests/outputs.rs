//! Every output made from a CAPT file, against expected files: `dialog.capt` (made from a real
//! Soniox transcript, no marks) and `marks.capt` (written by hand with every mark: italic, a word
//! in another language, cut points moved, a sentence end, a voice, the top, a rectangle, sounds).
//!
//! `BLESS=1 cargo test --test outputs` rewrites the expected files from what the writers make now;
//! read the diff before committing it.

mod common;

use sonisub::captions::capt::Doc;
use sonisub::captions::{Layout, ass, sdh, srt, ttml, vtt};

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
fn outputs_of_both_fixtures() {
    for name in ["dialog", "marks"] {
        let d = doc(name);
        let (l, _) = layouts(&d);
        check(name, "sdh.srt", &sdh::srt(&d, &l, "en").0);
        check(name, "ttml", &ttml::build(&d, &l).0);
        check(name, "sdh.ttml", &sdh::ttml(&d, &l, "en").0);
    }
}

#[test]
fn marks_in_srt_webvtt_and_ass() {
    let d = doc("marks");
    let (l, _) = layouts(&d);
    let words = d.to_words();
    check("marks", "srt", &srt::build(&words, &l).0);
    check("marks", "vtt", &vtt::build(&words, &l).0);
    check("marks", "ass", &ass::build(&words, &l, &ass::Options::default()).0);
    check("marks", "sdh.ru.srt", &sdh::srt(&d, &l, "ru").0);
    check("marks", "sdh.vtt", &sdh::vtt(&d, &l, "en").0);
    check("marks", "sdh.ass", &sdh::ass(&d, &l, &ass::Options::default(), "en").0);
}

#[test]
fn sdh_is_the_same_cues_with_sounds_and_voices() {
    let d = doc("marks");
    let (l, _) = layouts(&d);
    let plain = srt::build(&d.to_words(), &l);
    let (text, n) = sdh::srt(&d, &l, "en");
    assert!(n > plain.1, "{text}");
    assert!(text.contains("[phone ringing]"), "{text}");
    assert!(text.contains("[whispering]"), "{text}");
    // Plain SRT has neither.
    assert!(!plain.0.contains("[phone ringing]") && !plain.0.contains("[whispering]"), "{}", plain.0);
}

#[test]
fn a_vertical_frame_keeps_text_clear_of_the_phone_app() {
    let d = doc("marks");
    let (_, v) = layouts(&d);
    let words = d.to_words();
    let karaoke = ass::Options { karaoke: true, ..Default::default() };
    check("marks", "vertical.ass", &ass::build(&words, &v, &karaoke).0);
    check("marks", "vertical.vtt", &vtt::build(&words, &v).0);
    check("marks", "vertical.ttml", &ttml::build(&d, &v).0);
    let ass = ass::build(&words, &v, &karaoke).0;
    assert!(ass.contains("PlayResX: 1080\nPlayResY: 1920\n"));
    // The bottom stays clear: 28% of 1920 px.
    assert!(ass.contains(",Anna,Anna,0,0,538,,"), "{ass}");
}

#[test]
fn a_real_dialog_reads_well_on_a_vertical_frame() {
    let d = doc("dialog");
    let (_, v) = layouts(&d);
    let text = srt::build(&d.to_words(), &v).0;
    check("dialog", "vertical.srt", &text);
    // Short lines, at most two of them, and no cue ends on a word that leads into the next one.
    for cue in text.split("\n\n").filter(|c| !c.trim().is_empty()) {
        let lines: Vec<&str> = cue.lines().skip(2).collect();
        assert!(lines.len() <= 2, "{cue}");
        assert!(lines.iter().all(|l| l.chars().count() <= 22), "{cue}");
        let last = lines.last().and_then(|l| l.split(' ').next_back()).unwrap_or_default();
        assert!(!["the", "a", "and", "to", "of"].contains(&last.to_lowercase().as_str()), "{cue}");
    }
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

#[test]
fn karaoke_keeps_voice_labels_with_sdh() {
    let d = doc("marks");
    let (l, _) = layouts(&d);
    let ass = sdh::ass(&d, &l, &ass::Options { karaoke: true, ..Default::default() }, "en").0;
    assert!(ass.contains("[whispering]"), "{ass}");
    assert!(ass.contains("\\k"), "{ass}");
}

#[test]
fn karaoke_and_plain_ass_label_the_same_voices() {
    let d = doc("marks");
    let (l, _) = layouts(&d);
    let labels = |karaoke: bool| {
        let ass = sdh::ass(&d, &l, &ass::Options { karaoke, ..Default::default() }, "en").0;
        ass.lines()
            .filter(|l| l.starts_with("Dialogue:"))
            .map(|l| l.matches("[whispering]").count())
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(true), labels(false));
}

#[test]
fn a_sound_is_labelled_with_its_speaker_only_when_speech_is() {
    // One speaker: no "Anna:" on her speech, so none on her laugh.
    let d = Doc::parse(
        "CAPT 1
speaker:1 name:Anna

00:00:01.000 --> 00:00:02.000 speaker:1
{0}Hi.{50}

00:00:02.500 --> 00:00:03.000 speaker:1 sound:laughter
",
    )
    .unwrap();
    let l = Layout { speaker_labels: true, speaker_names: vec!["Anna".into()], ..Layout::default() };
    let text = sdh::srt(&d, &l, "en").0;
    assert!(
        text.contains(
            "
[laughs]
"
        ) && !text.contains("Anna:"),
        "{text}"
    );
}

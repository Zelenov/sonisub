//! `Doc::merge_speakers`: speakers recognition split are made one before layout, so every format
//! sees one speaker.

use std::collections::BTreeMap;

use captions::capt::Doc;
use captions::{Layout, adr, ass, srt};

/// Anna is heard as speakers 1 and 3: her sentence is split between them around Boris (2).
const FILE: &str = "CAPT 1
lang:en
speaker:1
speaker:2 name:Boris
speaker:3 name:Anna mood:calm
speaker:4

00:00:01.000 --> 00:00:02.000 id:aaaaaa speaker:1
{0}So{30} {40}we{60} {70}went{100}

00:00:02.200 --> 00:00:03.000 id:bbbbbb speaker:3
{0}home{40} {50}early.{80}

00:00:04.000 --> 00:00:05.000 id:cccccc speaker:2
{0}Really?{100}

00:00:06.000 --> 00:00:07.000 id:dddddd speaker:4
{0}Yes.{100}
";

fn doc() -> Doc {
    Doc::parse(FILE).unwrap()
}

fn merged(map: &[(u32, u32)]) -> Doc {
    let mut d = doc();
    d.merge_speakers(&map.iter().copied().collect()).unwrap_or_else(|e| panic!("{e}"));
    d
}

fn speakers(d: &Doc) -> Vec<(&str, Option<&str>)> {
    d.speakers.iter().map(|s| (s.id.as_str(), s.name.as_deref())).collect()
}

fn blocks(d: &Doc) -> Vec<Option<&str>> {
    d.blocks.iter().map(|b| b.speaker.as_deref()).collect()
}

fn srt(d: &Doc) -> String {
    srt::build(&d.to_words(), &Layout::default()).0
}

#[test]
fn a_merged_speaker_s_neighbouring_lines_join_into_one_cue() {
    let before = srt(&doc());
    assert!(before.contains("So we went\n\n2\n"), "{before}");
    let after = srt(&merged(&[(3, 1)]));
    assert!(after.starts_with("1\n00:00:01,000 --> 00:00:03,000\nSo we went home early.\n\n2\n"), "{after}");
    assert_eq!(srt::build(&merged(&[(3, 1)]).to_words(), &Layout::default()).1, 3);
}

#[test]
fn the_others_keep_their_numbers_and_the_target_its_place() {
    let d = merged(&[(3, 1)]);
    assert_eq!(blocks(&d), [Some("1"), Some("1"), Some("2"), Some("4")]);
    // Speaker 1 had no name: it takes Anna's from speaker 3, and her other options.
    assert_eq!(speakers(&d), [("1", Some("Anna")), ("2", Some("Boris")), ("4", None)]);
    assert_eq!(d.speakers[0].other, [("mood".to_string(), vec!["calm".to_string()])]);
    assert_eq!(d.speaker_names(), ["Anna", "Boris"]);
    // It writes and reads back the same.
    assert_eq!(Doc::parse(&d.write()), Ok(d));
}

#[test]
fn the_target_keeps_its_own_name() {
    let d = merged(&[(3, 2)]);
    assert_eq!(speakers(&d), [("1", None), ("2", Some("Boris")), ("4", None)]);
    assert_eq!(blocks(&d), [Some("1"), Some("2"), Some("2"), Some("4")]);
}

#[test]
fn chains_resolve_to_their_last_speaker() {
    let d = merged(&[(3, 2), (2, 1)]);
    assert_eq!(blocks(&d), [Some("1"), Some("1"), Some("1"), Some("4")]);
    // Boris comes first in the header, so speaker 1 takes his name.
    assert_eq!(speakers(&d), [("1", Some("Boris")), ("4", None)]);
    let d = merged(&[(4, 3), (3, 1)]);
    assert_eq!(blocks(&d), [Some("1"), Some("1"), Some("2"), Some("1")]);
}

#[test]
fn a_target_heard_only_in_blocks_takes_the_source_s_header_line() {
    let mut d = doc();
    d.speakers.retain(|s| s.id != "1");
    d.merge_speakers(&BTreeMap::from([(3, 1)])).unwrap();
    assert_eq!(speakers(&d), [("2", Some("Boris")), ("1", Some("Anna")), ("4", None)]);
}

#[test]
fn ass_has_one_style_for_the_merged_speaker() {
    let d = merged(&[(3, 1)]);
    let styles =
        ass::styles(&captions::cues::cues(&d.to_words(), &Layout::default()), &Layout::default(), &Default::default());
    let names: Vec<_> = styles.iter().map(|s| s.speaker.as_deref()).collect();
    assert_eq!(names, [None, Some("1"), Some("2"), Some("4")]);
    let before = ass::styles(
        &captions::cues::cues(&doc().to_words(), &Layout::default()),
        &Layout::default(),
        &Default::default(),
    );
    // Colours go by order of appearance: speaker 1 keeps its colour, Boris takes the second.
    assert_eq!(styles[1].colour, before[1].colour);
    assert_eq!(styles[2].colour, before[2].colour);
    let (text, _) = ass::build(&d.to_words(), &Layout::default(), &Default::default());
    assert_eq!(text.lines().filter(|l| l.starts_with("Style: ")).count(), 4, "{text}");
}

#[test]
fn adr_has_one_character_for_the_merged_speaker() {
    let d = merged(&[(3, 1)]);
    let layout = Layout { speaker_names: d.speaker_names(), ..Layout::default() };
    let rows = adr::rows(&d, &layout, &adr::Options::default(), "en").unwrap();
    let characters: Vec<&str> = rows.iter().map(|r| r.character.as_str()).collect();
    assert_eq!(characters, ["Anna", "Boris", "Speaker 4"]);
    assert_eq!(rows[0].dialogue, "So we went home early.");
}

#[test]
fn a_speaker_onto_itself_or_an_unknown_one_is_an_error_and_nothing_changes() {
    let err = |map: &[(u32, u32)]| {
        let mut d = doc();
        let e = d.merge_speakers(&map.iter().copied().collect()).unwrap_err();
        assert_eq!(d, doc(), "{e}");
        e
    };
    assert_eq!(err(&[(1, 1)]), "speaker 1 is mapped onto itself");
    assert_eq!(err(&[(1, 2), (2, 1)]), "speaker 1 is mapped onto itself");
    assert_eq!(err(&[(3, 9)]), "the file has no speaker 9");
    assert_eq!(err(&[(9, 1)]), "the file has no speaker 9");
    // An empty map changes nothing.
    assert_eq!(merged(&[]), doc());
}

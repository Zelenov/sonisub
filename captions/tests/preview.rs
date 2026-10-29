//! What a preview needs beside the files: where each word of a cue is shown, the pieces of every
//! word with their times, the ASS styles and the ADR rows.

use std::collections::BTreeMap;

use captions::capt::{Doc, Piece};
use captions::{Layout, adr, ass, cues};

const FILE: &str = r#"CAPT 1
lang:en
speaker:1 name:Anna
speaker:2

00:00:01.000 --> 00:00:03.000 speaker:1
{0}fin{10}ally—{30}and{50} {60}New\ York.{100}

00:00:04.000 --> 00:00:05.000 speaker:2
{0}Yes.{50}
"#;

fn piece(text: &str, start: u64) -> Piece {
    Piece { text: text.into(), start }
}

#[test]
fn a_word_cut_at_a_dash_gives_each_part_its_pieces() {
    let doc = Doc::parse(FILE).unwrap();
    let list = cues::cues(&doc.to_words(), &Layout::default());
    let words: Vec<&str> = list[0].words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(words, ["finally—", "and", "New York."]);
    let pieces = doc.pieces(&list).unwrap();
    assert_eq!(pieces[0][0], [piece("fin", 1000), piece("ally—", 1100)]);
    assert_eq!(pieces[0][1], [piece("and", 1300)]);
    assert_eq!(pieces[0][2], [piece("New York.", 1600)]);
    assert_eq!(pieces[1][0], [piece("Yes.", 4000)]);
    // Cues of other words are not this file's.
    assert!(doc.pieces(&list[1..]).is_none());
}

#[test]
fn shown_puts_every_word_on_its_line_after_the_label() {
    let doc = Doc::parse(FILE).unwrap();
    let layout = Layout {
        max_line: 18,
        wrap_lines: true,
        speaker_labels: true,
        speaker_names: doc.speaker_names(),
        ..Layout::default()
    };
    let list = cues::cues(&doc.to_words(), &layout);
    let shown = list[0].shown().unwrap();
    let at: Vec<(usize, &str, Option<usize>)> = shown.iter().map(|s| (s.line, s.text.as_str(), s.word)).collect();
    assert_eq!(
        at,
        [(0, "Anna:", None), (0, "finally—", Some(0)), (1, "and", Some(1)), (1, "New", Some(2)), (1, "York.", Some(2))]
    );
    assert!(!shown[2].space && shown[4].space);
}

#[test]
fn ass_styles_are_default_then_each_speaker_with_a_colour() {
    let doc = Doc::parse(FILE).unwrap();
    let layout = Layout { speaker_names: doc.speaker_names(), ..Layout::default() };
    let list = cues::cues(&doc.to_words(), &layout);
    let names = BTreeMap::from([("2".to_string(), "Boris".to_string())]);
    let styles = ass::styles(&list, &layout, &ass::Options { names, ..Default::default() });
    let got: Vec<(Option<&str>, &str, &str, u32)> =
        styles.iter().map(|s| (s.speaker.as_deref(), s.name.as_str(), s.actor.as_str(), s.colour)).collect();
    assert_eq!(
        got,
        [
            (None, "Default", "", 0xFFFFFF),
            (Some("1"), "Anna", "Anna", 0xFFFFFF),
            (Some("2"), "Boris", "Boris", 0xFFE14D)
        ]
    );
    assert_eq!(styles[2].dimmed, 0x7F7026);
}

#[test]
fn adr_rows_are_the_cue_list_with_the_moments_they_are_heard() {
    let doc = Doc::parse(FILE).unwrap();
    let layout = Layout { speaker_names: doc.speaker_names(), ..Layout::default() };
    let o = adr::Options::default();
    let rows = adr::rows(&doc, &layout, &o, "en").unwrap();
    assert_eq!((rows[0].id, rows[0].start, rows[0].end), (1, 1000, 2000));
    assert_eq!((rows[0].tc_in.to_string(), rows[0].tc_out.to_string()), ("01:00:01:00".into(), "01:00:02:00".into()));
    assert_eq!((rows[1].character.as_str(), rows[1].dialogue.as_str()), ("Speaker 2", "Yes."));
    let (csv, n) = adr::build(&doc, &layout, &o, "en").unwrap();
    assert_eq!(n, rows.len());
    assert!(
        csv.starts_with("\u{feff}\"1\",\"01:00:01:00\",\"01:00:02:00\",\"Anna\",\"finally—and New York.\"\r\n"),
        "{csv}"
    );
}

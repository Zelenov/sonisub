//! CAPT: every part of the format read and written back, and the errors a reader reports.

use captions::capt::{Doc, Pos};

/// A file with every part of the format, as `Doc::write` writes it.
const FILE: &str = r#"CAPT 1
id:691529b1-72ac-4e08-8fe1-4c3a84d29b28
title:"Moon Interview"
lang:en
media:"moon interview.mp4" duration:00:12:31.040
studio:north
speaker:1 name:Anna
speaker:2 name:"Ann \"Lee\"" colour:red

00:00:29.000 --> 00:00:31.500 id:h2k9wq sound:music

00:00:33.000 --> 00:00:36.100 id:w3n8fd speaker:2 lang:ru
{0}Короче,{40} / {52}этот{70} {76}deploy{98}ment{120} {126}упал,{160}
{0..40} conf:0.68 alt:Корочe,0.20
{76..120} lang:en

00:00:36.600 --> 00:00:38.300 id:r4c7tz speaker:1 pos:0.05,0.05,0.4,0.2 voice:whisper
{0}Type{30} {36}\{name\}{90} / {100}and{112} {116}\/{122} {126}C:\\temp{140} {150}etc.{170}
{36..90} style:italic
{150..170} sentence:continues mood:calm

00:00:39.000 --> 00:00:41.000 speaker:1
plain words, no marks
"#;

#[test]
fn every_part_reads_and_writes_back() {
    let doc = Doc::parse(FILE).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(doc.title.as_deref(), Some("Moon Interview"));
    assert_eq!(doc.media.as_deref(), Some("moon interview.mp4"));
    assert_eq!(doc.duration, Some(751_040));
    assert_eq!(doc.other, vec![("studio".to_string(), vec!["north".to_string()])]);
    assert_eq!(doc.speakers[1].name.as_deref(), Some("Ann \"Lee\""));

    let [sound, mixed, styled, plain] = &doc.blocks[..] else { panic!("four blocks") };
    assert_eq!((sound.sound.as_deref(), sound.phrases.len()), (Some("music"), 0));

    assert_eq!(mixed.lang.as_deref(), Some("ru"));
    assert_eq!(mixed.phrases.len(), 2);
    let deploy = &mixed.phrases[1][1];
    assert_eq!(deploy.text(), "deployment");
    assert_eq!((deploy.start(), deploy.end), (33_760, 34_200));
    assert_eq!(mixed.span_option(33_760, "lang"), Some(&["en".to_string()][..]));
    assert_eq!(mixed.span_option(33_000, "alt"), Some(&["Корочe".to_string(), "0.20".to_string()][..]));

    assert_eq!(styled.pos, Some(Pos::Rect { x: 0.05, y: 0.05, w: 0.4, h: 0.2 }));
    assert_eq!(styled.voice.as_deref(), Some("whisper"));
    let texts: Vec<String> = styled.words().map(|w| w.text()).collect();
    assert_eq!(texts, ["Type", "{name}", "and", "/", "C:\\temp", "etc."]);
    assert_eq!(styled.span_option(38_100, "mood"), Some(&["calm".to_string()][..]));

    assert!(plain.untimed);
    let w: Vec<(String, u64, u64)> = plain.words().map(|w| (w.text(), w.start(), w.end)).collect();
    assert_eq!(w.first().map(|w| w.1), Some(39_000));
    assert_eq!(w.last().map(|w| w.2), Some(41_000));
    assert!(w.windows(2).all(|p| p[0].2 <= p[1].1), "{w:?}");

    assert_eq!(doc.write(), FILE);
}

#[test]
fn words_take_the_span_language_and_the_mean_confidence() {
    let doc = Doc::parse(FILE).unwrap();
    let words = doc.to_words();
    let get = |t: &str| words.iter().find(|w| w.text == t).unwrap();
    assert_eq!(get("Короче,").confidence, 0.68);
    assert_eq!(get("Короче,").language.as_deref(), Some("ru"));
    assert_eq!(get("deployment").language.as_deref(), Some("en"));
    assert_eq!(get("Type").language.as_deref(), Some("en"), "the file's language");
    assert_eq!(get("Type").speaker.as_deref(), Some("1"));
    assert!(words.iter().all(|w| w.text != "music"), "sounds are not words");
}

#[test]
fn from_words_makes_a_block_per_sentence_with_cut_points() {
    let w = |text: &str, start: u64, end: u64| captions::Word {
        text: text.into(),
        start,
        end,
        speaker: Some("1".into()),
        language: Some("en".into()),
        confidence: if text == "fine." { 0.5 } else { 1.0 },
        glued: false,
    };
    let words =
        [w("Well,", 0, 300), w("this", 400, 600), w("is", 600, 700), w("fine.", 700, 1000), w("Yes.", 2000, 2300)];
    let doc = Doc::from_words(&words, Some("seed".into()));
    let file = doc.write();
    assert_eq!(
        file.split("\n\n").skip(1).collect::<Vec<_>>(),
        [
            "00:00:00.000 --> 00:00:01.000 id:f31x18 speaker:1\n{0}Well,{30} / {40}this{60} {60}is{70} {70}fine.{100}\n{70..100} conf:0.50",
            "00:00:02.000 --> 00:00:02.300 id:gemtxg speaker:1\n{0}Yes.{30}\n"
        ]
    );
    assert_eq!(Doc::parse(&file).unwrap(), doc);
}

fn errors(file: &str) -> Vec<String> {
    match Doc::parse(file) {
        Ok(_) => Vec::new(),
        Err(e) => e.0.iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn a_reader_reports_every_problem_with_its_line() {
    let file = "CAPT 2\n\
                title:\"open\n\
                duration:12.5\n\
                \n\
                00:00:01.000 --> 00:00:02.000 pos:left\n\
                {0}a{ 5}\n\
                \n\
                00:00:03.000 --> 00:00:04.000 pos:0.8,0.1,0.4,0.2\n\
                {0}b} c{10}\n\
                \n\
                00:00:05.000 --> 00:00:06.000\n\
                {0}one{20} {30}two{50}\n\
                {30..40} conf:0.5\n\
                {35..50} conf:0.5\n\
                {0..30} lang:en\n\
                junk\n\
                \n\
                00:00:07.000 --> 00:00:08.000 sound:phone\n\
                ring\n\
                \n\
                1:2 --> later\n";
    assert_eq!(
        errors(file),
        [
            "line 1: a CAPT file starts with the line “CAPT 1”",
            "line 2: the quoted value of “title:” is never closed",
            "line 3: duration is HH:MM:SS.mmm, not “12.5”",
            "line 5: pos is bottom, top, middle, a point x,y or a rectangle x,y,w,h, as parts of the frame from 0 to 1; not “left”",
            "line 6: a mark holds a time in hundredths, not “ 5”",
            "line 8: the rectangle 0.8,0.1,0.4,0.2 runs out of the frame or has no size",
            "line 9: a “}” at column 5 closes nothing: write \\} for a brace in the text",
            "line 13: {30..40}: {40} is not a later mark of this block",
            "line 14: {35..50}: no piece starts at {35}",
            "line 15: {0..30}: “lang:” covers whole words: from a word start to a word end",
            "line 16: after the text a block has only spans, “{from..to} key:value”",
            "line 19: a sound block has no text: the sound is its code",
            "line 21: a block line is “HH:MM:SS.mmm --> HH:MM:SS.mmm key:value…”",
        ]
    );
}

#[test]
fn values_that_need_quotes_get_them() {
    let mut doc = Doc { title: Some("a, b".into()), lang: Some("en".into()), ..Doc::default() };
    doc.other.push(("note".into(), vec![String::new(), "x\\y".into()]));
    let file = doc.write();
    assert_eq!(file, "CAPT 1\ntitle:\"a, b\"\nlang:en\nnote:\"\",x\\y\n");
    assert_eq!(Doc::parse(&file).unwrap(), doc);
}

fn spoken(pieces: &[(&str, u64, f64)], end: u64, lang: &str) -> captions::capt::Spoken {
    captions::capt::Spoken {
        pieces: pieces
            .iter()
            .map(|(t, s, c)| captions::capt::SpokenPiece { text: t.to_string(), start: *s, confidence: *c })
            .collect(),
        end,
        speaker: Some("1".into()),
        language: Some(lang.into()),
    }
}

#[test]
fn words_in_another_language_share_a_span_and_a_tie_moves_by_10_ms() {
    let words = [
        spoken(&[("Короче,", 0, 1.0)], 400, "ru"),
        spoken(&[("этот", 520, 1.0)], 700, "ru"),
        spoken(&[("deploy", 760, 1.0), ("ment", 980, 1.0)], 1200, "en"),
        spoken(&[("pipe", 1260, 1.0), ("line", 1260, 1.0)], 1600, "en"),
        spoken(&[("упал.", 1700, 1.0)], 2000, "ru"),
    ];
    let doc = Doc::from_spoken(&words, Some("seed".into()));
    let file = doc.write();
    let block = file.split("\n\n").nth(1).unwrap();
    assert_eq!(
        block.lines().skip(1).collect::<Vec<_>>(),
        [
            "{0}Короче,{40} / {52}этот{70} {76}deploy{98}ment{120} {126}pipe{127}line{160} {170}упал.{200}",
            "{76..160} lang:en"
        ]
    );
    assert!(file.contains("\nlang:ru\n"), "the file's language is the most common one:\n{file}");
    assert_eq!(Doc::parse(&file).unwrap(), doc);
}

#[test]
fn a_piece_with_no_length_still_reads_back() {
    let cases = [
        vec![spoken(&[("a", 1000, 0.5)], 1004, "en"), spoken(&[("b.", 1100, 1.0)], 1200, "en")],
        vec![spoken(&[("a", 1000, 1.0), ("b", 1004, 0.5)], 1008, "en")],
        vec![
            spoken(&[("Да", 1000, 1.0)], 1300, "ru"),
            spoken(&[("ok", 1400, 1.0)], 1400, "en"),
            spoken(&[("да.", 1500, 1.0)], 1800, "ru"),
        ],
    ];
    for words in cases {
        let doc = Doc::from_spoken(&words, None);
        assert_eq!(Doc::parse(&doc.write()), Ok(doc.clone()), "{}", doc.write());
    }
}

#[test]
fn huge_numbers_are_errors_not_panics() {
    for (file, says) in [
        ("CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{99999999999999999999}a{1}\n", "is too large"),
        ("CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{1844674407370955162}a{1}\n", "is too large"),
        ("CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}a{10}\n{1844674407370955162..0} conf:0.5\n", "is too large"),
        ("CAPT 1\nduration:99999999999999999:00:00.000\n", "duration is HH:MM:SS.mmm"),
    ] {
        let e = errors(file);
        assert!(e.iter().any(|m| m.contains(says)), "{file:?}: {e:?}");
    }
}

#[test]
fn an_arrow_in_a_header_value_is_not_a_block() {
    let mut doc = Doc { title: Some("a --> b".into()), ..Doc::default() };
    doc.speakers.push(captions::capt::Speaker { id: "1".into(), name: Some("x --> y".into()), other: Vec::new() });
    assert_eq!(Doc::parse(&doc.write()), Ok(doc));
}

#[test]
fn a_space_inside_a_word_is_escaped() {
    let w = captions::Word {
        text: "New York".into(),
        start: 0,
        end: 500,
        speaker: None,
        language: None,
        confidence: 1.0,
        glued: false,
    };
    let doc = Doc::from_words(&[w], None);
    let file = doc.write();
    assert!(file.contains(r"{0}New\ York{50}"), "{file}");
    assert_eq!(Doc::parse(&file).unwrap().to_words()[0].text, "New York");
}

#[test]
fn a_speaker_line_keeps_its_other_options_and_may_start_with_any_key() {
    let doc = Doc::parse("CAPT 1\nname:Bob speaker:3 colour:red\n").unwrap();
    assert_eq!(doc.speakers[0].id, "3");
    assert_eq!(doc.speakers[0].name.as_deref(), Some("Bob"));
    assert_eq!(doc.write(), "CAPT 1\nspeaker:3 name:Bob colour:red\n");
}

#[test]
fn a_block_built_by_hand_writes_its_times() {
    use captions::capt::{Block, Piece, Word};
    let word = |t: &str, s: u64, e: u64| Word { pieces: vec![Piece { text: t.into(), start: s }], end: e };
    let b = Block {
        start: 1000,
        end: 2000,
        phrases: vec![vec![word("Hi", 1000, 1400), word("there.", 1500, 2000)]],
        ..Block::default()
    };
    let doc = Doc { blocks: vec![b], ..Doc::default() };
    assert_eq!(doc.write(), "CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}Hi{40} {50}there.{100}\n");
    assert_eq!(Doc::parse(&doc.write()), Ok(doc));
}

#[test]
fn errors_are_one_error_with_a_line_each() {
    fn read(text: &str) -> Result<Doc, Box<dyn std::error::Error>> {
        Ok(Doc::parse(text)?)
    }
    let e = read("CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}a{x}\n{0..9} lang:en\n").unwrap_err();
    assert_eq!(e.to_string(), "line 4: a mark holds a time in hundredths, not “x”");
}

#[test]
fn words_from_another_recogniser_may_overlap_or_be_empty() {
    let w = |text: &str, start: u64, end: u64, lang: &str| captions::Word {
        text: text.into(),
        start,
        end,
        speaker: Some("1".into()),
        language: Some(lang.into()),
        confidence: 1.0,
        glued: false,
    };
    let words = [
        w("Мы", 0, 300, "ru"),
        w("used", 280, 500, "en"),
        w("", 500, 510, "en"),
        w("the", 520, 600, "en"),
        w("API.", 620, 900, "en"),
        w("Да.", 1000, 1200, "ru"),
    ];
    let doc = Doc::from_words(&words, None);
    let read = Doc::parse(&doc.write()).unwrap_or_else(|e| panic!("{e}\n{}", doc.write()));
    assert_eq!(read, doc);
    let texts: Vec<String> = read.to_words().into_iter().map(|w| w.text).collect();
    assert_eq!(texts, ["Мы", "used", "the", "API.", "Да."], "the empty word is dropped");
    assert!(read.to_words()[0].end <= read.to_words()[1].start, "a word ends before the next one starts");
}

#[test]
fn a_huge_time_from_a_recogniser_does_not_panic() {
    let w = captions::Word {
        text: "x".into(),
        start: u64::MAX,
        end: u64::MAX,
        speaker: None,
        language: None,
        confidence: 0.5,
        glued: false,
    };
    let _ = Doc::from_words(&[w], None).write();
}

#[test]
fn marks_that_go_back_or_cover_only_part_of_a_line_are_errors() {
    for (text, says) in [
        ("{0}Hello{30} {20}there{40}", "the mark {20} is earlier than the mark {30} before it"),
        ("{0}Hello{50} brave {60}new{100}", "every word of a line has marks, or none does: {start}word{end}"),
        ("{0}a{0}b{20}", "two pieces start at {0}: a piece starts after the one before it"),
    ] {
        let file = format!("CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{text}\n");
        assert_eq!(errors(&file), [format!("line 4: {says}")], "{text}");
    }
}

#[test]
fn a_missing_blank_line_between_blocks_is_named() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}a{10}\n00:00:03.000 --> 00:00:04.000\n{0}b{10}\n";
    assert_eq!(errors(file), ["line 5: put a blank line before this block line"]);
}

#[test]
fn blocks_out_of_order_and_marks_past_the_end_are_errors() {
    let file = "CAPT 1\n\n00:00:03.000 --> 00:00:04.000\n{0}Hi{20}\n\n00:00:01.000 --> 00:00:02.000\n{0}Yo{150}\n";
    assert_eq!(
        errors(file),
        [
            "line 6: this block starts before the block on line 3: blocks are in order of start",
            "line 7: the mark {150} is after the block's end",
        ]
    );
}

#[test]
fn a_line_break_or_tab_in_a_word_or_value_does_not_break_the_file() {
    let w = |text: &str| captions::Word {
        text: text.into(),
        start: 0,
        end: 500,
        speaker: Some("x\ty".into()),
        language: Some("en".into()),
        confidence: 1.0,
        glued: false,
    };
    let doc = Doc::from_words(&[w("a\nb.")], None);
    let read = Doc::parse(&doc.write()).unwrap_or_else(|e| panic!("{e}\n{}", doc.write()));
    assert_eq!(read.to_words()[0].text, "a b.");
    assert_eq!(read.blocks[0].speaker.as_deref(), Some("x\ty"));
}

//! The formats on hand-made words: one test per rule. Golden output for real transcripts is in
//! sonisub's tests (tests/formats.rs), next to the fixtures.

use std::collections::BTreeMap;

use captions::{Layout, Word, ass, cues, srt, vtt};

fn word(text: &str, start: u64, end: u64, speaker: Option<&str>) -> Word {
    Word {
        text: text.into(),
        start,
        end,
        speaker: speaker.map(str::to_string),
        language: None,
        confidence: 1.0,
        glued: false,
        marks: Default::default(),
    }
}

/// "a b c." spoken by `speaker`, one word every 300 ms from `start`.
fn sentence(text: &str, start: u64, speaker: Option<&str>) -> Vec<Word> {
    text.split(' ')
        .enumerate()
        .map(|(i, t)| word(t, start + i as u64 * 300, start + i as u64 * 300 + 250, speaker))
        .collect()
}

fn dialogue(ass: &str) -> Vec<Vec<&str>> {
    ass.lines().filter_map(|l| l.strip_prefix("Dialogue: ")).map(|l| l.splitn(10, ',').collect()).collect()
}

fn styles(ass: &str) -> Vec<Vec<&str>> {
    ass.lines().filter_map(|l| l.strip_prefix("Style: ")).map(|l| l.split(',').collect()).collect()
}

fn two_speakers() -> Vec<Word> {
    let mut w = sentence("Hello there.", 0, Some("1"));
    w.extend(sentence("Hi.", 2000, Some("2")));
    w.extend(sentence("Bye now.", 4000, Some("1")));
    w
}

#[test]
fn word_json_needs_only_text_and_timing() {
    let w: Word = serde_json::from_str(r#"{"text": "Hi,", "start": 10, "end": 20}"#).unwrap();
    assert_eq!(w, word("Hi,", 10, 20, None));
    let full = Word { language: Some("en".into()), confidence: 0.5, ..word("Hi,", 10, 20, Some("2")) };
    let json = serde_json::to_string(&full).unwrap();
    assert_eq!(json, r#"{"text":"Hi,","start":10,"end":20,"speaker":"2","language":"en","confidence":0.5}"#);
    assert_eq!(serde_json::from_str::<Word>(&json).unwrap(), full);
}

#[test]
fn every_format_has_the_same_cues() {
    let words = two_speakers();
    let layout = Layout::default();
    let n = cues::cues(&words, &layout).len();
    assert_eq!(n, 3);
    assert_eq!(srt::build(&words, &layout).1, n);
    assert_eq!(vtt::build(&words, &layout).1, n);
    assert_eq!(ass::build(&words, &layout, &ass::Options::default()).1, n);
}

#[test]
fn vtt_has_a_header_dotted_timestamps_and_escaped_markup() {
    let words = vec![
        word("Tom", 3_723_004, 3_723_300, None),
        word("&", 3_723_400, 3_723_500, None),
        word("<Jerry>.", 3_723_600, 3_724_000, None),
    ];
    let (text, n) = vtt::build(&words, &Layout::default());
    assert_eq!(n, 1);
    assert_eq!(text, "WEBVTT\n\n1\n01:02:03.004 --> 01:02:04.000\nTom &amp; &lt;Jerry&gt;.\n\n");
}

#[test]
fn ass_gives_each_speaker_a_style_and_names_them_on_every_line() {
    let (text, _) = ass::build(&two_speakers(), &Layout::default(), &ass::Options::default());
    let st = styles(&text);
    let names: Vec<&str> = st.iter().map(|s| s[0]).collect();
    assert_eq!(names, ["Default", "Speaker 1", "Speaker 2"]);
    assert_ne!(st[1][3], st[2][3], "speakers get different colours");
    let lines = dialogue(&text);
    let who: Vec<(&str, &str)> = lines.iter().map(|l| (l[3], l[4])).collect();
    assert_eq!(who, [("Speaker 1", "Speaker 1"), ("Speaker 2", "Speaker 2"), ("Speaker 1", "Speaker 1")]);
    assert_eq!(lines[0][1], "0:00:00.00");
    assert_eq!(lines[2][9], "Bye now.");
}

#[test]
fn ass_names_come_from_the_caller_then_the_layout() {
    let names = BTreeMap::from([("2".to_string(), "Anna, the host".to_string())]);
    let layout = Layout { speaker_names: vec!["Boris".into()], ..Layout::default() };
    let (text, _) = ass::build(&two_speakers(), &layout, &ass::Options { names, karaoke: false });
    let names: Vec<&str> = styles(&text).iter().map(|s| s[0]).collect();
    // A comma would end the field: it is dropped from style names and Name.
    assert_eq!(names, ["Default", "Boris", "Anna  the host"]);
    assert_eq!(dialogue(&text)[1][4], "Anna  the host");
}

#[test]
fn ass_speakers_with_the_same_name_keep_their_own_styles() {
    let names = BTreeMap::from([("1".to_string(), "Host".to_string()), ("2".to_string(), "Host".to_string())]);
    let (text, _) = ass::build(&two_speakers(), &Layout::default(), &ass::Options { names, karaoke: false });
    let names: Vec<&str> = styles(&text).iter().map(|s| s[0]).collect();
    assert_eq!(names, ["Default", "Host", "Host 2"]);
    let lines = dialogue(&text);
    assert_eq!((lines[1][3], lines[1][4]), ("Host 2", "Host"));
}

#[test]
fn ass_without_speakers_uses_default() {
    let words = sentence("No diarization here.", 0, None);
    let (text, _) = ass::build(&words, &Layout::default(), &ass::Options::default());
    assert_eq!(styles(&text).len(), 1);
    let l = &dialogue(&text)[0];
    assert_eq!((l[3], l[4]), ("Default", ""));
}

#[test]
fn ass_palette_repeats_after_the_last_colour() {
    let mut words = Vec::new();
    for i in 1..=10u64 {
        words.extend(sentence("Yes.", i * 3000, Some(&i.to_string())));
    }
    let (text, _) = ass::build(&words, &Layout::default(), &ass::Options::default());
    let st = styles(&text);
    assert_eq!(st.len(), 11);
    assert_eq!(st[9][3], st[1][3]);
    assert_eq!(st[10][3], st[2][3]);
    let mut colours: Vec<&str> = st[1..9].iter().map(|s| s[3]).collect();
    colours.dedup();
    assert_eq!(colours.len(), 8);
}

#[test]
fn ass_text_never_becomes_markup() {
    let words = vec![word("{loud}", 0, 300, None), word("back\\slash.", 400, 900, None)];
    let (text, _) = ass::build(&words, &Layout::default(), &ass::Options::default());
    assert_eq!(dialogue(&text)[0][9], "(loud) back/slash.");
}

#[test]
fn karaoke_times_every_word_from_its_start_to_the_next() {
    let words = vec![word("One,", 1000, 1200, None), word("two,", 1500, 1800, None), word("three.", 2000, 2600, None)];
    let opts = ass::Options { karaoke: true, ..Default::default() };
    let (text, _) = ass::build(&words, &Layout::default(), &opts);
    assert_eq!(dialogue(&text)[0][9], r"{\k50}One, {\k50}two, {\k60}three.");
}

#[test]
fn karaoke_keeps_line_breaks_dashes_and_the_speaker_label() {
    let mut words = sentence("Short one.", 0, Some("2"));
    let long = "This is a rather long sentence that has to wrap—onto a second line of the cue.";
    words.extend(sentence(long, 2000, Some("1")));
    let layout = Layout { wrap_lines: true, speaker_labels: true, ..Layout::default() };
    let opts = ass::Options { karaoke: true, ..Default::default() };
    let (text, _) = ass::build(&words, &layout, &opts);
    let plain = cues::cues(&words, &layout);
    let lines = dialogue(&text);
    assert_eq!(lines.len(), plain.len());
    for (line, cue) in lines.iter().zip(&plain) {
        let shown: String = line[9].split('{').map(|p| p.split_once('}').map_or(p, |(_, t)| t)).collect();
        assert_eq!(shown, cue.lines.join("\\N"));
        let total: u64 = line[9]
            .match_indices("\\k")
            .map(|(i, _)| line[9][i + 2..].split('}').next().unwrap().parse::<u64>().unwrap())
            .sum();
        assert_eq!(total, cue.end / 10 - cue.start / 10, "{}", line[9]);
    }
    let second = lines[1][9];
    assert!(second.starts_with(r"Speaker 1: {\k30}This "), "{second}");
    assert!(second.contains("\\N"), "{second}");
    assert!(second.contains("wrap—{\\k"), "the glued half of a dash word is timed on its own: {second}");
}

#[test]
fn a_word_with_a_space_keeps_the_marks_of_its_cue() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:03.000\n{0}I{10} {20}love{50} {60}New\\ York{120} {130}a{140} {150}lot.{180}\n{20..50} style:italic\n";
    let doc = captions::capt::Doc::parse(file).unwrap();
    let text = srt::build(&doc.to_words(), &Layout::default()).0;
    assert!(text.contains("I <i>love</i> New York a lot."), "{text}");
    let karaoke =
        ass::build(&doc.to_words(), &Layout::default(), &ass::Options { karaoke: true, ..Default::default() }).0;
    assert!(karaoke.contains(r"{\i1}{\k40}love {\i0}{\k70}New York {\k20}a"), "{karaoke}");
}

#[test]
fn an_adr_row_names_a_voice_once_where_it_starts() {
    let file = "CAPT 1\nspeaker:1 name:Anna\n\n\
                00:00:01.000 --> 00:00:02.000 speaker:1 voice:whisper\n{0}Come{30} {40}here.{90}\n\n\
                00:00:02.500 --> 00:00:03.500 speaker:1 voice:whisper\n{0}Quietly{40} {50}now.{90}\n";
    let doc = captions::capt::Doc::parse(file).unwrap();
    // Two subtitles, joined into one row.
    assert_eq!(srt::build(&doc.to_words(), &Layout::default()).1, 2);
    let (csv, rows) = captions::adr::build(&doc, &Layout::default(), &Default::default(), "en").unwrap();
    assert_eq!(rows, 1, "{csv}");
    assert_eq!(csv.matches("[whispering]").count(), 1, "{csv}");
}

fn adr(file: &str) -> Vec<String> {
    let doc = captions::capt::Doc::parse(file).unwrap();
    let (csv, _) = captions::adr::build(&doc, &Layout::default(), &Default::default(), "en").unwrap();
    csv.trim_start_matches('\u{feff}').split("\r\n").filter(|l| !l.is_empty()).map(str::to_string).collect()
}

#[test]
fn an_adr_row_puts_the_speakers_sound_where_it_is_heard() {
    let rows = adr("CAPT 1\nspeaker:1 name:Anna\n\n\
                    00:00:00.000 --> 00:00:06.000 speaker:1\n{0}One{50} {100}two{150} {200}three{250} {300}four.{350}\n\n\
                    00:00:01.200 --> 00:00:01.600 speaker:1 sound:laughter\n");
    assert_eq!(rows, [r#""1","01:00:00:00","01:00:03:13","Anna","One two [laughs] three four.""#]);
}

#[test]
fn adr_rows_join_across_another_speakers_sound_but_not_their_speech() {
    let file = "CAPT 1\nspeaker:1 name:Anna\nspeaker:2 name:Boris\n\n\
                00:00:00.000 --> 00:00:03.000 speaker:1\n{0}First{100} {150}part.{300}\n\n\
                00:00:01.000 --> 00:00:02.000 speaker:2 sound:laughter\n\n\
                00:00:03.200 --> 00:00:05.000 speaker:1\n{0}Second{100} {120}part.{180}\n";
    let rows = adr(file);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows[0].ends_with(r#""Anna","First part. Second part.""#), "{rows:?}");
    assert!(rows[1].ends_with(r#""Boris","[laughs]""#), "{rows:?}");
    // Boris speaking between them ends Anna's row.
    let rows = adr(&file.replace("speaker:2 sound:laughter\n", "speaker:2\n{0}Ha.{50}\n"));
    assert_eq!(rows.len(), 3, "{rows:?}");
}

#[test]
fn adr_rows_end_at_a_long_pause_and_at_eight_seconds() {
    let file = "CAPT 1\n\n00:00:00.000 --> 00:00:01.000 speaker:1\n{0}One.{100}\n\n\
                00:00:02.600 --> 00:00:03.000 speaker:1\n{0}Two.{40}\n";
    assert_eq!(adr(file).len(), 2, "a pause over 1.5 s");
    let mut long = String::from("CAPT 1\n");
    for i in 0..5u64 {
        long.push_str(&format!(
            "\n00:00:{:02}.000 --> 00:00:{:02}.900 speaker:1\n{{0}}Word{i}.{{90}}\n",
            i * 2,
            i * 2 + 1
        ));
    }
    let rows = adr(&long);
    assert_eq!(rows.len(), 2, "{rows:?}");
}

#[test]
fn an_adr_row_without_a_speaker_names_speaker_1_and_the_list_ends_with_the_day() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}Hi.{50}\n";
    assert!(adr(file)[0].contains(r#""Speaker 1","Hi.""#));
    let doc = captions::capt::Doc::parse(file).unwrap();
    let late = captions::adr::Timecode::parse("23:59:59:00").unwrap();
    let o = captions::adr::Options { start: late, ..Default::default() };
    let err = captions::adr::build(&doc, &Layout::default(), &o, "en").unwrap_err();
    assert!(err.contains("23:59:59"), "{err}");
}

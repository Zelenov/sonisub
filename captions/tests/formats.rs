//! The formats on hand-made words: one test per rule. Golden output for real transcripts is in
//! sonisub's tests (tests/formats.rs), next to the fixtures.

use std::collections::BTreeMap;

use captions::{Layout, Word, ass, cues, sbv, srt, vtt};

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
    assert_eq!(sbv::build(&words, &layout).1, n);
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
    // SBV: hours unpadded, no markup to escape.
    assert_eq!(sbv::build(&words, &Layout::default()).0, "1:02:03.004,1:02:04.000\nTom & <Jerry>.\n\n");
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
fn lrc_glues_words_keeps_long_times_and_writes_nothing_for_nothing() {
    let mut words = sentence("don’t stop.", 0, None);
    words[1].glued = true;
    words[1].text = "’t".into();
    words[0].text = "don".into();
    let (text, n) = captions::lrc::build(&words, &Layout::default());
    assert_eq!(n, 1);
    assert!(text.contains("<00:00.00>don<00:00.30>’t"), "{text}");
    // Minutes past 99 keep their digits.
    let late = vec![word("Late.", 6_000_000, 6_000_500, None)];
    assert!(captions::lrc::build(&late, &Layout::default()).0.starts_with("[100:00.00]"));
    assert_eq!(captions::lrc::build(&[], &Layout::default()), (String::new(), 0));
}
fn transcript(file: &str) -> String {
    captions::transcript::build(&captions::capt::Doc::parse(file).unwrap(), &Layout::default(), "en")
}

#[test]
fn a_transcript_keeps_a_sentence_whole_when_someone_else_talks_or_laughs_in_it() {
    let overlap = "CAPT 1\nspeaker:1 name:Anna\nspeaker:2 name:Boris\n\n\
                   00:00:01.000 --> 00:00:04.000 speaker:1\n{0}So{20} {50}I{70} {100}read{150} {200}twice.{300}\n\n\
                   00:00:01.300 --> 00:00:02.500 speaker:2\n{0}Yeah{40} {60}right.{120}\n\n\
                   00:00:02.000 --> 00:00:02.500 speaker:2 sound:laughter\n";
    assert_eq!(
        transcript(overlap),
        "Anna  [00:00:01]\nSo I read twice.\n\nBoris  [00:00:01]\nYeah right. [laughs]\n\n"
    );
}

#[test]
fn a_sound_of_no_one_comes_after_the_sentence_it_falls_in() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:01.500\n{0}One{20} {30}two.{50}\n\n\
                00:00:01.350 --> 00:00:01.900 sound:phone\n\n\
                00:00:02.000 --> 00:00:02.500\n{0}Three{20} {30}four.{50}\n";
    assert_eq!(transcript(file), "[00:00:01]\nOne two.\n\n[00:00:01]\n[phone ringing]\n\n[00:00:02]\nThree four.\n\n");
}

#[test]
fn a_long_turn_is_cut_into_paragraphs_at_pauses() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}Hello{30} {40}there.{90}\n\n\
                00:01:10.000 --> 00:01:11.000\n{0}Much{30} {40}later.{90}\n\n\
                00:01:12.000 --> 00:01:13.000\n{0}Right{30} {40}after.{90}\n";
    assert_eq!(transcript(file), "[00:00:01]\nHello there.\n\n[00:01:10]\nMuch later. Right after.\n\n");
}

#[test]
fn what_waits_for_a_sentence_that_never_ends_comes_after_it() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:02.000 speaker:1\n{0}And{30} {40}so{90}\n\n\
                00:00:01.500 --> 00:00:02.000 sound:phone\n";
    assert_eq!(transcript(file), "Speaker 1  [00:00:01]\nAnd so\n\n[00:00:01]\n[phone ringing]\n\n");
}

#[test]
fn an_unfinished_sentence_does_not_hold_back_what_is_said_after_it() {
    let file = "CAPT 1\nspeaker:1 name:Anna\nspeaker:2 name:Boris\n\n\
                00:00:01.000 --> 00:00:01.500 speaker:1\n{0}I{20} {30}think{50}\n\n\
                00:00:02.000 --> 00:00:10.000 speaker:2\n{0}No.{50} {100}That{150} {200}is{250} {300}wrong.{400}\n\n\
                00:01:00.000 --> 00:01:00.500 speaker:1\n{0}Okay.{50}\n";
    assert_eq!(
        transcript(file),
        "Anna  [00:00:01]\nI think\n\nBoris  [00:00:02]\nNo. That is wrong.\n\nAnna  [00:01:00]\nOkay.\n\n"
    );
}

#[test]
fn a_sound_of_no_one_ends_the_paragraph_at_its_sentence() {
    let file = "CAPT 1\n\n00:00:10.000 --> 00:00:11.000 speaker:1\n{0}Still{40} {50}here.{100}\n\n\
                00:00:10.200 --> 00:00:11.000 sound:phone\n\n\
                00:00:11.200 --> 00:00:11.800 speaker:1 sound:laughter\n";
    // The speaker goes on under a new heading after the sound.
    assert_eq!(
        transcript(file),
        "Speaker 1  [00:00:10]\nStill here.\n\n[00:00:10]\n[phone ringing]\n\nSpeaker 1  [00:00:11]\n[laughs]\n\n"
    );
}

#[test]
fn a_minute_long_paragraph_is_cut_at_its_next_sentence_end() {
    let file = "CAPT 1\n\n00:00:00.000 --> 00:00:40.000\n{0}One{100} {2000}long{3000} {3900}sentence.{4000}\n\n\
                00:00:41.000 --> 00:01:05.000\n{0}And{100} {2300}more.{2400}\n\n\
                00:01:06.000 --> 00:01:07.000\n{0}Then{40} {50}this.{100}\n";
    assert_eq!(transcript(file), "[00:00:00]\nOne long sentence. And more.\n\n[00:01:06]\nThen this.\n\n");
}

#[test]
fn speech_without_punctuation_is_cut_after_two_minutes() {
    let mut file = String::from("CAPT 1\n");
    for i in 0..50u64 {
        let t = i * 3;
        file.push_str(&format!(
            "\n00:{:02}:{:02}.000 --> 00:{:02}:{:02}.500\n{{0}}word{{50}}\n",
            t / 60,
            t % 60,
            t / 60,
            t % 60
        ));
    }
    let text = transcript(&file);
    assert_eq!(text.matches("[00:").count(), 2, "{text}");
    assert!(text.contains("[00:02:03]"), "{text}");
}

#[test]
fn what_waited_comes_before_a_later_sound() {
    let file = "CAPT 1\nspeaker:1 name:Anna\nspeaker:2 name:Boris\n\n\
                00:00:00.000 --> 00:00:02.000 speaker:1\n{0}Hello{100} {110}there{200}\n\n\
                00:00:01.000 --> 00:00:01.500 speaker:2\n{0}Yes.{50}\n\n\
                00:00:02.100 --> 00:00:03.000 speaker:1\n{0}friend.{90}\n\n\
                00:01:00.000 --> 00:01:01.000 sound:phone\n";
    assert_eq!(
        transcript(file),
        "Anna  [00:00:00]\nHello there friend.\n\nBoris  [00:00:01]\nYes.\n\n[00:01:00]\n[phone ringing]\n\n"
    );
}

#[test]
fn a_speakers_own_sound_does_not_keep_their_sentence_open() {
    let file = "CAPT 1\nspeaker:1 name:Anna\nspeaker:2 name:Boris\n\n\
                00:00:00.000 --> 00:00:01.000 speaker:1\n{0}I{20} {30}think{100}\n\n\
                00:00:01.000 --> 00:00:30.000 speaker:1 sound:laughter\n\n\
                00:00:05.000 --> 00:00:05.500 speaker:2\n{0}No.{50}\n\n\
                00:00:31.000 --> 00:00:31.500 speaker:1\n{0}so.{50}\n";
    assert_eq!(
        transcript(file),
        "Anna  [00:00:00]\nI think [laughs]\n\nBoris  [00:00:05]\nNo.\n\nAnna  [00:00:31]\nso.\n\n"
    );
}

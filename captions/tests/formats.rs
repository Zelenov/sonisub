//! The formats on hand-made words: one test per rule. Golden output for real transcripts is in
//! sonisub's tests (tests/formats.rs), next to the fixtures.

use std::collections::BTreeMap;

use captions::{Layout, Word, ass, cues, srt, vtt, ytt};

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
    assert_eq!(ytt::build(&words, &layout).1, n);
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

fn ytt_of(file: &str) -> String {
    let doc = captions::capt::Doc::parse(file).unwrap();
    let layout = Layout { speaker_labels: true, by_speaker: true, ..Layout::default() };
    ytt::build(&doc.to_words(), &layout).0
}

#[test]
fn ytt_escapes_text_and_keeps_a_word_with_a_space_in_one_section() {
    let ytt = ytt_of(
        "CAPT 1\n\n00:00:01.000 --> 00:00:03.000\n{0}Tom{20} {30}&{40} {50}<Jerry>{80} {90}in{100} {110}New\\ York.{150}\n",
    );
    assert!(ytt.contains("<s>Tom</s>\u{200b}<s t=\"300\"> &amp;</s><s t=\"500\"> &lt;Jerry&gt;</s>"), "{ytt}");
    assert!(ytt.contains(r#"<s t="1100"> New York.</s>"#), "{ytt}");
}

#[test]
fn ytt_times_a_dashed_word_and_puts_the_speaker_on_the_first_word() {
    let ytt = ytt_of(
        "CAPT 1\n\n00:00:01.000 --> 00:00:02.000 speaker:1\n{0}Well{20} {30}finally—and{100}\n\n\
                      00:00:02.500 --> 00:00:03.000 speaker:2\n{0}Yes.{50}\n",
    );
    // The dash splits the word: its two parts, each with its own time (by characters), the second
    // glued on.
    assert!(
        ytt.contains("<s>Speaker 1:</s>\u{200b}<s t=\"1\"> Well</s><s t=\"300\"> finally—</s><s t=\"809\">and</s>"),
        "{ytt}"
    );
}

#[test]
fn a_ytt_line_starts_with_an_empty_section_and_never_at_zero() {
    let ytt =
        ytt_of("CAPT 1\n\n00:00:00.000 --> 00:00:01.000\n{0}Sing{20} {30}a{40} {50}song.{80}\n{0..80} style:italic\n");
    // The server drops the first section's pen: the italic words come after an empty one.
    assert!(
        ytt.contains("<p t=\"1\" d=\"799\" wp=\"1\" ws=\"1\"><s p=\"2\">Sing</s>\u{200b}<s p=\"2\" t=\"299\"> a</s>"),
        "{ytt}"
    );
    // A line of one italic word: the pen on the line.
    let one = ytt_of("CAPT 1\n\n00:00:01.000 --> 00:00:01.500\n{0}Who?{50}\n{0..50} style:italic\n");
    assert!(one.contains("<p t=\"1000\" d=\"500\" wp=\"1\" ws=\"1\" p=\"2\">Who?</p>"), "{one}");
}

#[test]
fn a_ytt_sound_during_speech_at_the_bottom_goes_to_the_top() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:04.000\n{0}Hello{100} {110}there{200} {210}friend.{300}\n\n\
                00:00:02.000 --> 00:00:03.000 sound:phone\n\n00:00:05.000 --> 00:00:06.000 sound:music\n";
    let doc = captions::capt::Doc::parse(file).unwrap();
    let ytt = captions::sdh::ytt(&doc, &Layout::default(), "en").0;
    let top = ytt.lines().find(|l| l.contains(r#"ap="1""#)).unwrap();
    let top_id = top.split("id=\"").nth(1).unwrap().split('"').next().unwrap();
    assert!(ytt.contains(&format!("wp=\"{top_id}\" ws=\"1\">[phone ringing]")), "{ytt}");
    // A sound alone stays at the bottom.
    assert!(ytt.contains(r#"wp="1" ws="1">[music]"#), "{ytt}");
}

#[test]
fn ytt_lines_at_the_same_time_never_share_a_window() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:04.000\n{0}Hello{100} {110}there{200} {210}friend.{300}\n\n\
                00:00:01.500 --> 00:00:03.000 sound:phone\n\n00:00:02.000 --> 00:00:03.500 sound:music\n";
    let doc = captions::capt::Doc::parse(file).unwrap();
    let ytt = captions::sdh::ytt(&doc, &Layout::default(), "en").0;
    let ps: Vec<(u64, u64, String)> = ytt
        .lines()
        .filter(|l| l.starts_with("<p "))
        .map(|l| {
            let attr = |n: &str| l.split(&format!("{n}=\"")).nth(1).unwrap().split('"').next().unwrap().to_string();
            let t: u64 = attr("t").parse().unwrap();
            (t, t + attr("d").parse::<u64>().unwrap(), attr("wp"))
        })
        .collect();
    for (i, a) in ps.iter().enumerate() {
        for b in &ps[i + 1..] {
            assert!(!(a.2 == b.2 && a.0 < b.1 && b.0 < a.1), "{a:?} and {b:?} share a window\n{ytt}");
        }
    }
    // Sections start a millisecond apart at least, a voice label and its word too.
    let voiced = {
        let doc = captions::capt::Doc::parse(
            "CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}Go{40} {50}now.{90}\n{50..90} voice:whisper\n",
        )
        .unwrap();
        captions::sdh::ytt(&doc, &Layout::default(), "en").0
    };
    assert!(voiced.contains("<s t=\"500\"> [whispering] </s><s t=\"501\">now.</s>"), "{voiced}");
}

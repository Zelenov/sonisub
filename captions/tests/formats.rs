//! The formats on hand-made words: one test per rule. Golden output for real transcripts is in
//! sonisub's tests (tests/formats.rs), next to the fixtures.

use std::collections::BTreeMap;

use captions::{Layout, Word, ass, cues, sbv, srt, vtt, ytt};

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
#[test]
fn ttml_agent_ids_are_xml_names_and_differ() {
    let file = "CAPT 1\nspeaker:\"Anna Lee\"\nspeaker:Anna_Lee\n\n\
                00:00:01.000 --> 00:00:02.000 speaker:\"Anna Lee\"\n{0}Hi.{50}\n\n\
                00:00:02.500 --> 00:00:03.500 speaker:Anna_Lee\n{0}Hello.{50}\n\n\
                00:00:04.000 --> 00:00:05.000 speaker:3\n{0}Hey.{50}\n";
    let doc = captions::capt::Doc::parse(file).unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::default()).0;
    for id in ["speaker_Anna_Lee", "speaker_Anna_Lee_2", "speaker_3"] {
        assert!(ttml.contains(&format!("xml:id=\"{id}\"")), "{id}\n{ttml}");
    }
    assert!(ttml.contains("ttm:agent=\"speaker_Anna_Lee_2\">Hello."), "{ttml}");
}

#[test]
fn a_point_at_the_edge_gets_a_whole_region_and_tt_always_has_a_language() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:02.000 pos:0.5,0\n{0}Top.{50}\n\n\
                00:00:03.000 --> 00:00:04.000 pos:0.03,0.5\n{0}Near{20} {30}the{40} {50}edge.{80}\n";
    let doc = captions::capt::Doc::parse(file).unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::default()).0;
    assert!(ttml.contains(r#"<region xml:id="r1" tts:origin="10% 0%" tts:extent="80% 20%" tts:displayAlign="before" tts:textAlign="center"/>"#), "{ttml}");
    assert!(ttml.contains(r#"<region xml:id="r2" tts:origin="0% 40%" tts:extent="80% 20%" tts:displayAlign="center" tts:textAlign="left"/>"#), "{ttml}");
    assert!(ttml.contains(r#"xml:lang="und""#), "{ttml}");
}

/// Every pair of regions shown at the same moment in a TTML document, by id.
fn ttml_regions_shown_together(ttml: &str) -> Vec<(String, String)> {
    let attr = |line: &str, name: &str| -> String {
        let start = line.find(&format!("{name}=\"")).unwrap() + name.len() + 2;
        line[start..].split('"').next().unwrap().to_string()
    };
    let ms = |t: &str| -> u64 {
        let p: Vec<f64> = t.split(':').map(|x| x.parse().unwrap()).collect();
        ((p[0] * 3600.0 + p[1] * 60.0 + p[2]) * 1000.0).round() as u64
    };
    let ps: Vec<(u64, u64, String)> = ttml
        .lines()
        .filter(|l| l.trim_start().starts_with("<p "))
        .map(|l| (ms(&attr(l, "begin")), ms(&attr(l, "end")), attr(l, "region")))
        .collect();
    let mut pairs = Vec::new();
    for (i, a) in ps.iter().enumerate() {
        for b in &ps[i + 1..] {
            if a.0 < b.1 && b.0 < a.1 && a.2 != b.2 {
                pairs.push((a.2.clone(), b.2.clone()));
            }
        }
    }
    pairs
}

#[test]
fn ttml_never_shows_two_overlapping_regions_at_once() {
    // A point over the bottom region, with someone else speaking at the bottom during it.
    let near_bottom = "CAPT 1\n\n00:00:01.000 --> 00:00:03.000 speaker:1 pos:0.5,0.85\n{0}Low{150}\n\n\
                       00:00:01.500 --> 00:00:02.500 speaker:2\n{0}Hi.{50}\n";
    let doc = captions::capt::Doc::parse(near_bottom).unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::default()).0;
    assert!(ttml.contains(r#"region="r1" ttm:agent="speaker_2">Hi."#), "{ttml}");
    // The top and a rectangle over it, spoken at once.
    let top_and_box = "CAPT 1\n\n00:00:01.000 --> 00:00:03.000 speaker:1 pos:top\n{0}Up{50}\n\n\
                       00:00:01.500 --> 00:00:02.500 speaker:2 pos:0.05,0.05,0.4,0.2\n{0}Box{50}\n";
    let doc = captions::capt::Doc::parse(top_and_box).unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::default()).0;
    assert_eq!(ttml_regions_shown_together(&ttml), Vec::<(String, String)>::new(), "{ttml}");
}

#[test]
fn ttml_regions_stay_inside_the_frame_and_fit_the_layout() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:03.000 pos:0.0005,0,0.9995,0.5\n{0}Wide{50}\n";
    let doc = captions::capt::Doc::parse(file).unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::default()).0;
    assert!(ttml.contains(r#"xml:id="r1" tts:origin="0.1% 0%" tts:extent="99.9% 50%""#), "{ttml}");
    // A line of 22 characters fits with room to spare: 15 rows (IMSC's default) is enough.
    let ttml = captions::ttml::build(&doc, &Layout { max_line: 22, ..Layout::default() }).0;
    assert!(ttml.contains(r#"ttp:cellResolution="27 15""#), "{ttml}");
}

#[test]
fn a_ttml_region_grows_to_hold_everything_shown_in_it() {
    // Two lines of speech at the bottom and another speaker's line under them, at once: three lines and more.
    let file = "CAPT 1\nlang:ru\n\n00:00:01.000 --> 00:00:03.000\n\
                {0}Один{20} {30}два{50} {60}три{80} {90}четыре{110} {120}пять{140} {150}шесть{170} {175}семь{185} \
                {186}восемь{190} {191}девять{195} {196}десять.{199}\n\n\
                00:00:02.000 --> 00:00:03.000 speaker:2\n{0}Алло?{50}\n";
    let doc = captions::capt::Doc::parse(file).unwrap();
    let layout = Layout { wrap_lines: true, max_line: 20, max_lines: 4, ..Layout::default() };
    let ttml = captions::ttml::build(&doc, &layout).0;
    let bottom = ttml.lines().find(|l| l.contains(r#"xml:id="bottom""#)).unwrap();
    let height: f64 = bottom.split("tts:extent=\"80% ").nth(1).unwrap().split('%').next().unwrap().parse().unwrap();
    // A line is 1.25 of 1080 / 15 rows.
    let lines = ttml.lines().filter(|l| l.contains("<p ")).map(|l| l.matches("<br/>").count() + 1).sum::<usize>();
    assert!(height >= lines as f64 * 1.25 / 15.0 * 100.0 - 0.2, "{lines} lines in {height}%\n{ttml}");
}

#[test]
fn ttml_takes_the_first_word_language_when_the_file_has_none() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:02.000 lang:ru\n{0}Привет.{50}\n";
    let ttml = captions::ttml::build(&captions::capt::Doc::parse(file).unwrap(), &Layout::default()).0;
    assert!(ttml.contains(r#"xml:lang="ru">"#), "{ttml}");
    // An empty lang: is no language.
    let file = "CAPT 1\nlang:\n\n00:00:01.000 --> 00:00:02.000\n{0}Hi.{50}\n";
    let ttml = captions::ttml::build(&captions::capt::Doc::parse(file).unwrap(), &Layout::default()).0;
    assert!(ttml.contains(r#"xml:lang="und">"#), "{ttml}");
}

#[test]
fn a_ttml_region_that_takes_a_moved_subtitle_grows_for_both() {
    // 59 characters each: two lines each at the default 50; the box overlaps the top, so it moves in.
    let long = |from: u64| {
        (0..10).map(|i| format!("{{{}}}word{i}{{{}}}", from + i * 9, from + i * 9 + 8)).collect::<Vec<_>>().join(" ")
    };
    let file = format!(
        "CAPT 1\n\n00:00:01.000 --> 00:00:03.000 speaker:1 pos:top\n{}\n\n\
         00:00:01.500 --> 00:00:03.000 speaker:2 pos:0.05,0.05,0.4,0.2\n{}\n",
        long(0),
        long(0)
    );
    let doc = captions::capt::Doc::parse(&file).unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::default()).0;
    assert!(ttml.contains(r#"region="top" ttm:agent="speaker_2""#), "{ttml}");
    let top = ttml.lines().find(|l| l.contains(r#"xml:id="top""#)).unwrap();
    let height: f64 = top.split("tts:extent=\"80% ").nth(1).unwrap().split('%').next().unwrap().parse().unwrap();
    // Four lines of 1.25 cells, 20 rows.
    assert!(height >= 4.0 * 1.25 / 20.0 * 100.0 - 0.2, "{height}%\n{ttml}");
}

/// Checks a TTML document the way IMSC 1.1 Text asks: at any moment at most four regions are
/// shown, none of them overlap, and each is tall enough for its lines at `line` (a share of the
/// frame). Returns the problems found.
fn imsc_problems(ttml: &str) -> Vec<String> {
    let attr = |line: &str, name: &str| -> Option<String> {
        let start = line.find(&format!("{name}=\""))? + name.len() + 2;
        Some(line[start..].split('"').next()?.to_string())
    };
    let pct = |s: &str| -> Vec<f64> {
        s.split(' ').map(|v| v.trim_end_matches('%').parse::<f64>().unwrap() / 100.0).collect()
    };
    let regions: Vec<(String, [f64; 4])> = ttml
        .lines()
        .filter(|l| l.contains("<region "))
        .map(|l| {
            let (o, e) = (pct(&attr(l, "tts:origin").unwrap()), pct(&attr(l, "tts:extent").unwrap()));
            (attr(l, "xml:id").unwrap(), [o[0], o[1], e[0], e[1]])
        })
        .collect();
    let rect = |id: &str| regions.iter().find(|(r, _)| r == id).unwrap().1;
    let ms = |t: &str| -> u64 {
        let p: Vec<f64> = t.split(':').map(|x| x.parse().unwrap()).collect();
        ((p[0] * 3600.0 + p[1] * 60.0 + p[2]) * 1000.0).round() as u64
    };
    let ps: Vec<(u64, u64, String)> = ttml
        .lines()
        .filter(|l| l.trim_start().starts_with("<p "))
        .map(|l| (ms(&attr(l, "begin").unwrap()), ms(&attr(l, "end").unwrap()), attr(l, "region").unwrap()))
        .collect();
    let mut problems = Vec::new();
    for (t, _, _) in &ps {
        let mut on: Vec<&str> = ps.iter().filter(|(b, e, _)| b <= t && e > t).map(|(_, _, r)| r.as_str()).collect();
        on.sort_unstable();
        on.dedup();
        if on.len() > 4 {
            problems.push(format!("{} regions at {t}", on.len()));
        }
        for (i, a) in on.iter().enumerate() {
            for b in &on[i + 1..] {
                let ([ax, ay, aw, ah], [bx, by, bw, bh]) = (rect(a), rect(b));
                if ax < bx + bw - 1e-6 && bx < ax + aw - 1e-6 && ay < by + bh - 1e-6 && by < ay + ah - 1e-6 {
                    problems.push(format!("{a} over {b} at {t}"));
                }
            }
        }
    }
    problems
}

fn timed(text: &str, from: u64) -> String {
    text.split(' ')
        .enumerate()
        .map(|(i, w)| format!("{{{}}}{w}{{{}}}", from + i as u64 * 9, from + i as u64 * 9 + 8))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_narrow_ttml_box_grows_for_its_wrapped_lines() {
    let file = format!(
        "CAPT 1\n\n00:00:01.000 --> 00:00:03.000 pos:0.45,0.45,0.1,0.05\n{}\n",
        timed("Forty characters of text here, okay.", 0)
    );
    let doc = captions::capt::Doc::parse(&file).unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::default()).0;
    let r1 = ttml.lines().find(|l| l.contains(r#"xml:id="r1""#)).unwrap();
    let height: f64 =
        r1.split("tts:extent=\"").nth(1).unwrap().split(' ').nth(1).unwrap().trim_end_matches("%\"").parse().unwrap();
    // 10% of 1920 px holds 6 characters of 54 px text: 36 characters are 6 lines of 1.25 cells, 20 rows.
    assert!(height >= 6.0 * 1.25 / 20.0 * 100.0, "{height}%\n{ttml}");
}

#[test]
fn ttml_shows_at_most_four_regions_and_none_overlap() {
    let file = format!(
        "CAPT 1\n\n00:00:01.000 --> 00:00:03.000 speaker:1 pos:top\n{}\n\n\
         00:00:01.000 --> 00:00:03.000 speaker:2 pos:middle\n{}\n\n\
         00:00:01.000 --> 00:00:03.000 speaker:3\n{}\n\n\
         00:00:01.000 --> 00:00:03.000 speaker:4 pos:0.0,0.3,0.09,0.05\n{}\n\n\
         00:00:01.000 --> 00:00:03.000 speaker:5 pos:0.91,0.3,0.09,0.05\n{}\n",
        timed("Up.", 0),
        timed("Mid.", 0),
        timed("Low.", 0),
        timed("Left.", 0),
        timed("Right.", 0)
    );
    let doc = captions::capt::Doc::parse(&file).unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::default()).0;
    assert_eq!(imsc_problems(&ttml), Vec::<String>::new(), "{ttml}");
    // Nine thin boxes at once, no length limit: they end in regions that do not overlap.
    let boxes: Vec<String> = (0..9)
        .map(|i| {
            format!(
                "00:00:01.000 --> 00:00:03.000 speaker:{i} pos:0.1,{},0.8,0.01\n{}\n",
                i as f64 * 0.05,
                timed("Some words here.", 0)
            )
        })
        .collect();
    let doc = captions::capt::Doc::parse(&format!("CAPT 1\n\n{}", boxes.join("\n"))).unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::unlimited()).0;
    assert_eq!(imsc_problems(&ttml), Vec::<String>::new(), "{ttml}");
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

#[test]
fn an_adr_cue_lasts_as_long_as_the_speakers_sound_in_it() {
    let rows = adr("CAPT 1\nspeaker:1 name:Anna\n\n\
                    00:00:00.000 --> 00:00:02.000 speaker:1\n{0}So{50} {100}funny.{200}\n\n\
                    00:00:01.800 --> 00:00:04.000 speaker:1 sound:laughter\n");
    assert_eq!(rows, [r#""1","01:00:00:00","01:00:04:00","Anna","So funny. [laughs]""#]);
    // A sound of no length is still a frame long.
    let rows = adr("CAPT 1\n\n00:00:01.000 --> 00:00:01.000 speaker:1 sound:laughter\n");
    assert!(rows[0].starts_with(r#""1","01:00:01:00","01:00:01:01""#), "{rows:?}");
}

#[test]
fn an_adr_row_counts_its_labels_in_its_length() {
    let mut file = String::from("CAPT 1\n");
    for i in 0..6u64 {
        let voice = if i % 2 == 0 { "whisper" } else { "shout" };
        file.push_str(&format!(
            "\n00:00:{:02}.000 --> 00:00:{:02}.900 speaker:1 voice:{voice}\n{{0}}aaaaaaaaaaaaaaaaaaaa{{20}} {{30}}bbbbbbbbbbbbbbbbbbbb.{{50}}\n",
            i, i
        ));
    }
    for row in adr(&file) {
        let dialogue = row.rsplit("\",\"").next().unwrap().trim_end_matches('"');
        assert!(dialogue.chars().count() <= 200, "{row}");
    }
}

#[test]
fn another_speakers_speech_in_between_ends_an_adr_row_whatever_the_order_of_rows() {
    let rows = adr("CAPT 1\nspeaker:1 name:Anna\nspeaker:2 name:Boris\n\n\
                    00:00:00.000 --> 00:00:01.000 speaker:1\n{0}One.{100}\n\n\
                    00:00:00.200 --> 00:00:00.800 speaker:2 sound:laughter\n\n\
                    00:00:01.200 --> 00:00:02.000 speaker:1\n{0}Two.{80}\n\n\
                    00:00:02.100 --> 00:00:03.000 speaker:2\n{0}Hey.{90}\n");
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows[2].ends_with(r#""Boris","Hey.""#), "{rows:?}");
}

#[test]
fn an_adr_row_keeps_room_for_its_voice_label_and_a_sound_stops_at_eight_seconds() {
    let words: Vec<String> = (0..40).map(|i| format!("{{{}}}word{{{}}}", i * 10, i * 10 + 8)).collect();
    let file = format!("CAPT 1\n\n00:00:00.000 --> 00:00:05.000 speaker:1 voice:whisper\n{}\n", words.join(" "));
    for row in adr(&file) {
        let dialogue = row.rsplit("\",\"").next().unwrap().trim_end_matches('"');
        assert!(dialogue.chars().count() <= 200, "{row}");
    }
    let rows = adr("CAPT 1\n\n00:00:00.000 --> 00:00:07.500 speaker:1\n{0}Long{700} {710}one.{750}\n\n\
                    00:00:07.000 --> 00:00:12.000 speaker:1 sound:laughter\n");
    assert!(rows[0].contains(r#""01:00:00:00","01:00:08:00""#), "{rows:?}");
}

#[test]
fn speech_of_no_one_among_named_speakers_is_unknown_in_the_labels_language() {
    let file = "CAPT 1\nspeaker:1 name:Anna\n\n00:00:00.000 --> 00:00:01.000\n{0}Hm.{50}\n";
    let doc = captions::capt::Doc::parse(file).unwrap();
    let (csv, _) = captions::adr::build(&doc, &Layout::default(), &Default::default(), "ru").unwrap();
    assert!(csv.contains(r#""Неизвестный","Hm.""#), "{csv}");
}

fn max_dialogue(rows: &[String]) -> usize {
    rows.iter().map(|r| r.rsplit("\",\"").next().unwrap().trim_end_matches('"').chars().count()).max().unwrap_or(0)
}

#[test]
fn an_adr_row_stays_within_200_characters_with_its_sounds_and_repeated_voices() {
    let word = "abcdefghijklmnopqrst";
    let words: Vec<String> = (0..9).map(|i| format!("{{{}}}{word}{{{}}}", i * 50, i * 50 + 40)).collect();
    let file = format!(
        "CAPT 1\n\n00:00:00.000 --> 00:00:05.000 speaker:1\n{}\n\n\
         00:00:01.000 --> 00:00:01.400 speaker:1 sound:laughter\n\n00:00:03.000 --> 00:00:03.400 speaker:1 sound:laughter\n",
        words.join(" ")
    );
    assert!(max_dialogue(&adr(&file)) <= 200, "{:?}", adr(&file));
    let words: Vec<String> = (0..31).map(|i| format!("{{{}}}abcde{{{}}}", i * 13, i * 13 + 10)).collect();
    let file = format!(
        "CAPT 1\n\n00:00:00.000 --> 00:00:05.000 speaker:1\n{}\n{{0..101}} voice:whisper\n{{299..400}} voice:whisper\n",
        words.join(" ")
    );
    assert!(max_dialogue(&adr(&file)) <= 200, "{:?}", adr(&file));
}

#[test]
fn a_sound_belongs_to_the_cue_whose_words_it_is_heard_among() {
    // Words cut at 8 s, the next cue at 7.95 s; a laugh from 7.4 s runs into it, a cough is in it.
    let first: Vec<String> = (0..16).map(|i| format!("{{{}}}w{i}{{{}}}", i * 49, i * 49 + 40)).collect();
    let file = format!(
        "CAPT 1\n\n00:00:00.000 --> 00:00:08.000 speaker:1\n{}\n\n\
         00:00:07.400 --> 00:00:09.000 speaker:1 sound:laughter\n\n\
         00:00:07.950 --> 00:00:10.000 speaker:1\n{{0}}v0{{40}} {{50}}v1{{90}}\n\n\
         00:00:07.970 --> 00:00:08.100 speaker:1 sound:cough\n",
        first.join(" ")
    );
    let rows = adr(&file);
    let first_row = rows.iter().find(|r| r.contains("w0")).unwrap();
    assert!(!first_row.contains("[coughs]"), "{rows:?}");
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
fn a_ytt_line_during_speech_at_the_bottom_goes_to_the_top() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:04.000 speaker:1\n{0}Hello{100} {110}there{200} {210}friend.{300}\n\n\
                00:00:02.000 --> 00:00:03.000 speaker:2\n{0}Hi.{50}\n\n00:00:05.000 --> 00:00:06.000 speaker:2\n{0}Bye.{50}\n";
    let ytt = ytt_of(file);
    let top = ytt.lines().find(|l| l.contains(r#"ap="1""#)).unwrap();
    let top_id = top.split("id=\"").nth(1).unwrap().split('"').next().unwrap();
    assert!(ytt.contains(&format!("wp=\"{top_id}\" ws=\"1\"><s>Speaker 2:</s>")), "{ytt}");
    // A line alone stays at the bottom.
    assert!(ytt.contains(r#"wp="1" ws="1">Bye."#), "{ytt}");
}

#[test]
fn ytt_lines_at_the_same_time_never_share_a_window() {
    let file = "CAPT 1\n\n00:00:01.000 --> 00:00:04.000 speaker:1\n{0}Hello{100} {110}there{200} {210}friend.{300}\n\n\
                00:00:01.500 --> 00:00:03.000 speaker:2\n{0}Hi.{50}\n\n00:00:02.000 --> 00:00:03.500 speaker:3\n{0}Hey.{50}\n";
    let ytt = ytt_of(file);
    assert_eq!(ytt.matches("<p ").count(), 3, "{ytt}");
    assert_eq!(ytt_shared_windows(&ytt), Vec::<(String, String)>::new(), "{ytt}");
}

/// The value of attribute `n` in a YTT line.
fn ytt_attr(l: &str, n: &str) -> String {
    l.split(&format!(" {n}=\"")).nth(1).unwrap().split('"').next().unwrap().to_string()
}

/// Every pair of `<p>` shown at the same time in one window.
fn ytt_shared_windows(ytt: &str) -> Vec<(String, String)> {
    let ps: Vec<(u64, u64, String, String)> = ytt
        .lines()
        .filter(|l| l.starts_with("<p "))
        .map(|l| {
            let t: u64 = ytt_attr(l, "t").parse().unwrap();
            (t, t + ytt_attr(l, "d").parse::<u64>().unwrap(), ytt_attr(l, "wp"), l.to_string())
        })
        .collect();
    let mut shared = Vec::new();
    for (i, a) in ps.iter().enumerate() {
        for b in &ps[i + 1..] {
            if a.2 == b.2 && a.0 < b.1 && b.0 < a.1 {
                shared.push((a.3.clone(), b.3.clone()));
            }
        }
    }
    shared
}

#[test]
fn ytt_up_to_eight_lines_at_once_each_get_a_window() {
    let file: String = (0..8)
        .map(|i| format!("00:00:01.{i}00 --> 00:00:04.000 speaker:{i}\n{{0}}Line{i}.{{100}}\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let ytt = ytt_of(&format!("CAPT 1\n\n{file}"));
    assert_eq!(ytt.matches("<p ").count(), 8, "{ytt}");
    assert_eq!(ytt_shared_windows(&ytt), Vec::<(String, String)>::new(), "{ytt}");
    // A ninth goes where it wants, the bottom, over another: there is no window left.
    let nine = format!("CAPT 1\n\n{file}\n00:00:01.950 --> 00:00:04.000 speaker:9\n{{0}}Line9.{{100}}\n");
    assert_eq!(ytt_shared_windows(&ytt_of(&nine)).len(), 1);
}

#[test]
fn ytt_words_said_at_once_stay_inside_their_line() {
    let words: Vec<Word> =
        (0..600).map(|i| Word { text: format!("w{i}"), start: 1000, end: 1000, ..Word::default() }).collect();
    let ytt = ytt::build(&words, &Layout::unlimited()).0;
    for l in ytt.lines().filter(|l| l.starts_with("<p ")) {
        let d: u64 = ytt_attr(l, "d").parse().unwrap();
        let last: u64 = l.rsplit(" t=\"").next().unwrap().split('"').next().unwrap().parse().unwrap();
        assert!(last < d, "the last section starts at {last} ms, the line ends at {d} ms");
    }
}

#[test]
fn ytt_leaves_out_characters_xml_forbids() {
    let ytt = ytt_of("CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}a\u{1}b{100}\n");
    assert!(!ytt.contains('\u{1}') && ytt.contains(">ab</p>"), "{ytt:?}");
}

#[test]
fn ytt_places_the_middle_and_a_point() {
    let ytt = ytt_of(
        "CAPT 1\n\n00:00:01.000 --> 00:00:02.000 pos:middle\n{0}Mid.{50}\n\n\
         00:00:03.000 --> 00:00:04.000 pos:0.25,0.75\n{0}Point.{50}\n",
    );
    // A point of v percent is written (v - 2) / 0.96: 25% -> 24, 75% -> 76.
    assert!(ytt.contains(r#"ap="4" ah="50" av="50""#), "{ytt}");
    assert!(ytt.contains(r#"ap="4" ah="24" av="76""#), "{ytt}");
}

/// In, Out and dialogue of each ADR row.
fn adr_spans(rows: &[String]) -> Vec<(String, String, String)> {
    rows.iter()
        .map(|r| {
            let f: Vec<&str> = r.trim_matches('"').split("\",\"").collect();
            (f[1].to_string(), f[2].to_string(), f[4].to_string())
        })
        .collect()
}

#[test]
fn a_cut_adr_cue_gives_each_row_its_own_out() {
    let word = "abcdefghijklmnopqrstu";
    let words: Vec<String> = (0..9).map(|i| format!("{{{}}}{word}{{{}}}", i * 50, i * 50 + 40)).collect();
    let file = format!(
        "CAPT 1\nspeaker:1 name:Anna\n\n00:00:00.000 --> 00:00:05.000 speaker:1\n{}\n\n\
         00:00:00.420 --> 00:00:00.480 speaker:1 sound:laughter\n",
        words.join(" ")
    );
    let spans = adr_spans(&adr(&file));
    assert_eq!(spans.len(), 2, "{spans:?}");
    assert!(spans[0].1 <= spans[1].0, "rows of one actor overlap: {spans:?}");
}

#[test]
fn an_adr_sound_alone_stays_within_the_row_limit() {
    let spans =
        adr_spans(&adr("CAPT 1\nspeaker:1 name:Anna\n\n00:00:00.000 --> 00:00:20.000 speaker:1 sound:laughter\n"));
    assert!(spans.iter().all(|(_, out, _)| out.as_str() <= "01:00:08:00"), "{spans:?}");
}

#[test]
fn a_long_adr_sound_and_the_speech_during_it_do_not_overlap() {
    let words: Vec<String> = (0..8).map(|i| format!("{{{}}}w{i}{{{}}}", i * 100, i * 100 + 50)).collect();
    let file = format!(
        "CAPT 1\nspeaker:1 name:Anna\n\n00:00:10.000 --> 00:00:13.000 speaker:1 sound:laughter\n\n\
         00:00:11.000 --> 00:00:18.500 speaker:1\n{}\n",
        words.join(" ")
    );
    let spans = adr_spans(&adr(&file));
    for p in spans.windows(2) {
        assert!(p[0].1 <= p[1].0, "rows of one actor overlap: {spans:?}");
    }
}

#[test]
fn an_adr_list_at_29_97_fps_from_a_minute_before_the_hour() {
    let doc = captions::capt::Doc::parse(
        "CAPT 1\nspeaker:1 name:Anna\n\n00:00:01.000 --> 00:00:01.500 speaker:1\n{0}Hi.{50}\n",
    )
    .unwrap();
    let start = captions::adr::Timecode::parse("00:59:59:00").unwrap();
    let o = captions::adr::Options { rate: captions::adr::Rate::Fps29_97, start, ..Default::default() };
    let rows: Vec<String> = captions::adr::build(&doc, &Layout::default(), &o, "en")
        .unwrap()
        .0
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::to_string)
        .collect();
    // 1 s of media is 29.97 frames: In at frame 29 (rounded down), Out at 1.5 s = 44.955, up to 45.
    let spans = adr_spans(&rows);
    assert_eq!((spans[0].0.as_str(), spans[0].1.as_str()), ("00:59:59:29", "01:00:00:15"), "{spans:?}");
}

#[test]
fn adr_rows_of_one_actor_never_share_a_frame() {
    // The speech starts off a frame (11.010 s at 25 fps): the laugh before it ends by that frame.
    let words: Vec<String> = (0..8).map(|i| format!("{{{}}}w{i}{{{}}}", i * 100, i * 100 + 50)).collect();
    let file = format!(
        "CAPT 1\nspeaker:1 name:Anna\n\n00:00:10.000 --> 00:00:13.000 speaker:1 sound:laughter\n\n\
         00:00:11.010 --> 00:00:18.600 speaker:1\n{}\n",
        words.join(" ")
    );
    let spans = adr_spans(&adr(&file));
    for p in spans.windows(2) {
        assert!(p[0].1 <= p[1].0, "rows of one actor overlap: {spans:?}");
    }
}

#[test]
fn a_ytt_line_holds_its_window_while_its_last_word_shows() {
    let mut words: Vec<Word> = (0..20)
        .map(|i| Word { text: format!("x{i}"), start: 1000, end: 1000, speaker: Some("1".into()), ..Word::default() })
        .collect();
    words.push(Word { text: "Next.".into(), start: 1005, end: 2000, speaker: Some("2".into()), ..Word::default() });
    let layout = Layout { by_speaker: true, ..Layout::unlimited() };
    let ytt = ytt::build(&words, &layout).0;
    assert_eq!(ytt_shared_windows(&ytt), Vec::<(String, String)>::new(), "{ytt}");
}

#[test]
fn adr_rows_of_one_actor_starting_in_one_frame_do_not_overlap() {
    // A laugh alone, then the same actor's speech 20 ms later (one frame at 25 fps), too long to join.
    let word = "abcdefghijklmnopqrstu";
    let words: Vec<String> = (0..9).map(|i| format!("{{{}}}{word}{{{}}}", i * 50, i * 50 + 40)).collect();
    let file = format!(
        "CAPT 1\nspeaker:1 name:Anna\n\n00:00:10.000 --> 00:00:10.010 speaker:1 sound:laughter\n\n\
         00:00:10.020 --> 00:00:15.000 speaker:1\n{}\n",
        words.join(" ")
    );
    let spans = adr_spans(&adr(&file));
    assert_eq!(spans.len(), 2, "{spans:?}");
    for p in spans.windows(2) {
        assert!(p[0].1 <= p[1].0 && p[1].0 < p[1].1, "rows of one actor overlap: {spans:?}");
    }
}

#[test]
fn ttml_leaves_out_characters_xml_forbids_and_keeps_tabs() {
    let doc =
        captions::capt::Doc::parse("CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}a\u{1}b\u{fffe}c\td{100}\n").unwrap();
    let ttml = captions::ttml::build(&doc, &Layout::default()).0;
    assert!(ttml.contains("abc\td</p>"), "{ttml:?}");
}

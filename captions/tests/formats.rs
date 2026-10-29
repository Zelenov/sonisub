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

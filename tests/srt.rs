//! Subtitle building. Golden output for real Soniox transcripts (tests/fixtures), and one test per
//! rule with hand-made tokens: where a sentence may be cut, in which order, and what is never cut.

mod common;

use serde_json::{Value, json};
use sonisub::srt::{Layout, build};

// ---------------------------------------------------------------- helpers

/// Transcript from words spoken back to back by one speaker, `ms` each.
/// A word may carry its own pause before it: "<pause 800>word".
fn say(text: &str, ms: u64) -> Value {
    say_as(&[("1", text)], ms)
}

/// Several speakers in turn: [("1", "Hello there."), ("2", "Hi.")].
fn say_as(turns: &[(&str, &str)], ms: u64) -> Value {
    let mut tokens = Vec::new();
    let mut t = 0u64;
    for (speaker, text) in turns {
        for word in text.split(' ') {
            let (pause, word) = match word.strip_prefix("<pause").and_then(|r| r.split_once('>')) {
                Some((p, w)) => (p.trim().parse::<u64>().unwrap(), w),
                None => (0, word),
            };
            t += pause;
            tokens.push(json!({"text": format!(" {word}"), "start_ms": t, "end_ms": t + ms, "speaker": speaker}));
            t += ms;
        }
    }
    json!({ "tokens": tokens })
}

struct Cue {
    start: String,
    end: String,
    lines: Vec<String>,
}

impl Cue {
    fn text(&self) -> String {
        self.lines.join(" ")
    }
}

fn cues(srt: &str) -> Vec<Cue> {
    srt.trim()
        .split("\n\n")
        .filter(|b| !b.is_empty())
        .map(|b| {
            let mut l = b.lines();
            l.next();
            let (start, end) = l.next().unwrap().split_once(" --> ").unwrap();
            Cue { start: start.into(), end: end.into(), lines: l.map(str::to_string).collect() }
        })
        .collect()
}

fn texts(t: &Value, layout: &Layout) -> Vec<String> {
    cues(&build(t, layout).0).iter().map(Cue::text).collect()
}

/// One line of `max_line` chars per cue, no duration limit: the length alone decides the cuts.
fn narrow(max_line: usize) -> Layout {
    Layout { max_line, max_lines: 1, max_duration: 0.0, ..Layout::default() }
}

fn ends_with_punctuation(s: &str) -> bool {
    s.trim_end_matches(['"', '”', '»', ')']).ends_with(['.', ',', ';', ':', '?', '!', '—', '–', '…'])
}

/// Words of a text; a break after a glued dash ("finally—" / "and") doesn't count as a new word.
fn words(s: &str) -> Vec<String> {
    s.replace("— ", "—").replace("– ", "–").split_whitespace().map(str::to_string).collect()
}

// ---------------------------------------------------------------- real transcripts

/// Two lines of 50, broken inside the cue (`--wrap`).
fn wrapped() -> Layout {
    Layout { wrap_lines: true, ..Layout::default() }
}

#[test]
fn golden_subtitles_for_real_transcripts() {
    let speakers = Layout { speaker_labels: true, ..Layout::default() };
    for name in ["dialog", "punctuation"] {
        let t = common::transcript_of(name);
        assert_eq!(build(&t, &Layout::default()).0, common::golden(&format!("{name}.srt")), "{name}.srt");
        assert_eq!(build(&t, &Layout::unlimited()).0, common::golden(&format!("{name}.unlimited.srt")), "{name}.unlimited");
        assert_eq!(build(&t, &speakers).0, common::golden(&format!("{name}.speakers.srt")), "{name}.speakers");
        assert_eq!(build(&t, &wrapped()).0, common::golden(&format!("{name}.wrap.srt")), "{name}.wrap");
    }
}

#[test]
fn by_default_a_cue_is_one_line_of_up_to_100_chars() {
    for name in ["dialog", "punctuation"] {
        for c in cues(&build(&common::transcript_of(name), &Layout::default()).0) {
            assert_eq!(c.lines.len(), 1, "{name}: {:?}", c.lines);
            assert!(c.lines[0].chars().count() <= 100, "{name}: {:?}", c.lines);
        }
    }
}

#[test]
fn real_punctuation_text_is_only_ever_cut_at_punctuation() {
    let t = common::transcript_of("punctuation");
    for layout in [Layout::default(), Layout::unlimited()] {
        for c in cues(&build(&t, &layout).0) {
            assert!(ends_with_punctuation(&c.text()), "max_line {}: cue {:?}", layout.max_line, c.text());
        }
    }
}

#[test]
fn real_unpunctuated_clause_is_the_only_thing_cut_between_words() {
    // At 2×42 only "and we watched the boats go by until the sun went down and it got too cold to stay
    // outside." (91 chars, no punctuation) can't fit; everything else is still cut at punctuation.
    let t = common::transcript_of("punctuation");
    let c = texts(&t, &Layout { max_line: 42, max_duration: 0.0, ..Layout::default() });
    let clause = "and we watched the boats go by until the sun went down and it got too cold to stay outside.";
    for cue in &c {
        assert!(ends_with_punctuation(cue) || clause.starts_with(cue.as_str()), "{cue:?}");
    }
    assert!(c.iter().any(|x| x.ends_with("for a long time,")), "{c:?}");
}

#[test]
fn real_punctuation_text_breaks_lines_at_commas() {
    let t = common::transcript_of("punctuation");
    let c = cues(&build(&t, &wrapped()).0);
    let kenya = c.iter().find(|c| c.text().starts_with("For example")).unwrap();
    assert_eq!(kenya.lines, ["For example, for the videos from Kenya,", "Nairobi, and Mombasa."]);
    let plan = &c[0];
    assert!(plan.lines.iter().all(|l| ends_with_punctuation(l)), "{:?}", plan.lines);
}

#[test]
fn real_transcripts_respect_every_layout() {
    for name in ["dialog", "punctuation"] {
        let t = common::transcript_of(name);
        for (max_line, max_lines) in [(50, 2), (42, 2), (32, 2), (24, 3), (70, 1)] {
            for wrap_lines in [true, false] {
                let layout = Layout { max_line, max_lines, wrap_lines, ..Layout::default() };
                let (lines, width) = if wrap_lines { (max_lines, max_line) } else { (1, max_line * max_lines) };
                for c in cues(&build(&t, &layout).0) {
                    assert!(c.lines.len() <= lines, "{name} {max_line}x{max_lines}: {:?}", c.lines);
                    for l in &c.lines {
                        assert!(l.chars().count() <= width, "{name} {max_line}x{max_lines}: {l:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn real_transcripts_lose_and_reorder_nothing() {
    for name in ["dialog", "punctuation"] {
        let t = common::transcript_of(name);
        for layout in [Layout::default(), Layout::unlimited(), narrow(24)] {
            let joined = texts(&t, &layout).join(" ");
            assert_eq!(words(&joined), words(t["text"].as_str().unwrap()), "{name} {}", layout.max_line);
        }
    }
}

#[test]
fn unlimited_gives_one_cue_per_sentence() {
    let t = common::transcript_of("punctuation");
    let c = texts(&t, &Layout::unlimited());
    assert_eq!(c.len(), 15);
    assert_eq!(c[3], "No.");
    assert_eq!(c[4], "Dr. Smith wrote a tool, a small one, written in Rust, that does it for us.");
    assert!(c.iter().all(|s| ends_with_punctuation(s)));
    // The long sentence stays whole, on one line.
    assert!(c.last().unwrap().starts_with("And then we went down to the river"));
    assert_eq!(cues(&build(&t, &Layout::unlimited()).0).last().unwrap().lines.len(), 1);
}

#[test]
fn real_dialog_never_mixes_speakers_or_languages() {
    let t = common::transcript_of("dialog");
    let c = texts(&t, &Layout { gap: 10.0, ..Layout::unlimited() });
    assert!(c.contains(&"А теперь короткая фраза по-русски.".to_string()), "{c:?}");
    assert!(c.contains(&"Y una frase corta en español.".to_string()), "{c:?}");
    assert!(c.contains(&"Yes.".to_string()), "{c:?}");
}

// ---------------------------------------------------------------- sentence ends

#[test]
fn each_sentence_is_its_own_cue() {
    assert_eq!(texts(&say("Work is done. Painting buttons next.", 300), &Layout::default()), [
        "Work is done.",
        "Painting buttons next."
    ]);
}

#[test]
fn question_and_exclamation_end_sentences() {
    assert_eq!(texts(&say("Really?! Yes! Why?.. Because.", 200), &Layout::default()), [
        "Really?!",
        "Yes!",
        "Why?..",
        "Because."
    ]);
}

#[test]
fn ellipsis_is_a_hesitation_not_an_end() {
    assert_eq!(texts(&say("What do we have on... on... what is next?", 200), &Layout::default()), [
        "What do we have on... on... what is next?"
    ]);
    assert_eq!(texts(&say("Well… maybe later.", 200), &Layout::default()), ["Well… maybe later."]);
}

#[test]
fn abbreviations_and_initials_do_not_end_sentences() {
    for s in [
        "Dr. Smith wrote it.",
        "Mr. and Mrs. Brown came.",
        "We used tools, e.g. Rust and ffmpeg.",
        "It works, i.e. mostly.",
        "Made in the U.S. by hand.",
        "J. R. R. Tolkien wrote it.",
        "Say hi to St. Petersburg.",
        "Это было, т.е. почти готово.",
    ] {
        assert_eq!(texts(&say(s, 200), &Layout::default()), [s], "{s}");
    }
}

#[test]
fn numbers_with_dots_do_not_end_sentences() {
    let s = "Version 3.5 costs $12.50 per hour. That is fine.";
    assert_eq!(texts(&say(s, 200), &Layout::default()), ["Version 3.5 costs $12.50 per hour.", "That is fine."]);
}

#[test]
fn quoted_sentence_end_is_an_end() {
    assert_eq!(texts(&say("He said: \"it works.\" Then he left.", 200), &Layout::default()), [
        "He said: \"it works.\"",
        "Then he left."
    ]);
    assert_eq!(texts(&say("Он сказал: «готово.» И ушёл.", 200), &Layout::default()), [
        "Он сказал: «готово.»",
        "И ушёл."
    ]);
}

// ---------------------------------------------------------------- cutting a long sentence

#[test]
fn a_sentence_that_fits_is_never_cut_even_at_commas() {
    let s = "First, we record; then, we transcribe.";
    assert_eq!(texts(&say(s, 200), &Layout::default()), [s]);
}

#[test]
fn long_sentence_is_cut_at_punctuation_not_mid_phrase() {
    let s = "First we record the whole interview, then we transcribe it with Soniox, and finally we cut the text into subtitles.";
    let c = texts(&say(s, 150), &narrow(45));
    assert_eq!(c.len(), 3);
    for cue in &c {
        assert!(ends_with_punctuation(cue), "{c:?}");
    }
}

#[test]
fn every_kind_of_clause_mark_is_a_cut_point() {
    for (mark, s) in [
        (",", "We had a very long and tiring day on the road, and the car broke down twice before lunch."),
        (";", "We had a very long and tiring day on the road; the car broke down twice before lunch."),
        (":", "We had a very long and tiring day on the road: the car broke down twice before lunch."),
        ("—", "We had a very long and tiring day on the road — the car broke down twice before lunch."),
        ("...", "We had a very long and tiring day on the road... the car broke down twice before lunch."),
    ] {
        let c = texts(&say(s, 150), &narrow(50));
        assert_eq!(c.len(), 2, "{mark}: {c:?}");
        assert!(c[0].ends_with(mark) || c[1].starts_with(mark), "{mark}: {c:?}");
    }
}

#[test]
fn cut_before_an_opening_bracket_or_quote() {
    let s = "Dr. Smith wrote a small command line tool for this (in Rust, of course) during one weekend.";
    let c = texts(&say(s, 150), &narrow(50));
    assert!(c.iter().any(|x| x.starts_with('(') || x.ends_with(')') || x.ends_with(',')), "{c:?}");
    assert_eq!(words(&c.join(" ")), words(s));
}

#[test]
fn dash_glued_to_words_is_a_cut_point_and_text_is_unchanged() {
    // Soniox writes em dashes without spaces: "finally—and this is the tricky part—we".
    let s = "Well, here we go; finally—and this is the really tricky part of it—we cut the text into small subtitles.";
    let c = texts(&say(s, 150), &narrow(45));
    assert_eq!(c, [
        "Well, here we go; finally—",
        "and this is the really tricky part of it—",
        "we cut the text into small subtitles."
    ]);
    // Whole cue on one line: the dash stays glued.
    assert_eq!(texts(&say(s, 150), &Layout::unlimited()), [s]);
}

#[test]
fn a_pause_is_the_next_best_cut_when_there_is_no_punctuation() {
    let s = "and we sat there by the river for a very long time <pause600>watching the boats go by until the sun went down.";
    let c = texts(&say(s, 150), &narrow(50));
    assert_eq!(c, [
        "and we sat there by the river for a very long time",
        "watching the boats go by until the sun went down."
    ]);
}

#[test]
fn words_are_cut_only_when_nothing_else_fits() {
    let s = "and we sat there by the river for a very long time watching the boats go by until the sun went down and it got too cold to stay.";
    let layout = narrow(30);
    let c = cues(&build(&say(s, 150), &layout).0);
    assert!(c.len() > 1);
    assert_eq!(words(&c.iter().map(Cue::text).collect::<Vec<_>>().join(" ")), words(s));
    for cue in &c {
        assert!(cue.lines.len() <= 2 && cue.lines.iter().all(|l| l.chars().count() <= 30), "{:?}", cue.lines);
    }
}

#[test]
fn punctuation_wins_over_even_length() {
    // A balanced cut would be after "road"; the comma is further left but clean.
    let s = "On the road, we talked about the weather and the traffic.";
    let c = texts(&say(s, 150), &narrow(50));
    assert_eq!(c, ["On the road,", "we talked about the weather and the traffic."]);
}

#[test]
fn lines_inside_a_cue_break_at_punctuation_when_possible() {
    let s = "Well, here's the plan: first we record, then we transcribe.";
    let c = cues(&build(&say(s, 150), &Layout { max_line: 40, max_duration: 0.0, ..wrapped() }).0);
    assert_eq!(c.len(), 1);
    assert!(ends_with_punctuation(&c[0].lines[0]), "{:?}", c[0].lines);
}

#[test]
fn long_duration_is_cut_even_without_length_limit() {
    let s = "one, two, three, four, five, six, seven, eight, nine, ten.";
    let layout = Layout { max_line: 0, max_duration: 3.0, ..Layout::default() };
    let c = texts(&say(s, 1000), &layout);
    assert!(c.len() >= 4, "{c:?}");
    assert!(c.iter().all(|x| ends_with_punctuation(x)), "{c:?}");
}

#[test]
fn unlimited_never_cuts_a_sentence() {
    let s = "and we sat there by the river for a very long time, watching the boats go by, until the sun went down and it got too cold to stay outside any longer.";
    let c = cues(&build(&say(s, 400), &Layout::unlimited()).0);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].lines, [s]);
}

// ---------------------------------------------------------------- pauses and timing

#[test]
fn long_silence_ends_a_cue() {
    let t = say("Business demo <pause3000>today at noon", 300);
    assert_eq!(texts(&t, &Layout::default()), ["Business demo", "today at noon"]);
    assert_eq!(texts(&t, &Layout { gap: 5.0, ..Layout::default() }), ["Business demo today at noon"]);
}

#[test]
fn short_sentence_tail_after_a_long_pause_stays_with_its_sentence() {
    let t = say("This corridor is, I think, closest to <pause2500>an office. The next one.", 300);
    assert_eq!(texts(&t, &Layout::default()), ["This corridor is, I think, closest to an office.", "The next one."]);
}

#[test]
fn short_cue_is_extended_but_never_overlaps_the_next() {
    let t = json!({"tokens": [
        {"text": " Yes.", "start_ms": 1000, "end_ms": 1100, "speaker": "1"},
        {"text": " No.", "start_ms": 1300, "end_ms": 1400, "speaker": "2"},
        {"text": " Fine.", "start_ms": 5000, "end_ms": 5100, "speaker": "1"}
    ]});
    let c = cues(&build(&t, &Layout::default()).0);
    assert_eq!((c[0].start.as_str(), c[0].end.as_str()), ("00:00:01,000", "00:00:01,299"));
    assert_eq!((c[1].start.as_str(), c[1].end.as_str()), ("00:00:01,300", "00:00:01,800"));
    assert_eq!(c[2].end, "00:00:05,500");
}

#[test]
fn timestamps_past_one_hour() {
    let t = json!({"tokens": [{"text": " Late.", "start_ms": 3_723_004, "end_ms": 3_724_000, "speaker": "1"}]});
    assert!(build(&t, &Layout::default()).0.contains("01:02:03,004 --> 01:02:04,000"));
}

// ---------------------------------------------------------------- speakers

#[test]
fn speaker_change_always_ends_a_cue() {
    let t = say_as(&[("1", "Did you close the tickets"), ("2", "yes all of them")], 200);
    assert_eq!(texts(&t, &Layout::default()), ["Did you close the tickets", "yes all of them"]);
    let merged = Layout { by_speaker: false, ..Layout::default() };
    assert_eq!(texts(&t, &merged), ["Did you close the tickets yes all of them"]);
}

#[test]
fn speaker_labels_appear_when_the_speaker_changes() {
    let t = say_as(&[("1", "Hi. How are you?"), ("2", "Fine."), ("1", "Good.")], 200);
    let layout = Layout { speaker_labels: true, ..Layout::default() };
    assert_eq!(texts(&t, &layout), ["Speaker 1: Hi.", "How are you?", "Speaker 2: Fine.", "Speaker 1: Good."]);
}

#[test]
fn speaker_names_replace_numbers() {
    let t = say_as(&[("1", "Hi."), ("2", "Hello."), ("3", "Hey.")], 200);
    let layout = Layout { speaker_labels: true, speaker_names: vec!["Eugene".into(), "Sasha".into()], ..Layout::default() };
    assert_eq!(texts(&t, &layout), ["Eugene: Hi.", "Sasha: Hello.", "Speaker 3: Hey."]);
}

#[test]
fn no_labels_for_a_single_speaker() {
    let t = say("Just me talking. Still me.", 200);
    let layout = Layout { speaker_labels: true, ..Layout::default() };
    assert_eq!(texts(&t, &layout), ["Just me talking.", "Still me."]);
}

#[test]
fn speaker_label_counts_towards_line_length() {
    let t = say_as(&[("1", "This sentence is exactly long enough to fill a line."), ("2", "Ok.")], 150);
    let layout = Layout { speaker_labels: true, max_line: 52, max_lines: 1, max_duration: 0.0, ..wrapped() };
    for c in cues(&build(&t, &layout).0) {
        assert!(c.lines.iter().all(|l| l.chars().count() <= 52), "{:?}", c.lines);
    }
}

// ---------------------------------------------------------------- tokens as Soniox sends them

#[test]
fn space_sent_as_a_separate_token_separates_words() {
    // Real Soniox output: "And", ",", " ", "wo", "w", "," — the space arrives on its own.
    let t = json!({"tokens": [
        {"text": " And", "start_ms": 0, "end_ms": 100, "speaker": "1"},
        {"text": ",", "start_ms": 100, "end_ms": 150, "speaker": "1"},
        {"text": " ", "start_ms": 150, "end_ms": 160, "speaker": "1"},
        {"text": "wo", "start_ms": 160, "end_ms": 200, "speaker": "1"},
        {"text": "w", "start_ms": 200, "end_ms": 300, "speaker": "1"},
        {"text": ",", "start_ms": 300, "end_ms": 350, "speaker": "1"},
        {"text": " rea", "start_ms": 400, "end_ms": 500, "speaker": "1"},
        {"text": "lly.", "start_ms": 500, "end_ms": 700, "speaker": "1"}
    ]});
    assert_eq!(texts(&t, &Layout::default()), ["And, wow, really."]);
}

#[test]
fn audio_events_are_ignored() {
    let t = json!({"tokens": [
        {"text": " Hello.", "start_ms": 0, "end_ms": 500, "speaker": "1"},
        {"text": " <laugh>", "start_ms": 600, "end_ms": 900, "speaker": "1", "is_audio_event": true}
    ]});
    assert_eq!(texts(&t, &Layout::default()), ["Hello."]);
}

#[test]
fn empty_transcripts_give_nothing() {
    for t in [json!({"tokens": []}), json!({"text": "", "tokens": [{"text": " ", "start_ms": 0, "end_ms": 1}]})] {
        assert_eq!(build(&t, &Layout::default()), (String::new(), 0));
        assert!(!sonisub::srt::has_speech(&t));
    }
    assert!(!sonisub::srt::has_speech(&common::transcript_of("nospeech")));
}

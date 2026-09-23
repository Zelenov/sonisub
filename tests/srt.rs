//! Subtitle building: golden output for the real fixture transcript, plus the edge cases
//! seen on real footage, reproduced with hand-made tokens.

mod common;

use serde_json::{Value, json};
use sonisub::srt::{Layout, build};

/// Tokens as Soniox sends them: (text, start_ms, end_ms, speaker).
fn tokens(list: &[(&str, u64, u64, &str)]) -> Value {
    let t: Vec<Value> = list
        .iter()
        .map(|(text, s, e, sp)| json!({"text": text, "start_ms": s, "end_ms": e, "speaker": sp, "confidence": 0.99}))
        .collect();
    json!({ "text": "", "tokens": t })
}

/// Words spoken back to back by one speaker, `ms` each.
fn speech(words: &[&str], start: u64, ms: u64, speaker: &str) -> Vec<(String, u64, u64, String)> {
    words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let s = start + i as u64 * ms;
            (if i == 0 && start == 0 { w.to_string() } else { format!(" {w}") }, s, s + ms, speaker.to_string())
        })
        .collect()
}

fn to_value(list: &[(String, u64, u64, String)]) -> Value {
    let refs: Vec<(&str, u64, u64, &str)> = list.iter().map(|(a, b, c, d)| (a.as_str(), *b, *c, d.as_str())).collect();
    tokens(&refs)
}

struct Cue {
    start: String,
    end: String,
    lines: Vec<String>,
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

fn texts(srt: &str) -> Vec<String> {
    cues(srt).iter().map(|c| c.lines.join(" ")).collect()
}

#[test]
fn fixture_transcript_matches_golden_srt() {
    let (srt, n) = build(&common::transcript(), &Layout::default());
    assert_eq!(srt, common::golden_srt());
    assert_eq!(n, 9);
}

#[test]
fn fixture_speaker_changes_start_new_cues() {
    let (srt, _) = build(&common::transcript(), &Layout::default());
    let t = texts(&srt);
    // Yana ends, Andrei starts: never in one cue.
    assert!(t.iter().any(|c| c == "Привет, сегодня мы проверяем генератор субтитров."));
    assert!(t.iter().any(|c| c == "Отлично."));
    assert!(t.iter().any(|c| c == "Да."));
}

#[test]
fn fixture_without_diarization_merges_speakers() {
    let layout = Layout { by_speaker: false, gap: 5.0, ..Layout::default() };
    let (with, _) = build(&common::transcript(), &Layout { gap: 5.0, ..Layout::default() });
    let (without, _) = build(&common::transcript(), &layout);
    assert!(cues(&without).len() <= cues(&with).len());
}

#[test]
fn every_line_respects_layout_limits() {
    for (max_line, max_lines) in [(42, 2), (32, 2), (20, 3), (60, 1)] {
        let layout = Layout { max_line, max_lines, ..Layout::default() };
        let (srt, _) = build(&common::transcript(), &layout);
        for c in cues(&srt) {
            assert!(c.lines.len() <= max_lines, "{max_line}x{max_lines}: {:?}", c.lines);
            for l in &c.lines {
                assert!(l.chars().count() <= max_line, "{max_line}x{max_lines}: {l:?}");
            }
        }
    }
}

#[test]
fn nothing_is_lost_or_reordered() {
    let t = common::transcript();
    let spoken: String = t["tokens"].as_array().unwrap().iter().map(|x| x["text"].as_str().unwrap()).collect();
    let words = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    let (srt, _) = build(&t, &Layout { max_line: 20, max_lines: 3, ..Layout::default() });
    assert_eq!(words(&texts(&srt).join(" ")), words(&spoken));
}

#[test]
fn space_sent_as_separate_token_separates_words() {
    // Real Soniox output: "И", ",", " ", "ё", "п", ... — the space arrives on its own.
    let t = tokens(&[
        (" И", 0, 100, "1"),
        (",", 100, 150, "1"),
        (" ", 150, 160, "1"),
        ("ё", 160, 200, "1"),
        ("пт", 200, 300, "1"),
        (",", 300, 350, "1"),
        (" дей", 400, 500, "1"),
        ("ствительно.", 500, 700, "1"),
    ]);
    assert_eq!(texts(&build(&t, &Layout::default()).0), ["И, ёпт, действительно."]);
}

#[test]
fn ellipsis_is_a_hesitation_not_a_sentence_end() {
    let t = to_value(&speech(&["Что", "у", "нас", "на...", "на...", "что", "дальше?"], 0, 200, "1"));
    assert_eq!(texts(&build(&t, &Layout::default()).0), ["Что у нас на... на... что дальше?"]);
}

#[test]
fn sentence_end_starts_a_new_cue() {
    let t = to_value(&speech(&["Работа", "закончена.", "Крашу", "кнопки", "дальше."], 0, 300, "1"));
    assert_eq!(texts(&build(&t, &Layout::default()).0), ["Работа закончена.", "Крашу кнопки дальше."]);
}

#[test]
fn long_pause_starts_a_new_cue() {
    let mut w = speech(&["Сегодня", "показ"], 0, 300, "1");
    w.extend(speech(&["бизнесу"], 2000, 300, "1"));
    assert_eq!(texts(&build(&to_value(&w), &Layout::default()).0), ["Сегодня показ", "бизнесу"]);
    let relaxed = Layout { gap: 2.0, ..Layout::default() };
    assert_eq!(texts(&build(&to_value(&w), &relaxed).0), ["Сегодня показ бизнесу"]);
}

#[test]
fn short_sentence_tail_after_a_pause_joins_its_sentence() {
    // "...ближе всего <pause> описывается. Этот..." — the tail belongs to the first cue.
    let mut w = speech(&["Это,", "наверное,", "ближе", "всего"], 0, 300, "1");
    w.extend(speech(&["описывается.", "Этот", "перешеек."], 2500, 300, "1"));
    assert_eq!(
        texts(&build(&to_value(&w), &Layout::default()).0),
        ["Это, наверное, ближе всего описывается.", "Этот перешеек."]
    );
}

#[test]
fn full_cue_is_cut_after_a_comma() {
    let words = "И смотрите: поставил здесь компьютер, первый монитор, второй монитор, третий монитор.";
    let w = speech(&words.split(' ').collect::<Vec<_>>(), 0, 250, "1");
    let t = texts(&build(&to_value(&w), &Layout::default()).0);
    assert_eq!(t.len(), 2, "{t:?}");
    assert!(t[0].ends_with(','), "{t:?}");
    assert_eq!(t.join(" "), words);
}

#[test]
fn long_cues_are_split_by_duration() {
    let w = speech(&["раз", "два", "три", "четыре", "пять", "шесть", "семь", "восемь"], 0, 1000, "1");
    let (srt, _) = build(&to_value(&w), &Layout { max_duration: 3.0, ..Layout::default() });
    assert!(cues(&srt).len() >= 3);
}

#[test]
fn short_cue_is_extended_but_never_overlaps_the_next() {
    let t = tokens(&[(" Да.", 1000, 1100, "1"), (" Нет.", 1300, 1400, "2"), (" Ладно.", 5000, 5100, "1")]);
    let c = cues(&build(&t, &Layout::default()).0);
    assert_eq!((c[0].start.as_str(), c[0].end.as_str()), ("00:00:01,000", "00:00:01,299"));
    assert_eq!((c[1].start.as_str(), c[1].end.as_str()), ("00:00:01,300", "00:00:01,800"));
    assert_eq!(c[2].end, "00:00:05,500");
}

#[test]
fn audio_events_are_ignored() {
    let t = json!({"tokens": [
        {"text": " Привет.", "start_ms": 0, "end_ms": 500, "speaker": "1"},
        {"text": " <laugh>", "start_ms": 600, "end_ms": 900, "speaker": "1", "is_audio_event": true}
    ]});
    assert_eq!(texts(&build(&t, &Layout::default()).0), ["Привет."]);
}

#[test]
fn timestamps_past_one_hour() {
    let t = tokens(&[(" Поздно.", 3_723_004, 3_724_000, "1")]);
    assert!(build(&t, &Layout::default()).0.contains("01:02:03,004 --> 01:02:04,000"));
}

#[test]
fn empty_transcript_gives_empty_srt() {
    assert_eq!(build(&json!({"tokens": []}), &Layout::default()), (String::new(), 0));
}

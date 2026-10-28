//! The command line as a user sees it, with stderr piped (not a terminal).

#![cfg(feature = "cli")]

mod common;

use std::process::{Command, Output};

use common::MockSoniox;

fn sonisub(args: &[&std::ffi::OsStr]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sonisub"));
    c.args(args).env_remove("SONIOX_API_KEY");
    c
}

/// Runs `files` videos through a mock Soniox; the summary and result lines are on stderr.
fn run_with_mock(files: usize, extra: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockSoniox::happy(common::transcript());
    let mut inputs = Vec::new();
    for i in 0..files {
        let input = dir.path().join(format!("clip{i}.mp4"));
        std::fs::copy(common::fixture("dialog.mp4"), &input).unwrap();
        inputs.push(input);
    }
    let mut args: Vec<&std::ffi::OsStr> = inputs.iter().map(|p| p.as_os_str()).collect();
    let fixed = ["--api-key", "k", "--api-url", mock.url.as_str(), "--poll", "0.2"];
    for a in fixed.iter().chain(extra) {
        args.push(a.as_ref());
    }
    sonisub(&args).output().unwrap()
}

fn plain(out: &Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!err.contains('\x1b'), "escape codes: {err:?}");
    assert!(!err.contains('\r'), "redraws: {err:?}");
    err
}

#[test]
fn a_batch_without_a_terminal_leaves_only_plain_lines_and_a_summary() {
    for extra in [&[][..], &["--quiet"][..]] {
        let out = run_with_mock(2, extra);
        let err = plain(&out);
        assert!(out.status.success(), "{err}");
        assert!(err.contains("✓ clip0.mp4 — 10 cues"), "{err}");
        assert!(err.contains("✓ clip1.mp4 — 10 cues"), "{err}");
        assert_eq!(err.lines().filter(|l| l.starts_with("done: 2 processed")).count(), 1, "{err}");
    }
}

#[test]
fn one_file_without_a_terminal_is_plain_too() {
    for extra in [&[][..], &["-q"][..]] {
        let out = run_with_mock(1, extra);
        let err = plain(&out);
        assert!(out.status.success(), "{err}");
        assert!(err.contains("✓ clip0.mp4 — 10 cues"), "{err}");
    }
}

#[test]
fn saved_transcripts_need_no_key_and_print_plain_lines() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.soniox.json");
    let b = dir.path().join("b.soniox.json");
    std::fs::copy(common::fixture("dialog.soniox.json"), &a).unwrap();
    std::fs::copy(common::fixture("dialog.soniox.json"), &b).unwrap();
    let out = sonisub(&[a.as_os_str(), b.as_os_str()]).output().unwrap();
    let err = plain(&out);
    assert!(err.contains("done: 2 processed"), "{err}");
}

#[test]
fn render_writes_subtitles_from_saved_words() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("dialog.words.json");
    let words = sonisub::soniox::words(&common::transcript());
    std::fs::write(&input, serde_json::to_string(&words).unwrap()).unwrap();
    let render = |extra: &[&str]| {
        let mut args: Vec<&std::ffi::OsStr> =
            vec!["render".as_ref(), input.as_os_str(), "-t".as_ref(), "srt,vtt,ass".as_ref()];
        args.extend(extra.iter().map(std::ffi::OsStr::new));
        sonisub(&args).output().unwrap()
    };

    let out = render(&[]);
    let err = plain(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.starts_with("✓ dialog.words.json — 10 cues -> "), "{err}");
    for (file, golden) in [("dialog.srt", "dialog.srt"), ("dialog.ass", "dialog.ass")] {
        let written = std::fs::read_to_string(dir.path().join(file)).unwrap();
        assert_eq!(common::normalize(&written), common::golden(golden), "{file}");
    }
    // Saved words are laid out as they are: no CAPT marks, so no <lang> in WebVTT.
    let vtt = std::fs::read_to_string(dir.path().join("dialog.vtt")).unwrap();
    let layout = sonisub::captions::Layout::default();
    assert_eq!(common::normalize(&vtt), sonisub::captions::vtt::build(&words, &layout).0);

    // A glued word and times off the 10 ms grid come out as given.
    let glued = dir.path().join("glued.words.json");
    std::fs::write(
        &glued,
        r#"[{"text":"Well,","start":1234,"end":1567},{"text":"finally—","start":1601,"end":1999},
            {"text":"and","start":2003,"end":2111,"glued":true},{"text":"here.","start":3005,"end":3333}]"#,
    )
    .unwrap();
    let out = sonisub(&["render".as_ref(), glued.as_os_str(), "-t".as_ref(), "srt".as_ref()]).output().unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    let srt = std::fs::read_to_string(dir.path().join("glued.srt")).unwrap();
    assert_eq!(srt, "1\n00:00:01,234 --> 00:00:03,333\nWell, finally—and here.\n\n");

    let again = render(&[]);
    assert!(plain(&again).contains("skipped, .srt, .vtt, .ass exist (--force to redo)"));
    let forced = render(&["--force", "--karaoke"]);
    assert!(forced.status.success(), "{}", plain(&forced));
    let ass = std::fs::read_to_string(dir.path().join("dialog.ass")).unwrap();
    assert_eq!(common::normalize(&ass), common::golden("dialog.karaoke.ass"));
}

#[test]
fn render_rejects_what_is_not_words() {
    let dir = tempfile::tempdir().unwrap();
    for (name, body, says) in [
        ("t.soniox.json", r#"{"tokens": []}"#, "is neither a CAPT file nor a JSON list of words"),
        ("empty.json", "[]", "has no words"),
    ] {
        let input = dir.path().join(name);
        std::fs::write(&input, body).unwrap();
        let out = sonisub(&["render".as_ref(), input.as_os_str()]).output().unwrap();
        assert!(!out.status.success());
        assert!(plain(&out).contains(says), "{}", plain(&out));
    }
}

#[test]
fn a_saved_transcript_writes_capt_and_render_makes_the_other_formats_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path().join("dialog.soniox.json");
    std::fs::copy(common::fixture("dialog.soniox.json"), &t).unwrap();
    let out = sonisub(&[t.as_os_str(), "-t".as_ref(), "capt".as_ref()]).output().unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    let capt = dir.path().join("dialog.capt");
    assert_eq!(common::normalize(&std::fs::read_to_string(&capt).unwrap()), common::golden("dialog.capt"));

    let out = sonisub(&["render".as_ref(), capt.as_os_str(), "-t".as_ref(), "srt,vtt,ass,premiere".as_ref()])
        .output()
        .unwrap();
    let err = plain(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.starts_with("✓ dialog.capt — 10 cues -> "), "{err}");
    for file in ["dialog.srt", "dialog.vtt", "dialog.ass"] {
        let written = std::fs::read_to_string(dir.path().join(file)).unwrap();
        assert_eq!(common::normalize(&written), common::golden(file), "{file}");
    }
    let premiere: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("dialog.premiere.json")).unwrap()).unwrap();
    assert_eq!(premiere["speakers"], common::golden_json("dialog.premiere.json")["speakers"], "the same speaker ids");
}

#[test]
fn render_names_every_problem_of_a_capt_file() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("bad.capt");
    std::fs::write(&input, "CAPT 1\n\n00:00:01.000 --> 00:00:02.000\n{0}a{x}\n{0..9} lang:en\n").unwrap();
    let out = sonisub(&["render".as_ref(), input.as_os_str()]).output().unwrap();
    let err = plain(&out);
    assert!(!out.status.success());
    assert!(err.contains("is not a valid CAPT file"), "{err}");
    assert!(err.contains("line 4: a mark holds a time in hundredths, not “x”"), "{err}");
}

#[test]
fn speaker_names_go_into_capt_and_come_back_out_of_it() {
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path().join("dialog.soniox.json");
    std::fs::copy(common::fixture("dialog.soniox.json"), &t).unwrap();
    let out =
        sonisub(&[t.as_os_str(), "-t".as_ref(), "capt".as_ref(), "--speaker-names".as_ref(), "Anna,Boris".as_ref()])
            .output()
            .unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    let capt = dir.path().join("dialog.capt");
    let text = std::fs::read_to_string(&capt).unwrap();
    assert!(text.contains("speaker:1 name:Anna\nspeaker:2 name:Boris\nspeaker:3\n"), "{text}");

    let out = sonisub(&["render".as_ref(), capt.as_os_str(), "-t".as_ref(), "srt,ass".as_ref(), "-s".as_ref()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    let srt = std::fs::read_to_string(dir.path().join("dialog.srt")).unwrap();
    assert!(srt.contains("Anna: Hi, today"), "{srt}");
    let ass = std::fs::read_to_string(dir.path().join("dialog.ass")).unwrap();
    assert!(ass.contains("Style: Anna,") && ass.contains(",Boris,"), "{ass}");
}

#[test]
fn vertical_sdh_subtitles_from_a_saved_transcript_and_the_frame_in_its_capt() {
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path().join("dialog.soniox.json");
    std::fs::copy(common::fixture("dialog.soniox.json"), &t).unwrap();
    let all = "capt,srt,vtt,ass";
    let args = [t.as_os_str(), "-t".as_ref(), all.as_ref(), "--vertical".as_ref(), "--sdh".as_ref()];
    let out = sonisub(&args).output().unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    for ext in ["capt", "srt", "vtt", "ass"] {
        assert!(dir.path().join(format!("dialog.{ext}")).is_file(), "dialog.{ext}");
    }
    let capt = std::fs::read_to_string(dir.path().join("dialog.capt")).unwrap();
    assert!(capt.contains("\nframe:1080,1920\n"), "the frame goes into the CAPT file:\n{capt}");
    let ass = std::fs::read_to_string(dir.path().join("dialog.ass")).unwrap();
    assert!(ass.contains("PlayResX: 1080\nPlayResY: 1920\n"), "{ass}");

    // render takes the frame from the CAPT file: a vertical ASS again, without --vertical.
    let capt_path = dir.path().join("dialog.capt");
    let out = sonisub(&["render".as_ref(), capt_path.as_os_str(), "-t".as_ref(), "ass".as_ref(), "-f".as_ref()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    let ass = std::fs::read_to_string(dir.path().join("dialog.ass")).unwrap();
    assert!(ass.contains("PlayResX: 1080\nPlayResY: 1920\n"), "{ass}");
}

#[test]
fn render_writes_the_frame_it_is_given_into_the_capt_file() {
    let dir = tempfile::tempdir().unwrap();
    let capt = dir.path().join("dialog.capt");
    std::fs::copy(common::fixture("dialog.capt"), &capt).unwrap();
    let base = dir.path().join("phone.srt");
    let args = ["render".as_ref(), capt.as_os_str(), "-t".as_ref(), "capt".as_ref(), "-o".as_ref(), base.as_os_str()];
    let out = sonisub(&[&args[..], &["--vertical".as_ref()]].concat()).output().unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    let written = dir.path().join("phone.capt");
    assert!(std::fs::read_to_string(&written).unwrap().contains("\nframe:1080,1920\n"));
    // …so the next render of it is vertical without being told.
    let out = sonisub(&["render".as_ref(), written.as_os_str(), "-t".as_ref(), "ass".as_ref()]).output().unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    let ass = std::fs::read_to_string(dir.path().join("phone.ass")).unwrap();
    assert!(ass.contains("PlayResX: 1080\nPlayResY: 1920\n"), "{ass}");
}

#[test]
fn sdh_adds_sounds_and_voices_to_the_subtitles_it_writes() {
    let dir = tempfile::tempdir().unwrap();
    let capt = dir.path().join("marks.capt");
    std::fs::copy(common::fixture("marks.capt"), &capt).unwrap();
    let out = sonisub(&["render".as_ref(), capt.as_os_str(), "-t".as_ref(), "srt,vtt".as_ref(), "--sdh".as_ref()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    for ext in ["srt", "vtt"] {
        let text = std::fs::read_to_string(dir.path().join(format!("marks.{ext}"))).unwrap();
        assert!(text.contains("[phone ringing]") && text.contains("[whispering]"), "{text}");
    }
}

#[test]
fn a_capt_file_of_sounds_only_gives_sdh_subtitles() {
    let dir = tempfile::tempdir().unwrap();
    let capt = dir.path().join("intro.capt");
    std::fs::write(&capt, "CAPT 1\n\n00:00:00.000 --> 00:00:04.000 sound:music\n").unwrap();
    let out = sonisub(&["render".as_ref(), capt.as_os_str(), "-t".as_ref(), "srt".as_ref(), "--sdh".as_ref()])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    let srt = std::fs::read_to_string(dir.path().join("intro.srt")).unwrap();
    assert_eq!(srt, "1\n00:00:00,000 --> 00:00:04,000\n[music]\n\n");
    // Without --sdh there is nothing to write.
    let out =
        sonisub(&["render".as_ref(), capt.as_os_str(), "-t".as_ref(), "srt".as_ref(), "-f".as_ref()]).output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn labels_without_sdh_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let capt = dir.path().join("marks.capt");
    std::fs::copy(common::fixture("marks.capt"), &capt).unwrap();
    let out = sonisub(&["render".as_ref(), capt.as_os_str(), "--labels".as_ref(), "ru".as_ref()]).output().unwrap();
    assert!(!out.status.success());
    assert!(plain(&out).contains("--sdh"), "{}", plain(&out));
}

#[test]
fn render_writes_sbv() {
    let dir = tempfile::tempdir().unwrap();
    let capt = dir.path().join("dialog.capt");
    std::fs::copy(common::fixture("dialog.capt"), &capt).unwrap();
    let out =
        sonisub(&["render".as_ref(), capt.as_os_str(), "-t".as_ref(), "sbv".as_ref(), "-s".as_ref()]).output().unwrap();
    assert!(out.status.success(), "{}", plain(&out));
    let written = std::fs::read_to_string(dir.path().join("dialog.sbv")).unwrap();
    assert_eq!(common::normalize(&written), common::golden("dialog.sbv"));
}

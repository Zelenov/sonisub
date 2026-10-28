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

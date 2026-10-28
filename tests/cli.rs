//! The command line as a user sees it, with stderr piped (not a terminal).

#![cfg(feature = "cli")]

use std::process::Command;

fn run(extra: &[&str]) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let src = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dialog.soniox.json");
    let input = dir.path().join("dialog.soniox.json");
    std::fs::copy(src, &input).unwrap();
    let other = dir.path().join("second.soniox.json");
    std::fs::copy(src, &other).unwrap();
    Command::new(env!("CARGO_BIN_EXE_sonisub"))
        .args([&input, &other])
        .args(extra)
        .env_remove("SONIOX_API_KEY")
        .output()
        .unwrap()
}

#[test]
fn without_a_terminal_there_are_only_plain_lines_and_a_summary() {
    for extra in [&[][..], &["--quiet"][..]] {
        let out = run(extra);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains('\x1b'), "escape codes: {err:?}");
        assert!(!err.contains('\r'), "redraws: {err:?}");
        assert!(err.contains("✓ dialog.soniox.json") || err.contains("✓ second.soniox.json"), "{err}");
        assert!(err.contains("done: 2 from saved transcripts") || err.contains("done: 2 processed"), "{err}");
    }
}

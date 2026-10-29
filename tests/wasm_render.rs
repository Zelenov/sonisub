//! The files `captions-wasm`'s `render` must give, byte for byte, are what `sonisub render` writes:
//! every case of `captions-wasm/tests/cases` against `captions-wasm/tests/expected`.
//!
//! `BLESS=1 cargo test --test wasm_render` rewrites the expected files; read the diff first.

#[path = "../captions-wasm/tests/cases/mod.rs"]
mod cases;

use std::path::PathBuf;
use std::process::Command;

#[test]
fn every_render_case_is_what_the_command_line_writes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bless = std::env::var_os("BLESS").is_some();
    for c in cases::CASES {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join(format!("{}.capt", c.capt));
        std::fs::copy(root.join("tests/fixtures").join(format!("{}.capt", c.capt)), &input).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_sonisub"))
            .arg("render")
            .arg(&input)
            .args(["-t", c.format, "-o"])
            .arg(dir.path().join("out.srt"))
            .args(c.args)
            .env_remove("SONIOX_API_KEY")
            .output()
            .unwrap();
        assert!(out.status.success(), "{} {}: {}", c.name, c.format, String::from_utf8_lossy(&out.stderr));
        let written = std::fs::read(dir.path().join(format!("out.{}", c.ext))).unwrap();
        let expected = root.join("captions-wasm/tests/expected").join(format!("{}.{}", c.name, c.ext));
        if bless {
            std::fs::write(&expected, &written).unwrap();
        }
        let expected = std::fs::read(&expected).unwrap_or_default();
        assert!(written == expected, "{} {}: differs from {}", c.name, c.format, c.ext);
    }
}

//! What `render` is checked on: a CAPT fixture of `tests/fixtures`, a format, options as
//! JavaScript gives them, and the same options as `sonisub render` arguments. The expected file
//! `tests/expected/<name>.<ext>` is what the command line writes: sonisub's `tests/wasm_render.rs`
//! checks that (`BLESS=1` rewrites them), `tests/node.rs` that `render` gives it byte for byte.

#![allow(dead_code)]

pub struct Case {
    pub name: &'static str,
    /// `dialog` or `marks`: `tests/fixtures/<capt>.capt`.
    pub capt: &'static str,
    pub format: &'static str,
    /// The extension `sonisub render` writes the format with.
    pub ext: &'static str,
    /// `render`'s options, JSON.
    pub options: &'static str,
    /// The same for `sonisub render`.
    pub args: &'static [&'static str],
}

const fn case(
    name: &'static str,
    capt: &'static str,
    format: &'static str,
    ext: &'static str,
    options: &'static str,
    args: &'static [&'static str],
) -> Case {
    Case { name, capt, format, ext, options, args }
}

pub const CASES: &[Case] = &[
    // Every format with its defaults, of both files.
    case("dialog", "dialog", "srt", "srt", "{}", &[]),
    case("dialog", "dialog", "vtt", "vtt", "{}", &[]),
    case("dialog", "dialog", "ass", "ass", "{}", &[]),
    case("dialog", "dialog", "premiere", "premiere.json", "{}", &[]),
    case("dialog", "dialog", "capt", "capt", "{}", &[]),
    case("dialog", "dialog", "sbv", "sbv", "{}", &[]),
    case("dialog", "dialog", "lrc", "lrc", "{}", &[]),
    case("dialog", "dialog", "transcript", "txt", "{}", &[]),
    case("dialog", "dialog", "ttml", "ttml", "{}", &[]),
    case("dialog", "dialog", "adr", "adr.csv", "{}", &[]),
    case("dialog", "dialog", "ytt", "ytt", "{}", &[]),
    case("marks", "marks", "srt", "srt", "{}", &[]),
    case("marks", "marks", "vtt", "vtt", "{}", &[]),
    case("marks", "marks", "ass", "ass", "{}", &[]),
    case("marks", "marks", "premiere", "premiere.json", "{}", &[]),
    case("marks", "marks", "capt", "capt", "{}", &[]),
    case("marks", "marks", "sbv", "sbv", "{}", &[]),
    case("marks", "marks", "lrc", "lrc", "{}", &[]),
    case("marks", "marks", "transcript", "txt", "{}", &[]),
    case("marks", "marks", "ttml", "ttml", "{}", &[]),
    case("marks", "marks", "adr", "adr.csv", "{}", &[]),
    case("marks", "marks", "ytt", "ytt", "{}", &[]),
    // Options that are not the defaults.
    case("dialog.speakers", "dialog", "srt", "srt", r#"{"speakerLabels": true}"#, &["-s"]),
    case("dialog.wrap", "dialog", "srt", "srt", r#"{"wrapLines": true}"#, &["-w"]),
    case("dialog.unlimited", "dialog", "srt", "srt", r#"{"maxLine": 0, "maxDuration": 0}"#, &["-u"]),
    case(
        "marks.narrow",
        "marks",
        "srt",
        "srt",
        r#"{"maxLine": 22, "maxDuration": 4, "wrapLines": true}"#,
        &["--max-line", "22", "--max-duration", "4", "-w"],
    ),
    case(
        "dialog.names",
        "dialog",
        "vtt",
        "vtt",
        r#"{"speakerNames": ["Anna", "Boris"], "speakerLabels": true}"#,
        &["--speaker-names", "Anna,Boris"],
    ),
    case("dialog.karaoke", "dialog", "ass", "ass", r#"{"karaoke": true}"#, &["--karaoke"]),
    case(
        "marks.karaoke",
        "marks",
        "ass",
        "ass",
        r#"{"karaoke": true, "speakerLabels": true, "wrapLines": true, "maxLine": 30, "maxLines": 3}"#,
        &["--karaoke", "-s", "-w", "--max-line", "30", "--max-lines", "3"],
    ),
    case(
        "dialog.gap",
        "dialog",
        "lrc",
        "lrc",
        r#"{"gap": 0.5, "minDuration": 1}"#,
        &["--gap", "0.5", "--min-duration", "1"],
    ),
    case("dialog.gap", "dialog", "premiere", "premiere.json", r#"{"gap": 0.5}"#, &["--gap", "0.5"]),
    case("dialog.speakers", "dialog", "ytt", "ytt", r#"{"speakerLabels": true}"#, &["-s"]),
    case("dialog.speakers", "dialog", "sbv", "sbv", r#"{"speakerLabels": true}"#, &["-s"]),
    case(
        "dialog.wrap",
        "dialog",
        "ttml",
        "ttml",
        r#"{"wrapLines": true, "maxLine": 30, "maxLines": 3}"#,
        &["-w", "--max-line", "30", "--max-lines", "3"],
    ),
    case("marks.ru", "marks", "transcript", "txt", r#"{"labels": "ru"}"#, &["--labels", "ru"]),
    case(
        "marks.2997",
        "marks",
        "adr",
        "adr.csv",
        r#"{"fps": "29.97", "timelineStart": "00:59:00:00", "labels": "ru", "speakerNames": ["Ann", "Bob"]}"#,
        &["--fps", "29.97", "--timeline-start", "00:59:00:00", "--labels", "ru", "--speaker-names", "Ann,Bob"],
    ),
    case(
        "marks.names",
        "marks",
        "capt",
        "capt",
        r#"{"speakerNames": ["Ann", "Bob"]}"#,
        &["--speaker-names", "Ann,Bob"],
    ),
];

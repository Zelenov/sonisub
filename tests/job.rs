//! The whole single-file pipeline against a scripted Soniox: output, API calls, errors and cleanup.

mod common;

use std::path::Path;
use std::time::Duration;

use common::{MockSoniox, ok_route, soniox_error};
use serde_json::{Value, json};
use sonisub::job::{self, Options, Outcome};
use sonisub::soniox::{Client, api_error};

fn options(temp: &Path) -> Options {
    Options { temp_dir: Some(temp.to_path_buf()), poll: Duration::from_millis(10), ..Options::default() }
}

fn assert_empty(dir: &Path) {
    let left: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    assert!(left.is_empty(), "temp files left: {left:?}");
}

struct Setup {
    dir: tempfile::TempDir,
    temp: std::path::PathBuf,
    srt: std::path::PathBuf,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("tmp");
    std::fs::create_dir(&temp).unwrap();
    let srt = dir.path().join("dialog.srt");
    Setup { dir, temp, srt }
}

#[test]
fn video_to_srt_and_everything_cleaned_up() {
    let s = setup();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();

    let out = job::process(&common::fixture("dialog.mp4"), &s.srt, &options(&s.temp), Some(&client)).unwrap();

    assert!(matches!(out, Outcome::Written { cues: 9, json: None, .. }), "{out:?}");
    assert_eq!(common::normalize(&std::fs::read_to_string(&s.srt).unwrap()), common::golden_srt());
    assert_eq!(
        mock.calls(),
        [
            "POST /v1/files",
            "POST /v1/transcriptions",
            "GET /v1/transcriptions/t1",
            "GET /v1/transcriptions/t1",
            "GET /v1/transcriptions/t1/transcript",
            "DELETE /v1/transcriptions/t1",
            "DELETE /v1/files/f1",
        ]
    );
    assert_empty(&s.temp);
}

#[test]
fn uploads_flac_and_sends_the_options() {
    let s = setup();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();
    let opts = Options {
        languages: vec!["en".into()],
        strict_languages: true,
        context: Some("Nairobi, UAT".into()),
        diarization: false,
        model: "stt-async-test".into(),
        ..options(&s.temp)
    };
    job::process(&common::fixture("dialog.mp4"), &s.srt, &opts, Some(&client)).unwrap();

    let upload = mock.find("POST", "/v1/files").unwrap();
    assert!(upload.body.windows(4).any(|w| w == b"fLaC"), "upload is not FLAC");
    assert!(upload.body.len() > 100_000, "upload too small: {}", upload.body.len());

    let cfg = mock.find("POST", "/v1/transcriptions").unwrap().json();
    assert_eq!(cfg["model"], "stt-async-test");
    assert_eq!(cfg["file_id"], "f1");
    assert_eq!(cfg["language_hints"], json!(["en"]));
    assert_eq!(cfg["language_hints_strict"], true);
    assert_eq!(cfg["enable_language_identification"], false);
    assert_eq!(cfg["enable_speaker_diarization"], false);
    assert_eq!(cfg["context"], "Nairobi, UAT");
}

#[test]
fn keep_json_saves_the_transcript() {
    let s = setup();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();
    let opts = Options { keep_json: true, ..options(&s.temp) };
    let out = job::process(&common::fixture("dialog.mp4"), &s.srt, &opts, Some(&client)).unwrap();

    let Outcome::Written { json: Some(json), .. } = out else { panic!("{out:?}") };
    assert_eq!(json, s.dir.path().join("dialog.soniox.json"));
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(json).unwrap()).unwrap();
    assert_eq!(saved, common::transcript());
}

#[test]
fn balance_exhausted_on_create_is_fatal_and_cleaned_up() {
    let s = setup();
    let t = common::transcript();
    let mock = MockSoniox::start(move |r, n| match (r.method.as_str(), r.path.as_str()) {
        ("POST", "/v1/transcriptions") => soniox_error(
            402,
            "organization_balance_exhausted",
            "Organization balance exhausted. Please either add funds manually or enable autopay.",
        ),
        _ => ok_route(r, n, &t).unwrap(),
    });
    let client = Client::new(&mock.url, "key").unwrap();

    let err = job::process(&common::fixture("dialog.mp4"), &s.srt, &options(&s.temp), Some(&client)).unwrap_err();

    let api = api_error(&err).expect("an ApiError");
    assert_eq!((api.status, api.error_type.as_str()), (Some(402), "organization_balance_exhausted"));
    assert_eq!(api.request_id.as_deref(), Some("req-test"));
    assert!(api.is_fatal());
    let text = format!("{err:#}");
    assert!(text.contains("top up"), "{text}");
    assert!(mock.calls().contains(&"DELETE /v1/files/f1".to_string()));
    assert!(!s.srt.exists());
    assert_empty(&s.temp);
}

#[test]
fn failed_async_job_is_reported_and_cleaned_up() {
    let s = setup();
    let t = common::transcript();
    let mock = MockSoniox::start(move |r, n| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/v1/transcriptions/t1") => (
            200,
            json!({"id": "t1", "status": "error", "error_type": "organization_balance_exhausted",
                   "error_message": "Organization balance exhausted."}),
        ),
        _ => ok_route(r, n, &t).unwrap(),
    });
    let client = Client::new(&mock.url, "key").unwrap();

    let err = job::process(&common::fixture("dialog.mp4"), &s.srt, &options(&s.temp), Some(&client)).unwrap_err();

    let api = api_error(&err).expect("an ApiError");
    assert_eq!(api.error_type, "organization_balance_exhausted");
    assert!(api.is_fatal());
    let calls = mock.calls();
    assert!(calls.contains(&"DELETE /v1/transcriptions/t1".to_string()), "{calls:?}");
    assert!(calls.contains(&"DELETE /v1/files/f1".to_string()), "{calls:?}");
    assert_empty(&s.temp);
}

#[test]
fn ordinary_job_failure_is_not_fatal() {
    let s = setup();
    let t = common::transcript();
    let mock = MockSoniox::start(move |r, n| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/v1/transcriptions/t1") => (
            200,
            json!({"id": "t1", "status": "error", "error_type": "invalid_audio_file", "error_message": "bad audio"}),
        ),
        _ => ok_route(r, n, &t).unwrap(),
    });
    let client = Client::new(&mock.url, "key").unwrap();
    let err = job::process(&common::fixture("dialog.mp4"), &s.srt, &options(&s.temp), Some(&client)).unwrap_err();
    assert!(!api_error(&err).unwrap().is_fatal());
}

#[test]
fn transient_server_error_is_retried() {
    let s = setup();
    let t = common::transcript();
    let mock = MockSoniox::start(move |r, n| match (r.method.as_str(), r.path.as_str(), n) {
        ("POST", "/v1/transcriptions", 0) => soniox_error(503, "service_unavailable", "try later"),
        _ => ok_route(r, n, &t).unwrap(),
    });
    let client = Client::new(&mock.url, "key").unwrap();
    job::process(&common::fixture("dialog.mp4"), &s.srt, &options(&s.temp), Some(&client)).unwrap();
    assert_eq!(mock.calls().iter().filter(|c| *c == "POST /v1/transcriptions").count(), 2);
}

#[test]
fn bad_key_is_reported_by_check_auth() {
    let mock = MockSoniox::start(|_, _| soniox_error(401, "unauthenticated", "Incorrect API key provided."));
    let err = Client::new(&mock.url, "bad").unwrap().check_auth().unwrap_err();
    let api = api_error(&err).unwrap();
    assert!(api.is_fatal());
    assert!(format!("{err:#}").contains("SONIOX_API_KEY"));
}

#[test]
fn existing_srt_is_skipped_without_force() {
    let s = setup();
    std::fs::write(&s.srt, "old").unwrap();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();

    let out = job::process(&common::fixture("dialog.mp4"), &s.srt, &options(&s.temp), Some(&client)).unwrap();
    assert!(matches!(out, Outcome::Skipped { .. }));
    assert_eq!(std::fs::read_to_string(&s.srt).unwrap(), "old");
    assert!(mock.calls().is_empty());

    let force = Options { force: true, ..options(&s.temp) };
    job::process(&common::fixture("dialog.mp4"), &s.srt, &force, Some(&client)).unwrap();
    assert_eq!(common::normalize(&std::fs::read_to_string(&s.srt).unwrap()), common::golden_srt());
}

#[test]
fn saved_transcript_needs_no_api() {
    let s = setup();
    let out = job::process(&common::fixture("dialog.soniox.json"), &s.srt, &options(&s.temp), None).unwrap();
    assert!(matches!(out, Outcome::Written { cues: 9, .. }));
    assert_eq!(common::normalize(&std::fs::read_to_string(&s.srt).unwrap()), common::golden_srt());
}

#[test]
fn media_without_client_is_an_error() {
    let s = setup();
    let err = job::process(&common::fixture("dialog.mp4"), &s.srt, &options(&s.temp), None).unwrap_err();
    assert!(format!("{err}").contains("API key"));
}

#[test]
fn missing_input_is_an_error() {
    let s = setup();
    let err = job::process(&s.dir.path().join("nope.mp4"), &s.srt, &options(&s.temp), None).unwrap_err();
    assert!(format!("{err}").contains("not found"));
}

#[test]
fn default_output_paths() {
    let p = |s: &str| Path::new(s).to_path_buf();
    assert_eq!(job::default_output(&p("a/clip.MP4"), None), p("a/clip.srt"));
    assert_eq!(job::default_output(&p("a/clip.soniox.json"), None), p("a/clip.srt"));
    assert_eq!(job::default_output(&p("a/v.1.mov"), Some(Path::new("out"))), p("out/v.1.srt"));
}

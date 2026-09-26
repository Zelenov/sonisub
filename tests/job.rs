//! The whole single-file pipeline against a scripted Soniox: output, API calls, errors and cleanup.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{MockSoniox, ok_route, soniox_error};
use serde_json::{Value, json};
use sonisub::cancel::{CancelToken, Cancelled};
use sonisub::job::{self, Format, Options, Outcome};
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

    assert!(matches!(out, Outcome::Written { cues: 10, json: Some(_), uploaded_s: Some(s), .. } if (s - 29.25).abs() < 0.1), "{out:?}");
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
fn the_transcript_is_kept_by_default() {
    let s = setup();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();
    let out = job::process(&common::fixture("dialog.mp4"), &s.srt, &options(&s.temp), Some(&client)).unwrap();

    let Outcome::Written { json: Some(json), .. } = out else { panic!("{out:?}") };
    assert_eq!(json, s.dir.path().join("dialog.soniox.json"));
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(json).unwrap()).unwrap();
    assert_eq!(saved, common::transcript());
}

#[test]
fn no_json_leaves_no_transcript() {
    let s = setup();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();
    let opts = Options { keep_json: false, ..options(&s.temp) };
    let out = job::process(&common::fixture("dialog.mp4"), &s.srt, &opts, Some(&client)).unwrap();

    assert!(matches!(out, Outcome::Written { json: None, .. }), "{out:?}");
    assert!(!s.dir.path().join("dialog.soniox.json").exists());
}

#[test]
fn srt_and_premiere_together() {
    let s = setup();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();
    let opts = Options { formats: vec![Format::Srt, Format::Premiere], ..options(&s.temp) };
    let out = job::process(&common::fixture("dialog.mp4"), &s.srt, &opts, Some(&client)).unwrap();

    let premiere = s.dir.path().join("dialog.premiere.json");
    let Outcome::Written { files, cues: 10, .. } = out else { panic!("{out:?}") };
    assert_eq!(files, [s.srt.clone(), premiere.clone()]);
    assert_eq!(common::normalize(&std::fs::read_to_string(&s.srt).unwrap()), common::golden_srt());
    let t: Value = serde_json::from_str(&std::fs::read_to_string(&premiere).unwrap()).unwrap();
    assert_eq!(t, common::golden_json("dialog.premiere.json"));
}

#[test]
fn a_new_format_is_made_from_the_saved_transcript_leaving_the_srt_alone() {
    let s = setup();
    let input = copy_fixture(&s, "dialog.mp4");
    copy_fixture(&s, "dialog.soniox.json");
    std::fs::write(&s.srt, "edited by hand").unwrap();
    let opts = Options { formats: vec![Format::Srt, Format::Premiere], ..options(&s.temp) };

    let out = job::process(&input, &s.srt, &opts, None).unwrap();
    let Outcome::Written { files, cached: true, uploaded_s: None, .. } = out else { panic!("{out:?}") };
    assert_eq!(files, [s.dir.path().join("dialog.premiere.json")]);
    assert_eq!(std::fs::read_to_string(&s.srt).unwrap(), "edited by hand");

    let again = job::process(&input, &s.srt, &opts, None).unwrap();
    assert!(matches!(again, Outcome::Skipped { .. }), "{again:?}");
}

#[test]
fn output_base_accepts_any_output_extension() {
    for given in ["out/clip.srt", "out/clip.premiere.json", "out/clip.json", "out/clip.SRT", "out/clip"] {
        assert_eq!(job::output_base(Path::new(given)), Path::new("out/clip.srt"), "{given}");
    }
    assert_eq!(Format::Premiere.path(Path::new("out/clip.v2.srt")), Path::new("out/clip.v2.premiere.json"));
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
    assert!(matches!(out, Outcome::Written { cues: 10, .. }));
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

// ---------------------------------------------------------------- nothing to transcribe

fn copy_fixture(s: &Setup, name: &str) -> std::path::PathBuf {
    let to = s.dir.path().join(name);
    std::fs::copy(common::fixture(name), &to).unwrap();
    to
}

#[test]
fn empty_media_file_is_an_error_without_api_calls() {
    let s = setup();
    let input = s.dir.path().join("empty.mp4");
    std::fs::write(&input, b"").unwrap();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();
    let err = job::process(&input, &s.srt, &options(&s.temp), Some(&client)).unwrap_err();
    assert!(format!("{err}").contains("empty file"), "{err}");
    assert!(mock.calls().is_empty());
    assert!(!s.srt.exists());
}

#[test]
fn empty_or_foreign_json_is_an_error() {
    let s = setup();
    for (name, body, expected) in [
        ("blank.json", "  \n", "empty transcript"),
        ("broken.json", "{\"tokens\": [", "not valid JSON"),
        ("other.json", "{\"hello\": 1}", "not a Soniox transcript"),
    ] {
        let input = s.dir.path().join(name);
        std::fs::write(&input, body).unwrap();
        let err = job::process(&input, &s.srt, &options(&s.temp), None).unwrap_err();
        assert!(format!("{err:#}").contains(expected), "{name}: {err:#}");
        assert!(!s.srt.exists());
    }
}

#[test]
fn video_without_audio_track_is_reported_not_uploaded() {
    let s = setup();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();
    let out = job::process(&common::fixture("noaudio.mp4"), &s.srt, &options(&s.temp), Some(&client)).unwrap();
    assert!(matches!(&out, Outcome::NoAudio { reason } if reason == "no audio track"), "{out:?}");
    assert!(mock.calls().is_empty());
    assert!(!s.srt.exists());
    assert_empty(&s.temp);
}

#[test]
fn digital_silence_is_not_uploaded() {
    let s = setup();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();
    let out = job::process(&common::fixture("silence.m4a"), &s.srt, &options(&s.temp), Some(&client)).unwrap();
    let marker = job::transcript_path(&s.srt);
    assert!(matches!(&out, Outcome::NoSpeech { marker: Some(m), cached: false, uploaded_s: None } if *m == marker), "{out:?}");
    assert!(mock.calls().is_empty());
    // Next time it is known without decoding.
    let again = job::process(&common::fixture("silence.m4a"), &s.srt, &options(&s.temp), Some(&client)).unwrap();
    assert!(matches!(again, Outcome::NoSpeech { cached: true, .. }), "{again:?}");
    assert!(!s.srt.exists());
    assert_empty(&s.temp);
}

#[test]
fn no_speech_leaves_a_marker_and_is_not_paid_for_twice() {
    let s = setup();
    let input = copy_fixture(&s, "nospeech.mp4");
    let srt = s.dir.path().join("nospeech.srt");
    // Real Soniox answer for this video: no tokens.
    let mock = MockSoniox::happy(common::transcript_of("nospeech"));
    let client = Client::new(&mock.url, "key").unwrap();

    let first = job::process(&input, &srt, &options(&s.temp), Some(&client)).unwrap();
    let marker = s.dir.path().join("nospeech.soniox.json");
    assert!(matches!(&first, Outcome::NoSpeech { marker: Some(m), cached: false, uploaded_s: Some(_) } if *m == marker), "{first:?}");
    assert!(!srt.exists(), "no .srt for a file without speech");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&marker).unwrap()).unwrap();
    assert_eq!(saved["tokens"], json!([]));
    let paid = mock.calls().len();
    assert!(mock.calls().contains(&"POST /v1/files".to_string()));

    let second = job::process(&input, &srt, &options(&s.temp), Some(&client)).unwrap();
    assert!(matches!(second, Outcome::NoSpeech { marker: Some(_), cached: true, uploaded_s: None }), "{second:?}");
    assert_eq!(mock.calls().len(), paid, "second run must not call Soniox");

    // --force asks Soniox again.
    let force = Options { force: true, ..options(&s.temp) };
    job::process(&input, &srt, &force, Some(&client)).unwrap();
    assert!(mock.calls().len() > paid);
}

#[test]
fn saved_transcript_next_to_the_output_is_reused() {
    let s = setup();
    let input = copy_fixture(&s, "dialog.mp4");
    copy_fixture(&s, "dialog.soniox.json");
    let mock = MockSoniox::happy(json!({"tokens": []}));
    let client = Client::new(&mock.url, "key").unwrap();

    let out = job::process(&input, &s.srt, &options(&s.temp), Some(&client)).unwrap();
    assert!(matches!(out, Outcome::Written { cues: 10, cached: true, uploaded_s: None, .. }), "{out:?}");
    assert!(mock.calls().is_empty(), "{:?}", mock.calls());
    assert_eq!(common::normalize(&std::fs::read_to_string(&s.srt).unwrap()), common::golden_srt());
}

#[test]
fn transcript_input_without_words_writes_nothing() {
    let s = setup();
    let out = job::process(&common::fixture("nospeech.soniox.json"), &s.srt, &options(&s.temp), None).unwrap();
    assert!(matches!(out, Outcome::NoSpeech { marker: None, cached: false, uploaded_s: None }), "{out:?}");
    assert!(!s.srt.exists());
}

/// A Soniox whose transcription never finishes.
fn stuck_soniox() -> MockSoniox {
    MockSoniox::start(|r, n| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/v1/transcriptions/t1") => (200, json!({"id": "t1", "status": "processing"})),
        _ => ok_route(r, n, &Value::Null).unwrap_or((404, json!({}))),
    })
}

#[test]
fn cancelling_a_job_stops_it_and_cleans_up_and_the_next_job_runs() {
    let s = setup();
    let mock = stuck_soniox();
    let cancel = CancelToken::new();
    let client = Client::new(&mock.url, "key").unwrap().with_cancel(cancel.clone());
    let opts = Options { cancel: cancel.clone(), ..options(&s.temp) };

    // Cancelled while Soniox is transcribing: once the job has been created and polled.
    let err = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !mock.calls().iter().any(|c| c == "GET /v1/transcriptions/t1") {
                std::thread::sleep(Duration::from_millis(20));
            }
            cancel.cancel();
        });
        job::process(&common::fixture("dialog.mp4"), &s.srt, &opts, Some(&client)).unwrap_err()
    });

    assert!(err.is::<Cancelled>(), "{err:#}");
    assert!(!s.srt.exists());
    let calls = mock.calls();
    assert!(calls.contains(&"DELETE /v1/transcriptions/t1".to_string()), "{calls:?}");
    assert!(calls.contains(&"DELETE /v1/files/f1".to_string()), "{calls:?}");
    assert_eq!(client.failed_deletes(), 0);
    assert_empty(&s.temp);

    // A new job with its own token is not stopped by the cancelled one.
    let mock = MockSoniox::happy(common::transcript());
    let next = CancelToken::new();
    let client = Client::new(&mock.url, "key").unwrap().with_cancel(next.clone());
    let opts = Options { cancel: next, ..options(&s.temp) };
    let out = job::process(&common::fixture("dialog.mp4"), &s.srt, &opts, Some(&client)).unwrap();
    assert!(matches!(out, Outcome::Written { .. }), "{out:?}");
}

#[test]
fn a_cancelled_job_sends_nothing() {
    let s = setup();
    let mock = MockSoniox::happy(common::transcript());
    let cancel = CancelToken::new();
    cancel.cancel();
    let client = Client::new(&mock.url, "key").unwrap().with_cancel(cancel.clone());
    let opts = Options { cancel, ..options(&s.temp) };
    let err = job::process(&common::fixture("dialog.mp4"), &s.srt, &opts, Some(&client)).unwrap_err();
    assert!(err.is::<Cancelled>(), "{err:#}");
    assert!(mock.calls().is_empty(), "{:?}", mock.calls());
}

#[test]
fn cancel_cuts_the_wait_between_retries() {
    let mock = MockSoniox::start(|_, _| soniox_error(503, "unavailable", "busy"));
    let cancel = CancelToken::new();
    let client = Client::new(&mock.url, "key").unwrap().with_cancel(cancel.clone());
    let canceller = cancel.clone();
    let waker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        canceller.cancel();
    });
    let started = Instant::now();
    let err = client.check_auth().unwrap_err();
    waker.join().unwrap();
    // Without the cancel: 2 + 4 + 8 s of waits between four attempts.
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
    assert!(err.is::<Cancelled>(), "{err:#}");
}

#[test]
fn a_connection_that_never_answers_times_out() {
    // Accepts connections and never answers, like a connection that died silently.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let mut open = Vec::new();
        for stream in listener.incoming().flatten() {
            open.push(stream);
        }
    });
    let cancel = CancelToken::new();
    let client = Client::new(&url, "key").unwrap().with_timeout(Duration::from_millis(200)).with_cancel(cancel.clone());
    let canceller = cancel.clone();
    let waker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1000));
        canceller.cancel();
    });
    let started = Instant::now();
    // Timed out, retried after 2 s, then cancelled during the wait; without the timeout the first
    // request would never return and the cancel could not reach it.
    assert!(client.status("t1").is_err());
    waker.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3), "took {:?}", started.elapsed());
}

#[test]
fn a_failed_remote_delete_is_counted() {
    let s = setup();
    let transcript = common::transcript();
    let mock = MockSoniox::start(move |r, n| match r.method.as_str() {
        "DELETE" => soniox_error(404, "not_found", "gone"),
        _ => ok_route(r, n, &transcript).unwrap_or((404, json!({}))),
    });
    let client = Client::new(&mock.url, "key").unwrap();
    let out = job::process(&common::fixture("dialog.mp4"), &s.srt, &options(&s.temp), Some(&client)).unwrap();
    assert!(matches!(out, Outcome::Written { .. }), "{out:?}");
    assert_eq!(client.failed_deletes(), 2);
}

#[test]
fn a_create_that_times_out_is_not_sent_twice() {
    // Soniox answers too late: it may have created the transcription, so a retry would make a
    // second one that nothing deletes.
    let mock = MockSoniox::start(|_, _| {
        std::thread::sleep(Duration::from_millis(600));
        (201, json!({"id": "t1"}))
    });
    let client = Client::new(&mock.url, "key").unwrap().with_timeout(Duration::from_millis(200));
    assert!(client.create(&json!({})).is_err());
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(mock.calls(), ["POST /v1/transcriptions"]);
}

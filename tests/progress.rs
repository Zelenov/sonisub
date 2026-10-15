//! The progress callback: stages in order, only forward, an estimate that holds at its cap, and cancel at every stage.

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{MockSoniox, ok_route};
use serde_json::{Value, json};
use sonisub::cancel::{CancelToken, Cancelled};
use sonisub::job::{self, Options, Outcome};
use sonisub::progress::{Callback, Progress, Stage, TRANSCRIBE_CAP};
use sonisub::soniox::Client;

type Log = Arc<Mutex<Vec<Progress>>>;

fn options(temp: &Path, log: &Log) -> Options {
    let log = log.clone();
    Options {
        temp_dir: Some(temp.to_path_buf()),
        poll: Duration::from_millis(10),
        on_progress: Some(Callback::new(move |p| log.lock().unwrap().push(p))),
        ..Options::default()
    }
}

fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("tmp");
    std::fs::create_dir(&temp).unwrap();
    let srt = dir.path().join("dialog.srt");
    (dir, temp, srt)
}

/// Soniox that stays "processing" for `polls` status checks, then completes.
fn slow_soniox(polls: usize) -> MockSoniox {
    let transcript = common::transcript();
    MockSoniox::start(move |r, n| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/v1/transcriptions/t1") if n < polls => (200, json!({"id": "t1", "status": "processing"})),
        _ => ok_route(r, n, &transcript).unwrap_or((404, json!({}))),
    })
}

/// A client whose server is never asked.
fn idle() -> Client {
    Client::new("http://127.0.0.1:9/v1", "key").unwrap()
}

fn stages(log: &[Progress]) -> Vec<Stage> {
    let mut out: Vec<Stage> = Vec::new();
    for p in log {
        if out.last() != Some(&p.stage) {
            out.push(p.stage);
        }
    }
    out
}

fn assert_only_forward(log: &[Progress]) {
    for w in log.windows(2) {
        assert!(w[1].overall >= w[0].overall, "overall went back: {:?} -> {:?}", w[0], w[1]);
        assert!(w[1].stage >= w[0].stage, "stage went back: {:?} -> {:?}", w[0], w[1]);
        if w[1].stage == w[0].stage {
            assert!(w[1].fraction >= w[0].fraction, "fraction went back: {:?} -> {:?}", w[0], w[1]);
        }
    }
    for p in log {
        assert!((0.0..=1.0).contains(&p.overall), "{p:?}");
    }
}

#[test]
fn a_run_passes_through_every_stage_and_only_goes_forward() {
    let (_dir, temp, srt) = setup();
    let log = Log::default();
    let mock = slow_soniox(3);
    let client = Client::new(&mock.url, "key").unwrap();

    let out = job::process(&common::fixture("dialog.mp4"), &srt, &options(&temp, &log), Some(&client)).unwrap();
    assert!(matches!(out, Outcome::Written { .. }), "{out:?}");

    let log = log.lock().unwrap();
    assert_eq!(
        stages(&log),
        [Stage::Extracting, Stage::Uploading, Stage::Transcribing, Stage::Downloading, Stage::Writing]
    );
    assert_only_forward(&log);
    for stage in [Stage::Extracting, Stage::Uploading, Stage::Transcribing, Stage::Downloading, Stage::Writing] {
        let of_stage: Vec<_> = log.iter().filter(|p| p.stage == stage).collect();
        // `None` while the length is not known yet.
        assert!(matches!(of_stage.first().unwrap().fraction, None | Some(0.0)), "{stage:?} starts at 0");
        assert_eq!(of_stage.last().unwrap().fraction, Some(1.0), "{stage:?} ends at 1");
    }
    assert!((log.last().unwrap().overall - 1.0).abs() < 1e-4);
    // Not 0 % for the whole run: the first report is the start of extraction, and the bar is well on its
    // way before transcribing begins.
    let first_transcribing = log.iter().find(|p| p.stage == Stage::Transcribing).unwrap();
    assert!(first_transcribing.overall >= 0.29, "{first_transcribing:?}");
}

#[test]
fn the_transcribing_estimate_moves_and_never_decreases() {
    let (_dir, temp, srt) = setup();
    let log = Log::default();
    // Soniox is slow: far more polls than the estimate for 29 s of audio allows.
    let mock = slow_soniox(10_000);
    let cancel = CancelToken::new();
    let client = Client::new(&mock.url, "key").unwrap().with_cancel(cancel.clone());
    let mut opts = options(&temp, &log);
    opts.cancel = cancel.clone();
    let watcher = log.clone();
    opts.on_progress = Some(Callback::new(move |p| {
        let mut l = watcher.lock().unwrap();
        l.push(p);
        // Stop once the estimate has been moving for a while.
        let moving = l.iter().filter(|p| p.stage == Stage::Transcribing && p.fraction > Some(0.0)).count();
        if moving >= 8 {
            cancel.cancel();
        }
    }));

    let err = job::process(&common::fixture("dialog.mp4"), &srt, &opts, Some(&client)).unwrap_err();
    assert!(err.is::<Cancelled>(), "{err:#}");

    let log = log.lock().unwrap();
    assert_only_forward(&log);
    let transcribing: Vec<_> = log.iter().filter(|p| p.stage == Stage::Transcribing).collect();
    assert!(transcribing.iter().any(|p| p.fraction.is_some_and(|f| f > 0.0 && f < TRANSCRIBE_CAP)), "it moves");
    assert!(transcribing.iter().all(|p| p.fraction.unwrap() <= TRANSCRIBE_CAP), "never past the cap before completion");
    assert!(transcribing.first().unwrap().remaining.is_some(), "the time left is there from the start");
    assert!(transcribing.iter().any(|p| p.detail.as_deref() == Some("processing")));
    assert!(log.iter().all(|p| p.stage <= Stage::Transcribing), "nothing after transcribing without `completed`");
}

#[test]
fn a_saved_transcript_only_writes() {
    let (dir, temp, srt) = setup();
    let log = Log::default();
    let t: Value = common::transcript();
    let input = dir.path().join("dialog.soniox.json");
    std::fs::write(&input, serde_json::to_string(&t).unwrap()).unwrap();

    job::process(&input, &srt, &options(&temp, &log), None).unwrap();
    let log = log.lock().unwrap();
    assert_eq!(stages(&log), [Stage::Writing]);
    assert!((log.last().unwrap().overall - 1.0).abs() < 1e-4);
}

#[test]
fn files_without_speech_end_the_bar_and_files_without_audio_or_work_report_nothing_more() {
    // Digital silence: nothing is sent, the bar still ends.
    let (_dir, temp, srt) = setup();
    let log = Log::default();
    let out = job::process(&common::fixture("silence.m4a"), &srt, &options(&temp, &log), Some(&idle())).unwrap();
    assert!(matches!(out, Outcome::NoSpeech { .. }), "{out:?}");
    assert!((log.lock().unwrap().last().unwrap().overall - 1.0).abs() < 1e-4);

    // No audio track: it ends at extraction.
    let (_dir, temp, srt) = setup();
    let log = Log::default();
    let out = job::process(&common::fixture("noaudio.mp4"), &srt, &options(&temp, &log), Some(&idle())).unwrap();
    assert!(matches!(out, Outcome::NoAudio { .. }), "{out:?}");
    assert!(log.lock().unwrap().iter().all(|p| p.stage == Stage::Extracting));

    let (_dir, temp, srt) = setup();
    std::fs::write(&srt, "x").unwrap();
    let log = Log::default();
    let out = job::process(&common::fixture("dialog.mp4"), &srt, &options(&temp, &log), None).unwrap();
    assert!(matches!(out, Outcome::Skipped { .. }));
    assert!(log.lock().unwrap().is_empty());
}

/// Cancels the job when it reaches `stage`; returns what the job and the mock saw.
fn cancel_at(stage: Stage) -> (anyhow::Error, Vec<Progress>, Vec<String>) {
    let (_dir, temp, srt) = setup();
    let log = Log::default();
    let mock = slow_soniox(10_000);
    let cancel = CancelToken::new();
    let client = Client::new(&mock.url, "key").unwrap().with_cancel(cancel.clone());
    let mut opts = options(&temp, &log);
    opts.cancel = cancel.clone();
    let watcher = log.clone();
    opts.on_progress = Some(Callback::new(move |p| {
        if p.stage == stage {
            cancel.cancel();
        }
        watcher.lock().unwrap().push(p);
    }));
    let err = job::process(&common::fixture("dialog.mp4"), &srt, &opts, Some(&client)).unwrap_err();
    assert!(!srt.exists());
    let left: Vec<_> = std::fs::read_dir(&temp).unwrap().flatten().collect();
    assert!(left.is_empty(), "temp files left");
    assert_eq!(client.failed_deletes(), 0);
    let log = log.lock().unwrap().clone();
    (err, log, mock.calls())
}

#[test]
fn cancel_while_extracting() {
    let (err, log, calls) = cancel_at(Stage::Extracting);
    assert!(err.is::<Cancelled>(), "{err:#}");
    assert!(log.iter().all(|p| p.stage == Stage::Extracting));
    assert!(calls.is_empty(), "{calls:?}");
}

#[test]
fn cancel_while_uploading() {
    let (err, log, calls) = cancel_at(Stage::Uploading);
    assert!(err.is::<Cancelled>(), "{err:#}");
    assert!(log.iter().all(|p| p.stage <= Stage::Uploading));
    assert!(!calls.iter().any(|c| c == "POST /v1/transcriptions"), "{calls:?}");
}

#[test]
fn cancel_while_transcribing_deletes_what_was_created() {
    let (err, log, calls) = cancel_at(Stage::Transcribing);
    assert!(err.is::<Cancelled>(), "{err:#}");
    assert!(log.iter().all(|p| p.stage <= Stage::Transcribing));
    assert!(calls.contains(&"DELETE /v1/transcriptions/t1".to_string()), "{calls:?}");
    assert!(calls.contains(&"DELETE /v1/files/f1".to_string()), "{calls:?}");
}

#[test]
fn stage_weights_add_up_to_one() {
    let all = [Stage::Extracting, Stage::Uploading, Stage::Transcribing, Stage::Downloading, Stage::Writing];
    assert!((all.iter().map(|s| s.weight()).sum::<f32>() - 1.0).abs() < 1e-6);
    assert_eq!(Stage::Extracting.start(), 0.0);
    assert!((Stage::Writing.start() + Stage::Writing.weight() - 1.0).abs() < 1e-6);
}

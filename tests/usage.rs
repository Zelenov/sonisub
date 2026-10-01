//! Spending from Soniox usage logs: totals, price per hour, one run's cost, paging and date windows.

mod common;

use std::time::{Duration, SystemTime};

use common::MockSoniox;
use serde_json::{Value, json};
use sonisub::soniox::Client;
use sonisub::usage::{self, Summary};

fn entry(model: &str, audio_ms: u64, cost: &str, reference: &str) -> Value {
    json!({"uuid": "u", "model": model, "request_scope": "project", "client_reference_id": reference,
           "start_time": "2026-09-20T10:00:00Z", "end_time": "2026-09-20T10:00:05Z",
           "input_audio_duration_ms": audio_ms, "cost_usd": cost})
}

#[test]
fn summary_adds_up_per_model_most_expensive_first() {
    let logs = [
        entry("stt-async-v5", 60_000, "0.0016250000", "a"),
        entry("tts-rt-v2", 0, "0.0100000000", "b"),
        entry("stt-async-v5", 120_000, "0.0032500000", "a"),
    ];
    let s = Summary::of(&logs);
    assert_eq!(s.models.len(), 2);
    assert_eq!(s.models[0].model, "tts-rt-v2");
    assert_eq!((s.models[1].requests, s.models[1].audio_ms), (2, 180_000));
    assert!((s.models[1].cost_usd - 0.004875).abs() < 1e-9);
    assert!((s.cost_usd - 0.014875).abs() < 1e-9);
}

#[test]
fn price_per_hour_comes_from_transcription_only() {
    let logs = [entry("stt-async-v5", 3_600_000, "0.0975", "a"), entry("tts-rt-v2", 0, "5.0", "b")];
    assert!((Summary::of(&logs).stt_usd_per_hour().unwrap() - 0.0975).abs() < 1e-9);
    assert_eq!(Summary::of(&[entry("tts-rt-v2", 0, "1.0", "b")]).stt_usd_per_hour(), None);
    assert_eq!(Summary::of(&[]).stt_usd_per_hour(), None);
}

#[test]
fn numeric_costs_are_accepted_too() {
    let logs = [json!({"model": "stt-async-v5", "input_audio_duration_ms": 1000, "cost_usd": 0.5})];
    assert!((Summary::of(&logs).cost_usd - 0.5).abs() < 1e-9);
}

#[test]
fn one_run_is_found_by_its_reference() {
    let logs = [entry("stt-async-v5", 1, "0.1", "sonisub-1"), entry("stt-async-v5", 1, "0.2", "sonisub-2")];
    let mine = usage::of_run(&logs, "sonisub-2");
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["cost_usd"], "0.2");
}

#[test]
fn report_shows_models_total_today_and_price() {
    let logs = [entry("stt-async-v5", 11 * 60_000, "0.0180", "a"), entry("tts-rt-v2", 0, "0.5015", "b")];
    let r = usage::report(&Summary::of(&logs), &Summary::of(&logs[..1]), 30);
    for part in [
        "last 30 days",
        "tts-rt-v2",
        "stt-async-v5",
        "11:00 audio",
        "$0.5195",
        "today (UTC): $0.0180",
        "per hour",
        "console.soniox.com",
    ] {
        assert!(r.contains(part), "{part:?} missing in:\n{r}");
    }
}

#[test]
fn formatting() {
    assert_eq!(usage::fmt_usd(0.0009381), "$0.0009");
    assert_eq!(usage::fmt_usd(12.5), "$12.50");
    assert_eq!(usage::fmt_minutes(29_400), "0:29");
    assert_eq!(usage::fmt_minutes(3_725_000), "1:02:05");
}

#[test]
fn logs_are_paged_through() {
    let mock = MockSoniox::start(|r, n| {
        assert_eq!(r.path, "/v1/usage-logs");
        match n {
            0 => (200, json!({"usage_logs": [entry("stt-async-v5", 1000, "0.1", "a")], "next_page_cursor": "p2"})),
            _ => (200, json!({"usage_logs": [entry("stt-async-v5", 1000, "0.2", "a")], "next_page_cursor": null})),
        }
    });
    let client = Client::new(&mock.url, "key").unwrap();
    let now = SystemTime::now();
    let logs = client.usage_logs(now - Duration::from_secs(3600), now).unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(mock.calls().len(), 2);
}

#[test]
fn ninety_one_days_are_fetched_in_31_day_windows_inside_retention() {
    let mock = MockSoniox::start(|_, _| (200, json!({"usage_logs": [], "next_page_cursor": null})));
    let client = Client::new(&mock.url, "key").unwrap();
    let now = SystemTime::now();
    usage::fetch(&client, now - Duration::from_secs(91 * 86_400), now).unwrap();
    // 91 days minus the safety margin: 31 + 31 + ~29.
    assert_eq!(mock.calls().len(), 3);
}

#[test]
fn run_reference_is_sent_to_soniox() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockSoniox::happy(common::transcript());
    let client = Client::new(&mock.url, "key").unwrap();
    let opts = sonisub::job::Options { reference: "sonisub-42".into(), ..Default::default() };
    sonisub::job::process(&common::fixture("dialog.mp4"), &dir.path().join("d.srt"), &opts, Some(&client)).unwrap();
    let cfg = mock.find("POST", "/v1/transcriptions").unwrap().json();
    assert_eq!(cfg["client_reference_id"], "sonisub-42");
}

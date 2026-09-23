//! Spending, from Soniox usage logs. Soniox exposes what was spent, not what is left.

use std::time::{Duration, SystemTime};

use anyhow::Result;
use serde_json::Value;

use crate::soniox::Client;

/// Soniox keeps usage logs this long.
pub const MAX_DAYS: u64 = 91;
/// Used when there is no transcription in the logs to learn the price from.
pub const FALLBACK_USD_PER_HOUR: f64 = 0.10;

/// One model's totals.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelUsage {
    pub model: String,
    pub requests: usize,
    pub audio_ms: u64,
    pub cost_usd: f64,
}

/// Totals over a set of log entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    /// Per model, most expensive first.
    pub models: Vec<ModelUsage>,
    pub cost_usd: f64,
}

impl Summary {
    pub fn of(logs: &[Value]) -> Self {
        let mut models: Vec<ModelUsage> = Vec::new();
        for e in logs {
            let model = e["model"].as_str().unwrap_or("unknown");
            let i = match models.iter().position(|m| m.model == model) {
                Some(i) => i,
                None => {
                    models.push(ModelUsage { model: model.into(), requests: 0, audio_ms: 0, cost_usd: 0.0 });
                    models.len() - 1
                }
            };
            let m = &mut models[i];
            m.requests += 1;
            m.audio_ms += e["input_audio_duration_ms"].as_u64().unwrap_or(0);
            m.cost_usd += cost(e);
        }
        models.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd));
        let cost_usd = models.iter().map(|m| m.cost_usd).sum();
        Self { models, cost_usd }
    }

    /// Transcription price per hour of audio, learned from speech-to-text entries.
    pub fn stt_usd_per_hour(&self) -> Option<f64> {
        let (ms, usd) = self
            .models
            .iter()
            .filter(|m| m.model.starts_with("stt"))
            .fold((0u64, 0.0), |(ms, usd), m| (ms + m.audio_ms, usd + m.cost_usd));
        (ms > 0 && usd > 0.0).then(|| usd / (ms as f64 / 3_600_000.0))
    }
}

/// `cost_usd` is a decimal string in the API; accept numbers too.
fn cost(e: &Value) -> f64 {
    match &e["cost_usd"] {
        Value::String(s) => s.parse().unwrap_or(0.0),
        v => v.as_f64().unwrap_or(0.0),
    }
}

/// Log entries between `from` and `to`, fetched in the ≤31-day windows the API allows.
pub fn fetch(client: &Client, from: SystemTime, to: SystemTime) -> Result<Vec<Value>> {
    const WINDOW: Duration = Duration::from_secs(31 * 24 * 3600);
    // Soniox refuses a start even seconds past its retention window.
    let oldest = to - Duration::from_secs(MAX_DAYS * 86_400 - 600);
    let mut out = Vec::new();
    let mut start = from.max(oldest);
    while start < to {
        let end = (start + WINDOW).min(to);
        out.extend(client.usage_logs(start, end)?);
        start = end;
    }
    Ok(out)
}

/// Entries of one run, recognised by the `client_reference_id` sonisub sent.
pub fn of_run<'a>(logs: &'a [Value], reference: &str) -> Vec<&'a Value> {
    logs.iter().filter(|e| e["client_reference_id"].as_str() == Some(reference)).collect()
}

pub fn fmt_usd(usd: f64) -> String {
    if usd >= 1.0 {
        format!("${usd:.2}")
    } else {
        format!("${usd:.4}")
    }
}

pub fn fmt_minutes(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// Human report for `sonisub usage`.
pub fn report(summary: &Summary, today: &Summary, days: u64) -> String {
    let mut out = format!("Soniox spending, last {days} days\n");
    if summary.models.is_empty() {
        out.push_str("  nothing\n");
    }
    for m in &summary.models {
        let audio = if m.audio_ms > 0 { format!("{} audio", fmt_minutes(m.audio_ms)) } else { String::new() };
        out.push_str(&format!("  {:<18} {:>5} req  {:>12}  {:>9}\n", m.model, m.requests, audio, fmt_usd(m.cost_usd)));
    }
    out.push_str(&format!("  {:<18} {:>5}      {:>12}  {:>9}\n", "total", "", "", fmt_usd(summary.cost_usd)));
    out.push_str(&format!("  today (UTC): {}\n", fmt_usd(today.cost_usd)));
    match summary.stt_usd_per_hour() {
        Some(p) => out.push_str(&format!("  transcription: {} per hour of audio ({} per minute)\n", fmt_usd(p), fmt_usd(p / 60.0))),
        None => out.push_str(&format!("  transcription: no data yet (about {} per hour)\n", fmt_usd(FALLBACK_USD_PER_HOUR))),
    }
    out.push_str("  remaining balance is not available through the API: https://console.soniox.com/org/billing/overview\n");
    out
}

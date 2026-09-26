//! Minimal blocking client for the Soniox async transcription REST API.

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use indicatif::ProgressBar;
use reqwest::blocking::{Client as Http, RequestBuilder, Response, multipart};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cancel::{CancelToken, Cancelled};

/// The Soniox REST API.
pub const DEFAULT_BASE: &str = "https://api.soniox.com/v1";

/// How long a request other than an upload may take before it is given up (and retried): a
/// connection that silently died (Wi-Fi gone, laptop asleep) otherwise never returns.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// An error reported by Soniox, either as an HTTP error or as a failed transcription job.
#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: Option<u16>,
    pub error_type: String,
    pub message: String,
    pub request_id: Option<String>,
}

impl ApiError {
    /// Errors after which processing further files is pointless.
    pub fn is_fatal(&self) -> bool {
        matches!(self.status, Some(401..=403))
            || self.error_type.ends_with("_exhausted")
            || self.error_type == "unauthenticated"
    }

    fn hint(&self) -> Option<&'static str> {
        Some(match self.error_type.as_str() {
            "organization_balance_exhausted" => "Soniox balance is empty: top up or enable autopay at https://console.soniox.com",
            "organization_monthly_budget_exhausted" | "project_monthly_budget_exhausted" => {
                "monthly budget limit reached: raise it in https://console.soniox.com"
            }
            "unauthenticated" => "invalid API key: check SONIOX_API_KEY or --api-key",
            "permission_denied" => "the API key has no access to this operation",
            "max_duration_reached" | "max_audio_duration_reached" => "audio is longer than Soniox allows: split the file",
            "limit_exceeded" => "Soniox rate/usage limit hit: try again later",
            "invalid_audio_file" => "Soniox could not read the extracted audio",
            _ => return None,
        })
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Soniox error")?;
        if let Some(s) = self.status {
            write!(f, " {s}")?;
        }
        write!(f, " [{}]: {}", self.error_type, self.message)?;
        if let Some(h) = self.hint() {
            write!(f, " — {h}")?;
        }
        if let Some(r) = &self.request_id {
            write!(f, " (request_id {r})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

pub struct Client {
    http: Http,
    base: String,
    key: String,
    cancel: CancelToken,
    timeout: Duration,
    /// Remote files and transcriptions that could not be deleted (see [`RemoteGuard`]).
    failed_deletes: Arc<AtomicUsize>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

pub struct Status {
    pub status: String,
}

impl Client {
    pub fn new(base: &str, key: &str) -> Result<Self> {
        let http = Http::builder()
            .user_agent(concat!("sonisub/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            .timeout(None::<Duration>)
            .build()?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            key: key.to_string(),
            cancel: CancelToken::new(),
            timeout: REQUEST_TIMEOUT,
            failed_deletes: Arc::default(),
        })
    }

    /// Stops uploads and retries soon after `cancel` is set (the process-wide flag always does).
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Time limit for each request except uploads; [`REQUEST_TIMEOUT`] by default.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// How many remote files and transcriptions this client failed to delete after use.
    /// Each one is also logged as a warning with its id; `sonisub purge` removes them.
    pub fn failed_deletes(&self) -> usize {
        self.failed_deletes.load(Ordering::Relaxed)
    }

    /// A request limited to the client's timeout.
    fn req(&self, method: reqwest::Method, path: &str) -> RequestBuilder {
        self.req_untimed(method, path).timeout(self.timeout)
    }

    /// A request without a total time limit: an upload of a long file takes as long as it takes.
    fn req_untimed(&self, method: reqwest::Method, path: &str) -> RequestBuilder {
        self.http.request(method, format!("{}{}", self.base, path)).bearer_auth(&self.key)
    }

    /// Sends a request, retrying transient failures (network, timeout, 429, 5xx).
    fn send(&self, make: impl Fn() -> Result<RequestBuilder>) -> Result<Response> {
        let mut delay = Duration::from_secs(2);
        for attempt in 1.. {
            let res = make()?.send();
            // Cancelled: no retries, but the request itself is still made, so cleanup after a
            // cancel still deletes what was created.
            let cancelled = self.cancel.is_cancelled();
            let retry = !cancelled
                && match &res {
                    Ok(r) => r.status().as_u16() == 429 || r.status().is_server_error(),
                    Err(e) => e.is_connect() || e.is_timeout() || e.is_request(),
                };
            if !retry || attempt >= 4 {
                let r = res.context("request to Soniox failed")?;
                return check(r);
            }
            if !self.cancel.sleep(delay) {
                return Err(Cancelled.into());
            }
            delay *= 2;
        }
        unreachable!()
    }

    /// Cheap authenticated call to fail fast on a bad key before extracting and uploading anything.
    pub fn check_auth(&self) -> Result<()> {
        self.send(|| Ok(self.req(reqwest::Method::GET, "/files").query(&[("limit", "1")]))).map(drop)
    }

    pub fn upload(&self, path: &Path, pb: &ProgressBar) -> Result<String> {
        let len = std::fs::metadata(path)?.len();
        pb.set_length(len);
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("audio.flac").to_string();
        let r = self.send(|| {
            pb.set_position(0);
            let reader = Progress { inner: File::open(path)?, pb: pb.clone(), cancel: self.cancel.clone() };
            let part = multipart::Part::reader_with_length(reader, len).file_name(name.clone());
            Ok(self.req_untimed(reqwest::Method::POST, "/files").multipart(multipart::Form::new().part("file", part)))
        })?;
        Ok(r.json::<Id>()?.id)
    }

    pub fn create(&self, config: &Value) -> Result<String> {
        let r = self.send(|| Ok(self.req(reqwest::Method::POST, "/transcriptions").json(config)))?;
        Ok(r.json::<Id>()?.id)
    }

    /// Current job status; a failed job is returned as an [`ApiError`].
    pub fn status(&self, id: &str) -> Result<Status> {
        let v: Value = self.send(|| Ok(self.req(reqwest::Method::GET, &format!("/transcriptions/{id}"))))?.json()?;
        let status = v["status"].as_str().unwrap_or_default().to_string();
        if status == "error" || status == "failed" {
            return Err(ApiError {
                status: None,
                error_type: v["error_type"].as_str().unwrap_or("transcription_failed").to_string(),
                message: v["error_message"].as_str().unwrap_or("transcription failed").to_string(),
                request_id: None,
            }
            .into());
        }
        Ok(Status { status })
    }

    pub fn transcript(&self, id: &str) -> Result<Value> {
        Ok(self.send(|| Ok(self.req(reqwest::Method::GET, &format!("/transcriptions/{id}/transcript"))))?.json()?)
    }

    pub fn delete_file(&self, id: &str) -> Result<()> {
        self.send(|| Ok(self.req(reqwest::Method::DELETE, &format!("/files/{id}")))).map(drop)
    }

    pub fn delete_transcription(&self, id: &str) -> Result<()> {
        self.send(|| Ok(self.req(reqwest::Method::DELETE, &format!("/transcriptions/{id}")))).map(drop)
    }

    /// Usage log entries (one per request) between two moments, at most 31 days apart.
    pub fn usage_logs(&self, from: std::time::SystemTime, to: std::time::SystemTime) -> Result<Vec<Value>> {
        let (from, to) = (humantime::format_rfc3339_seconds(from).to_string(), humantime::format_rfc3339_seconds(to).to_string());
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let v: Value = self
                .send(|| {
                    let mut r = self
                        .req(reqwest::Method::GET, "/usage-logs")
                        .query(&[("start_time", from.as_str()), ("end_time", to.as_str()), ("limit", "1000")]);
                    if let Some(c) = &cursor {
                        r = r.query(&[("cursor", c)]);
                    }
                    Ok(r)
                })?
                .json()?;
            out.extend(v["usage_logs"].as_array().into_iter().flatten().cloned());
            match v["next_page_cursor"].as_str() {
                Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
                _ => return Ok(out),
            }
        }
    }

    /// All ids of a paginated collection ("files" or "transcriptions"), with a short description each.
    pub fn list(&self, what: &str) -> Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let v: Value = self
                .send(|| {
                    let mut r = self.req(reqwest::Method::GET, &format!("/{what}")).query(&[("limit", "1000")]);
                    if let Some(c) = &cursor {
                        r = r.query(&[("cursor", c)]);
                    }
                    Ok(r)
                })?
                .json()?;
            for item in v[what].as_array().into_iter().flatten() {
                let id = item["id"].as_str().unwrap_or_default().to_string();
                let desc = [&item["filename"], &item["status"], &item["created_at"]]
                    .iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join("  ");
                out.push((id, desc));
            }
            match v["next_page_cursor"].as_str() {
                Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
                _ => return Ok(out),
            }
        }
    }
}

pub fn transcription_config(
    model: &str,
    file_id: &str,
    langs: &[String],
    strict: bool,
    diarization: bool,
    context: Option<&str>,
    reference: &str,
) -> Value {
    let mut cfg = json!({
        "model": model,
        "file_id": file_id,
        "enable_speaker_diarization": diarization,
        "enable_language_identification": langs.len() != 1,
        "client_reference_id": reference,
    });
    let langs: Vec<&String> = langs.iter().filter(|l| !l.is_empty()).collect();
    if !langs.is_empty() {
        cfg["language_hints"] = json!(langs);
        cfg["language_hints_strict"] = json!(strict);
    }
    if let Some(c) = context {
        cfg["context"] = json!(c);
    }
    cfg
}

fn check(r: Response) -> Result<Response> {
    if r.status().is_success() {
        return Ok(r);
    }
    let code = r.status().as_u16();
    let text = r.text().unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let mut message = v["message"].as_str().or(v["error_message"].as_str()).map(str::to_string).unwrap_or_else(|| text.trim().chars().take(300).collect());
    if let Some(errs) = v["validation_errors"].as_array().filter(|e| !e.is_empty()) {
        let details: Vec<String> = errs
            .iter()
            .map(|e| format!("{}: {}", e["location"].as_str().unwrap_or("?"), e["message"].as_str().unwrap_or("?")))
            .collect();
        message = format!("{message} ({})", details.join("; "));
    }
    Err(ApiError {
        status: Some(code),
        error_type: v["error_type"].as_str().unwrap_or("http_error").to_string(),
        message: if message.is_empty() { format!("HTTP {code}") } else { message },
        request_id: v["request_id"].as_str().map(str::to_string),
    }
    .into())
}

/// Reader that drives an upload progress bar and aborts on cancel.
struct Progress {
    inner: File,
    pb: ProgressBar,
    cancel: CancelToken,
}

impl Read for Progress {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "interrupted"));
        }
        let n = self.inner.read(buf)?;
        self.pb.inc(n as u64);
        Ok(n)
    }
}

/// Deletes the remote file and transcription when dropped, whatever happened.
/// A failed delete is logged as a warning (`log` crate) and counted in [`Client::failed_deletes`].
pub struct RemoteGuard<'a> {
    pub client: &'a Client,
    pub file_id: Option<String>,
    pub transcription_id: Option<String>,
}

impl RemoteGuard<'_> {
    pub fn cleanup(&mut self) {
        // After a cancel each delete is still tried, once (see `Client::send`).
        let client = self.client;
        if let Some(id) = self.transcription_id.take()
            && let Err(e) = client.delete_transcription(&id)
        {
            self.client.failed_deletes.fetch_add(1, Ordering::Relaxed);
            log::warn!("could not delete Soniox transcription {id}: {e:#} (run `sonisub purge`)");
        }
        if let Some(id) = self.file_id.take()
            && let Err(e) = client.delete_file(&id)
        {
            self.client.failed_deletes.fetch_add(1, Ordering::Relaxed);
            log::warn!("could not delete Soniox file {id}: {e:#} (run `sonisub purge`)");
        }
    }
}

impl Drop for RemoteGuard<'_> {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub fn api_error(e: &anyhow::Error) -> Option<&ApiError> {
    e.chain().find_map(|c| c.downcast_ref::<ApiError>())
}

pub fn missing_key() -> anyhow::Error {
    anyhow!("no Soniox API key: set SONIOX_API_KEY or pass --api-key")
}

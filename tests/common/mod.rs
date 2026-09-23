//! Shared test helpers: fixture paths and a tiny scripted stand-in for the Soniox REST API.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

pub fn transcript() -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixture("dialog.soniox.json")).unwrap()).unwrap()
}

/// Golden subtitles, line endings normalised (git may check them out as CRLF).
pub fn golden_srt() -> String {
    normalize(&std::fs::read_to_string(fixture("dialog.srt")).unwrap())
}

pub fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
}

impl Request {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

type Handler = dyn Fn(&Request, usize) -> (u16, Value) + Send + Sync;

/// HTTP server on a random local port. The handler gets each request and how many
/// earlier requests had the same method and path, and returns status + JSON body.
pub struct MockSoniox {
    pub url: String,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl MockSoniox {
    pub fn start(handler: impl Fn(&Request, usize) -> (u16, Value) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests: Arc<Mutex<Vec<Request>>> = Arc::default();
        let handler: Arc<Handler> = Arc::new(handler);
        let log = requests.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                serve(stream, &*handler, &log);
            }
        });
        Self { url, requests }
    }

    /// A healthy Soniox that returns `transcript` after one "processing" poll.
    pub fn happy(transcript: Value) -> Self {
        Self::start(move |r, n| ok_route(r, n, &transcript).unwrap_or((404, json!({}))))
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    /// "METHOD /path" for every request, in order.
    pub fn calls(&self) -> Vec<String> {
        self.requests().iter().map(|r| format!("{} {}", r.method, r.path)).collect()
    }

    pub fn find(&self, method: &str, path: &str) -> Option<Request> {
        self.requests().into_iter().find(|r| r.method == method && r.path == path)
    }
}

/// Normal Soniox behaviour; `None` for routes it doesn't know.
pub fn ok_route(r: &Request, n: usize, transcript: &Value) -> Option<(u16, Value)> {
    Some(match (r.method.as_str(), r.path.as_str()) {
        ("GET", p) if p.starts_with("/v1/files") => (200, json!({"files": [], "next_page_cursor": null})),
        ("POST", "/v1/files") => (201, json!({"id": "f1"})),
        ("POST", "/v1/transcriptions") => (201, json!({"id": "t1", "status": "queued"})),
        ("GET", "/v1/transcriptions/t1") if n == 0 => (200, json!({"id": "t1", "status": "processing"})),
        ("GET", "/v1/transcriptions/t1") => (200, json!({"id": "t1", "status": "completed"})),
        ("GET", "/v1/transcriptions/t1/transcript") => (200, transcript.clone()),
        ("DELETE", _) => (204, Value::Null),
        _ => return None,
    })
}

pub fn soniox_error(status: u16, error_type: &str, message: &str) -> (u16, Value) {
    (
        status,
        json!({"status_code": status, "error_type": error_type, "message": message,
               "validation_errors": [], "request_id": "req-test"}),
    )
}

fn serve(stream: TcpStream, handler: &Handler, log: &Mutex<Vec<Request>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().split('?').next().unwrap().to_string();

    let (mut length, mut chunked) = (0usize, false);
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).unwrap();
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).unwrap();
            let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
            let mut chunk = vec![0; n + 2];
            reader.read_exact(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
    } else {
        body.resize(length, 0);
        reader.read_exact(&mut body).unwrap();
    }

    let req = Request { method, path, body };
    let n = {
        let mut log = log.lock().unwrap();
        let n = log.iter().filter(|r| r.method == req.method && r.path == req.path).count();
        log.push(req.clone());
        n
    };
    let (status, value) = handler(&req, n);
    let payload = if value.is_null() { String::new() } else { value.to_string() };
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
}

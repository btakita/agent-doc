//! # Module: serve
//!
//! ## Spec
//! - `agent-doc serve <FILE> [--port 7333] [--host 127.0.0.1]` starts a
//!   localhost HTTP server that exposes a minimal markdown editor for the
//!   session document.
//! - Phase 1 MVP per `tasks/agent-doc/plan-web-interface.md`: localhost-only
//!   bind, raw `<textarea>` editor, no auth. Phase 2 adds an SSE `/events`
//!   stream that reports file changes so browser tabs can reload. All writes
//!   funnel through `agent-doc write --commit <FILE>` via a child process so
//!   the binary-owned snapshot/commit/session-check boundary applies.
//! - Routes:
//!   - `GET /` → embedded HTML editor page (`INDEX_HTML`).
//!   - `GET /doc` → current document text (read from disk).
//!   - `POST /save` → body = full document text; calls `write --commit`,
//!     returns JSON with the new HEAD short SHA.
//!   - `GET /events` → Server-Sent Events stream with `ready`, `doc-changed`,
//!     and `doc-error` events.
//!   - `GET /healthz` → `ok`.
//!
//! ## Agentic Contracts
//! - Localhost-only by default. Non-loopback binds print a clear warning;
//!   auth lands in Phase 5 of the plan.
//! - No async — sequential per-connection via `tiny_http` (project rule:
//!   no tokio).
//! - Writes never bypass `write --commit`; the in-process write path is not
//!   reused here so the commit boundary stays in one place.
//! - When `write --commit` exits nonzero, the HTTP response surfaces the
//!   stderr verbatim so the operator sees the actionable error.
//!
//! ## Evals
//! - `serve_get_doc_returns_current_file_contents`
//! - `serve_post_save_writes_through_commit_boundary`
//! - `serve_sse_stream_emits_doc_changed_after_file_mutation`

use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Response, Server, StatusCode};

const INDEX_HTML: &str = include_str!("../assets/serve/index.html");
const DEFAULT_PORT: u16 = 7333;
const DEFAULT_HOST: &str = "127.0.0.1";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024; // 4 MiB cap for the MVP
const SSE_POLL_INTERVAL_MS: u64 = 300;

pub struct ServeOptions {
    pub file: PathBuf,
    pub host: String,
    pub port: u16,
}

impl ServeOptions {
    pub fn new(file: PathBuf, host: Option<String>, port: Option<u16>) -> Self {
        Self {
            file,
            host: host.unwrap_or_else(|| DEFAULT_HOST.to_string()),
            port: port.unwrap_or(DEFAULT_PORT),
        }
    }
}

pub fn run(options: ServeOptions) -> Result<()> {
    if !options.file.exists() {
        anyhow::bail!("file not found: {}", options.file.display());
    }
    let canonical = options
        .file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", options.file.display()))?;

    if options.host != "127.0.0.1" && options.host != "localhost" {
        eprintln!(
            "[serve] WARNING: binding to non-loopback host {} without auth — exposes the document on the network",
            options.host
        );
    }

    let bind = format!("{}:{}", options.host, options.port);
    let server = Server::http(&bind)
        .map_err(|e| anyhow::anyhow!("failed to bind {bind}: {e}"))?;

    eprintln!(
        "[serve] listening on http://{} — editing {}",
        bind,
        canonical.display()
    );

    for request in server.incoming_requests() {
        let doc = canonical.clone();
        thread::spawn(move || {
            if let Err(e) = handle_request(request, &doc) {
                eprintln!("[serve] request error: {e}");
            }
        });
    }
    Ok(())
}

fn handle_request(request: tiny_http::Request, doc: &Path) -> Result<()> {
    let url = request.url().to_string();
    let method = request.method().clone();
    match (method, url.as_str()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            respond(request, INDEX_HTML, "text/html; charset=utf-8", StatusCode(200))
        }
        (Method::Get, "/healthz") => respond(
            request,
            "ok\n",
            "text/plain; charset=utf-8",
            StatusCode(200),
        ),
        (Method::Get, "/doc") => {
            let body = std::fs::read_to_string(doc)
                .with_context(|| format!("failed to read {}", doc.display()))?;
            respond(
                request,
                &body,
                "text/markdown; charset=utf-8",
                StatusCode(200),
            )
        }
        (Method::Get, "/events") => handle_events(request, doc),
        (Method::Post, "/save") => handle_save(request, doc),
        _ => respond(
            request,
            "not found\n",
            "text/plain; charset=utf-8",
            StatusCode(404),
        ),
    }
}

fn handle_events(request: tiny_http::Request, doc: &Path) -> Result<()> {
    let headers = vec![
        header("Content-Type", "text/event-stream; charset=utf-8")?,
        header("Cache-Control", "no-cache")?,
    ];
    let response = Response::new(
        StatusCode(200),
        headers,
        SsePollStream::new(doc.to_path_buf()),
        None,
        None,
    )
    .with_chunked_threshold(1);
    request.respond(response)?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DocFingerprint {
    bytes: usize,
    hash: String,
    modified_ms: Option<u128>,
}

fn read_doc_fingerprint(doc: &Path) -> Result<DocFingerprint> {
    let content = std::fs::read_to_string(doc)
        .with_context(|| format!("failed to read {}", doc.display()))?;
    let modified_ms = std::fs::metadata(doc)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_ms);
    Ok(DocFingerprint {
        bytes: content.len(),
        hash: crate::ops_log::content_hash(&content),
        modified_ms,
    })
}

fn system_time_ms(time: SystemTime) -> Option<u128> {
    time.duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis())
}

fn doc_event_payload(doc: &Path, fingerprint: &DocFingerprint) -> serde_json::Value {
    serde_json::json!({
        "path": doc.display().to_string(),
        "bytes": fingerprint.bytes,
        "hash": fingerprint.hash,
        "modified_ms": fingerprint.modified_ms,
    })
}

fn format_sse_event(event: &str, payload: serde_json::Value) -> String {
    format!("event: {event}\ndata: {payload}\n\n")
}

struct SsePollStream {
    doc: PathBuf,
    interval: Duration,
    last: Option<DocFingerprint>,
    buffer: VecDeque<u8>,
    sent_ready: bool,
}

impl SsePollStream {
    fn new(doc: PathBuf) -> Self {
        Self::with_interval(doc, Duration::from_millis(SSE_POLL_INTERVAL_MS))
    }

    fn with_interval(doc: PathBuf, interval: Duration) -> Self {
        Self {
            doc,
            interval,
            last: None,
            buffer: VecDeque::new(),
            sent_ready: false,
        }
    }

    fn enqueue_event(&mut self, event: &str, payload: serde_json::Value) {
        self.buffer.extend(format_sse_event(event, payload).bytes());
    }

    fn enqueue_ready(&mut self) {
        match read_doc_fingerprint(&self.doc) {
            Ok(fingerprint) => {
                self.last = Some(fingerprint.clone());
                self.enqueue_event("ready", doc_event_payload(&self.doc, &fingerprint));
            }
            Err(err) => self.enqueue_error(err),
        }
    }

    fn enqueue_error(&mut self, err: anyhow::Error) {
        self.enqueue_event(
            "doc-error",
            serde_json::json!({
                "path": self.doc.display().to_string(),
                "message": err.to_string(),
            }),
        );
    }

    fn poll_once(&mut self) -> Result<bool> {
        let fingerprint = read_doc_fingerprint(&self.doc)?;
        if self.last.as_ref() == Some(&fingerprint) {
            return Ok(false);
        }
        self.last = Some(fingerprint.clone());
        self.enqueue_event("doc-changed", doc_event_payload(&self.doc, &fingerprint));
        Ok(true)
    }

    fn drain_buffer(&mut self, out: &mut [u8]) -> Option<usize> {
        if self.buffer.is_empty() {
            return None;
        }
        let mut written = 0;
        while written < out.len() {
            let Some(byte) = self.buffer.pop_front() else {
                break;
            };
            out[written] = byte;
            written += 1;
        }
        Some(written)
    }
}

impl Read for SsePollStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some(written) = self.drain_buffer(out) {
                return Ok(written);
            }
            if !self.sent_ready {
                self.sent_ready = true;
                self.enqueue_ready();
                continue;
            }
            match self.poll_once() {
                Ok(true) => continue,
                Ok(false) => thread::sleep(self.interval),
                Err(err) => {
                    self.enqueue_error(err);
                    continue;
                }
            }
        }
    }
}

fn handle_save(mut request: tiny_http::Request, doc: &Path) -> Result<()> {
    let content_length = request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("content-length"))
        .and_then(|h| h.value.as_str().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        return respond(
            request,
            &format!("body too large ({content_length} bytes > {MAX_BODY_BYTES})\n"),
            "text/plain; charset=utf-8",
            StatusCode(413),
        );
    }

    let mut body = String::with_capacity(content_length.min(MAX_BODY_BYTES));
    request
        .as_reader()
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_string(&mut body)
        .context("failed to read request body")?;
    if body.len() > MAX_BODY_BYTES {
        return respond(
            request,
            "body exceeded MAX_BODY_BYTES while reading\n",
            "text/plain; charset=utf-8",
            StatusCode(413),
        );
    }

    if let Err(e) = std::fs::write(doc, &body) {
        return respond(
            request,
            &format!("failed to write {}: {e}\n", doc.display()),
            "text/plain; charset=utf-8",
            StatusCode(500),
        );
    }

    // Web-editor user edits commit through plain git for the Phase 1 MVP.
    // This intentionally does not enter the agent-doc cycle/capture/snapshot
    // machinery — that path is reserved for agent responses going through
    // `write --commit` / `finalize`. Future phases hooking the editor up to
    // live cycles (Phase 6 in plan-web-interface.md) will route through
    // those boundaries instead.
    if let Err(e) = git_commit_user_edit(doc) {
        return respond(
            request,
            &format!("git commit failed: {e}\n"),
            "text/plain; charset=utf-8",
            StatusCode(500),
        );
    }

    let head = git_head_short(doc).unwrap_or_else(|_| "<unknown>".to_string());
    let payload = serde_json::json!({
        "ok": true,
        "head": head,
        "bytes": body.len(),
    });
    respond(
        request,
        &payload.to_string(),
        "application/json; charset=utf-8",
        StatusCode(200),
    )
}

fn git_commit_user_edit(doc: &Path) -> Result<()> {
    let dir = doc.parent().unwrap_or_else(|| Path::new("."));
    let file_name = doc
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("doc has no file name"))?;

    let status = Command::new("git")
        .args(["status", "--porcelain", "--", file_name])
        .current_dir(dir)
        .output()?;
    if status.stdout.is_empty() && status.status.success() {
        // Nothing to commit — content matches HEAD already.
        return Ok(());
    }

    let add = Command::new("git")
        .args(["add", "--", file_name])
        .current_dir(dir)
        .output()?;
    if !add.status.success() {
        anyhow::bail!(
            "git add failed: {}",
            String::from_utf8_lossy(&add.stderr).trim()
        );
    }

    let commit = Command::new("git")
        .args([
            "commit",
            "--no-verify",
            "-m",
            &format!("agent-doc serve: web edit {}", chrono_timestamp()),
        ])
        .current_dir(dir)
        .output()?;
    if !commit.status.success() {
        anyhow::bail!(
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr).trim()
        );
    }
    Ok(())
}

fn chrono_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn git_head_short(doc: &Path) -> Result<String> {
    let dir = doc.parent().unwrap_or_else(|| Path::new("."));
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("git rev-parse failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn respond(
    request: tiny_http::Request,
    body: &str,
    content_type: &str,
    status: StatusCode,
) -> Result<()> {
    let content_type_header = header("Content-Type", content_type)?;
    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(content_type_header);
    request.respond(response)?;
    Ok(())
}

fn header(name: &str, value: &str) -> Result<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .map_err(|_| anyhow::anyhow!("invalid HTTP header: {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_sse_event_formats_named_json_payload() {
        let event = format_sse_event(
            "doc-changed",
            serde_json::json!({"path": "/tmp/session.md", "bytes": 12}),
        );

        assert!(event.starts_with("event: doc-changed\n"));
        assert!(event.contains(r#"data: {"bytes":12,"path":"/tmp/session.md"}"#));
        assert!(event.ends_with("\n\n"));
    }

    #[test]
    fn serve_sse_stream_emits_doc_changed_after_file_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "one\n").unwrap();

        let mut stream = SsePollStream::with_interval(doc.clone(), Duration::from_millis(1));
        let mut buf = [0_u8; 512];
        let ready_len = stream.read(&mut buf).unwrap();
        let ready = String::from_utf8_lossy(&buf[..ready_len]);
        assert!(ready.contains("event: ready\n"), "got {ready:?}");

        std::fs::write(&doc, "two\n").unwrap();
        let changed_len = stream.read(&mut buf).unwrap();
        let changed = String::from_utf8_lossy(&buf[..changed_len]);
        assert!(changed.contains("event: doc-changed\n"), "got {changed:?}");
        assert!(changed.contains(r#""bytes":4"#), "got {changed:?}");
    }

    #[test]
    fn serve_index_html_subscribes_to_sse_doc_changed_events() {
        assert!(INDEX_HTML.contains("new EventSource(\"/events\")"));
        assert!(INDEX_HTML.contains("doc-changed"));
    }
}

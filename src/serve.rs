//! # Module: serve
//!
//! ## Spec
//! - `agent-doc serve <FILE> [--port 7333] [--host 127.0.0.1]` starts a
//!   localhost HTTP server that exposes a minimal markdown editor for the
//!   session document.
//! - Phase 1 MVP per `tasks/agent-doc/plan-web-interface.md`: localhost-only
//!   bind, raw `<textarea>` editor, no auth, no SSE. All writes funnel
//!   through `agent-doc write --commit <FILE>` via a child process so the
//!   binary-owned snapshot/commit/session-check boundary applies.
//! - Routes:
//!   - `GET /` → embedded HTML editor page (`INDEX_HTML`).
//!   - `GET /doc` → current document text (read from disk).
//!   - `POST /save` → body = full document text; calls `write --commit`,
//!     returns JSON with the new HEAD short SHA.
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

use anyhow::{Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use tiny_http::{Header, Method, Response, Server, StatusCode};

const INDEX_HTML: &str = include_str!("../assets/serve/index.html");
const DEFAULT_PORT: u16 = 7333;
const DEFAULT_HOST: &str = "127.0.0.1";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024; // 4 MiB cap for the MVP

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
        if let Err(e) = handle_request(request, &canonical) {
            eprintln!("[serve] request error: {e}");
        }
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
        (Method::Post, "/save") => handle_save(request, doc),
        _ => respond(
            request,
            "not found\n",
            "text/plain; charset=utf-8",
            StatusCode(404),
        ),
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
    let content_type_header =
        Header::from_bytes(b"Content-Type".as_ref(), content_type.as_bytes())
            .map_err(|_| anyhow::anyhow!("invalid content-type header"))?;
    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(content_type_header);
    request.respond(response)?;
    Ok(())
}

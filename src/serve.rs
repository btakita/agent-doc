//! # Module: serve
//!
//! ## Spec
//! - `agent-doc serve [FILE_OR_DIR] [--port 7333] [--host 127.0.0.1]` starts a
//!   localhost HTTP server that exposes a minimal markdown editor for one
//!   session document or the project session list.
//! - Phase 1 MVP per `tasks/agent-doc/plan-web-interface.md`: localhost-only
//!   bind and a raw `<textarea>` editor. Later phases add SSE reloads, project
//!   document routing, bearer auth for non-loopback binds, and optional TLS.
//! - Routes:
//!   - `GET /` → embedded HTML editor page (`INDEX_HTML`).
//!   - `GET /doc` → current document text (read from disk).
//!   - `GET /doc/<path-or-hash>` → current text for a project session document.
//!   - `GET /api/auth` → current auth mode and token scope.
//!   - `GET /api/sessions` → project session list.
//!   - `POST /save` → body = full document text; calls `write --commit`,
//!     returns JSON with the new HEAD short SHA.
//!   - `GET /events` → Server-Sent Events stream with `ready`, `doc-changed`,
//!     `agent-response`, and `doc-error` events.
//!   - `GET /healthz` → `ok`.
//!
//! ## Agentic Contracts
//! - Localhost-only by default. Non-loopback binds require bearer auth; edit
//!   and read-only viewer tokens are printed on start unless supplied.
//! - Optional HTTPS uses tiny_http's rustls backend when `--tls-cert` and
//!   `--tls-key` are provided.
//! - No async — sequential per-connection via `tiny_http` (project rule:
//!   no tokio).
//! - Browser writes use plain git commits only when no agent-doc response cycle
//!   is open for the document; during an open cycle they close through
//!   `agent-doc write --commit --origin serve`.
//! - When `write --commit` exits nonzero, the HTTP response surfaces the
//!   stderr verbatim so the operator sees the actionable error.
//!
//! ## Evals
//! - `serve_get_doc_returns_current_file_contents`
//! - `serve_post_save_writes_through_commit_boundary`
//! - `serve_sse_stream_emits_doc_changed_after_file_mutation`

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Response, Server, SslConfig, StatusCode};

const INDEX_HTML: &str = include_str!("../assets/serve/index.html");
const DEFAULT_PORT: u16 = 7333;
const DEFAULT_HOST: &str = "127.0.0.1";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024; // 4 MiB cap for the MVP
const SSE_POLL_INTERVAL_MS: u64 = 300;

pub struct ServeOptions {
    pub target: Option<PathBuf>,
    pub host: String,
    pub port: u16,
    pub auth_token: Option<String>,
    pub read_only_token: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

impl ServeOptions {
    pub fn new(
        target: Option<PathBuf>,
        host: Option<String>,
        port: Option<u16>,
        auth_token: Option<String>,
        read_only_token: Option<String>,
        tls_cert: Option<PathBuf>,
        tls_key: Option<PathBuf>,
    ) -> Self {
        Self {
            target,
            host: host.unwrap_or_else(|| DEFAULT_HOST.to_string()),
            port: port.unwrap_or(DEFAULT_PORT),
            auth_token,
            read_only_token,
            tls_cert,
            tls_key,
        }
    }
}

pub fn run(options: ServeOptions) -> Result<()> {
    let auth = ServeAuth::from_options(&options.host, options.auth_token, options.read_only_token)?;
    let tls = ServeTls::from_options(options.tls_cert, options.tls_key)?;
    let scheme = if tls.is_some() { "https" } else { "http" };
    let state = Arc::new(ServeState::new(options.target, auth)?);

    let bind = format!("{}:{}", options.host, options.port);
    let server = bind_server(&bind, tls)?;

    match &state.default_doc {
        Some(doc) => eprintln!(
            "[serve] listening on {}://{} — editing {}",
            scheme,
            bind,
            doc.display()
        ),
        None => eprintln!(
            "[serve] listening on {}://{} — browsing {}",
            scheme,
            bind,
            state.root.display()
        ),
    }
    state.print_auth_urls(scheme, &bind);
    if state.auth.is_enabled() && !is_loopback_host(&options.host) && scheme == "http" {
        eprintln!(
            "[serve] WARNING: remote bearer tokens are sent over plain HTTP; use --tls-cert and --tls-key for HTTPS"
        );
    }

    for request in server.incoming_requests() {
        let state = Arc::clone(&state);
        thread::spawn(move || {
            if let Err(e) = handle_request(request, &state) {
                eprintln!("[serve] request error: {e}");
            }
        });
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct ServeState {
    root: PathBuf,
    default_doc: Option<PathBuf>,
    browse_root: bool,
    auth: ServeAuth,
}

impl ServeState {
    fn new(target: Option<PathBuf>, auth: ServeAuth) -> Result<Self> {
        let raw_target = match target {
            Some(target) => target,
            None => {
                let cwd = std::env::current_dir().context("failed to get current directory")?;
                agent_doc_orchestration::snapshot::find_project_root(&cwd).unwrap_or(cwd)
            }
        };
        if !raw_target.exists() {
            anyhow::bail!("file or directory not found: {}", raw_target.display());
        }
        let canonical = raw_target
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", raw_target.display()))?;
        if canonical.is_file() {
            let root = agent_doc_orchestration::snapshot::find_project_root(&canonical)
                .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
            Ok(Self {
                root,
                default_doc: Some(canonical),
                browse_root: false,
                auth,
            })
        } else {
            let root = agent_doc_orchestration::snapshot::find_project_root(&canonical)
                .unwrap_or(canonical);
            Ok(Self {
                root,
                default_doc: None,
                browse_root: true,
                auth,
            })
        }
    }

    fn resolve_doc(&self, token: Option<&str>) -> Result<PathBuf> {
        let sessions = list_sessions(self)?;
        if let Some(token) = token.filter(|token| !token.is_empty()) {
            let decoded = percent_decode(token.trim_start_matches('/'))?;
            if let Some(session) = sessions.iter().find(|session| {
                session.id == decoded || session.path == decoded || session.absolute == decoded
            }) {
                if !session.exists {
                    anyhow::bail!(
                        "session document is registered but missing: {}",
                        session.path
                    );
                }
                return Ok(PathBuf::from(&session.absolute));
            }
            return self.resolve_relative_doc(&decoded);
        }
        if let Some(doc) = &self.default_doc {
            return Ok(doc.clone());
        }
        sessions
            .iter()
            .find(|session| session.exists)
            .map(|session| PathBuf::from(&session.absolute))
            .ok_or_else(|| {
                anyhow::anyhow!("no session documents found under {}", self.root.display())
            })
    }

    fn resolve_relative_doc(&self, decoded: &str) -> Result<PathBuf> {
        let relative = Path::new(decoded);
        if decoded.is_empty()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            anyhow::bail!("invalid document path: {decoded}");
        }
        let candidate = self.root.join(relative);
        let canonical = candidate
            .canonicalize()
            .with_context(|| format!("session document not found: {decoded}"))?;
        if !canonical.starts_with(&self.root) {
            anyhow::bail!("document path escapes served root: {decoded}");
        }
        if !is_agent_doc_file(&canonical)? {
            anyhow::bail!("not an agent-doc session document: {decoded}");
        }
        Ok(canonical)
    }

    fn print_auth_urls(&self, scheme: &str, bind: &str) {
        if let ServeAuth::Enabled {
            edit_token,
            read_only_token,
        } = &self.auth
        {
            eprintln!("[serve] auth required");
            eprintln!("[serve] edit token: {edit_token}");
            eprintln!("[serve] edit URL: {scheme}://{bind}/?token={edit_token}");
            if let Some(token) = read_only_token {
                eprintln!("[serve] read-only token: {token}");
                eprintln!("[serve] read-only URL: {scheme}://{bind}/?token={token}");
            }
        }
    }
}

#[derive(Clone, Debug)]
enum ServeAuth {
    Disabled,
    Enabled {
        edit_token: String,
        read_only_token: Option<String>,
    },
}

impl ServeAuth {
    fn from_options(
        host: &str,
        auth_token: Option<String>,
        read_only_token: Option<String>,
    ) -> Result<Self> {
        let remote = !is_loopback_host(host);
        if !remote && auth_token.is_none() && read_only_token.is_none() {
            return Ok(Self::Disabled);
        }

        let edit_token = match auth_token {
            Some(token) => validate_token(token, "--auth-token")?,
            None => generate_bearer_token("edit"),
        };
        let read_only_token = match read_only_token {
            Some(token) => Some(validate_token(token, "--read-only-token")?),
            None if remote => Some(generate_bearer_token("read")),
            None => None,
        };
        if read_only_token.as_deref() == Some(edit_token.as_str()) {
            anyhow::bail!("--auth-token and --read-only-token must be distinct");
        }
        Ok(Self::Enabled {
            edit_token,
            read_only_token,
        })
    }

    fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled { .. })
    }

    fn scope_for_token(&self, token: Option<&str>) -> std::result::Result<AuthScope, AuthFailure> {
        match self {
            Self::Disabled => Ok(AuthScope::Edit),
            Self::Enabled {
                edit_token,
                read_only_token,
            } => {
                let token = token.ok_or(AuthFailure::Missing)?;
                if token == edit_token {
                    Ok(AuthScope::Edit)
                } else if read_only_token.as_deref() == Some(token) {
                    Ok(AuthScope::ReadOnly)
                } else {
                    Err(AuthFailure::Invalid)
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthScope {
    Edit,
    ReadOnly,
}

impl AuthScope {
    fn can_write(self) -> bool {
        matches!(self, Self::Edit)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::ReadOnly => "read-only",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthFailure {
    Missing,
    Invalid,
}

#[derive(Debug)]
struct ServeTls {
    cert: PathBuf,
    key: PathBuf,
}

impl ServeTls {
    fn from_options(tls_cert: Option<PathBuf>, tls_key: Option<PathBuf>) -> Result<Option<Self>> {
        match (tls_cert, tls_key) {
            (Some(cert), Some(key)) => Ok(Some(Self { cert, key })),
            (None, None) => Ok(None),
            (Some(_), None) => anyhow::bail!("--tls-cert requires --tls-key"),
            (None, Some(_)) => anyhow::bail!("--tls-key requires --tls-cert"),
        }
    }
}

fn bind_server(bind: &str, tls: Option<ServeTls>) -> Result<Server> {
    if let Some(tls) = tls {
        let certificate = std::fs::read(&tls.cert)
            .with_context(|| format!("failed to read TLS certificate {}", tls.cert.display()))?;
        let private_key = std::fs::read(&tls.key)
            .with_context(|| format!("failed to read TLS private key {}", tls.key.display()))?;
        return Server::https(
            bind,
            SslConfig {
                certificate,
                private_key,
            },
        )
        .map_err(|e| anyhow::anyhow!("failed to bind HTTPS {bind}: {e}"));
    }
    Server::http(bind).map_err(|e| anyhow::anyhow!("failed to bind {bind}: {e}"))
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

fn validate_token(token: String, name: &str) -> Result<String> {
    if token.trim().is_empty() {
        anyhow::bail!("{name} cannot be empty");
    }
    Ok(token)
}

fn generate_bearer_token(scope: &str) -> String {
    format!("adoc_{scope}_{}", uuid::Uuid::new_v4().simple())
}

fn handle_request(request: tiny_http::Request, state: &ServeState) -> Result<()> {
    let url = request.url().to_string();
    let route = route_path(&url);
    let method = request.method().clone();
    if method == Method::Get && route == "/healthz" {
        return respond(
            request,
            "ok\n",
            "text/plain; charset=utf-8",
            StatusCode(200),
        );
    }
    let scope = match authorize_request(&request, state, &url) {
        Ok(scope) => scope,
        Err(failure) => {
            return respond_auth_failure(request, failure);
        }
    };
    if route_requires_write(&method, route) && !scope.can_write() {
        return respond(
            request,
            "read-only bearer token cannot save\n",
            "text/plain; charset=utf-8",
            StatusCode(403),
        );
    }
    match (method, route) {
        (Method::Get, "/") | (Method::Get, "/index.html") => respond(
            request,
            INDEX_HTML,
            "text/html; charset=utf-8",
            StatusCode(200),
        ),
        (Method::Get, "/api/auth") => handle_auth(request, state, scope),
        (Method::Get, "/api/sessions") => handle_sessions(request, state),
        (Method::Get, "/doc") => handle_doc(request, state, None),
        (Method::Get, route) if route.starts_with("/doc/") => {
            handle_doc(request, state, Some(&route["/doc/".len()..]))
        }
        (Method::Get, "/api/projection") => handle_projection(request, state, None),
        (Method::Get, route) if route.starts_with("/api/projection/") => {
            handle_projection(request, state, Some(&route["/api/projection/".len()..]))
        }
        (Method::Get, "/events") => handle_events(request, state, None),
        (Method::Get, route) if route.starts_with("/events/") => {
            handle_events(request, state, Some(&route["/events/".len()..]))
        }
        (Method::Post, "/save") => handle_save(request, state, None),
        (Method::Post, route) if route.starts_with("/save/") => {
            handle_save(request, state, Some(&route["/save/".len()..]))
        }
        _ => respond(
            request,
            "not found\n",
            "text/plain; charset=utf-8",
            StatusCode(404),
        ),
    }
}

fn authorize_request(
    request: &tiny_http::Request,
    state: &ServeState,
    url: &str,
) -> std::result::Result<AuthScope, AuthFailure> {
    let token = bearer_token(request).or_else(|| query_param(url, "token"));
    state.auth.scope_for_token(token.as_deref())
}

fn route_requires_write(method: &Method, route: &str) -> bool {
    *method == Method::Post && (route == "/save" || route.starts_with("/save/"))
}

fn bearer_token(request: &tiny_http::Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| {
            header
                .field
                .as_str()
                .as_str()
                .eq_ignore_ascii_case("authorization")
        })
        .and_then(|header| {
            let value = header.value.as_str().trim();
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn handle_sessions(request: tiny_http::Request, state: &ServeState) -> Result<()> {
    let payload = build_sessions_payload(state)?;
    respond(
        request,
        &serde_json::to_string(&payload)?,
        "application/json; charset=utf-8",
        StatusCode(200),
    )
}

fn handle_auth(request: tiny_http::Request, state: &ServeState, scope: AuthScope) -> Result<()> {
    let payload = serde_json::json!({
        "authRequired": state.auth.is_enabled(),
        "scope": scope.as_str(),
        "canWrite": scope.can_write(),
    });
    respond(
        request,
        &payload.to_string(),
        "application/json; charset=utf-8",
        StatusCode(200),
    )
}

fn handle_doc(request: tiny_http::Request, state: &ServeState, token: Option<&str>) -> Result<()> {
    let doc = match state.resolve_doc(token) {
        Ok(doc) => doc,
        Err(e) => {
            return respond(
                request,
                &format!("{e}\n"),
                "text/plain; charset=utf-8",
                StatusCode(404),
            );
        }
    };
    let body = std::fs::read_to_string(&doc)
        .with_context(|| format!("failed to read {}", doc.display()))?;
    respond(
        request,
        &body,
        "text/markdown; charset=utf-8",
        StatusCode(200),
    )
}

fn handle_projection(
    request: tiny_http::Request,
    state: &ServeState,
    token: Option<&str>,
) -> Result<()> {
    let doc = match state.resolve_doc(token) {
        Ok(doc) => doc,
        Err(e) => {
            return respond(
                request,
                &format!("{e}\n"),
                "text/plain; charset=utf-8",
                StatusCode(404),
            );
        }
    };
    let body = std::fs::read_to_string(&doc)
        .with_context(|| format!("failed to read {}", doc.display()))?;
    let projection = build_projection(&body)?;
    respond(
        request,
        &serde_json::to_string(&projection)?,
        "application/json; charset=utf-8",
        StatusCode(200),
    )
}

#[derive(Debug, Serialize)]
struct ServeProjection {
    components: Vec<ServeProjectionComponent>,
}

#[derive(Debug, Serialize)]
struct ServeProjectionComponent {
    name: &'static str,
    present: bool,
    lines: usize,
    items: usize,
    content: String,
}

#[derive(Debug, Serialize)]
struct ServeSessions {
    root: String,
    selected: Option<String>,
    sessions: Vec<ServeSession>,
}

#[derive(Clone, Debug, Serialize)]
struct ServeSession {
    id: String,
    path: String,
    absolute: String,
    title: String,
    session_id: Option<String>,
    registered: bool,
    exists: bool,
}

fn build_sessions_payload(state: &ServeState) -> Result<ServeSessions> {
    let sessions = list_sessions(state)?;
    let selected = state
        .default_doc
        .as_ref()
        .and_then(|doc| relative_path(&state.root, doc).ok())
        .or_else(|| {
            sessions
                .iter()
                .find(|session| session.exists)
                .map(|session| session.path.clone())
        });
    Ok(ServeSessions {
        root: state.root.display().to_string(),
        selected,
        sessions,
    })
}

fn list_sessions(state: &ServeState) -> Result<Vec<ServeSession>> {
    let mut by_path: BTreeMap<PathBuf, ServeSession> = BTreeMap::new();

    if let Some(doc) = &state.default_doc {
        upsert_session(&mut by_path, &state.root, doc.clone(), None, false)?;
    }
    if !state.browse_root {
        return Ok(by_path.into_values().collect());
    }

    if let Ok(registry) = agent_doc_orchestration::sessions::load_in(&state.root) {
        for (key, entry) in registry {
            let path = registry_entry_path(&state.root, &key, &entry);
            let session_id = (!entry.session_id.is_empty()).then_some(entry.session_id);
            upsert_session(&mut by_path, &state.root, path, session_id, true)?;
        }
    }

    let mut scanned = Vec::new();
    scan_agent_docs(&state.root, &mut scanned)?;
    for path in scanned {
        let session_id = read_agent_doc_session_id(&path).ok().flatten();
        upsert_session(&mut by_path, &state.root, path, session_id, false)?;
    }

    Ok(by_path.into_values().collect())
}

fn registry_entry_path(
    root: &Path,
    key: &str,
    entry: &agent_doc_orchestration::sessions::SessionEntry,
) -> PathBuf {
    if !entry.file.is_empty() {
        let file = Path::new(&entry.file);
        if file.is_absolute() {
            return file.to_path_buf();
        }
        return root.join(file);
    }
    PathBuf::from(key)
}

fn upsert_session(
    by_path: &mut BTreeMap<PathBuf, ServeSession>,
    root: &Path,
    path: PathBuf,
    session_id: Option<String>,
    registered: bool,
) -> Result<()> {
    let display_path = canonicalize_or_normalize(&path);
    let key = display_path.clone();
    let session = make_session(root, &display_path, session_id, registered)?;
    by_path
        .entry(key)
        .and_modify(|existing| {
            existing.registered |= session.registered;
            if existing.session_id.is_none() {
                existing.session_id = session.session_id.clone();
            }
            existing.exists |= session.exists;
        })
        .or_insert(session);
    Ok(())
}

fn make_session(
    root: &Path,
    path: &Path,
    session_id: Option<String>,
    registered: bool,
) -> Result<ServeSession> {
    let absolute = canonicalize_or_normalize(path);
    let absolute_str = absolute.display().to_string();
    let hash = agent_doc_orchestration::snapshot::doc_hash_from_str(&absolute_str);
    let id = hash.chars().take(12).collect();
    let path = relative_path(root, &absolute).unwrap_or_else(|_| absolute_str.clone());
    let title = absolute
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&path)
        .to_string();
    Ok(ServeSession {
        id,
        path,
        absolute: absolute_str,
        title,
        session_id,
        registered,
        exists: absolute.exists(),
    })
}

fn scan_agent_docs(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in
        std::fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if is_ignored_session_scan_dir(&name) {
                continue;
            }
            scan_agent_docs(&path, out)?;
        } else if is_agent_doc_file(&path)? {
            out.push(path);
        }
    }
    Ok(())
}

fn is_ignored_session_scan_dir(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".agent-doc" | ".git" | "node_modules" | "target")
    )
}

fn is_agent_doc_file(path: &Path) -> Result<bool> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
        return Ok(false);
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let (frontmatter, _) = match crate::frontmatter::parse(&content) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(false),
    };
    Ok(frontmatter.session.is_some()
        || frontmatter.format.is_some()
        || content.contains("<!-- agent:exchange")
        || content.contains("<!-- agent:backlog")
        || content.contains("<!-- agent:queue"))
}

fn read_agent_doc_session_id(path: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let (frontmatter, _) = crate::frontmatter::parse(&content)?;
    Ok(frontmatter.session)
}

fn relative_path(root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn canonicalize_or_normalize(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| normalize_path(path))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn build_projection(doc: &str) -> Result<ServeProjection> {
    let components = crate::component::parse(doc)?;
    let names = ["exchange", "backlog", "queue", "review"];
    Ok(ServeProjection {
        components: names
            .into_iter()
            .map(|name| projection_component(doc, &components, name))
            .collect(),
    })
}

fn projection_component(
    doc: &str,
    components: &[crate::component::Component],
    name: &'static str,
) -> ServeProjectionComponent {
    let component = components.iter().find(|component| match name {
        "backlog" => crate::component::is_backlog_component(&component.name),
        "review" => crate::component::is_review_component(&component.name),
        _ => component.name == name,
    });
    let content = component
        .map(|component| component.content(doc).trim_matches('\n').to_string())
        .unwrap_or_default();
    ServeProjectionComponent {
        name,
        present: component.is_some(),
        lines: content.lines().count(),
        items: content
            .lines()
            .filter(|line| line.trim_start().starts_with("- ["))
            .count(),
        content,
    }
}

fn handle_events(
    request: tiny_http::Request,
    state: &ServeState,
    token: Option<&str>,
) -> Result<()> {
    let doc = match state.resolve_doc(token) {
        Ok(doc) => doc,
        Err(e) => {
            return respond(
                request,
                &format!("{e}\n"),
                "text/plain; charset=utf-8",
                StatusCode(404),
            );
        }
    };
    let headers = vec![
        header("Content-Type", "text/event-stream; charset=utf-8")?,
        header("Cache-Control", "no-cache")?,
    ];
    let response = Response::new(
        StatusCode(200),
        headers,
        SsePollStream::new(doc),
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct PartialResponseFingerprint {
    cycle_id: String,
    response_sha256: String,
    checkpoint_count: u64,
    updated_at: u64,
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
        hash: agent_doc_orchestration::ops_log::content_hash(&content),
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

fn active_partial_response(
    doc: &Path,
) -> Result<Option<agent_doc_orchestration::capture::PartialCaptureRecord>> {
    let Some(state) = agent_doc_orchestration::cycle_state::load(doc)? else {
        return Ok(None);
    };
    if !state.is_open() {
        return Ok(None);
    }
    agent_doc_orchestration::capture::load_partial_by_cycle(doc, &state.cycle_id)
}

fn partial_response_fingerprint(
    record: &agent_doc_orchestration::capture::PartialCaptureRecord,
) -> PartialResponseFingerprint {
    PartialResponseFingerprint {
        cycle_id: record.cycle_id.clone(),
        response_sha256: record.response_sha256.clone(),
        checkpoint_count: record.checkpoint_count,
        updated_at: record.updated_at,
    }
}

fn partial_response_payload(
    record: &agent_doc_orchestration::capture::PartialCaptureRecord,
) -> serde_json::Value {
    serde_json::json!({
        "path": record.file,
        "cycle_id": record.cycle_id,
        "checkpoint_id": record.checkpoint_id,
        "checkpoint_count": record.checkpoint_count,
        "updated_at": record.updated_at,
        "response_sha256": record.response_sha256,
        "response_body": record.response_body,
    })
}

fn format_sse_event(event: &str, payload: serde_json::Value) -> String {
    format!("event: {event}\ndata: {payload}\n\n")
}

struct SsePollStream {
    doc: PathBuf,
    interval: Duration,
    last: Option<DocFingerprint>,
    last_partial: Option<PartialResponseFingerprint>,
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
            last_partial: None,
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
        let mut emitted = false;
        let fingerprint = read_doc_fingerprint(&self.doc)?;
        if self.last.as_ref() != Some(&fingerprint) {
            self.last = Some(fingerprint.clone());
            self.enqueue_event("doc-changed", doc_event_payload(&self.doc, &fingerprint));
            emitted = true;
        }
        if self.poll_partial_response()? {
            emitted = true;
        }
        Ok(emitted)
    }

    fn poll_partial_response(&mut self) -> Result<bool> {
        let Some(record) = active_partial_response(&self.doc)? else {
            self.last_partial = None;
            return Ok(false);
        };
        let fingerprint = partial_response_fingerprint(&record);
        if self.last_partial.as_ref() == Some(&fingerprint) {
            return Ok(false);
        }
        self.last_partial = Some(fingerprint);
        self.enqueue_event("agent-response", partial_response_payload(&record));
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

fn handle_save(
    mut request: tiny_http::Request,
    state: &ServeState,
    token: Option<&str>,
) -> Result<()> {
    let doc = match state.resolve_doc(token) {
        Ok(doc) => doc,
        Err(e) => {
            return respond(
                request,
                &format!("{e}\n"),
                "text/plain; charset=utf-8",
                StatusCode(404),
            );
        }
    };
    let content_length = request
        .headers()
        .iter()
        .find(|h| {
            h.field
                .as_str()
                .as_str()
                .eq_ignore_ascii_case("content-length")
        })
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

    let active_cycle = match active_cycle_in_scope(&doc) {
        Ok(active) => active,
        Err(e) => {
            return respond(
                request,
                &format!("failed to inspect active cycle: {e}\n"),
                "text/plain; charset=utf-8",
                StatusCode(500),
            );
        }
    };
    let commit_mode = if active_cycle {
        if let Err(e) = write_and_close_active_cycle(&doc, &body) {
            return respond(
                request,
                &format!("agent-doc write --commit failed: {e}\n"),
                "text/plain; charset=utf-8",
                StatusCode(500),
            );
        }
        "agent-doc write --commit"
    } else {
        if let Err(e) = std::fs::write(&doc, &body) {
            return respond(
                request,
                &format!("failed to write {}: {e}\n", doc.display()),
                "text/plain; charset=utf-8",
                StatusCode(500),
            );
        }
        if let Err(e) = git_commit_user_edit(&doc) {
            return respond(
                request,
                &format!("git commit failed: {e}\n"),
                "text/plain; charset=utf-8",
                StatusCode(500),
            );
        }
        "git"
    };

    let head = git_head_short(&doc).unwrap_or_else(|_| "<unknown>".to_string());
    let payload = serde_json::json!({
        "ok": true,
        "head": head,
        "bytes": body.len(),
        "commit_mode": commit_mode,
    });
    respond(
        request,
        &payload.to_string(),
        "application/json; charset=utf-8",
        StatusCode(200),
    )
}

fn active_cycle_in_scope(doc: &Path) -> Result<bool> {
    Ok(agent_doc_orchestration::cycle_state::load(doc)?.is_some_and(|state| state.is_open()))
}

fn write_and_close_active_cycle(doc: &Path, body: &str) -> Result<()> {
    agent_doc_orchestration::write::atomic_write_pub(doc, body)
        .with_context(|| format!("failed to write {}", doc.display()))?;
    let binary = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("agent-doc"));
    let output = Command::new(binary)
        .args(["write", "--commit", "--origin", "serve"])
        .arg(doc)
        .stdin(Stdio::null())
        .output()
        .context("failed to run agent-doc write --commit")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "{}{}",
            stderr.trim(),
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!(" ({})", stdout.trim())
            }
        );
    }
    Ok(())
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

fn route_path(url: &str) -> &str {
    url.split_once('?').map(|(path, _)| path).unwrap_or(url)
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    let query = query
        .split_once('#')
        .map(|(query, _)| query)
        .unwrap_or(query);
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(name).ok().as_deref() == Some(key) {
            return percent_decode(value).ok();
        }
    }
    None
}

fn percent_decode(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                anyhow::bail!("invalid percent encoding in path");
            }
            let high = hex_value(bytes[i + 1])?;
            let low = hex_value(bytes[i + 2])?;
            decoded.push((high << 4) | low);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(decoded).context("path is not valid UTF-8")
}

fn hex_value(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => anyhow::bail!("invalid percent encoding in path"),
    }
}

fn respond(
    request: tiny_http::Request,
    body: &str,
    content_type: &str,
    status: StatusCode,
) -> Result<()> {
    respond_with_headers(request, body, content_type, status, Vec::new())
}

fn respond_auth_failure(request: tiny_http::Request, failure: AuthFailure) -> Result<()> {
    let body = match failure {
        AuthFailure::Missing => "missing bearer token\n",
        AuthFailure::Invalid => "invalid bearer token\n",
    };
    respond_with_headers(
        request,
        body,
        "text/plain; charset=utf-8",
        StatusCode(401),
        vec![header(
            "WWW-Authenticate",
            r#"Bearer realm="agent-doc serve""#,
        )?],
    )
}

fn respond_with_headers(
    request: tiny_http::Request,
    body: &str,
    content_type: &str,
    status: StatusCode,
    extra_headers: Vec<Header>,
) -> Result<()> {
    let content_type_header = header("Content-Type", content_type)?;
    let mut response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(content_type_header);
    for header in extra_headers {
        response.add_header(header);
    }
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
    fn serve_sse_stream_emits_active_partial_agent_response() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: live\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_orchestration::snapshot::save(&doc, content).unwrap();
        agent_doc_orchestration::cycle_state::start_preflight(&doc, Some(content), Some(content))
            .unwrap();
        let mut writer = agent_doc_orchestration::capture::PartialCheckpointWriter::with_interval(
            &doc,
            Duration::ZERO,
        );
        writer
            .maybe_checkpoint("### Re: live — gpt-5\n\nPartial response")
            .unwrap();

        let mut stream = SsePollStream::with_interval(doc, Duration::from_millis(1));
        let mut buf = vec![0; 8192];
        let _ = stream.read(&mut buf).unwrap();
        let n = stream.read(&mut buf).unwrap();
        let event = String::from_utf8_lossy(&buf[..n]);

        assert!(event.contains("event: agent-response\n"), "got {event:?}");
        assert!(event.contains("Partial response"), "got {event:?}");
    }

    #[test]
    fn serve_detects_open_cycle_for_write_commit_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(!active_cycle_in_scope(&doc).unwrap());
        agent_doc_orchestration::cycle_state::start_preflight(
            &doc,
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        assert!(active_cycle_in_scope(&doc).unwrap());
        agent_doc_orchestration::cycle_state::mark_committed(
            &doc,
            "test",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        assert!(!active_cycle_in_scope(&doc).unwrap());
    }

    #[test]
    fn serve_index_html_subscribes_to_sse_doc_changed_events() {
        assert!(INDEX_HTML.contains("new EventSource(docUrl(\"/events\"))"));
        assert!(INDEX_HTML.contains("doc-changed"));
        assert!(INDEX_HTML.contains("agent-response"));
    }

    #[test]
    fn serve_projection_extracts_component_summaries() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ hello\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#one] One\n",
            "- [x] [#two] Two\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do #one\n",
            "<!-- /agent:queue -->\n"
        );

        let projection = build_projection(doc).unwrap();
        let backlog = projection
            .components
            .iter()
            .find(|component| component.name == "backlog")
            .unwrap();
        assert!(backlog.present);
        assert_eq!(backlog.items, 2);
        assert!(backlog.content.contains("[#one] One"));

        let review = projection
            .components
            .iter()
            .find(|component| component.name == "review")
            .unwrap();
        assert!(!review.present);
    }

    #[test]
    fn serve_sessions_include_registry_and_scanned_docs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join("tasks/one.md"),
            "---\nagent_doc_session: one\nagent_doc_format: template\n---\n",
        )
        .unwrap();
        std::fs::write(root.join("registered.md"), "# registered\n").unwrap();

        let mut registry = agent_doc_orchestration::sessions::SessionRegistry::new();
        registry.insert(
            root.join("registered.md").display().to_string(),
            agent_doc_orchestration::sessions::SessionEntry {
                pane: "%1".to_string(),
                pid: 1,
                cwd: root.display().to_string(),
                started: "2026-05-25T00:00:00Z".to_string(),
                session_id: "registered".to_string(),
                file: "registered.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        agent_doc_orchestration::sessions::save_in(root, &registry).unwrap();

        let state = ServeState {
            root: root.to_path_buf(),
            default_doc: None,
            browse_root: true,
            auth: ServeAuth::Disabled,
        };
        let sessions = list_sessions(&state).unwrap();

        assert!(sessions.iter().any(|session| session.path == "tasks/one.md"
            && session.session_id.as_deref() == Some("one")));
        assert!(
            sessions
                .iter()
                .any(|session| session.path == "registered.md"
                    && session.registered
                    && session.session_id.as_deref() == Some("registered"))
        );
    }

    #[test]
    fn serve_resolves_doc_path_and_short_hash_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join("tasks/one.md"),
            "---\nagent_doc_session: one\nagent_doc_format: template\n---\n",
        )
        .unwrap();

        let state = ServeState {
            root: root.to_path_buf(),
            default_doc: None,
            browse_root: true,
            auth: ServeAuth::Disabled,
        };
        let sessions = list_sessions(&state).unwrap();
        let session = sessions
            .iter()
            .find(|session| session.path == "tasks/one.md")
            .unwrap();

        assert_eq!(
            state
                .resolve_doc(Some("tasks%2Fone.md"))
                .unwrap()
                .strip_prefix(root)
                .unwrap(),
            Path::new("tasks/one.md")
        );
        assert_eq!(
            state
                .resolve_doc(Some(&session.id))
                .unwrap()
                .strip_prefix(root)
                .unwrap(),
            Path::new("tasks/one.md")
        );
        assert!(state.resolve_doc(Some("..%2Fsecret.md")).is_err());
    }

    #[test]
    fn serve_index_html_loads_sessions_and_hash_routes() {
        assert!(INDEX_HTML.contains("/api/sessions"));
        assert!(INDEX_HTML.contains("docNav"));
        assert!(INDEX_HTML.contains("hashchange"));
    }

    #[test]
    fn serve_non_loopback_generates_edit_and_read_only_tokens() {
        let auth = ServeAuth::from_options("0.0.0.0", None, None).unwrap();
        match auth {
            ServeAuth::Enabled {
                edit_token,
                read_only_token,
            } => {
                assert!(edit_token.starts_with("adoc_edit_"));
                let read_only_token = read_only_token.unwrap();
                assert!(read_only_token.starts_with("adoc_read_"));
                assert_ne!(edit_token, read_only_token);
            }
            ServeAuth::Disabled => panic!("non-loopback bind must require auth"),
        }
    }

    #[test]
    fn serve_auth_scopes_allow_viewer_reads_not_writes() {
        let auth = ServeAuth::from_options(
            "0.0.0.0",
            Some("edit-token".to_string()),
            Some("read-token".to_string()),
        )
        .unwrap();

        assert_eq!(
            auth.scope_for_token(Some("edit-token")).unwrap(),
            AuthScope::Edit
        );
        assert_eq!(
            auth.scope_for_token(Some("read-token")).unwrap(),
            AuthScope::ReadOnly
        );
        assert!(
            !auth
                .scope_for_token(Some("read-token"))
                .unwrap()
                .can_write()
        );
        assert_eq!(
            auth.scope_for_token(Some("wrong")).unwrap_err(),
            AuthFailure::Invalid
        );
    }

    #[test]
    fn serve_tls_requires_certificate_and_key_pair() {
        assert!(ServeTls::from_options(Some(PathBuf::from("cert.pem")), None).is_err());
        assert!(ServeTls::from_options(None, Some(PathBuf::from("key.pem"))).is_err());
        assert!(
            ServeTls::from_options(
                Some(PathBuf::from("cert.pem")),
                Some(PathBuf::from("key.pem")),
            )
            .unwrap()
            .is_some()
        );
    }

    #[test]
    fn serve_index_html_sends_auth_to_fetch_and_eventsource() {
        assert!(INDEX_HTML.contains("/api/auth"));
        assert!(INDEX_HTML.contains("Authorization"));
        assert!(INDEX_HTML.contains("token=\" + encodeURIComponent(authToken)"));
        assert!(INDEX_HTML.contains("read-only token cannot save"));
    }
}

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
//!   - `GET /doc` → current authoritative text with a revision in `ETag`.
//!   - `GET /doc/<path-or-hash>` → current text for a project session document.
//!   - `GET /api/auth` → current auth mode and token scope.
//!   - `GET /api/sessions` → project session list.
//!   - `GET /board` → embedded per-project work board page (`BOARD_HTML`).
//!   - `GET /api/board` → read-only work board JSON for this server's project
//!     (`agent-doc board --json`).
//!   - `GET /fleet` → embedded global board page (`FLEET_HTML`).
//!   - `GET /api/fleet` → read-only board across every project a running
//!     controller serves. Cached for 30 seconds. Never writes.
//!   - `GET /tree` → embedded per-document queue page (`TREE_HTML`).
//!   - `GET /api/queue-tree/<doc>` → that document's queue tree. Each row's
//!     `queue_id` is the claim join key, and `claims` use the same key.
//!     Reads the claim ledger read-only and never creates `state.db`.
//!   - `POST /api/prompt[/<doc>]` → revision-bound prompt placement and dispatch on an opted-in local server.
//!   - `POST /save` → body = full document text, with `X-Agent-Doc-Base-Revision` naming its
//!     base cut. Merges independent changes and returns the committed text and
//!     its `ETag`. An overlap or unavailable authority returns `409 Conflict`.
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
//! - The existing document actor serializes the merge, authority write and
//!   commit decision. A live editor must project the exact merged text to
//!   disk using its own native save. A detached write checks its expected cut
//!   at the atomic rename and the Git transaction checks the persisted cut at
//!   staging and ref update.
//! - When `write --commit` exits nonzero, the HTTP response surfaces the
//!   stderr verbatim so the operator sees the actionable error.
//!
//! ## Evals
//! - `serve_get_doc_returns_current_file_contents`
//! - `serve_post_save_writes_through_commit_boundary`
//! - `serve_post_save_refuses_overlapping_editor_edits`
//! - `serve_sse_stream_emits_doc_changed_after_file_mutation`

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Response, Server, SslConfig, StatusCode};

const INDEX_HTML: &str = include_str!("../assets/serve/index.html");
const BOARD_HTML: &str = include_str!("../assets/serve/board.html");
const FLEET_HTML: &str = include_str!("../assets/serve/fleet.html");
const TREE_HTML: &str = include_str!("../assets/serve/tree.html");
const FLEET_CACHE_TTL: Duration = Duration::from_secs(30);
const DEFAULT_PORT: u16 = 7333;
const DEFAULT_HOST: &str = "127.0.0.1";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024; // 4 MiB cap for the MVP
const MAX_SAVED_REVISIONS: usize = 16;
const SSE_POLL_INTERVAL_MS: u64 = 300;

pub struct ServeOptions {
    pub target: Option<PathBuf>,
    pub host: String,
    pub port: u16,
    pub auth_token: Option<String>,
    pub read_only_token: Option<String>,
    pub allow_run: bool,
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
            allow_run: false,
            tls_cert,
            tls_key,
        }
    }
}

pub fn run(options: ServeOptions) -> Result<()> {
    anyhow::ensure!(
        !options.allow_run || is_loopback_host(&options.host),
        "--allow-run requires a loopback --host"
    );
    let auth_token = options
        .auth_token
        .or_else(|| options.allow_run.then(|| generate_bearer_token("edit")));
    let auth = ServeAuth::from_options(&options.host, auth_token, options.read_only_token)?;
    let tls = ServeTls::from_options(options.tls_cert, options.tls_key)?;
    let scheme = if tls.is_some() { "https" } else { "http" };
    let mut state = ServeState::new(options.target, auth)?;
    state.allow_run = options.allow_run;
    let state = Arc::new(state);

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
    allow_run: bool,
    prompt_runs: Arc<Mutex<std::collections::HashSet<PathBuf>>>,
    revisions: Arc<Mutex<VecDeque<BrowserRevision>>>,
    /// Last `/api/fleet` body. The scan of every running controller is slow,
    /// so a second read inside [`FLEET_CACHE_TTL`] returns this copy.
    fleet_cache: Arc<Mutex<Option<FleetCache>>>,
}

#[derive(Clone, Debug)]
struct FleetCache {
    at: Instant,
    body: String,
}

#[derive(Debug)]
struct BrowserRevision {
    doc: PathBuf,
    tag: String,
    text: Arc<str>,
}

impl ServeState {
    fn new(target: Option<PathBuf>, auth: ServeAuth) -> Result<Self> {
        let raw_target = match target {
            Some(target) => target,
            None => {
                let cwd = std::env::current_dir().context("failed to get current directory")?;
                agent_doc_fs::find_project_root(&cwd).unwrap_or(cwd)
            }
        };
        if !raw_target.exists() {
            anyhow::bail!("file or directory not found: {}", raw_target.display());
        }
        let canonical = raw_target
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", raw_target.display()))?;
        if canonical.is_file() {
            let root = agent_doc_fs::find_project_root(&canonical)
                .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
            Ok(Self {
                root,
                default_doc: Some(canonical),
                browse_root: false,
                auth,
                allow_run: false,
                prompt_runs: Arc::default(),
                revisions: Arc::default(),
                fleet_cache: Arc::default(),
            })
        } else {
            let root = agent_doc_fs::find_project_root(&canonical).unwrap_or(canonical);
            Ok(Self {
                root,
                default_doc: None,
                browse_root: true,
                auth,
                allow_run: false,
                prompt_runs: Arc::default(),
                revisions: Arc::default(),
                fleet_cache: Arc::default(),
            })
        }
    }

    fn remember_revision(&self, doc: &Path, text: &str) -> String {
        let tag = format!("\"{}\"", agent_doc_hash::content_hash(text));
        if text.len() <= MAX_BODY_BYTES {
            let mut revisions = self.revisions.lock();
            revisions.retain(|cut| cut.doc != doc || cut.tag != tag);
            revisions.push_back(BrowserRevision {
                doc: doc.to_path_buf(),
                tag: tag.clone(),
                text: Arc::from(text),
            });
            if revisions.len() > MAX_SAVED_REVISIONS {
                revisions.pop_front();
            }
        }
        tag
    }

    fn browser_base(&self, doc: &Path, revision: &str) -> Option<Arc<str>> {
        self.revisions
            .lock()
            .iter()
            .find(|cut| cut.doc == doc && cut.tag == revision)
            .map(|cut| Arc::clone(&cut.text))
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

    /// A queue-tree document is one this server already serves, or an
    /// agent-doc file under a project whose controller is running. Anything
    /// else is refused: the fleet view must not become a general file read.
    fn resolve_queue_tree_doc(&self, token: Option<&str>) -> Result<PathBuf> {
        let Some(token) = token.filter(|token| !token.is_empty()) else {
            return self.resolve_doc(None);
        };
        let decoded = percent_decode(token)?;
        let path = Path::new(&decoded);
        if path.is_absolute() {
            return self.resolve_absolute_session(path);
        }
        self.resolve_doc(Some(token))
    }

    fn resolve_absolute_session(&self, path: &Path) -> Result<PathBuf> {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("session document not found: {}", path.display()))?;
        let under_server = canonical.starts_with(&self.root);
        let under_controller =
            agent_doc_controller_io::process::controller_project_roots(std::process::id())
                .iter()
                .any(|root| canonical.starts_with(root));
        if !under_server && !under_controller {
            anyhow::bail!(
                "document is outside this server and the projects with a running controller: {}",
                path.display()
            );
        }
        if !is_agent_doc_file(&canonical)? {
            anyhow::bail!("not an agent-doc session document: {}", path.display());
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
        (Method::Get, "/board") => respond(
            request,
            BOARD_HTML,
            "text/html; charset=utf-8",
            StatusCode(200),
        ),
        (Method::Get, "/api/board") => handle_board(request, state),
        (Method::Get, "/fleet") => respond(
            request,
            FLEET_HTML,
            "text/html; charset=utf-8",
            StatusCode(200),
        ),
        (Method::Get, "/api/fleet") => handle_fleet(request, state),
        (Method::Get, "/tree") => respond(
            request,
            TREE_HTML,
            "text/html; charset=utf-8",
            StatusCode(200),
        ),
        (Method::Get, "/api/queue-tree") => handle_queue_tree(request, state, None),
        (Method::Get, route) if route.starts_with("/api/queue-tree/") => {
            handle_queue_tree(request, state, Some(&route["/api/queue-tree/".len()..]))
        }
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
        (Method::Post, "/api/prompt") => handle_prompt(request, state, None),
        (Method::Post, route) if route.starts_with("/api/prompt/") => {
            handle_prompt(request, state, Some(&route["/api/prompt/".len()..]))
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
    *method == Method::Post
        && (route == "/save"
            || route.starts_with("/save/")
            || route == "/api/prompt"
            || route.starts_with("/api/prompt/"))
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

/// Read-only fleet work board over this server's project root and its
/// submodules. Same model as `agent-doc board`; see `fleet_board_cmd`.
fn handle_board(request: tiny_http::Request, state: &ServeState) -> Result<()> {
    let board = crate::fleet_board_cmd::board_for_root(&state.root, false, false);
    respond(
        request,
        &serde_json::to_string(&board)?,
        "application/json; charset=utf-8",
        StatusCode(200),
    )
}

/// Read-only board of every project a live controller serves.
fn handle_fleet(request: tiny_http::Request, state: &ServeState) -> Result<()> {
    let body = match fleet_json(state) {
        Ok(body) => body,
        Err(error) => {
            return respond(
                request,
                &format!("{error:#}\n"),
                "text/plain; charset=utf-8",
                StatusCode(500),
            );
        }
    };
    respond(
        request,
        &body,
        "application/json; charset=utf-8",
        StatusCode(200),
    )
}

fn fleet_json(state: &ServeState) -> Result<String> {
    let mut cache = state.fleet_cache.lock();
    if let Some(hit) = cache.as_ref()
        && hit.at.elapsed() < FLEET_CACHE_TTL
    {
        return Ok(hit.body.clone());
    }
    let roots = controller_sections();
    let controllers = roots.len();
    let board = crate::fleet_board_cmd::board_for_projects(&roots, false);
    let body = serde_json::to_string(&ServeFleet {
        schema: "agent-doc-fleet-v1",
        controllers,
        board,
    })?;
    *cache = Some(FleetCache {
        at: Instant::now(),
        body: body.clone(),
    });
    Ok(body)
}

/// One section per running controller. The label is the directory name, or
/// the whole path when two controllers share a directory name, so the board
/// does not merge them into one group.
fn controller_sections() -> Vec<crate::fleet_board_cmd::ProjectRoot> {
    let roots: Vec<PathBuf> =
        agent_doc_controller_io::process::controller_project_roots(std::process::id())
            .into_iter()
            .collect();
    let names: Vec<String> = roots.iter().map(|root| controller_dir_name(root)).collect();
    roots
        .into_iter()
        .enumerate()
        .map(|(index, root)| {
            let name = &names[index];
            let duplicated = names.iter().filter(|other| *other == name).count() > 1;
            let label = if duplicated {
                root.display().to_string()
            } else {
                name.clone()
            };
            crate::fleet_board_cmd::ProjectRoot { label, root }
        })
        .collect()
}

fn controller_dir_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| root.display().to_string())
}

#[derive(Serialize)]
struct ServeFleet {
    schema: &'static str,
    controllers: usize,
    #[serde(flatten)]
    board: agent_doc_work_graph::fleet_board::FleetBoard,
}

/// One document's queue tree plus the claims that join its rows on `queue_id`.
fn handle_queue_tree(
    request: tiny_http::Request,
    state: &ServeState,
    token: Option<&str>,
) -> Result<()> {
    let doc = match state.resolve_queue_tree_doc(token) {
        Ok(doc) => doc,
        Err(error) => {
            return respond(
                request,
                &format!("{error:#}\n"),
                "text/plain; charset=utf-8",
                StatusCode(404),
            );
        }
    };
    let body = match agent_doc_document_realtime_io::peek_current_document_content(
        &doc,
        "serve_queue_tree",
    ) {
        Ok(body) => body,
        Err(error) => {
            return respond(
                request,
                &format!("document authority unavailable: {error:#}\n"),
                "text/plain; charset=utf-8",
                StatusCode(503),
            );
        }
    };
    let payload = match serve_queue_tree(&doc, &body) {
        Ok(payload) => payload,
        Err(error) => {
            return respond(
                request,
                &format!("{error:#}\n"),
                "text/plain; charset=utf-8",
                StatusCode(500),
            );
        }
    };
    respond(
        request,
        &serde_json::to_string(&payload)?,
        "application/json; charset=utf-8",
        StatusCode(200),
    )
}

fn serve_queue_tree(doc: &Path, body: &str) -> Result<ServeQueueTree> {
    let tree = agent_doc_queue::queue_tree::queue_tree(
        body,
        &agent_doc_queue::queue_tree::QueueTreeOptions::default(),
    )?;
    let (claims, claims_error) = queue_tree_claims(doc, body);
    Ok(ServeQueueTree {
        schema: "agent-doc-queue-tree-v1",
        document: doc.display().to_string(),
        summary: tree.summary,
        nodes: tree.nodes,
        claims,
        claims_error,
    })
}

/// Claims from the read-only ledger. A missing database is an empty list,
/// not an error and not a reason to create `state.db`.
fn queue_tree_claims(doc: &Path, body: &str) -> (Vec<ServeQueueClaim>, Option<String>) {
    match agent_doc_queue_io::queue_claim::load_ledger_following_read_only(doc, body) {
        Ok(ledger) => (
            ledger
                .claims
                .into_iter()
                .map(|claim| ServeQueueClaim {
                    queue_id: claim.identity.join_key(),
                    owner: claim.owner,
                    state: claim.state,
                })
                .collect(),
            None,
        ),
        Err(error) => (Vec::new(), Some(format!("{error:#}"))),
    }
}

#[derive(Serialize)]
struct ServeQueueTree {
    schema: &'static str,
    document: String,
    summary: agent_doc_queue::queue_tree::QueueTreeSummary,
    nodes: Vec<agent_doc_queue::queue_tree::QueueTreeNode>,
    claims: Vec<ServeQueueClaim>,
    #[serde(skip_serializing_if = "Option::is_none")]
    claims_error: Option<String>,
}

#[derive(Serialize)]
struct ServeQueueClaim {
    /// [`agent_doc_element_queue::QueueItemIdentity::join_key`], the same
    /// string each tree row's `queue_id` carries.
    queue_id: String,
    owner: String,
    state: agent_doc_queue::queue_claim::QueueClaimState,
}

fn handle_auth(request: tiny_http::Request, state: &ServeState, scope: AuthScope) -> Result<()> {
    let payload = serde_json::json!({
        "authRequired": state.auth.is_enabled(),
        "scope": scope.as_str(),
        "canWrite": scope.can_write(),
        "canRun": state.allow_run && scope.can_write(),
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
    let body =
        match agent_doc_document_realtime_io::peek_current_document_content(&doc, "serve_doc") {
            Ok(body) => body,
            Err(error) => {
                return respond(
                    request,
                    &format!("document authority unavailable: {error:#}\n"),
                    "text/plain; charset=utf-8",
                    StatusCode(503),
                );
            }
        };
    let revision = state.remember_revision(&doc, &body);
    respond_with_headers(
        request,
        &body,
        "text/markdown; charset=utf-8",
        StatusCode(200),
        vec![
            header("ETag", &revision)?,
            header("Cache-Control", "no-store")?,
        ],
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
    let body = match agent_doc_document_realtime_io::peek_current_document_content(
        &doc,
        "serve_projection",
    ) {
        Ok(body) => body,
        Err(error) => {
            return respond(
                request,
                &format!("document authority unavailable: {error:#}\n"),
                "text/plain; charset=utf-8",
                StatusCode(503),
            );
        }
    };
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
    document_prompts: serde_json::Value,
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

    if let Ok(registry) = agent_doc_session_registry_io::load_in(&state.root) {
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

fn registry_entry_path(root: &Path, key: &str, entry: &tmux_router::RegistryEntry) -> PathBuf {
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
    let hash = agent_doc_fs::document_state_hash_from_str(&absolute_str);
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
    if agent_doc_frontmatter::dashboard_projection::is_dashboard_projection(&content) {
        return Ok(false);
    }
    let (frontmatter, _) = match agent_doc_frontmatter::frontmatter::parse(&content) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(false),
    };
    Ok(frontmatter.session.is_some()
        || frontmatter.format.is_some()
        || content.contains("<!-- agent:exchange")
        || content.contains("<!-- agent:backlog")
        || content.contains("<!-- agent:run")
        || content.contains("<!-- agent:queue"))
}

fn read_agent_doc_session_id(path: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let (frontmatter, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
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
    let components = agent_doc_element::element::parse(doc)?;
    let names = ["exchange", "backlog", "queue", "review"];
    Ok(ServeProjection {
        components: names
            .into_iter()
            .map(|name| projection_component(doc, &components, name))
            .collect(),
        document_prompts: agent_doc_editor_surface_io::project_document_prompts(doc)
            .unwrap_or_else(|error| serde_json::json!({"error": error.to_string()})),
    })
}

fn projection_component(
    doc: &str,
    components: &[agent_doc_element::element::Component],
    name: &'static str,
) -> ServeProjectionComponent {
    let component = components.iter().find(|component| match name {
        "backlog" => agent_doc_element::element::is_backlog_component(&component.name),
        "review" => agent_doc_element::element::is_review_component(&component.name),
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
        hash: agent_doc_hash::content_hash(&content),
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
) -> Result<Option<agent_doc_capture_io::PartialCaptureRecord>> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(doc)? else {
        return Ok(None);
    };
    if !state.is_open() {
        return Ok(None);
    }
    agent_doc_capture_io::load_partial_by_cycle(doc, &state.cycle_id)
}

fn partial_response_fingerprint(
    record: &agent_doc_capture_io::PartialCaptureRecord,
) -> PartialResponseFingerprint {
    PartialResponseFingerprint {
        cycle_id: record.cycle_id.clone(),
        response_sha256: record.response_sha256.clone(),
        checkpoint_count: record.checkpoint_count,
        updated_at: record.updated_at,
    }
}

fn partial_response_payload(
    record: &agent_doc_capture_io::PartialCaptureRecord,
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

fn handle_prompt(
    mut request: tiny_http::Request,
    state: &ServeState,
    token: Option<&str>,
) -> Result<()> {
    const MAX_PROMPT_BYTES: usize = 16 * 1024;
    if !state.allow_run
        || !state.auth.is_enabled()
        || state
            .auth
            .scope_for_token(bearer_token(&request).as_deref())
            .ok()
            != Some(AuthScope::Edit)
    {
        return respond(
            request,
            "prompt submission requires --allow-run and an edit bearer header\n",
            "text/plain; charset=utf-8",
            StatusCode(403),
        );
    }
    let doc = match state.resolve_doc(token) {
        Ok(doc)
            if state
                .default_doc
                .as_ref()
                .is_none_or(|default| *default == doc) =>
        {
            doc
        }
        _ => {
            return respond(
                request,
                "session document not found\n",
                "text/plain; charset=utf-8",
                StatusCode(404),
            );
        }
    };
    let mut body = Vec::new();
    request
        .as_reader()
        .take(MAX_PROMPT_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .context("failed to read prompt request")?;
    if body.len() > MAX_PROMPT_BYTES {
        return respond(
            request,
            "prompt request is too large\n",
            "text/plain; charset=utf-8",
            StatusCode(413),
        );
    }
    let submission = match serde_json::from_slice(&body) {
        Ok(submission) => submission,
        Err(error) => {
            return respond(
                request,
                &format!("invalid prompt request: {error}\n"),
                "text/plain; charset=utf-8",
                StatusCode(400),
            );
        }
    };
    if !state.prompt_runs.lock().insert(doc.clone()) {
        return respond(
            request,
            "The previous browser submission is still running. Your draft is unchanged.\n",
            "text/plain; charset=utf-8",
            StatusCode(409),
        );
    }
    let (content, active_cycle) = match persist_browser_prompt(&doc, submission) {
        Ok(saved) => saved,
        Err(failure) => {
            state.prompt_runs.lock().remove(&doc);
            return respond(
                request,
                &failure.message,
                "text/plain; charset=utf-8",
                StatusCode(failure.status),
            );
        }
    };
    let dispatch = if active_cycle {
        Ok(false)
    } else {
        start_browser_prompt_run(state, &doc).map(|()| true)
    };
    if !matches!(dispatch, Ok(true)) {
        state.prompt_runs.lock().remove(&doc);
    }
    let dispatch_error = dispatch.as_ref().err().map(|error| format!("{error:#}"));
    let revision = state.remember_revision(&doc, &content);
    let payload = serde_json::json!({
        "content": content,
        "started": dispatch.unwrap_or(false),
        "dispatch_error": dispatch_error,
    });
    respond_with_headers(
        request,
        &payload.to_string(),
        "application/json; charset=utf-8",
        StatusCode(200),
        vec![
            header("ETag", &revision)?,
            header("Cache-Control", "no-store")?,
        ],
    )
}

fn persist_browser_prompt(
    doc: &Path,
    submission: agent_doc_editor_surface_io::DocumentPromptSubmission,
) -> std::result::Result<(String, bool), SaveFailure> {
    const SOURCE: &str = "serve_browser_prompt";
    let project = agent_doc_project_root_io::project_root_containing(doc).ok_or_else(|| {
        SaveFailure::new(
            409,
            "prompt submission requires an Agent Doc project\n".into(),
        )
    })?;
    let doc = doc.to_path_buf();
    agent_doc_queue_io::write_queue::DocumentWriteQueue::new(
        &agent_doc_document_realtime_io::SESSION_ACTOR_WRITE_QUEUE,
        &project,
        &doc.to_string_lossy(),
    )
    .agent_write(move || {
        let current = agent_doc_document_realtime_io::peek_current_document_content(&doc, SOURCE)
            .map_err(|error| {
            SaveFailure::new(409, format!("document authority unavailable: {error:#}\n"))
        })?;
        let content = agent_doc_editor_surface_io::insert_document_prompt(&current, &submission)
            .map_err(|error| SaveFailure::new(409, format!("{error:#}\n")))?;
        if content.len() > MAX_BODY_BYTES {
            return Err(SaveFailure::new(413, "the document is too large\n".into()));
        }
        let active_cycle = active_cycle_id(&doc)
            .map_err(|error| {
                SaveFailure::new(500, format!("failed to inspect active cycle: {error:#}\n"))
            })?
            .is_some();
        agent_doc_document_realtime_io::persist_exact_operator_edit_through_authority(
            &doc, &current, &content, SOURCE,
        )
        .map_err(|error| {
            SaveFailure::new(
                409,
                format!("prompt placement could not be confirmed: {error:#}\n"),
            )
        })?;
        // Admission consumes this operator edit. A browser save's Git commit
        // would instead establish it as the next baseline.
        Ok((content, active_cycle))
    })
    .map_err(|error| {
        SaveFailure::new(
            500,
            format!("document write queue unavailable: {error:#}\n"),
        )
    })?
}

fn start_browser_prompt_run(state: &ServeState, doc: &Path) -> Result<()> {
    use agent_doc_state_backbone::pane_execution_authority::{
        AuthorityOperation, AuthorityVerdict,
    };

    let scope = agent_doc_state_backbone::DocumentScope::new();
    let verdict = agent_doc_run_io::pane_execution_authority::verdict_in(
        &scope,
        doc,
        AuthorityOperation::Mutation,
    )?;
    let binary = std::env::current_exe().context("locate Agent Doc executable")?;
    let mut command = Command::new(binary);
    if matches!(
        verdict,
        AuthorityVerdict::PermitHeadless | AuthorityVerdict::PermitUnregistered
    ) {
        command.arg("run");
    } else {
        command.args(["route", "--dispatch-only", "--debounce", "0"]);
    }
    let mut child = command
        .arg(doc)
        .current_dir(&state.root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .context("start Agent Doc")?;
    let runs = Arc::clone(&state.prompt_runs);
    let doc = doc.to_path_buf();
    thread::spawn(move || {
        match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("[serve] prompt run for {} exited {status}", doc.display()),
            Err(error) => eprintln!(
                "[serve] prompt run for {} could not be observed: {error}",
                doc.display()
            ),
        }
        runs.lock().remove(&doc);
    });
    Ok(())
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
    let Some(base_revision) = request.headers().iter().find(|entry| {
        entry
            .field
            .as_str()
            .as_str()
            .eq_ignore_ascii_case("X-Agent-Doc-Base-Revision")
    }) else {
        return respond(
            request,
            "reload the document to obtain a base revision before saving\n",
            "text/plain; charset=utf-8",
            StatusCode(428),
        );
    };
    let Some(base) = state.browser_base(&doc, base_revision.value.as_str()) else {
        return respond(
            request,
            "This document's base revision is unavailable. Keep your draft and reload before merging it.\n",
            "text/plain; charset=utf-8",
            StatusCode(409),
        );
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

    let saved = match save_document(&doc, base, body) {
        Ok(saved) => saved,
        Err(failure) => {
            return respond(
                request,
                &failure.message,
                "text/plain; charset=utf-8",
                StatusCode(failure.status),
            );
        }
    };

    let revision = state.remember_revision(&doc, &saved.content);
    let payload = serde_json::json!({
        "ok": true,
        "head": saved.head,
        "bytes": saved.content.len(),
        "content": saved.content,
        "commit_mode": saved.commit_mode,
    });
    respond_with_headers(
        request,
        &payload.to_string(),
        "application/json; charset=utf-8",
        StatusCode(200),
        vec![
            header("ETag", &revision)?,
            header("Cache-Control", "no-store")?,
        ],
    )
}

/// A browser save that did not land, with the HTTP status that reports it.
#[derive(Debug)]
struct SaveFailure {
    status: u16,
    message: String,
}

impl SaveFailure {
    fn new(status: u16, message: String) -> Self {
        Self { status, message }
    }
}

impl std::fmt::Display for SaveFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SaveFailure {}

struct SavedDocument {
    content: String,
    head: String,
    commit_mode: &'static str,
}

fn save_document(
    doc: &Path,
    base: Arc<str>,
    body: String,
) -> std::result::Result<SavedDocument, SaveFailure> {
    let parent = doc.parent().unwrap_or(Path::new("."));
    let git_root = agent_doc_git_io::dirs::git_toplevel_at(parent)
        .ok_or_else(|| SaveFailure::new(500, "saving requires a Git worktree\n".into()))?;
    let document_dir =
        agent_doc_project_root_io::project_root_containing(doc).unwrap_or_else(|| git_root.clone());
    let owned_doc = doc.to_path_buf();
    agent_doc_queue_io::write_queue::DocumentWriteQueue::new(
        &agent_doc_document_realtime_io::SESSION_ACTOR_WRITE_QUEUE,
        &document_dir,
        &doc.to_string_lossy(),
    )
    .agent_write(move || save_document_in_actor(&owned_doc, &git_root, &base, &body))
    .map_err(|e| SaveFailure::new(500, format!("document write queue unavailable: {e:#}\n")))?
}

fn save_document_in_actor(
    doc: &Path,
    git_root: &Path,
    base: &str,
    body: &str,
) -> std::result::Result<SavedDocument, SaveFailure> {
    const SOURCE: &str = "serve_browser_save";
    let read = || {
        agent_doc_document_realtime_io::peek_current_document_content(doc, SOURCE)
            .map_err(|e| SaveFailure::new(409, format!("document authority unavailable: {e:#}\n")))
    };
    let current = read()?;
    let merged = if base == current {
        body.to_owned()
    } else {
        agent_doc_merge::captured_splice::line_merge(base, body, &current).ok_or_else(|| {
            SaveFailure::new(
                409,
                "The browser and current document edits overlap. Keep your draft and reload before merging it.\n".into(),
            )
        })?
    };
    if merged.len() > MAX_BODY_BYTES {
        return Err(SaveFailure::new(
            413,
            "merged document is too large\n".into(),
        ));
    }
    let cycle = active_cycle_id(doc)
        .map_err(|e| SaveFailure::new(500, format!("failed to inspect active cycle: {e:#}\n")))?;
    agent_doc_document_realtime_io::persist_exact_operator_edit_through_authority(
        doc, &current, &merged, SOURCE,
    )
    .map_err(|e| SaveFailure::new(409, format!("browser save could not be confirmed: {e:#}\n")))?;
    let fence = BrowserCommitFence {
        text: &merged,
        cycle: cycle.as_deref(),
    };
    let (content, commit_mode) = if cycle.is_some() {
        fence.check(doc)?;
        close_active_cycle(doc).map_err(|e| {
            SaveFailure::new(500, format!("agent-doc write --commit failed: {e:#}\n"))
        })?;
        (read()?, "agent-doc write --commit")
    } else {
        git_commit_user_edit(git_root, doc, &fence)?;
        (merged, "git")
    };
    let head = git_head_short(doc)
        .map_err(|e| SaveFailure::new(500, format!("failed to read committed HEAD: {e:#}\n")))?;
    Ok(SavedDocument {
        content,
        head,
        commit_mode,
    })
}

struct BrowserCommitFence<'a> {
    text: &'a str,
    cycle: Option<&'a str>,
}

impl BrowserCommitFence<'_> {
    fn check(&self, doc: &Path) -> std::result::Result<(), SaveFailure> {
        let authority = agent_doc_document_realtime_io::peek_current_document_content(
            doc,
            "serve_browser_commit",
        )
        .map_err(|e| {
            SaveFailure::new(
                409,
                format!("document authority unavailable before commit: {e:#}\n"),
            )
        })?;
        let disk = std::fs::read_to_string(doc)
            .map_err(|e| SaveFailure::new(500, format!("failed to read saved document: {e}\n")))?;
        let cycle = active_cycle_id(doc).map_err(|e| {
            SaveFailure::new(500, format!("failed to inspect active cycle: {e:#}\n"))
        })?;
        if self.text != authority || self.text != disk || self.cycle != cycle.as_deref() {
            return Err(SaveFailure::new(409, "The document revision or active cycle changed before commit. Keep your draft and reload.\n".into()));
        }
        Ok(())
    }
}

impl agent_doc_git_io::transaction::CommitTransactionFence for BrowserCommitFence<'_> {
    fn ensure(
        &self,
        doc: &Path,
        _phase: agent_doc_git_io::transaction::CommitFencePhase,
    ) -> Result<()> {
        Ok(self.check(doc)?)
    }

    fn ref_updated(&self, _doc: &Path) {}
}

#[cfg(test)]
fn active_cycle_in_scope(doc: &Path) -> Result<bool> {
    Ok(active_cycle_id(doc)?.is_some())
}

fn active_cycle_id(doc: &Path) -> Result<Option<String>> {
    Ok(
        agent_doc_cycle_state_io::load_with_closeout_projection(doc)?
            .filter(|state| state.is_open())
            .map(|state| state.cycle_id),
    )
}

fn close_active_cycle(doc: &Path) -> Result<()> {
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

fn git_commit_user_edit(
    git_root: &Path,
    doc: &Path,
    fence: &BrowserCommitFence<'_>,
) -> std::result::Result<(), SaveFailure> {
    use agent_doc_git_io::transaction::{
        CommitTransactionError, stage_and_commit_operator_cut_once,
    };
    let head = agent_doc_git_io::revision::show_head(doc)
        .map_err(|e| SaveFailure::new(500, format!("failed to read HEAD document: {e:#}\n")))?;
    if head.as_deref() == Some(fence.text) {
        return fence.check(doc);
    }
    stage_and_commit_operator_cut_once(
        git_root,
        doc,
        fence.text,
        &format!("agent-doc serve: web edit {}", chrono_timestamp()),
        fence,
    )
    .map(|_| ())
    .map_err(|e| match e {
        CommitTransactionError::Fatal(error) => {
            error.downcast::<SaveFailure>().unwrap_or_else(|error| {
                SaveFailure::new(
                    500,
                    format!("git commit failed after browser save: {error:#}\n"),
                )
            })
        }
        error @ (CommitTransactionError::RetryableHeadMoved { .. }
        | CommitTransactionError::RetryableIndexLock { .. }) => SaveFailure::new(
            409,
            format!("git state changed after browser save: {error:?}\n"),
        ),
        error => SaveFailure::new(
            500,
            format!("git commit failed after browser save: {error:?}\n"),
        ),
    })
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let mut writer =
            agent_doc_capture_io::PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
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
        agent_doc_cycle_state_io::start_preflight(&doc, Some("# Session\n"), Some("# Session\n"))
            .unwrap();
        assert!(active_cycle_in_scope(&doc).unwrap());
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "test",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        assert!(!active_cycle_in_scope(&doc).unwrap());
    }

    #[test]
    fn serve_active_cycle_scope_uses_terminal_projection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some("# Session\n"), Some("# Session\n"))
            .unwrap();
        assert!(active_cycle_in_scope(&doc).unwrap());

        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "test",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        assert!(
            !agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .is_open()
        );
        assert!(
            !active_cycle_in_scope(&doc).unwrap(),
            "serve write routing should honor terminal closeout projections"
        );
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

        let mut registry = tmux_router::Registry::new();
        registry.insert(
            root.join("registered.md").display().to_string(),
            tmux_router::RegistryEntry {
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
        agent_doc_session_registry_io::save_in(root, &registry).unwrap();

        let state = ServeState {
            root: root.to_path_buf(),
            default_doc: None,
            browse_root: true,
            auth: ServeAuth::Disabled,
            allow_run: false,
            prompt_runs: Arc::default(),
            revisions: Arc::default(),
            fleet_cache: Arc::default(),
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
            allow_run: false,
            prompt_runs: Arc::default(),
            revisions: Arc::default(),
            fleet_cache: Arc::default(),
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
        assert!(
            BOARD_HTML.contains("/api/board"),
            "the board page must fetch its own read-only endpoint"
        );
        assert!(
            BOARD_HTML.contains("/fleet"),
            "the board links to the fleet"
        );
        assert!(
            FLEET_HTML.contains("/api/fleet"),
            "the fleet page must fetch its own read-only endpoint"
        );
        assert!(
            TREE_HTML.contains("/api/queue-tree/"),
            "the document page must fetch the queue-tree endpoint"
        );
        assert!(
            TREE_HTML.contains("queue_id"),
            "the document page joins a row to its claim on queue_id"
        );
        assert!(INDEX_HTML.contains("docNav"));
        assert!(INDEX_HTML.contains("hashchange"));
    }

    #[test]
    fn serve_fleet_and_queue_tree_are_read_only_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(
            &doc,
            "<!-- agent:queue id=\"main\" -->\n- [#q-origin] follow-up: keep it [#q-aaaaaaaaaaaa]\n- yes\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let state = ServeState::new(Some(root), ServeAuth::Disabled).unwrap();

        let (status, _, body) = browser_request(&state, "GET", "/api/fleet", None, "");
        assert_eq!(status, 200, "{body}");
        let fleet: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(fleet["schema"], "agent-doc-fleet-v1");
        assert!(fleet["groups"].is_array());
        assert!(fleet["controllers"].is_number());

        let (status, _, body) =
            browser_request(&state, "GET", "/api/queue-tree/session.md", None, "");
        assert_eq!(status, 200, "{body}");
        let tree: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(tree["schema"], "agent-doc-queue-tree-v1");
        let nodes = tree["nodes"].as_array().unwrap();
        let follow = nodes
            .iter()
            .find(|node| node["text"].as_str().unwrap_or("").contains("keep it"))
            .unwrap();
        assert_eq!(follow["queue_id"], "queue-id:q-aaaaaaaaaaaa");
        let yes = nodes
            .iter()
            .find(|node| node["text"].as_str() == Some("yes"))
            .unwrap();
        assert_eq!(yes["queue_id"], "text:yes");
        assert!(tree["claims"].as_array().unwrap().is_empty());
        assert!(tree.get("claims_error").is_none());

        let (status, _, body) =
            browser_request(&state, "GET", "/api/queue-tree/%2Fetc%2Fpasswd", None, "");
        assert_eq!(status, 404, "{body}");

        let (status, _, body) = browser_request(&state, "GET", "/fleet", None, "");
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("/api/fleet"));
        let (status, _, body) = browser_request(&state, "GET", "/tree", None, "");
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("/api/queue-tree/"));
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

    // -------- `#servesavebypass`: POST /save routes through the authority --------

    const SERVE_SAVE_DISK: &str = concat!(
        "---\n",
        "agent_doc_session: serve-save\n",
        "agent_doc_format: template\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "on disk\n",
        "<!-- /agent:exchange -->\n"
    );

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// A temp git project holding one committed session document.
    fn serve_save_project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::write(root.join(".gitignore"), ".agent-doc/\n").unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, SERVE_SAVE_DISK).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.name", "serve test"]);
        git(
            &root,
            &["config", "user.email", "serve-test@example.invalid"],
        );
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "--no-verify", "-m", "seed"]);
        (dir, doc)
    }

    /// Serve one real HTTP request against the same revision cache as this browser.
    fn browser_request(
        state: &ServeState,
        method: &str,
        path: &str,
        base: Option<&str>,
        body: &str,
    ) -> (u16, Option<String>, String) {
        use std::io::Write;
        let state = state.clone();
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let handler = thread::spawn(move || {
            let request = server.recv().unwrap();
            handle_request(request, &state).unwrap();
        });
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let revision = base
            .map(|base| format!("X-Agent-Doc-Base-Revision: {base}\r\n"))
            .unwrap_or_default();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{revision}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        handler.join().unwrap();
        let (headers, payload) = response.split_once("\r\n\r\n").unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        let revision = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("etag")
                .then(|| value.trim().to_owned())
        });
        (status, revision, payload.to_owned())
    }

    fn browser_for(doc: &Path) -> ServeState {
        ServeState::new(Some(doc.to_path_buf()), ServeAuth::Disabled).unwrap()
    }

    fn browser_load(state: &ServeState) -> (String, String) {
        let (status, revision, content) = browser_request(state, "GET", "/doc", None, "");
        assert_eq!(status, 200, "{content}");
        (revision.unwrap(), content)
    }

    #[test]
    fn browser_prompt_persists_one_operator_edit_for_normal_admission() {
        use agent_doc_editor_surface_io::DocumentPromptSubmission;

        let (dir, doc) = serve_save_project();
        let head_before = git(dir.path(), &["rev-parse", "HEAD"]);
        let submit = || DocumentPromptSubmission {
            revision: agent_doc_hash::content_hash(SERVE_SAVE_DISK),
            target: "document".into(),
            draft: "Which steps remain?\nInclude the test status.".into(),
            request_id: "0123456789abcdef0123456789abcdef".into(),
        };
        let (content, active_cycle) = persist_browser_prompt(&doc, submit()).unwrap();
        assert!(!active_cycle);
        assert!(content.contains("> Which steps remain?\n> Include the test status."));
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
        assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head_before);
        assert_eq!(
            persist_browser_prompt(&doc, submit()).unwrap_err().status,
            409
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn browser_prompt_requires_opt_in_and_an_edit_bearer_header() {
        let (_dir, doc) = serve_save_project();
        let mut state = browser_for(&doc);
        assert_eq!(
            browser_request(&state, "POST", "/api/prompt", None, "{}").0,
            403
        );
        state.allow_run = true;
        state.auth = ServeAuth::Enabled {
            edit_token: "edit-secret".into(),
            read_only_token: Some("read-secret".into()),
        };
        for token in ["edit-secret", "read-secret"] {
            let path = format!("/api/prompt?token={token}");
            assert_eq!(browser_request(&state, "POST", &path, None, "{}").0, 403);
        }
        assert_eq!(std::fs::read_to_string(doc).unwrap(), SERVE_SAVE_DISK);
    }

    #[test]
    fn serve_post_save_writes_through_commit_boundary() {
        let (dir, doc) = serve_save_project();
        let root = dir.path();
        let head_before = git(root, &["rev-parse", "HEAD"]);
        let browser = browser_for(&doc);
        let (revision, initial) = browser_load(&browser);
        assert_eq!(initial, SERVE_SAVE_DISK);

        let unrelated = root.join("unrelated.txt");
        std::fs::write(&unrelated, "staged separately\n").unwrap();
        git(root, &["add", "unrelated.txt"]);
        let edited = format!("{initial}\n### Browser note (HEAD)\nbrowser edit\n");
        let (status, new_revision, payload) =
            browser_request(&browser, "POST", "/save", Some(&revision), &edited);

        assert_eq!(status, 200, "{payload}");
        assert_ne!(new_revision.as_deref(), Some(revision.as_str()));
        let json: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(json["commit_mode"], "git");
        assert_eq!(json["content"], edited);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), edited);
        assert_ne!(git(root, &["rev-parse", "HEAD"]), head_before);
        assert_eq!(git(root, &["show", "HEAD:session.md"]), edited.trim_end());
        assert_eq!(
            git(root, &["diff", "--cached", "--name-only"]),
            "unrelated.txt"
        );
    }

    #[test]
    fn serve_post_save_requires_a_known_revision() {
        let (dir, doc) = serve_save_project();
        let head_before = git(dir.path(), &["rev-parse", "HEAD"]);
        let browser = browser_for(&doc);
        assert_eq!(
            browser_request(&browser, "POST", "/save", None, "draft").0,
            428
        );
        assert_eq!(
            browser_request(&browser, "POST", "/save", Some("\"unknown\""), "draft").0,
            409
        );
        assert_eq!(std::fs::read_to_string(doc).unwrap(), SERVE_SAVE_DISK);
        assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head_before);
    }

    #[test]
    fn serve_post_save_merges_independent_edits_from_two_browser_tabs() {
        let (dir, doc) = serve_save_project();
        let browser = browser_for(&doc);
        let (revision, initial) = browser_load(&browser);
        let first = initial.replace("on disk", "tab one edit");
        let second = initial.replace(
            "<!-- /agent:exchange -->",
            "<!-- /agent:exchange -->\n\ntab two edit",
        );
        let wanted = second.replace("on disk", "tab one edit");
        let (status, _, payload) =
            browser_request(&browser, "POST", "/save", Some(&revision), &first);
        assert_eq!(status, 200, "{payload}");
        let (status, _, payload) =
            browser_request(&browser, "POST", "/save", Some(&revision), &second);
        assert_eq!(status, 200, "{payload}");
        let json: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(json["content"], wanted);
        assert_eq!(
            git(dir.path(), &["show", "HEAD:session.md"]),
            wanted.trim_end()
        );
    }

    #[test]
    fn serve_get_doc_returns_the_unsaved_editor_cut() {
        let (_dir, doc) = serve_save_project();
        let browser = browser_for(&doc);
        let (disk_revision, _) = browser_load(&browser);
        agent_doc_crdt_relay_io::seed_embedded_relay_for_file(&doc).unwrap();
        let mut editor = crate::sim_world::SimEditor::generic(&doc).unwrap();
        let operator_text = SERVE_SAVE_DISK.replace("on disk", "operator typed, unsaved");
        editor.type_unsaved(&operator_text).unwrap();

        let (live_revision, live) = browser_load(&browser);

        assert_eq!(live, operator_text);
        assert_ne!(live_revision, disk_revision);
        assert_eq!(std::fs::read_to_string(doc).unwrap(), SERVE_SAVE_DISK);
    }

    #[test]
    fn serve_post_save_merges_with_a_live_editor_and_waits_for_its_native_save() {
        let (dir, doc) = serve_save_project();
        let head_before = git(dir.path(), &["rev-parse", "HEAD"]);
        let browser = browser_for(&doc);
        let (revision, initial) = browser_load(&browser);
        agent_doc_crdt_relay_io::seed_embedded_relay_for_file(&doc).unwrap();
        let mut editor = crate::sim_world::SimEditor::generic(&doc).unwrap();
        let operator = initial.replace("on disk", "live operator text");
        editor.type_unsaved(&operator).unwrap();
        let browser_text = initial.replace(
            "<!-- /agent:exchange -->",
            "<!-- /agent:exchange -->\n\nbrowser text",
        );
        let expected = browser_text.replace("on disk", "live operator text");
        let client = thread::spawn(move || {
            browser_request(&browser, "POST", "/save", Some(&revision), &browser_text)
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut projected = false;
        while !client.is_finished() && std::time::Instant::now() < deadline {
            if editor.pull_peer_updates().unwrap() > 0 {
                assert_eq!(editor.resolve().unwrap().content, expected);
                assert_eq!(std::fs::read_to_string(&doc).unwrap(), initial);
                assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head_before);
                editor.save().unwrap();
                projected = true;
            } else {
                thread::sleep(Duration::from_millis(10));
            }
        }
        let (status, _, payload) = client.join().unwrap();
        assert!(projected, "{payload}");
        assert_eq!(status, 200, "{payload}");
        assert_eq!(
            git(dir.path(), &["show", "HEAD:session.md"]),
            expected.trim_end()
        );
    }

    #[test]
    fn serve_post_save_refuses_overlapping_editor_edits() {
        let (dir, doc) = serve_save_project();
        let head_before = git(dir.path(), &["rev-parse", "HEAD"]);
        let browser = browser_for(&doc);
        let (revision, initial) = browser_load(&browser);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&initial), Some(&initial)).unwrap();
        agent_doc_crdt_relay_io::seed_embedded_relay_for_file(&doc).unwrap();
        let mut editor = crate::sim_world::SimEditor::generic(&doc).unwrap();
        let operator_text = initial.replace("on disk", "operator typed mid-cycle");
        editor.type_unsaved(&operator_text).unwrap();
        let browser_text = initial.replace("on disk", "browser replacement");

        let (status, _, payload) =
            browser_request(&browser, "POST", "/save", Some(&revision), &browser_text);

        assert_eq!(status, 409, "{payload}");
        assert!(payload.contains("edits overlap"), "{payload}");
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), initial);
        assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head_before);
        assert_eq!(editor.resolve().unwrap().content, operator_text);
        assert!(active_cycle_in_scope(&doc).unwrap());
    }

    #[test]
    fn serve_commit_refuses_an_editor_edit_that_arrived_after_persistence() {
        let (dir, doc) = serve_save_project();
        let head_before = git(dir.path(), &["rev-parse", "HEAD"]);
        let merged = SERVE_SAVE_DISK.replace("on disk", "browser edit");
        std::fs::write(&doc, &merged).unwrap();
        agent_doc_crdt_relay_io::seed_embedded_relay_for_file(&doc).unwrap();
        let mut editor = crate::sim_world::SimEditor::generic(&doc).unwrap();
        let operator_text = merged.replace("browser edit", "newer unsaved editor text");
        editor.type_unsaved(&operator_text).unwrap();

        let error = git_commit_user_edit(
            dir.path(),
            &doc,
            &BrowserCommitFence {
                text: &merged,
                cycle: None,
            },
        )
        .unwrap_err();

        assert_eq!(error.status, 409);
        assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head_before);
        assert_eq!(editor.resolve().unwrap().content, operator_text);
        assert_eq!(std::fs::read_to_string(doc).unwrap(), merged);
    }
}

//! Supplemental Markdown LSP sidecar for the Zed agent-doc extension.
//!
//! The server is deliberately registered for Zed's existing `Markdown` language
//! rather than defining a competing `.md` language. A buffer enters agent-doc
//! mode only while its current text has parseable frontmatter with a non-empty
//! `agent_doc_session`. Every other Markdown buffer is a strict no-op.

mod handoff;
mod input;
mod queue_tree_view;
mod tree_status_action;

use agent_doc_document_realtime::crdt_relay::delivery_history_lineage_mismatch;
use agent_doc_document_realtime::editor_contract::{
    DOCUMENT_ONLY_LAYOUT_AUTHORITY, ZED_EDITOR_KIND, ZED_LSP_CAPABILITIES,
};
use agent_doc_merge::crdt_sync::{DivergentHistory, ReplicaState};
use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use handoff::{BinaryObservation, BinaryWatch, HandoffDocument, HandoffState};
use input::{LspInput, ReadOutcome, take_frame};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PULL_INTERVAL: Duration = Duration::from_millis(250);

/// `#divergentapplyreregister`: minimum gap between immediate re-registers of
/// one document after its replica refused a delivery as a divergent CRDT
/// history (FFI -4). One re-register installs a full bootstrap in the
/// controller's current history, so a second refusal inside the window means
/// the controller keeps encoding against another one. Inside the window the
/// refusal falls back to the controller's redelivery budget and lineage
/// retirement. Mirrors the JetBrains and VS Code constant.
const DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS: u64 = 30_000;

/// Whether a divergent-history re-register is allowed now.
///
/// A clock rollback (`now_ms < last`) permits the re-register instead of
/// suppressing it until the wall clock catches the stamp.
fn divergent_history_reregister_due(now_ms: u64, last_attempt_ms: Option<u64>) -> bool {
    match last_attempt_ms {
        None => true,
        Some(last) => {
            now_ms < last
                || now_ms.saturating_sub(last) >= DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS
        }
    }
}

fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// What one pulled delivery did to the open replica.
enum PulledUpdateEffect {
    /// `#deliverylineagestamp`: another history, refused before decode.
    /// Re-register immediately; this is not the -4 window.
    LineageMismatch,
    /// FFI -4. The document is marked. The caller runs the guarded re-register
    /// and does not project this batch.
    DivergentHistory,
    /// Already marked. Nothing in this pull was applied.
    AlreadyDivergent,
    /// Same-history updates. `changed` is false when the pull had nothing to apply.
    Applied { changed: bool },
}

/// Apply one `replica_pull` body, or refuse it.
///
/// A replica already marked divergent is not touched, even when this pull is
/// empty: the mark outlives the refused delivery.
fn apply_pulled_updates(document: &mut OpenDocument, data: &Value) -> PulledUpdateEffect {
    if document.history_divergent {
        return PulledUpdateEffect::AlreadyDivergent;
    }
    let Some(updates) = data.get("updates").and_then(Value::as_array) else {
        return PulledUpdateEffect::Applied { changed: false };
    };
    let pull_history_lineage = data.get("history_lineage").and_then(Value::as_str);
    let mut changed = false;
    for update in updates {
        // `#deliverylineagestamp`: an update stamped with another CRDT history
        // than this replica's registration is refused before it is decoded,
        // even when every op id is new here.
        let stamped = update
            .get("history_lineage")
            .and_then(Value::as_str)
            .or(pull_history_lineage);
        if delivery_history_lineage_mismatch(document.history_lineage.as_deref(), stamped) {
            return PulledUpdateEffect::LineageMismatch;
        }
        let Some(origin) = update.get("origin").and_then(Value::as_u64) else {
            continue;
        };
        if origin == document.client_id {
            continue;
        }
        let Some(bytes) = update
            .get("update_b64")
            .and_then(Value::as_str)
            .and_then(|encoded| BASE64_STANDARD.decode(encoded).ok())
        else {
            continue;
        };
        match document.replica.apply_update_verified(&bytes) {
            Ok(()) => changed = true,
            // `#replicapositiondrift`: encoded against another CRDT history.
            // Leave the buffer alone. The caller re-registers, inside the
            // one-per-document window.
            Err(error) if error.is::<DivergentHistory>() => {
                document.history_divergent = true;
                return PulledUpdateEffect::DivergentHistory;
            }
            Err(_) => continue,
        }
    }
    PulledUpdateEffect::Applied { changed }
}

enum DivergentReregisterDecision {
    NotDivergent,
    /// An in-flight editor edit. Do not stamp the window.
    Deferred,
    /// Inside the window. Leave the member for controller lineage retirement.
    Suppressed {
        since_last_ms: u64,
    },
    /// Stamp `now_ms` and re-register from the visible buffer.
    Attempt {
        visible: String,
    },
}

fn divergent_reregister_decision(
    document: &OpenDocument,
    now_ms: u64,
    last_attempt_ms: Option<u64>,
) -> DivergentReregisterDecision {
    if !document.history_divergent {
        return DivergentReregisterDecision::NotDivergent;
    }
    // The shadow is the visible buffer. An in-flight `workspace/applyEdit`
    // means that buffer has not echoed yet, so the swap is not stable.
    if document.pending_apply_target.is_some() {
        return DivergentReregisterDecision::Deferred;
    }
    if !divergent_history_reregister_due(now_ms, last_attempt_ms) {
        return DivergentReregisterDecision::Suppressed {
            since_last_ms: now_ms.saturating_sub(last_attempt_ms.unwrap_or(now_ms)),
        };
    }
    DivergentReregisterDecision::Attempt {
        visible: document.shadow.clone(),
    }
}

/// Set by the Zed extension: the version in `editors/zed/extension.toml` of
/// the extension that launched this server.
pub const ZED_EXTENSION_VERSION_ENV: &str = "AGENT_DOC_ZED_EXTENSION_VERSION";

struct OpenDocument {
    uri: String,
    file: PathBuf,
    project_root: PathBuf,
    identity: String,
    client_id: u64,
    /// `#deliverylineagestamp`: the CRDT history the registration bootstrapped
    /// [`Self::replica`] from; `None` from a controller that predates the stamp.
    history_lineage: Option<String>,
    replica: ReplicaState,
    shadow: String,
    pending_apply_target: Option<String>,
    /// `#divergentapplyreregister`: a delivery was refused as another CRDT
    /// history (FFI -4). Later polls must not apply to this replica; they
    /// retry a guarded re-register instead. Outlives the refused delivery.
    history_divergent: bool,
}

pub fn run() -> Result<()> {
    // The probe child of a binary handoff: validate the exact file the
    // running server wrote, answer, and never touch stdin or the controller.
    if let Some(path) = std::env::var_os(handoff::HANDOFF_CHECK_ENV) {
        let verdict = handoff::check_handoff_file(Path::new(&path))?;
        println!("{verdict}");
        return Ok(());
    }
    let resumed = take_resumed_session();
    let binary_watch = BinaryWatch::for_current_exe(Instant::now());
    #[cfg(unix)]
    let input = input::StdinFd;
    #[cfg(not(unix))]
    let input = input::ChunkInput::spawn_reader(std::io::stdin());
    let mut server = Server::new(std::io::stdout(), input);
    server.binary_watch = binary_watch;
    if let Some(session) = resumed {
        server.resume(session);
    }
    server.serve()
}

/// Adopt the session a previous image of this process handed over, if any.
///
/// Losing it is recoverable, not fatal: Zed re-sends each open buffer on its
/// next change, and `did_change` attaches a buffer this server does not know.
fn take_resumed_session() -> Option<handoff::ResumedSession> {
    let path = std::env::var_os(handoff::HANDOFF_ENV)?;
    // SAFETY: still single-threaded; nothing else reads the environment yet.
    unsafe { std::env::remove_var(handoff::HANDOFF_ENV) };
    let path = PathBuf::from(path);
    let state = handoff::read_handoff_file(&path);
    let _ = std::fs::remove_file(&path);
    match state.and_then(|state| {
        let from_version = state.from_version.clone();
        state
            .restore(Some(std::process::id()))
            .map(|session| (from_version, session))
    }) {
        Ok((from_version, session)) => {
            eprintln!(
                "zed_lsp_binary_handoff status=resumed pid={} from_version={} to_version={} documents={} pending_input_bytes={}",
                std::process::id(),
                from_version,
                env!("CARGO_PKG_VERSION"),
                session.documents.len(),
                session.pending_input.len(),
            );
            Some(session)
        }
        Err(error) => {
            eprintln!(
                "zed_lsp_binary_handoff status=resume_failed pid={} error={error:#} action=serve_fresh_and_reattach_on_next_change",
                std::process::id(),
            );
            None
        }
    }
}

struct Server<W, I> {
    writer: W,
    input: I,
    /// Bytes read from Zed that do not yet form a complete frame.
    pending_input: Vec<u8>,
    documents: HashMap<String, OpenDocument>,
    /// `#queuetreegutter`: the latest full text of each agent-doc-mode buffer,
    /// for queue tree folding ranges and inlay hints. Kept apart from
    /// `documents` so the view never depends on controller registration.
    queue_views: HashMap<String, String>,
    next_request_id: u64,
    shutdown_requested: bool,
    binary_watch: Option<BinaryWatch>,
    /// `#divergentapplyreregister`: last divergent-history re-register attempt
    /// per document URI. Survives the detach and attach that replace the
    /// document, and is carried through binary handoff.
    divergent_history_reregister_at_ms: HashMap<String, u64>,
}

impl<W: Write, I: LspInput> Server<W, I> {
    fn new(writer: W, input: I) -> Self {
        Self {
            writer,
            input,
            pending_input: Vec::new(),
            documents: HashMap::new(),
            queue_views: HashMap::new(),
            next_request_id: 1,
            shutdown_requested: false,
            binary_watch: None,
            divergent_history_reregister_at_ms: HashMap::new(),
        }
    }

    fn resume(&mut self, session: handoff::ResumedSession) {
        self.next_request_id = session.next_request_id;
        self.shutdown_requested = session.shutdown_requested;
        // Carried bytes precede anything Zed writes after the handoff.
        let mut pending = session.pending_input;
        pending.append(&mut self.pending_input);
        self.pending_input = pending;
        for document in session.documents {
            self.documents.insert(document.uri.clone(), document);
        }
        for stamp in session.divergent_history_reregister_at_ms {
            self.divergent_history_reregister_at_ms
                .insert(stamp.uri, stamp.at_ms);
        }
    }

    fn serve(mut self) -> Result<()> {
        'serve: loop {
            while let Some(message) = take_frame(&mut self.pending_input)? {
                if self.handle_message(message)? {
                    break 'serve;
                }
            }
            match self
                .input
                .read_available(&mut self.pending_input, PULL_INTERVAL)?
            {
                ReadOutcome::Data => {}
                ReadOutcome::Timeout => {
                    self.poll_remote_updates();
                    self.pick_up_replaced_binary(Instant::now());
                }
                ReadOutcome::Eof => break,
            }
        }
        self.detach_all();
        Ok(())
    }

    /// Hand the session to a replaced launch binary (`editors/SPEC.md`
    /// section 13). Runs only between handled messages, so every byte Zed has
    /// written is in the pipe or in `pending_input`.
    fn pick_up_replaced_binary(&mut self, now: Instant) {
        if self.shutdown_requested || !self.input.handoff_safe() {
            return;
        }
        let Some(watch) = self.binary_watch.as_mut() else {
            return;
        };
        let BinaryObservation::Replaced(identity) = watch.observe(now) else {
            return;
        };
        let candidate = watch.path().to_path_buf();
        let state = self.handoff_state();
        let refusal = match handoff::write_handoff_file(&state) {
            Err(error) => error,
            Ok(path) => match handoff::probe_replacement(&candidate, &path) {
                Err(error) => {
                    let _ = std::fs::remove_file(&path);
                    error
                }
                Ok(()) => {
                    eprintln!(
                        "zed_lsp_binary_handoff status=exec pid={} binary={} documents={} pending_input_bytes={}",
                        std::process::id(),
                        candidate.display(),
                        state.documents.len(),
                        self.pending_input.len(),
                    );
                    let error = handoff::exec_replacement(&candidate, &path);
                    let _ = std::fs::remove_file(&path);
                    error
                }
            },
        };
        if let Some(watch) = self.binary_watch.as_mut() {
            watch.refuse(identity);
        }
        eprintln!(
            "zed_lsp_binary_handoff status=refused pid={} binary={} error={refusal:#} action=restart_prompt",
            std::process::id(),
            candidate.display(),
        );
        let _ = self.send(restart_prompt(&candidate, &refusal));
    }

    fn handoff_state(&self) -> HandoffState {
        HandoffState {
            schema: handoff::HANDOFF_SCHEMA.to_string(),
            from_pid: std::process::id(),
            from_version: env!("CARGO_PKG_VERSION").to_string(),
            next_request_id: self.next_request_id,
            shutdown_requested: self.shutdown_requested,
            pending_input_b64: BASE64_STANDARD.encode(&self.pending_input),
            documents: self
                .documents
                .values()
                .map(HandoffDocument::capture)
                .collect(),
            divergent_history_reregister_at_ms: self
                .divergent_history_reregister_at_ms
                .iter()
                .map(|(uri, at_ms)| handoff::DivergentReregisterStamp {
                    uri: uri.clone(),
                    at_ms: *at_ms,
                })
                .collect(),
        }
    }

    fn handle_message(&mut self, message: Value) -> Result<bool> {
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned();
        match method {
            Some("initialize") => {
                if let Some(id) = id {
                    self.respond(
                        id,
                        json!({
                            "capabilities": {
                                "textDocumentSync": {
                                    "openClose": true,
                                    "change": 1,
                                    "save": { "includeText": true }
                                },
                                "foldingRangeProvider": true,
                                "inlayHintProvider": true,
                                "codeActionProvider": {
                                    "codeActionKinds": ["source", "quickfix"]
                                },
                                "executeCommandProvider": {
                                    "commands": [tree_status_action::SHOW_TREE_STATUS_COMMAND]
                                }
                            },
                            "serverInfo": {
                                "name": "agent-doc-zed-lsp",
                                "version": env!("CARGO_PKG_VERSION")
                            }
                        }),
                    )?;
                }
            }
            Some("shutdown") => {
                self.shutdown_requested = true;
                if let Some(id) = id {
                    self.respond(id, Value::Null)?;
                }
            }
            Some("exit") => return Ok(true),
            Some("textDocument/didOpen") => {
                if let Some(params) = message.get("params") {
                    self.did_open(params);
                }
            }
            Some("textDocument/didChange") => {
                if let Some(params) = message.get("params") {
                    self.did_change(params);
                }
            }
            Some("textDocument/didSave") => {
                if let Some(params) = message.get("params") {
                    self.did_save(params);
                }
            }
            Some("textDocument/didClose") => {
                if let Some(uri) = message
                    .pointer("/params/textDocument/uri")
                    .and_then(Value::as_str)
                {
                    self.queue_views.remove(uri);
                    self.detach(uri);
                }
            }
            Some("textDocument/foldingRange") => {
                if let Some(id) = id {
                    let result = self
                        .queue_view_text(&message)
                        .map(queue_tree_view::folding_ranges)
                        .unwrap_or(Value::Null);
                    self.respond(id, result)?;
                }
            }
            Some("textDocument/inlayHint") => {
                if let Some(id) = id {
                    let line = |pointer: &str| {
                        message
                            .pointer(pointer)
                            .and_then(Value::as_u64)
                            .map(|line| line as usize)
                    };
                    let start = line("/params/range/start/line").unwrap_or(0);
                    let end = line("/params/range/end/line").unwrap_or(usize::MAX);
                    let result = self
                        .queue_view_text(&message)
                        .map(|text| queue_tree_view::inlay_hints(text, start, end))
                        .unwrap_or(Value::Null);
                    self.respond(id, result)?;
                }
            }
            Some("textDocument/codeAction") => {
                if let Some(id) = id {
                    let caret = message
                        .pointer("/params/range/start/line")
                        .and_then(Value::as_u64)
                        .map(|line| line as usize);
                    let only = message
                        .pointer("/params/context/only")
                        .and_then(Value::as_array)
                        .map(|kinds| {
                            kinds
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        });
                    let trigger_kind = message
                        .pointer("/params/context/triggerKind")
                        .and_then(Value::as_u64);
                    let uri = message
                        .pointer("/params/textDocument/uri")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let result = match (self.queue_view_text(&message), caret) {
                        (Some(text), Some(caret)) => tree_status_action::code_actions(
                            uri,
                            text,
                            caret,
                            only.as_deref(),
                            trigger_kind,
                        ),
                        _ => Value::Null,
                    };
                    self.respond(id, result)?;
                }
            }
            Some("workspace/executeCommand") => {
                let command = message.pointer("/params/command").and_then(Value::as_str);
                if command == Some(tree_status_action::SHOW_TREE_STATUS_COMMAND)
                    && let Some(notification) =
                        tree_status_action::show_message(message.pointer("/params/arguments"))
                {
                    self.send(notification)?;
                    if let Some(id) = id {
                        self.respond(id, Value::Null)?;
                    }
                } else if let Some(id) = id {
                    self.error(id, -32602, "unknown agent-doc command")?;
                }
            }
            Some(_) if id.is_some() => {
                self.error(id.unwrap_or(Value::Null), -32601, "method not found")?;
            }
            _ => {}
        }
        Ok(self.shutdown_requested && method == Some("exit"))
    }

    /// The agent-doc-mode text a queue view request names, or None (a strict
    /// no-op answer) for any other Markdown buffer.
    fn queue_view_text(&self, message: &Value) -> Option<&str> {
        let uri = message
            .pointer("/params/textDocument/uri")
            .and_then(Value::as_str)?;
        self.queue_views.get(uri).map(String::as_str).or_else(|| {
            self.documents
                .get(uri)
                .map(|document| document.shadow.as_str())
        })
    }

    fn track_queue_view(&mut self, uri: &str, text: &str) {
        if is_agent_doc_markdown(text) {
            self.queue_views.insert(uri.to_string(), text.to_string());
        } else {
            self.queue_views.remove(uri);
        }
    }

    fn did_open(&mut self, params: &Value) {
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return;
        };
        let Some(text) = params.pointer("/textDocument/text").and_then(Value::as_str) else {
            return;
        };
        self.track_queue_view(uri, text);
        self.reconcile_mode(uri, text);
    }

    fn did_change(&mut self, params: &Value) {
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return;
        };
        let Some(text) = params
            .pointer("/contentChanges/0/text")
            .and_then(Value::as_str)
        else {
            return;
        };
        self.track_queue_view(uri, text);

        if !is_agent_doc_markdown(text) {
            self.detach(uri);
            return;
        }
        if !self.documents.contains_key(uri) {
            self.attach(uri, text);
            return;
        }

        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        if document.history_divergent {
            // The replica's op ids belong to a retired history. Publishing a
            // local edit from it would encode the operator's text there.
            // Keep the visible buffer; the guarded re-register bootstraps it.
            document.pending_apply_target = None;
            document.shadow = text.to_string();
            return;
        }
        document.pending_apply_target = None;
        if document.replica.text() == text {
            document.shadow = text.to_string();
            publish_replica_projection(document, false);
            return;
        }

        let before = document.replica.state_vector();
        let replica_text = document.replica.text();
        let Some(delta) = minimal_text_delta(&replica_text, text) else {
            return;
        };
        document
            .replica
            .apply_local_edit(delta.offset, delta.delete_len, &delta.insert);
        document.shadow = text.to_string();
        if let Ok(update) = document.replica.diff(&before)
            && !update.is_empty()
        {
            let _ = controller_replica_request(
                &document.project_root,
                &document.file,
                "replica_update",
                &document.identity,
                json!({ "update_b64": BASE64_STANDARD.encode(update) }),
            );
        }
        publish_replica_projection(document, false);
    }

    fn did_save(&mut self, params: &Value) {
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return;
        };
        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        if let Some(text) = params.get("text").and_then(Value::as_str)
            && text != document.shadow
        {
            return;
        }
        if document.replica.text() != document.shadow {
            return;
        }
        let Ok(disk_text) = std::fs::read_to_string(&document.file) else {
            return;
        };
        if disk_text != document.shadow {
            return;
        }
        publish_replica_projection(document, true);
    }

    fn reconcile_mode(&mut self, uri: &str, text: &str) {
        if is_agent_doc_markdown(text) {
            if !self.documents.contains_key(uri) {
                self.attach(uri, text);
            }
        } else {
            self.detach(uri);
        }
    }

    fn attach(&mut self, uri: &str, editor_text: &str) {
        let Ok(file) = file_uri_to_path(uri) else {
            return;
        };
        let file = file.canonicalize().unwrap_or(file);
        let Ok(project_root) =
            agent_doc_project_root_io::project_root_for_target_or_cwd(None, Some(&file))
        else {
            return;
        };
        let identity = format!(
            "zed-lsp:{}:{}",
            std::process::id(),
            agent_doc_hash::short_content_hash(uri)
        );
        let Ok(data) = controller_replica_request(
            &project_root,
            &file,
            "replica_register",
            &identity,
            registration_fields(),
        ) else {
            return;
        };
        let Some(client_id) = data.get("client_id").and_then(Value::as_u64) else {
            return;
        };
        let Some(bootstrap) = data
            .get("bootstrap_b64")
            .and_then(Value::as_str)
            .and_then(|encoded| BASE64_STANDARD.decode(encoded).ok())
        else {
            return;
        };
        let replica = ReplicaState::new(client_id);
        if replica.apply_update(&bootstrap).is_err() {
            return;
        }
        let history_lineage = data
            .get("history_lineage")
            .and_then(Value::as_str)
            .map(str::to_string);
        let canonical = replica.text();
        let mut document = OpenDocument {
            uri: uri.to_string(),
            file,
            project_root,
            identity,
            client_id,
            history_lineage,
            replica,
            shadow: editor_text.to_string(),
            pending_apply_target: None,
            history_divergent: false,
        };
        publish_replica_projection(&document, false);

        // Registration bootstraps from the controller-owned canonical revision.
        // A divergent opening buffer is a downstream delivery target, never an
        // upstream whole-document replacement.
        let opening_projection = (canonical != editor_text).then(|| {
            document.pending_apply_target = Some(canonical.clone());
            json!({
                "edit": {
                    "changes": {
                        uri: [{
                            "range": full_document_range(editor_text),
                            "newText": canonical
                        }]
                    }
                },
                "label": "agent-doc canonical projection"
            })
        });
        self.documents.insert(uri.to_string(), document);
        if let Some(edit) = opening_projection {
            let request_id = self.next_request_id;
            self.next_request_id += 1;
            let _ = self.send(json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "workspace/applyEdit",
                "params": edit
            }));
        }
    }

    fn detach(&mut self, uri: &str) {
        let Some(document) = self.documents.remove(uri) else {
            return;
        };
        let _ = controller_replica_request(
            &document.project_root,
            &document.file,
            "replica_deregister",
            &document.identity,
            json!({ "editor_pid": std::process::id() }),
        );
    }

    fn detach_all(&mut self) {
        let uris: Vec<String> = self.documents.keys().cloned().collect();
        for uri in uris {
            self.detach(&uri);
        }
    }

    fn poll_remote_updates(&mut self) {
        let uris: Vec<String> = self.documents.keys().cloned().collect();
        for uri in uris {
            let data = {
                let Some(document) = self.documents.get(&uri) else {
                    continue;
                };
                match controller_replica_request(
                    &document.project_root,
                    &document.file,
                    "replica_pull",
                    &document.identity,
                    Value::Null,
                ) {
                    Ok(data) => data,
                    Err(_) => continue,
                }
            };
            if data.get("kind").and_then(Value::as_str) == Some("replace") {
                // A replace-capable recovery is fenced through a fresh registration
                // so the local node receives the controller's current CRDT lineage.
                // This is not the -4 window: the controller asked for the replace.
                let Some(visible) = self
                    .documents
                    .get(&uri)
                    .map(|document| document.shadow.clone())
                else {
                    continue;
                };
                self.detach(&uri);
                self.attach(&uri, &visible);
                continue;
            }
            let effect = {
                let Some(document) = self.documents.get_mut(&uri) else {
                    continue;
                };
                apply_pulled_updates(document, &data)
            };
            match effect {
                PulledUpdateEffect::LineageMismatch => {
                    let Some(visible) = self
                        .documents
                        .get(&uri)
                        .map(|document| document.shadow.clone())
                    else {
                        continue;
                    };
                    self.detach(&uri);
                    self.attach(&uri, &visible);
                }
                PulledUpdateEffect::DivergentHistory | PulledUpdateEffect::AlreadyDivergent => {
                    // Never project a divergent replica, including updates applied
                    // earlier in this batch. The mark outlives an empty pull.
                    self.reregister_after_divergent_history(&uri);
                }
                PulledUpdateEffect::Applied { changed: false } => {}
                PulledUpdateEffect::Applied { changed: true } => {
                    let Some(document) = self.documents.get_mut(&uri) else {
                        continue;
                    };
                    let target = document.replica.text();
                    if target == document.shadow {
                        publish_replica_projection(document, false);
                        continue;
                    }
                    if document.pending_apply_target.as_deref() == Some(&target) {
                        continue;
                    }
                    let range = full_document_range(&document.shadow);
                    let edit = json!({
                        "edit": {
                            "changes": {
                                document.uri.clone(): [{
                                    "range": range,
                                    "newText": target
                                }]
                            }
                        },
                        "label": "agent-doc realtime update"
                    });
                    let request_id = self.next_request_id;
                    self.next_request_id += 1;
                    document.pending_apply_target = Some(target);
                    let _ = self.send(json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "method": "workspace/applyEdit",
                        "params": edit
                    }));
                }
            }
        }
    }

    /// `#divergentapplyreregister`: re-register from the visible buffer after
    /// FFI -4, at most once per document per 30 s. The window is stamped only
    /// when a registration is attempted.
    fn reregister_after_divergent_history(&mut self, uri: &str) -> bool {
        self.reregister_after_divergent_history_at(uri, unix_epoch_ms())
    }

    fn reregister_after_divergent_history_at(&mut self, uri: &str, now_ms: u64) -> bool {
        let Some(document) = self.documents.get(uri) else {
            return false;
        };
        let file_name = document
            .file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(uri)
            .to_string();
        let decision = divergent_reregister_decision(
            document,
            now_ms,
            self.divergent_history_reregister_at_ms.get(uri).copied(),
        );
        match decision {
            DivergentReregisterDecision::NotDivergent => false,
            DivergentReregisterDecision::Deferred => {
                eprintln!(
                    "[crdt-replica] divergent-history re-register deferred for {file_name}; \
                     reason=editor_not_stable (#divergentapplyreregister)"
                );
                false
            }
            DivergentReregisterDecision::Suppressed { since_last_ms } => {
                eprintln!(
                    "[crdt-replica] divergent-history re-register suppressed for {file_name}; \
                     since_last_ms={since_last_ms} \
                     min_interval_ms={DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS} \
                     fallback=controller_lineage_retirement (#divergentapplyreregister)"
                );
                false
            }
            DivergentReregisterDecision::Attempt { visible } => {
                self.divergent_history_reregister_at_ms
                    .insert(uri.to_string(), now_ms);
                self.detach(uri);
                self.attach(uri, &visible);
                if self.documents.contains_key(uri) {
                    eprintln!(
                        "[crdt-replica] re-registered {file_name} from the buffer after a \
                         divergent-history delivery (#divergentapplyreregister)"
                    );
                    true
                } else {
                    eprintln!(
                        "[crdt-replica] divergent-history re-register did not commit for {file_name}; \
                         reason=swap_refused (#divergentapplyreregister)"
                    );
                    false
                }
            }
        }
    }

    fn respond(&mut self, id: Value, result: Value) -> Result<()> {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    fn error(&mut self, id: Value, code: i64, message: &str) -> Result<()> {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        }))
    }

    fn send(&mut self, message: Value) -> Result<()> {
        write_lsp_message(&mut self.writer, &message)
    }
}

/// The `replica_register` payload: process identity plus the editor kind,
/// generation, capabilities, and layout authority (`editors/SPEC.md`
/// section 13).
///
/// `editor_version` is the Zed extension generation the controller compares
/// with `editors/zed/extension.toml`; `editor_binary_version` is this image.
/// The capability list is the `zed` column of `editors/plugin-parity.tsv`
/// (every Zed capability is staged today), and Zed exposes no layout, so it
/// is always routed by document only.
fn registration_fields() -> Value {
    let extension_version = std::env::var(ZED_EXTENSION_VERSION_ENV)
        .ok()
        .map(|version| version.trim().to_string())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    json!({
        "editor_pid": std::process::id(),
        "editor_kind": ZED_EDITOR_KIND,
        "editor_version": extension_version,
        "editor_binary_version": env!("CARGO_PKG_VERSION"),
        "capabilities": ZED_LSP_CAPABILITIES,
        "layout_authority": DOCUMENT_ONLY_LAYOUT_AUTHORITY,
    })
}

/// `window/showMessage` asking the operator to restart the language server
/// when the replaced binary could not take the session over in place.
fn restart_prompt(binary: &Path, refusal: &anyhow::Error) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "window/showMessage",
        "params": {
            "type": 2,
            "message": format!(
                "agent-doc: {} was replaced, but this language server could not hand its session to the new binary ({refusal:#}). Run `editor: restart language server` to load it.",
                binary.display()
            )
        }
    })
}

fn publish_replica_projection(document: &OpenDocument, disk_persisted: bool) {
    let _ = controller_replica_request(
        &document.project_root,
        &document.file,
        "replica_projection",
        &document.identity,
        json!({
            "content_hash": agent_doc_hash::content_hash(&document.shadow),
            "disk_persisted": disk_persisted
        }),
    );
}

fn controller_replica_request(
    project_root: &Path,
    file: &Path,
    method: &str,
    identity: &str,
    fields: Value,
) -> Result<Value> {
    let mut payload = fields.as_object().cloned().unwrap_or_default();
    payload.insert("method".to_string(), Value::String(method.to_string()));
    payload.insert("identity".to_string(), Value::String(identity.to_string()));
    payload.insert("source".to_string(), Value::String("zed_lsp".to_string()));
    agent_doc_controller_io::project_controller::request_crdt_replica(
        project_root,
        file,
        Value::Object(payload),
    )
}

pub fn is_agent_doc_markdown(content: &str) -> bool {
    agent_doc_frontmatter::parse(content)
        .ok()
        .and_then(|(frontmatter, _)| frontmatter.session)
        .is_some_and(|session| !session.trim().is_empty())
}

fn file_uri_to_path(uri: &str) -> Result<PathBuf> {
    let encoded = uri
        .strip_prefix("file://")
        .ok_or_else(|| anyhow!("only file:// URIs are supported"))?;
    let decoded = percent_decode(encoded)?;
    #[cfg(windows)]
    let decoded = decoded.strip_prefix('/').unwrap_or(&decoded).to_string();
    Ok(PathBuf::from(decoded))
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes
                .get(index + 1..index + 3)
                .ok_or_else(|| anyhow!("truncated percent escape"))?;
            let pair = std::str::from_utf8(pair)?;
            decoded.push(u8::from_str_radix(pair, 16).context("invalid percent escape")?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).context("file URI is not UTF-8")
}

fn full_document_range(text: &str) -> Value {
    let mut lines = text.split('\n');
    let mut line = 0u64;
    let mut last = lines.next().unwrap_or_default();
    for value in lines {
        line += 1;
        last = value;
    }
    let character = last.encode_utf16().count() as u64;
    json!({
        "start": { "line": 0, "character": 0 },
        "end": { "line": line, "character": character }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextDelta {
    offset: u32,
    delete_len: u32,
    insert: String,
}

/// Project one full-sync LSP observation into the smallest contiguous
/// code-point edit. This keeps Zed at parity with the editor adapters that
/// publish operator deltas and never recover by adopting a whole buffer.
fn minimal_text_delta(before: &str, after: &str) -> Option<TextDelta> {
    if before == after {
        return None;
    }
    let before_chars: Vec<char> = before.chars().collect();
    let after_chars: Vec<char> = after.chars().collect();
    let mut prefix = 0usize;
    while prefix < before_chars.len()
        && prefix < after_chars.len()
        && before_chars[prefix] == after_chars[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while prefix + suffix < before_chars.len()
        && prefix + suffix < after_chars.len()
        && before_chars[before_chars.len() - 1 - suffix]
            == after_chars[after_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }
    Some(TextDelta {
        offset: prefix.min(u32::MAX as usize) as u32,
        delete_len: (before_chars.len() - prefix - suffix).min(u32::MAX as usize) as u32,
        insert: after_chars[prefix..after_chars.len() - suffix]
            .iter()
            .collect(),
    })
}

fn write_lsp_message(writer: &mut impl Write, message: &Value) -> Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_document(uri: &str, text: &str) -> OpenDocument {
        OpenDocument {
            uri: uri.to_string(),
            file: PathBuf::from("/tmp/session.md"),
            project_root: PathBuf::from("/tmp"),
            identity: format!("zed-lsp:{}:test", std::process::id()),
            client_id: 7,
            history_lineage: Some("history-a".into()),
            replica: ReplicaState::from_text(7, text),
            shadow: text.to_string(),
            pending_apply_target: None,
            history_divergent: false,
        }
    }

    /// A controller rebuilt from different text under the same canonical client
    /// id, so its snapshot restates stored op ids with other characters.
    fn fresh_epoch_snapshot(editor_text: &str) -> (ReplicaState, String) {
        let epoch_a = ReplicaState::new(1);
        epoch_a.apply_local_edit(0, 0, editor_text);
        let editor = ReplicaState::from_encoded(7, &epoch_a.encode_state()).unwrap();
        let epoch_b = ReplicaState::new(1);
        epoch_b.apply_local_edit(0, 0, &format!("other {editor_text}"));
        let encoded = BASE64_STANDARD.encode(epoch_b.encode_state());
        (editor, encoded)
    }

    fn same_history_peer_update(editor: &ReplicaState) -> String {
        let peer = ReplicaState::from_encoded(1, &editor.encode_state()).unwrap();
        let before = peer.state_vector();
        peer.apply_local_edit(0, 0, "Z");
        BASE64_STANDARD.encode(peer.diff(&before).unwrap())
    }

    #[test]
    fn the_re_register_loop_guard_allows_one_attempt_per_window() {
        assert!(divergent_history_reregister_due(100_000, None));
        assert!(!divergent_history_reregister_due(100_000, Some(100_000)));
        assert!(!divergent_history_reregister_due(
            100_000 + DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS - 1,
            Some(100_000),
        ));
        assert!(divergent_history_reregister_due(
            100_000 + DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS,
            Some(100_000),
        ));
    }

    #[test]
    fn a_clock_rollback_permits_the_re_register_instead_of_suppressing_it() {
        assert!(divergent_history_reregister_due(90_000, Some(100_000)));
    }

    #[test]
    fn the_decision_attempts_once_per_window_from_the_visible_buffer() {
        let mut document = sample_document("file:///a.md", "alpha\n");
        document.history_divergent = true;
        document.shadow = "visible\n".into();
        assert!(matches!(
            divergent_reregister_decision(&document, 1_000, None),
            DivergentReregisterDecision::Attempt { visible } if visible == "visible\n"
        ));

        document.pending_apply_target = Some("pending".into());
        assert!(matches!(
            divergent_reregister_decision(&document, 1_000, None),
            DivergentReregisterDecision::Deferred
        ));

        document.pending_apply_target = None;
        assert!(matches!(
            divergent_reregister_decision(
                &document,
                1_000 + DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS - 1,
                Some(1_000),
            ),
            DivergentReregisterDecision::Suppressed { .. }
        ));
        assert!(matches!(
            divergent_reregister_decision(
                &document,
                1_000 + DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS,
                Some(1_000),
            ),
            DivergentReregisterDecision::Attempt { .. }
        ));
        assert!(matches!(
            divergent_reregister_decision(&document, 500, Some(1_000)),
            DivergentReregisterDecision::Attempt { .. }
        ));
    }

    #[test]
    fn a_divergent_delivery_is_refused_whole_and_later_pulls_are_not_applied() {
        let (editor, bad) = fresh_epoch_snapshot("alpha\n");
        let before = editor.text();
        let mut document = sample_document("file:///a.md", "alpha\n");
        document.replica = editor;
        let effect = apply_pulled_updates(
            &mut document,
            &json!({ "updates": [{ "origin": 1_u64, "update_b64": bad }] }),
        );
        assert!(matches!(effect, PulledUpdateEffect::DivergentHistory));
        assert!(document.history_divergent);
        assert_eq!(document.replica.text(), before);

        let peer = same_history_peer_update(&document.replica);
        let effect = apply_pulled_updates(
            &mut document,
            &json!({ "updates": [{ "origin": 1_u64, "update_b64": peer }] }),
        );
        assert!(matches!(effect, PulledUpdateEffect::AlreadyDivergent));
        assert_eq!(
            document.replica.text(),
            before,
            "a replica known to hold another history must not take a later delivery"
        );

        let effect = apply_pulled_updates(&mut document, &json!({ "updates": [] }));
        assert!(
            matches!(effect, PulledUpdateEffect::AlreadyDivergent),
            "the mark outlives the refused delivery"
        );
    }

    #[test]
    fn an_ordinary_apply_failure_is_not_a_divergent_history() {
        let mut document = sample_document("file:///a.md", "alpha\n");
        let before = document.replica.text();
        let garbage = BASE64_STANDARD.encode(b"not-a-crdt-update");
        let effect = apply_pulled_updates(
            &mut document,
            &json!({ "updates": [{ "origin": 1_u64, "update_b64": garbage }] }),
        );
        assert!(matches!(
            effect,
            PulledUpdateEffect::Applied { changed: false }
        ));
        assert!(!document.history_divergent);
        assert_eq!(document.replica.text(), before);
    }

    #[test]
    fn a_same_history_peer_update_still_applies() {
        let mut document = sample_document("file:///a.md", "alpha\n");
        let before = document.replica.text();
        let peer = same_history_peer_update(&document.replica);
        let effect = apply_pulled_updates(
            &mut document,
            &json!({ "updates": [{ "origin": 1_u64, "update_b64": peer }] }),
        );
        assert!(matches!(
            effect,
            PulledUpdateEffect::Applied { changed: true }
        ));
        assert!(!document.history_divergent);
        assert_ne!(document.replica.text(), before);
    }

    #[test]
    fn a_history_lineage_mismatch_is_not_the_divergent_window() {
        let mut document = sample_document("file:///a.md", "alpha\n");
        let before = document.replica.text();
        let peer = same_history_peer_update(&document.replica);
        let effect = apply_pulled_updates(
            &mut document,
            &json!({
                "history_lineage": "history-b",
                "updates": [{
                    "origin": 1_u64,
                    "history_lineage": "history-b",
                    "update_b64": peer
                }]
            }),
        );
        assert!(matches!(effect, PulledUpdateEffect::LineageMismatch));
        assert!(!document.history_divergent);
        assert_eq!(document.replica.text(), before);
    }

    #[test]
    fn a_second_refusal_inside_the_window_does_not_reregister() {
        let (mut server, _sender) = scripted_server();
        let mut kept = sample_document("file:///kept.md", "alpha\n");
        kept.history_divergent = true;
        let mut other = sample_document("file:///other.md", "beta\n");
        other.history_divergent = true;
        let kept_uri = kept.uri.clone();
        let other_uri = other.uri.clone();
        server.documents.insert(kept_uri.clone(), kept);
        server.documents.insert(other_uri.clone(), other);
        let now_ms = 1_000_000_u64;
        server
            .divergent_history_reregister_at_ms
            .insert(kept_uri.clone(), now_ms);

        let attempted = server.reregister_after_divergent_history_at(
            &kept_uri,
            now_ms + DIVERGENT_HISTORY_REREGISTER_MIN_INTERVAL_MS - 1,
        );
        assert!(!attempted);
        assert!(
            server.documents.contains_key(&kept_uri),
            "inside the window the member stays attached"
        );
        assert_eq!(
            server.divergent_history_reregister_at_ms[&kept_uri], now_ms,
            "a suppressed refusal must not refresh the stamp"
        );
        let other_decision = divergent_reregister_decision(
            server.documents.get(&other_uri).unwrap(),
            now_ms,
            server
                .divergent_history_reregister_at_ms
                .get(&other_uri)
                .copied(),
        );
        assert!(
            matches!(other_decision, DivergentReregisterDecision::Attempt { .. }),
            "the window is one document, not the whole sidecar"
        );
    }

    #[test]
    fn an_inflight_apply_edit_defers_without_spending_the_window() {
        let (mut server, _sender) = scripted_server();
        let mut document = sample_document("file:///s.md", "alpha\n");
        document.history_divergent = true;
        document.pending_apply_target = Some("alpha\nremote\n".into());
        let uri = document.uri.clone();
        server.documents.insert(uri.clone(), document);

        let attempted = server.reregister_after_divergent_history_at(&uri, 1_000_000);
        assert!(!attempted);
        assert!(
            !server.divergent_history_reregister_at_ms.contains_key(&uri),
            "a buffer that moved must not spend the window"
        );
        assert!(server.documents.contains_key(&uri));
    }

    #[test]
    fn a_local_edit_on_a_divergent_replica_updates_the_buffer_and_does_not_publish() {
        let (mut server, _sender) = scripted_server();
        let mut document =
            sample_document("file:///s.md", "---\nagent_doc_session: s\n---\nbody\n");
        document.history_divergent = true;
        let before = document.replica.text();
        server.documents.insert(document.uri.clone(), document);
        server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": "file:///s.md" },
                    "contentChanges": [{
                        "text": "---\nagent_doc_session: s\n---\nbody\ntyped\n"
                    }]
                }
            }))
            .unwrap();
        let document = server.documents.get("file:///s.md").unwrap();
        assert!(document.history_divergent);
        assert_eq!(document.replica.text(), before);
        assert!(document.shadow.ends_with("typed\n"));
        assert!(document.pending_apply_target.is_none());
        assert!(
            server.writer.is_empty(),
            "a retired history must not emit an editor edit"
        );
    }

    #[test]
    fn resume_restores_the_divergent_reregister_stamp() {
        let (mut server, _sender) = scripted_server();
        let mut document = sample_document("file:///s.md", "alpha\n");
        document.history_divergent = true;
        server.resume(handoff::ResumedSession {
            next_request_id: 4,
            shutdown_requested: false,
            pending_input: Vec::new(),
            documents: vec![document],
            divergent_history_reregister_at_ms: vec![handoff::DivergentReregisterStamp {
                uri: "file:///s.md".into(),
                at_ms: 42,
            }],
        });
        assert!(server.documents["file:///s.md"].history_divergent);
        assert_eq!(
            server
                .divergent_history_reregister_at_ms
                .get("file:///s.md"),
            Some(&42)
        );
        let state = server.handoff_state();
        assert_eq!(state.divergent_history_reregister_at_ms.len(), 1);
        assert_eq!(state.divergent_history_reregister_at_ms[0].at_ms, 42);
        assert!(state.documents[0].history_divergent);
    }

    #[test]
    fn the_poll_routes_divergent_history_through_the_guard() {
        let source = include_str!("lib.rs");
        let poll = source
            .split("fn poll_remote_updates(&mut self) {")
            .nth(1)
            .and_then(|tail| tail.split("fn reregister_after_divergent_history(").next())
            .expect("poll body");
        assert!(
            poll.contains(
                "PulledUpdateEffect::DivergentHistory | PulledUpdateEffect::AlreadyDivergent"
            ),
            "a -4 mark must reach the guarded re-register"
        );
        assert!(poll.contains("self.reregister_after_divergent_history(&uri)"));
        assert!(
            poll.contains("PulledUpdateEffect::LineageMismatch"),
            "a history-lineage mismatch stays on its own immediate path"
        );
        let mismatch = poll
            .split("PulledUpdateEffect::LineageMismatch =>")
            .nth(1)
            .and_then(|tail| tail.split("PulledUpdateEffect::DivergentHistory").next())
            .expect("lineage arm");
        assert!(
            !mismatch.contains("reregister_after_divergent_history"),
            "the lineage mismatch must not spend the -4 window"
        );
        let reregister = source
            .split("fn reregister_after_divergent_history_at(")
            .nth(1)
            .and_then(|tail| tail.split("fn respond(&mut self").next())
            .expect("reregister body");
        let decision = reregister
            .find("divergent_reregister_decision(")
            .expect("decision");
        let stamp = reregister
            .find(".insert(uri.to_string(), now_ms)")
            .expect("stamp");
        let detach = reregister.find("self.detach(uri)").expect("detach");
        assert!(decision < stamp, "decide before stamping");
        assert!(
            stamp < detach,
            "the window is stamped when the registration is attempted"
        );
        let guard = source
            .split("fn divergent_reregister_decision(")
            .nth(1)
            .and_then(|tail| tail.split("pub fn is_agent_doc_markdown").next())
            .expect("decision body");
        let fence = guard.find("pending_apply_target.is_some()").expect("fence");
        let due = guard
            .find("divergent_history_reregister_due(")
            .expect("due");
        assert!(
            fence < due,
            "an in-flight edit must not reach the window check"
        );
    }

    #[test]
    fn agent_doc_mode_requires_a_nonempty_session_marker() {
        assert!(is_agent_doc_markdown(
            "---\nagent_doc_session: session-1\n---\n# Task\n"
        ));
        assert!(!is_agent_doc_markdown("# ordinary markdown\n"));
        assert!(!is_agent_doc_markdown(
            "---\ntitle: Notes\ntags: [work]\n---\n# Notes\n"
        ));
        assert!(!is_agent_doc_markdown(
            "---\nagent_doc_session: ''\n---\n# Not active\n"
        ));
        assert!(!is_agent_doc_markdown(
            "---\nagent_doc_session: [invalid\n---\n# Broken\n"
        ));
    }

    #[test]
    fn file_uri_decoding_preserves_spaces_and_unicode() {
        assert_eq!(
            file_uri_to_path("file:///tmp/agent%20doc/%E6%97%A5%E6%9C%AC.md").unwrap(),
            PathBuf::from("/tmp/agent doc/日本.md")
        );
    }

    #[test]
    fn full_document_range_uses_utf16_positions() {
        assert_eq!(
            full_document_range("first\n😀x"),
            json!({
                "start": { "line": 0, "character": 0 },
                "end": { "line": 1, "character": 3 }
            })
        );
    }

    #[test]
    fn full_sync_queue_additions_project_as_incremental_operator_deltas() {
        let before = "header\n<!-- agent:queue go -->\n- existing\n<!-- /agent:queue -->\n";
        let after =
            "header\n<!-- agent:queue go -->\n- existing\n- newly added\n<!-- /agent:queue -->\n";
        let delta =
            minimal_text_delta(before, after).expect("queue addition should produce a delta");
        assert_eq!(delta.delete_len, 0);
        assert_eq!(delta.insert, "- newly added\n");
        assert_eq!(
            before
                .chars()
                .take(delta.offset as usize)
                .collect::<String>()
                + &delta.insert
                + &before
                    .chars()
                    .skip(delta.offset as usize)
                    .collect::<String>(),
            after
        );
    }

    #[test]
    fn full_sync_projection_never_replaces_an_unchanged_buffer() {
        assert_eq!(minimal_text_delta("same\n", "same\n"), None);
    }

    #[test]
    fn did_save_requires_replica_shadow_and_disk_exactness() {
        let source = include_str!("lib.rs");
        let body = source
            .split("fn did_save(&mut self, params: &Value) {")
            .nth(1)
            .and_then(|tail| tail.split("fn reconcile_mode").next())
            .expect("did_save body");
        assert!(body.contains("document.replica.text() != document.shadow"));
        assert!(body.contains("std::fs::read_to_string(&document.file)"));
        assert!(body.contains("disk_text != document.shadow"));
        assert!(body.contains("publish_replica_projection(document, true)"));
    }

    #[test]
    fn lsp_framing_round_trips() {
        let message = json!({"jsonrpc": "2.0", "method": "initialized", "params": {}});
        let mut bytes = Vec::new();
        write_lsp_message(&mut bytes, &message).unwrap();
        let decoded = take_frame(&mut bytes).unwrap().unwrap();
        assert_eq!(decoded, message);
        assert!(bytes.is_empty());
    }

    #[test]
    fn registration_names_kind_generation_capabilities_and_layout_authority() {
        let fields = registration_fields();
        assert_eq!(fields["editor_pid"], json!(std::process::id()));
        assert_eq!(fields["editor_kind"], json!("zed"));
        assert_eq!(
            fields["editor_binary_version"],
            json!(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(fields["capabilities"], json!(ZED_LSP_CAPABILITIES));
        assert_eq!(fields["layout_authority"], json!("document_only"));
        assert!(fields["editor_version"].is_string());
    }

    /// `#queuetreegutter`: an agent-doc buffer answers queue folds and glyph
    /// hints from its tracked text; any other Markdown buffer answers null.
    #[test]
    fn queue_view_requests_answer_only_agent_doc_buffers() {
        let (mut server, _sender) = scripted_server();
        let doc = concat!(
            "---\nagent_doc_session: s\n---\n",
            "<!-- agent:queue id=\"main\" -->\n",
            "- ### ~~Ship alpha [#q-alpha]~~\n",
            "  - ~~step one~~\n",
            "<!-- /agent:queue -->\n",
        );
        server.track_queue_view("file:///s.md", doc);
        server.track_queue_view("file:///plain.md", "# notes\n<!-- agent:queue -->\n");
        let request = |id: u64, method: &str, uri: &str| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": {
                    "textDocument": { "uri": uri },
                    "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 9, "character": 0 } }
                }
            })
        };
        for message in [
            request(1, "textDocument/foldingRange", "file:///s.md"),
            request(2, "textDocument/inlayHint", "file:///s.md"),
            request(3, "textDocument/foldingRange", "file:///plain.md"),
        ] {
            server.handle_message(message).unwrap();
        }
        let output = decode_output(std::mem::take(&mut server.writer));
        assert_eq!(output[0]["result"][0]["startLine"], 4, "{output:?}");
        assert_eq!(output[0]["result"][0]["endLine"], 5);
        assert_eq!(output[1]["result"][0]["label"], "✓");
        assert_eq!(output[1]["result"][0]["tooltip"], "agent-doc queue: done");
        assert_eq!(output[2]["result"], Value::Null);
    }

    /// `#treestatuszed`: the code action menu on an agent-doc buffer's queue
    /// offers the tree-status answer and one deletion per deletable subtree;
    /// running the answer's command shows its text; any other Markdown buffer
    /// answers null.
    #[test]
    fn tree_status_code_actions_answer_only_agent_doc_buffers() {
        let (mut server, _sender) = scripted_server();
        let doc = concat!(
            "---\nagent_doc_session: s\n---\n",
            "<!-- agent:queue id=\"main\" -->\n",
            "- ~~do [#alpha]~~\n",
            "  - ~~step one~~\n",
            "- 🚧 [#subagent:beta#1] do [#beta]\n",
            "<!-- /agent:queue -->\n",
        );
        server.track_queue_view("file:///s.md", doc);
        server.track_queue_view("file:///plain.md", "# notes\n<!-- agent:queue -->\n");
        let code_action = |id: u64, uri: &str| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/codeAction",
                "params": {
                    "textDocument": { "uri": uri },
                    "range": { "start": { "line": 4, "character": 0 }, "end": { "line": 4, "character": 0 } },
                    "context": { "diagnostics": [], "triggerKind": 1 }
                }
            })
        };
        server
            .handle_message(code_action(1, "file:///s.md"))
            .unwrap();
        server
            .handle_message(code_action(2, "file:///plain.md"))
            .unwrap();
        let output = decode_output(std::mem::take(&mut server.writer));
        let actions = output[0]["result"].as_array().expect("actions");
        assert_eq!(actions.len(), 2, "{output:?}");
        assert_eq!(
            actions[1]["edit"]["changes"]["file:///s.md"][0]["range"],
            json!({ "start": { "line": 4, "character": 0 }, "end": { "line": 6, "character": 0 } })
        );
        assert_eq!(output[1]["result"], Value::Null);

        let command = actions[0]["command"].clone();
        server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "workspace/executeCommand",
                "params": { "command": command["command"], "arguments": command["arguments"] }
            }))
            .unwrap();
        let output = decode_output(std::mem::take(&mut server.writer));
        assert_eq!(output[0]["method"], "window/showMessage", "{output:?}");
        assert_eq!(output[0]["params"]["message"], command["arguments"][0]);
        assert!(
            output[0]["params"]["message"]
                .as_str()
                .unwrap()
                .contains("Deletable:")
        );
        assert_eq!(output[1]["id"], 3);
        assert_eq!(output[1]["result"], Value::Null);
    }

    #[test]
    fn initialize_advertises_tree_status_code_actions_and_command() {
        let (mut server, _sender) = scripted_server();
        server
            .handle_message(
                json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            )
            .unwrap();
        let output = decode_output(std::mem::take(&mut server.writer));
        let capabilities = &output[0]["result"]["capabilities"];
        assert_eq!(
            capabilities["codeActionProvider"]["codeActionKinds"],
            json!(["source", "quickfix"])
        );
        assert_eq!(
            capabilities["executeCommandProvider"]["commands"],
            json!(["agent-doc.treeStatus"])
        );
    }

    type ScriptedInput = std::sync::mpsc::Sender<std::io::Result<Vec<u8>>>;

    fn scripted_server() -> (Server<Vec<u8>, input::ChunkInput>, ScriptedInput) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (
            Server::new(Vec::new(), input::ChunkInput::from_receiver(receiver)),
            sender,
        )
    }

    fn decode_output(mut bytes: Vec<u8>) -> Vec<Value> {
        let mut messages = Vec::new();
        while let Some(message) = take_frame(&mut bytes).unwrap() {
            messages.push(message);
        }
        assert!(bytes.is_empty(), "output must be whole frames");
        messages
    }

    /// A resumed image parses the bytes the old image had read but not yet
    /// framed before anything Zed writes afterwards, and continues the
    /// server request ids.
    #[test]
    fn a_resumed_session_serves_carried_partial_input_first() {
        let request = json!({"jsonrpc": "2.0", "id": 9, "method": "shutdown"});
        let mut framed = Vec::new();
        write_lsp_message(&mut framed, &request).unwrap();
        let (head, tail) = framed.split_at(framed.len() / 2);

        let (mut server, sender) = scripted_server();
        server.resume(handoff::ResumedSession {
            next_request_id: 12,
            shutdown_requested: false,
            pending_input: head.to_vec(),
            documents: Vec::new(),
            divergent_history_reregister_at_ms: Vec::new(),
        });
        let exit = json!({"jsonrpc": "2.0", "method": "exit"});
        let mut rest = tail.to_vec();
        write_lsp_message(&mut rest, &exit).unwrap();
        sender.send(Ok(rest)).unwrap();
        drop(sender);

        assert_eq!(server.next_request_id, 12);
        let output = run_to_completion(server);
        assert_eq!(
            output,
            vec![json!({"jsonrpc": "2.0", "id": 9, "result": null})]
        );
    }

    fn run_to_completion(mut server: Server<Vec<u8>, input::ChunkInput>) -> Vec<Value> {
        // `serve` consumes the server; drive the same loop to keep the writer.
        loop {
            let mut done = false;
            while let Some(message) = take_frame(&mut server.pending_input).unwrap() {
                if server.handle_message(message).unwrap() {
                    done = true;
                    break;
                }
            }
            if done {
                break;
            }
            match server
                .input
                .read_available(&mut server.pending_input, PULL_INTERVAL)
                .unwrap()
            {
                ReadOutcome::Data | ReadOutcome::Timeout => {}
                ReadOutcome::Eof => break,
            }
        }
        decode_output(std::mem::take(&mut server.writer))
    }

    #[test]
    fn handoff_state_captures_unframed_input_and_request_ids() {
        let (mut server, _sender) = scripted_server();
        server.pending_input = b"Content-Length: 99\r\n".to_vec();
        server.next_request_id = 5;
        let state = server.handoff_state();
        assert_eq!(state.schema, handoff::HANDOFF_SCHEMA);
        assert_eq!(state.from_pid, std::process::id());
        assert_eq!(state.next_request_id, 5);
        let session = state.restore(Some(std::process::id())).unwrap();
        assert_eq!(session.pending_input, b"Content-Length: 99\r\n");
    }

    /// Replace an executable launch path with a file that cannot validate a
    /// handoff, then tick the server's binary pickup three times.
    #[cfg(unix)]
    fn pick_up_unloadable_replacement<I: LspInput>(server: &mut Server<Vec<u8>, I>) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let launch = dir.path().join("agent-doc");
        let write = |path: &Path, bytes: &[u8]| {
            std::fs::write(path, bytes).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        write(&launch, b"#!/bin/sh\nexit 0\n");
        let start = Instant::now();
        server.binary_watch = Some(BinaryWatch::for_test(launch.clone(), start));
        let next = dir.path().join("agent-doc.next");
        // Exits 0 but never prints the handoff acceptance token.
        write(&next, b"#!/bin/sh\nexit 0\n# replaced\n");
        std::fs::rename(&next, &launch).unwrap();
        for n in 0..3u32 {
            server.pick_up_replaced_binary(start + handoff::BINARY_CHECK_INTERVAL * n);
        }
    }

    /// A replacement that cannot prove it accepts the session is refused
    /// once, the running image keeps serving, and the operator is told to
    /// restart the language server.
    #[cfg(unix)]
    #[test]
    fn a_refused_replacement_keeps_serving_and_prompts_a_restart_once() {
        let (mut server, _sender) = scripted_server();
        server.pending_input = b"Content-Length: 3".to_vec();
        pick_up_unloadable_replacement(&mut server);
        let output = decode_output(std::mem::take(&mut server.writer));
        assert_eq!(output.len(), 1, "exactly one prompt: {output:?}");
        assert_eq!(output[0]["method"], json!("window/showMessage"));
        let message = output[0]["params"]["message"].as_str().unwrap();
        assert!(message.contains("restart language server"), "{message}");
        assert_eq!(server.pending_input, b"Content-Length: 3");
    }

    /// A reader thread may hold bytes the serving thread has not seen, so a
    /// server reading through one never replaces its image.
    #[cfg(unix)]
    #[test]
    fn a_threaded_reader_never_hands_over() {
        let mut server = Server::new(
            Vec::new(),
            input::ChunkInput::spawn_reader(std::io::empty()),
        );
        pick_up_unloadable_replacement(&mut server);
        assert!(server.writer.is_empty(), "no probe, no prompt");
    }

    #[test]
    fn the_restart_prompt_names_the_binary_and_the_refusal() {
        let prompt = restart_prompt(Path::new("/opt/agent-doc"), &anyhow!("probe refused"));
        assert_eq!(prompt["method"], json!("window/showMessage"));
        let message = prompt["params"]["message"].as_str().unwrap();
        assert!(message.contains("/opt/agent-doc"));
        assert!(message.contains("probe refused"));
        assert!(message.contains("restart language server"));
    }
}

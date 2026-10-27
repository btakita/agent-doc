//! Supplemental Markdown LSP sidecar for the Zed agent-doc extension.
//!
//! The server is deliberately registered for Zed's existing `Markdown` language
//! rather than defining a competing `.md` language. A buffer enters agent-doc
//! mode only while its current text has parseable frontmatter with a non-empty
//! `agent_doc_session`. Every other Markdown buffer is a strict no-op.

mod handoff;
mod input;

use agent_doc_document_realtime::editor_contract::{
    DOCUMENT_ONLY_LAYOUT_AUTHORITY, ZED_EDITOR_KIND, ZED_LSP_CAPABILITIES,
};
use agent_doc_merge::crdt_sync::ReplicaState;
use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use handoff::{BinaryObservation, BinaryWatch, HandoffDocument, HandoffState};
use input::{LspInput, ReadOutcome, take_frame};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PULL_INTERVAL: Duration = Duration::from_millis(250);

/// Set by the Zed extension: the version in `editors/zed/extension.toml` of
/// the extension that launched this server.
pub const ZED_EXTENSION_VERSION_ENV: &str = "AGENT_DOC_ZED_EXTENSION_VERSION";

struct OpenDocument {
    uri: String,
    file: PathBuf,
    project_root: PathBuf,
    identity: String,
    client_id: u64,
    replica: ReplicaState,
    shadow: String,
    pending_apply_target: Option<String>,
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
    next_request_id: u64,
    shutdown_requested: bool,
    binary_watch: Option<BinaryWatch>,
}

impl<W: Write, I: LspInput> Server<W, I> {
    fn new(writer: W, input: I) -> Self {
        Self {
            writer,
            input,
            pending_input: Vec::new(),
            documents: HashMap::new(),
            next_request_id: 1,
            shutdown_requested: false,
            binary_watch: None,
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
                    self.detach(uri);
                }
            }
            Some(_) if id.is_some() => {
                self.error(id.unwrap_or(Value::Null), -32601, "method not found")?;
            }
            _ => {}
        }
        Ok(self.shutdown_requested && method == Some("exit"))
    }

    fn did_open(&mut self, params: &Value) {
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return;
        };
        let Some(text) = params.pointer("/textDocument/text").and_then(Value::as_str) else {
            return;
        };
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
        let canonical = replica.text();
        let mut document = OpenDocument {
            uri: uri.to_string(),
            file,
            project_root,
            identity,
            client_id,
            replica,
            shadow: editor_text.to_string(),
            pending_apply_target: None,
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
            let Some(document) = self.documents.get_mut(&uri) else {
                continue;
            };
            let Ok(data) = controller_replica_request(
                &document.project_root,
                &document.file,
                "replica_pull",
                &document.identity,
                Value::Null,
            ) else {
                continue;
            };
            if data.get("kind").and_then(Value::as_str) == Some("replace") {
                // A replace-capable recovery is fenced through a fresh registration
                // so the local node receives the controller's current CRDT lineage.
                let visible = document.shadow.clone();
                let uri = document.uri.clone();
                self.detach(&uri);
                self.attach(&uri, &visible);
                continue;
            }
            let Some(updates) = data.get("updates").and_then(Value::as_array) else {
                continue;
            };
            let mut changed = false;
            for update in updates {
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
                if document.replica.apply_update(&bytes).is_err() {
                    continue;
                }
                changed = true;
            }
            if !changed {
                continue;
            }
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

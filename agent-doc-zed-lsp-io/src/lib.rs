//! Supplemental Markdown LSP sidecar for the Zed agent-doc extension.
//!
//! The server is deliberately registered for Zed's existing `Markdown` language
//! rather than defining a competing `.md` language. A buffer enters agent-doc
//! mode only while its current text has parseable frontmatter with a non-empty
//! `agent_doc_session`. Every other Markdown buffer is a strict no-op.

use agent_doc_merge::crdt_sync::ReplicaState;
use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

const PULL_INTERVAL: Duration = Duration::from_millis(250);

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

#[derive(Debug)]
enum InputEvent {
    Message(Value),
    Closed,
    Failed(String),
}

pub fn run() -> Result<()> {
    run_with_io(std::io::stdin(), std::io::stdout())
}

fn run_with_io<R, W>(reader: R, writer: W) -> Result<()>
where
    R: Read + Send + 'static,
    W: Write,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        loop {
            match read_lsp_message(&mut reader) {
                Ok(Some(message)) => {
                    if tx.send(InputEvent::Message(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = tx.send(InputEvent::Closed);
                    break;
                }
                Err(error) => {
                    let _ = tx.send(InputEvent::Failed(format!("{error:#}")));
                    break;
                }
            }
        }
    });
    Server::new(writer, rx).serve()
}

struct Server<W> {
    writer: W,
    receiver: Receiver<InputEvent>,
    documents: HashMap<String, OpenDocument>,
    next_request_id: u64,
    shutdown_requested: bool,
}

impl<W: Write> Server<W> {
    fn new(writer: W, receiver: Receiver<InputEvent>) -> Self {
        Self {
            writer,
            receiver,
            documents: HashMap::new(),
            next_request_id: 1,
            shutdown_requested: false,
        }
    }

    fn serve(mut self) -> Result<()> {
        loop {
            match self.receiver.recv_timeout(PULL_INTERVAL) {
                Ok(InputEvent::Message(message)) => {
                    if self.handle_message(message)? {
                        break;
                    }
                }
                Ok(InputEvent::Closed) => break,
                Ok(InputEvent::Failed(error)) => return Err(anyhow!(error)),
                Err(RecvTimeoutError::Timeout) => self.poll_remote_updates(),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.detach_all();
        Ok(())
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
            json!({ "editor_pid": std::process::id() }),
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

fn read_lsp_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.trim().strip_prefix("Content-Length:").map(str::trim) {
            content_length = Some(value.parse::<usize>().context("invalid Content-Length")?);
        }
    }
    let length = content_length.ok_or_else(|| anyhow!("missing Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .context("invalid LSP JSON payload")
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
    use std::io::Cursor;

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
    fn lsp_framing_round_trips() {
        let message = json!({"jsonrpc": "2.0", "method": "initialized", "params": {}});
        let mut bytes = Vec::new();
        write_lsp_message(&mut bytes, &message).unwrap();
        let decoded = read_lsp_message(&mut BufReader::new(Cursor::new(bytes)))
            .unwrap()
            .unwrap();
        assert_eq!(decoded, message);
    }
}

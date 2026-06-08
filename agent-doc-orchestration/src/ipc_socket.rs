//! Socket-based IPC for editor plugin communication.
//!
//! Uses Unix domain sockets (Linux/macOS) or Windows named pipes via the
//! `interprocess` crate. The socket replaces the file-based IPC mechanism
//! (NIO WatchService + patch files) for lower latency and no inotify issues.
//!
//! ## Architecture
//!
//! - **Listener** (plugin side): The editor plugin starts a socket listener
//!   at `.agent-doc/ipc.sock`. It accepts connections and processes JSON messages.
//! - **Sender** (CLI side): The `agent-doc write` command connects to the socket
//!   and sends patch JSON. Falls back to file-based IPC if socket unavailable.
//!
//! ## Protocol
//!
//! Messages are newline-delimited JSON (NDJSON). Each message is a single line
//! terminated by `\n`. The receiver reads lines and parses each as JSON.
//!
//! Message types:
//! - `{"type": "patch", "file": "...", "patches": [...], "frontmatter": "..."}` — apply patches
//! - `{"type": "reposition", "file": "...", "boundary_id": "..."}` — reposition
//!   boundary marker; `boundary_id` is optional and lets the plugin reuse the
//!   already-committed marker instead of generating a fresh boundary-only diff
//! - `{"type": "vcs_refresh"}` — trigger VCS refresh
//! - `{"type": "ack", "id": "..."}` — acknowledgment from plugin

use anyhow::{Context, Result};
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, ToFsName,
    traits::{Listener as _, Stream as _},
};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Socket filename within `.agent-doc/` directory.
const SOCKET_FILENAME: &str = "ipc.sock";

/// How long the sender waits for the plugin's delivery ack before treating the
/// socket as timed out (`#ipc-ack-timeout-align`).
///
/// `send_message` connects first, so a dead listener fails fast at
/// `try_connect`; this budget only applies to a *connected but slow* plugin.
/// The JB plugin blocks the socket handler on `agent_doc_await_idle(... , 5_000)`
/// — a typing-debounce wait capped at 5s — before applying the patch and acking.
/// A 2s budget was below that legitimate apply window, so a plugin that was
/// merely busy/typing tripped a false "ack timeout" that voted toward the
/// de-wedge degrade latch. Align the sender to just above the plugin's idle cap
/// so only a genuinely wedged listener times out.
const IPC_ACK_TIMEOUT_SECS: u64 = 6;

/// Get the socket path for a project.
pub fn socket_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(SOCKET_FILENAME)
}

/// Check if a socket listener is active.
pub fn is_listener_active(project_root: &Path) -> bool {
    let sock = socket_path(project_root);
    if !sock.exists() {
        return false;
    }
    // Try connecting — if it succeeds, the listener is active
    match try_connect(project_root) {
        Ok(_) => true,
        Err(_) => {
            // Stale socket file — clean it up
            let _ = std::fs::remove_file(&sock);
            false
        }
    }
}

/// Connect to the socket. Returns a stream for sending messages.
fn try_connect(project_root: &Path) -> Result<interprocess::local_socket::Stream> {
    let path = socket_path(project_root);
    let name = path.to_fs_name::<GenericFilePath>()?;
    let opts = interprocess::local_socket::ConnectOptions::new().name(name);
    let stream = opts
        .connect_sync()
        .context("failed to connect to IPC socket")?;
    Ok(stream)
}

/// Send a JSON message to the plugin via socket IPC.
/// Returns Ok(response) if the plugin acknowledges, Err if socket unavailable.
pub fn send_message(project_root: &Path, message: &serde_json::Value) -> Result<Option<String>> {
    let stream = try_connect(project_root)?;

    // interprocess Stream implements Read + Write via halves
    let (reader_half, mut writer_half) = stream.split();

    // Send NDJSON message
    let mut msg = serde_json::to_string(message)?;
    msg.push('\n');
    writer_half.write_all(msg.as_bytes())?;
    writer_half.flush()?;

    // Read ack (with manual timeout via thread)
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader_half);
        let mut ack_line = String::new();
        let result = reader.read_line(&mut ack_line);
        let _ = tx.send((result, ack_line));
    });

    match rx.recv_timeout(Duration::from_secs(IPC_ACK_TIMEOUT_SECS)) {
        Ok((Ok(0), _)) => Err(anyhow::anyhow!(
            "IPC ack: plugin closed connection without responding"
        )),
        Ok((Ok(_), line)) => {
            let ack = line.trim().to_string();
            match classify_ack(&ack) {
                AckClassification::Ok => Ok(Some(ack)),
                AckClassification::AlreadyApplied => {
                    Err(anyhow::anyhow!("IPC ack already_applied: {}", ack))
                }
                AckClassification::Failed => Err(anyhow::anyhow!("IPC ack status error: {}", ack)),
            }
        }
        Ok((Err(e), _)) => Err(anyhow::anyhow!("IPC ack read error: {}", e)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(anyhow::anyhow!(
            "IPC ack timeout ({IPC_ACK_TIMEOUT_SECS}s)"
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(anyhow::anyhow!("IPC reader thread disconnected"))
        }
    }
}

/// Classification of a plugin-sent IPC ack line.
///
/// The plugin (JetBrains / VS Code) sends a JSON ack after applying a patch.
/// `Ok` means the patch was applied normally. `AlreadyApplied` means the
/// plugin detected the response body is already present in the live buffer
/// and chose NOT to re-apply it — this is the signal the binary needs to
/// skip the file-IPC fallback (which would otherwise re-write the same
/// content and produce a duplicate response). `Failed` covers any other
/// `status: error` ack.
///
/// Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
/// Phase 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckClassification {
    Ok,
    AlreadyApplied,
    Failed,
}

/// Classify a plugin-sent IPC ack line. See [`AckClassification`].
pub fn classify_ack(ack: &str) -> AckClassification {
    let Some(value) = serde_json::from_str::<serde_json::Value>(ack).ok() else {
        return AckClassification::Ok;
    };
    let status_is_error = value
        .get("status")
        .and_then(|s| s.as_str())
        .map(|s| s.eq_ignore_ascii_case("error"))
        .unwrap_or(false);
    if !status_is_error {
        return AckClassification::Ok;
    }
    let reason = value
        .get("reason")
        .and_then(|r| r.as_str())
        .map(|r| r.to_ascii_lowercase());
    match reason.as_deref() {
        Some("already_applied") => AckClassification::AlreadyApplied,
        _ => AckClassification::Failed,
    }
}

/// True when a `send_message` error string indicates the plugin reported
/// `already_applied` rather than a genuine apply failure. Callers should use
/// this to short-circuit the file-IPC fallback so they do not re-write a
/// response the plugin already has in the live buffer.
pub fn is_already_applied_error(err: &anyhow::Error) -> bool {
    err.to_string().starts_with("IPC ack already_applied")
}

/// Send a patch message to the plugin.
pub fn send_patch(
    project_root: &Path,
    file: &str,
    patches_json: &str,
    frontmatter_yaml: Option<&str>,
) -> Result<bool> {
    let message = serde_json::json!({
        "type": "patch",
        "file": file,
        "patches": serde_json::from_str::<serde_json::Value>(patches_json)?,
        "frontmatter": frontmatter_yaml,
    });

    match send_message(project_root, &message) {
        Ok(Some(ack)) => {
            eprintln!("[ipc-socket] patch sent, ack: {}", ack);
            Ok(true)
        }
        Ok(None) => {
            eprintln!("[ipc-socket] patch sent, no ack");
            Ok(true)
        }
        Err(e) => Err(e),
    }
}

/// Send a queue-tag + frontmatter convergence patch to the plugin.
///
/// After queue maintenance halts/drains an auto-queue, the corrected shape is
/// written to disk + snapshot, but a live route-owned editor buffer keeps its
/// own working copy and would overwrite that disk write on its next flush —
/// re-adding `auto` to the `<!-- agent:queue auto -->` opening tag and the
/// `queue_active:` frontmatter, regenerating the snapshot/HEAD drift loop on
/// every preflight (`#adoc-queue-ipc-buffer-divergence`). A content-only patch
/// cannot converge an opening-tag attribute or frontmatter, so this message
/// carries `queue_auto` (the desired state of the queue tag's `auto` attribute,
/// applied via the `agent_doc_converge_queue_auto` FFI seam) alongside the
/// `frontmatter` field (`queue_active: …`, applied via the existing
/// frontmatter-merge seam). No component patches are sent; component bodies are
/// converged by the normal disk write + editor reload.
pub fn send_queue_convergence(
    project_root: &Path,
    file: &str,
    queue_auto: bool,
    frontmatter_yaml: Option<&str>,
) -> Result<bool> {
    let message = serde_json::json!({
        "type": "patch",
        "file": file,
        "patches": [],
        "unmatched": "",
        "frontmatter": frontmatter_yaml,
        "queue_auto": queue_auto,
    });

    match send_message(project_root, &message) {
        Ok(Some(ack)) => {
            eprintln!("[ipc-socket] queue convergence sent, ack: {}", ack);
            Ok(true)
        }
        Ok(None) => {
            eprintln!("[ipc-socket] queue convergence sent, no ack");
            Ok(true)
        }
        Err(e) => Err(e),
    }
}

/// Send a reposition boundary message.
///
/// When `preserve_head` is true, the plugin should use
/// `agent_doc_reposition_boundary_to_end_preserve_head_with_id` (FFI) so
/// `(HEAD)` annotations remain in the editor buffer. The committed blob and
/// snapshot are already clean; only the working-tree / editor buffer keeps them.
pub fn send_reposition(
    project_root: &Path,
    file: &str,
    boundary_id: Option<&str>,
    preserve_head: bool,
) -> Result<bool> {
    let mut message = serde_json::json!({
        "type": "reposition",
        "file": file,
    });
    if let Some(boundary_id) = boundary_id {
        message["boundary_id"] = serde_json::Value::String(boundary_id.to_string());
    }
    if preserve_head {
        message["preserve_head"] = serde_json::Value::Bool(true);
    }

    send_message(project_root, &message).map(|_| true)
}

/// Send a VCS refresh signal.
pub fn send_vcs_refresh(project_root: &Path) -> Result<bool> {
    let message = serde_json::json!({
        "type": "vcs_refresh",
    });

    send_message(project_root, &message).map(|_| true)
}

/// Start a socket listener (for use by the FFI library / plugin).
/// This blocks the calling thread — run it on a background thread.
#[allow(unreachable_code)]
pub fn start_listener<F>(project_root: &Path, handler: F) -> Result<()>
where
    F: Fn(&str) -> Option<String> + Send + 'static,
{
    let sock_path = socket_path(project_root);

    // Clean up stale socket
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }

    // Ensure parent directory exists
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    eprintln!("[ipc-socket] listening on {:?}", sock_path);

    let name = sock_path.to_fs_name::<GenericFilePath>()?;
    let opts = ListenerOptions::new().name(name);
    let listener = opts.create_sync()?;

    loop {
        match listener.accept() {
            Ok(stream) => {
                let (reader_half, mut writer_half) = stream.split();
                let mut reader = BufReader::new(reader_half);
                let mut line = String::new();

                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    let trimmed = line.trim();
                    if !trimmed.is_empty()
                        && let Some(response) = handler(trimmed)
                    {
                        let mut resp = response;
                        resp.push('\n');
                        if let Err(e) = writer_half.write_all(resp.as_bytes()) {
                            eprintln!("[ipc-socket] handler write error: {}", e);
                        }
                        if let Err(e) = writer_half.flush() {
                            eprintln!("[ipc-socket] handler flush error: {}", e);
                        }
                    }
                    line.clear();
                }
            }
            Err(e) => {
                eprintln!("[ipc-socket] accept error: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn socket_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                Some(serde_json::json!({"type": "ack", "id": v["type"]}).to_string())
            })
            .ok();
        });

        // Give the server time to start
        thread::sleep(Duration::from_millis(100));

        // Send a message
        let msg = serde_json::json!({"type": "vcs_refresh"});
        let result = send_message(&root, &msg).unwrap();
        assert!(result.is_some());
        let ack: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(ack["type"], "ack");
        assert_eq!(ack["id"], "vcs_refresh");

        // Clean up — remove socket to stop listener
        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

    #[test]
    fn socket_error_ack_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, |_msg| {
                Some(serde_json::json!({"type": "ack", "status": "error"}).to_string())
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        let msg = serde_json::json!({"type": "patch"});
        let err = send_message(&root, &msg).unwrap_err().to_string();
        assert!(
            err.contains("IPC ack status error"),
            "error ack should fail the socket IPC send, got: {err}"
        );

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

    #[test]
    fn classify_ack_treats_ok_status_as_ok() {
        let ack = r#"{"type":"ack","status":"ok","id":"patch-123"}"#;
        assert_eq!(classify_ack(ack), AckClassification::Ok);
    }

    #[test]
    fn classify_ack_treats_ack_without_status_as_ok() {
        // Legacy ack shape: just `{"type":"ack","id":"..."}` with no status.
        let ack = r#"{"type":"ack","id":"patch-123"}"#;
        assert_eq!(classify_ack(ack), AckClassification::Ok);
    }

    #[test]
    fn classify_ack_treats_already_applied_reason_as_already_applied() {
        let ack = r#"{"type":"ack","status":"error","reason":"already_applied"}"#;
        assert_eq!(classify_ack(ack), AckClassification::AlreadyApplied);
    }

    #[test]
    fn classify_ack_treats_already_applied_reason_uppercase_as_already_applied() {
        // Plugin implementations should send the canonical lowercase form,
        // but the classifier matches case-insensitively as a forgiving
        // protocol contract.
        let ack = r#"{"type":"ack","status":"ERROR","reason":"Already_Applied"}"#;
        assert_eq!(classify_ack(ack), AckClassification::AlreadyApplied);
    }

    #[test]
    fn classify_ack_treats_other_error_reasons_as_failed() {
        let ack = r#"{"type":"ack","status":"error","reason":"apply_failed"}"#;
        assert_eq!(classify_ack(ack), AckClassification::Failed);
    }

    #[test]
    fn classify_ack_treats_error_status_without_reason_as_failed() {
        let ack = r#"{"type":"ack","status":"error"}"#;
        assert_eq!(classify_ack(ack), AckClassification::Failed);
    }

    #[test]
    fn classify_ack_treats_malformed_json_as_ok() {
        // Backwards compat: unparseable acks (e.g. plain text) are not
        // treated as error so existing plugins keep working.
        let ack = "not json at all";
        assert_eq!(classify_ack(ack), AckClassification::Ok);
    }

    #[test]
    fn is_already_applied_error_matches_classifier_output() {
        let err = anyhow::anyhow!(
            "IPC ack already_applied: {}",
            r#"{"type":"ack","status":"error","reason":"already_applied"}"#
        );
        assert!(super::is_already_applied_error(&err));
    }

    #[test]
    fn is_already_applied_error_rejects_other_errors() {
        let err = anyhow::anyhow!("IPC ack status error: something else");
        assert!(!super::is_already_applied_error(&err));
        let err = anyhow::anyhow!("IPC ack timeout (2s)");
        assert!(!super::is_already_applied_error(&err));
    }
}

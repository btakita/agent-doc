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
//! - `{"type": "refresh_content", "file": "...", "content": "...",
//!   "expected_content_hash": "...", "expected_content_len": N}` — replace a
//!   stale editor buffer with committed content after a HEAD-authoritative repair
//! - `{"type": "publish_live_buffer", "file": "..."}` — ask the editor to
//!   republish its current visible-buffer proof without mutating the document
//! - `{"type": "vcs_refresh"}` — trigger VCS refresh
//! - `{"type": "ack", "id": "..."}` — acknowledgment from plugin
//!
//! VS Code does not run the socket listener. For read-only live-buffer proof
//! refreshes it consumes `.agent-doc/patches/publish-live-buffer.signal` with the
//! same `{type,file}` payload; for editor-owned save recovery it consumes
//! `.agent-doc/patches/save-document.signal` with the same
//! `{type,file,patch_id}` payload as the socket `save_document` message.

use agent_doc_ipc_protocol::{
    AckClassification, classify_ack, early_ack_line, early_ack_ops_marker,
    early_ack_tagged_message, message_requests_early_ack,
};
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

/// Early-ack opt-in for the sender (`#ipc-early-ack` / `#saev`, Phase 2).
///
/// When `true`, [`send_message`] tags outgoing `patch` messages with
/// `early_ack: true`, which makes an early-ack-aware [`start_listener`] emit a
/// `pending` ack the instant it receives the patch (before the blocking apply),
/// then the terminal ack as usual. The sender's [`send_message`] read loop
/// already understands the two-phase sequence regardless of this flag, so the
/// protocol is fully wired and unit-tested; only the auto-injection of the
/// opt-in flag onto live closeout patches is gated here.
///
/// Activated (`#saevon`, 2026-06-09): the sender auto-tags live closeout `patch`
/// messages with `early_ack: true`, so an early-ack-aware listener emits a
/// `pending` ack on receipt (before the blocking apply) and the sender's
/// liveness probe is decoupled from plugin apply latency. The protocol was
/// landed dormant and unit-tested before this flip; activation is verified live
/// under real typing load (`#xkpf` / `#lvb-run`) by grepping `ops.log` for
/// `[ipc-socket] early-ack pending emitted before apply` with a paired terminal
/// ack and no `ack_timeout` / `false_success`. Older listeners ignore the
/// unknown `early_ack` field (skew-safe), so a non-early-ack plugin still gets
/// exactly the prior single terminal ack.
const EARLY_ACK_ENABLED: bool = true;

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
    send_message_with_timeout(
        project_root,
        message,
        Duration::from_secs(IPC_ACK_TIMEOUT_SECS),
    )
}

/// Send a JSON message with an explicit ack timeout.
///
/// Most production sends use [`send_message`]. The explicit-timeout variant is
/// reserved for liveness probes that must not clear a degraded-socket latch
/// just because the OS accepted a connection while the plugin accept/apply loop
/// is no longer returning acks.
pub fn send_message_with_timeout(
    project_root: &Path,
    message: &serde_json::Value,
    ack_timeout: Duration,
) -> Result<Option<String>> {
    let stream = try_connect(project_root)?;

    // interprocess Stream implements Read + Write via halves
    let (reader_half, mut writer_half) = stream.split();

    // Send NDJSON message. When early-ack is enabled, tag outgoing `patch`
    // messages so an early-ack-aware listener emits a `pending` ack on receipt;
    // older listeners ignore the unknown field (skew-safe). Non-patch messages
    // (queue convergence, etc.) are sent verbatim.
    let outgoing = early_ack_tagged_message(message, EARLY_ACK_ENABLED);
    let mut msg = serde_json::to_string(&outgoing)?;
    msg.push('\n');
    writer_half.write_all(msg.as_bytes())?;
    writer_half.flush()?;

    // Read ack(s) (with manual timeout via thread). The listener may send an
    // early `pending` ack (liveness only) before the terminal ack, so the reader
    // thread loops until it yields a non-pending line (terminal ack / EOF /
    // error). For a single terminal ack — every current plugin — the loop runs
    // exactly once, identical to the prior single-read behavior.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader_half);
        loop {
            let mut ack_line = String::new();
            let result = reader.read_line(&mut ack_line);
            let stop = match &result {
                Ok(0) => true,
                Ok(_) => classify_ack(ack_line.trim()) != AckClassification::Pending,
                Err(_) => true,
            };
            if tx.send((result, ack_line)).is_err() {
                break;
            }
            if stop {
                break;
            }
        }
    });

    // Each phase gets its own ack-timeout budget: an early `pending` ack proves
    // liveness and lets the binary keep waiting for the terminal ack instead of
    // declaring a false timeout while the plugin is still applying.
    loop {
        match rx.recv_timeout(ack_timeout) {
            Ok((Ok(0), _)) => {
                return Err(anyhow::anyhow!(
                    "IPC ack: plugin closed connection without responding"
                ));
            }
            Ok((Ok(_), line)) => {
                let ack = line.trim().to_string();
                match classify_ack(&ack) {
                    // Liveness-only: listener received the patch but has not yet
                    // applied it. Keep waiting for the terminal ack.
                    AckClassification::Pending => continue,
                    AckClassification::Ok => return Ok(Some(ack)),
                    AckClassification::AlreadyApplied => {
                        return Err(anyhow::anyhow!("IPC ack already_applied: {}", ack));
                    }
                    AckClassification::Failed => {
                        return Err(anyhow::anyhow!("IPC ack status error: {}", ack));
                    }
                }
            }
            Ok((Err(e), _)) => return Err(anyhow::anyhow!("IPC ack read error: {}", e)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return Err(anyhow::anyhow!(
                    "IPC ack timeout ({}ms)",
                    ack_timeout.as_millis()
                ));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow::anyhow!("IPC reader thread disconnected"));
            }
        }
    }
}

/// Probe whether the socket listener can accept and ack a lightweight message.
pub fn probe_listener_ack(project_root: &Path, timeout: Duration) -> Result<bool> {
    let message = serde_json::json!({
        "type": "vcs_refresh",
        "probe": "ipc_degraded_self_heal",
    });
    send_message_with_timeout(project_root, &message, timeout).map(|_| true)
}

/// `#jbacceptwedge`: number of per-connection handler threads currently
/// in flight. Under the old single-threaded accept loop this count could
/// never exceed 1 — the loop blocked on the (potentially slow) apply
/// handler before returning to `accept()`, so connections piled up in
/// the socket backlog ("22 unaccepted connections"). Any live ops.log
/// entry showing `ipc_accept_thread_spawned inflight>=2` proves the
/// per-connection-thread fix is exercising concurrent connections
/// without a backlog.
static INFLIGHT_CONNECTION_HANDLERS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// RAII guard decrements [`INFLIGHT_CONNECTION_HANDLERS`] on drop so a
/// panicking handler still releases its slot.
struct InflightConnectionGuard;

impl Drop for InflightConnectionGuard {
    fn drop(&mut self) {
        INFLIGHT_CONNECTION_HANDLERS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// `#jbacceptwedge`: ops.log marker emitted when a connection is accepted
/// and handed to a fresh thread. Carries the `ipc_accept_thread_spawned`
/// predicate token plus the live inflight count so a single grep both
/// proves the per-connection-thread fix is live and bounds how many
/// concurrent connections the listener is juggling.
pub fn ipc_accept_thread_ops_marker(inflight: u64) -> String {
    format!(
        "[ipc-socket] ipc_accept_thread_spawned inflight={}",
        inflight
    )
}

/// `#jbacceptwedge`: current in-flight handler-thread count. Exposed for
/// tests so the regression can assert the listener actually reached a
/// concurrent state (rather than only asserting wall-clock timing).
pub fn inflight_connection_handlers() -> u64 {
    INFLIGHT_CONNECTION_HANDLERS.load(std::sync::atomic::Ordering::SeqCst)
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
/// applied via the `agent_doc_converge_queue_auto` FFI seam), the `frontmatter`
/// field (`queue: …`, applied via the existing frontmatter-merge seam), and the
/// corrected queue component body. Sending the body closes the live-editor gap
/// where an open buffer can keep stale queue lines and flush them back over the
/// disk/snapshot repair.
pub fn send_queue_convergence(
    project_root: &Path,
    file: &str,
    queue_auto: bool,
    frontmatter_yaml: Option<&str>,
    queue_body: Option<&str>,
) -> Result<bool> {
    let patches = queue_body
        .map(|content| {
            serde_json::json!([{
                "component": "queue",
                "content": content,
            }])
        })
        .unwrap_or_else(|| serde_json::json!([]));
    let message = serde_json::json!({
        "type": "patch",
        "file": file,
        "patches": patches,
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

/// Ask the live editor to flush (save) its buffer for `file` to disk.
///
/// Used to resolve a `live_prompt_drift_after_preflight` divergence at its
/// source: the JB editor buffer holds unsaved edits ahead of disk, so the binary
/// reads stale disk content and the cycle would otherwise stall by adopting
/// `content_ours` as a next-cycle carry-forward snapshot (which also leaves the
/// IntelliJ buffer dirty, re-drifting + raising a File Cache Conflict on every
/// later cycle). Instead the plugin runs `FileDocumentManager.saveDocument()`,
/// flushing the buffer to disk AND clearing the editor's dirty flag, then writes
/// the saved buffer to the ack-content sidecar keyed by `patch_id` so the binary
/// can read exactly what was persisted and adopt it as a clean on-disk snapshot.
pub fn send_save_document(project_root: &Path, file: &str, patch_id: &str) -> Result<bool> {
    let message = serde_json::json!({
        "type": "save_document",
        "file": file,
        "patch_id": patch_id,
    });

    match send_message(project_root, &message) {
        Ok(Some(ack)) => {
            eprintln!("[ipc-socket] save_document sent, ack: {}", ack);
            Ok(true)
        }
        Ok(None) => {
            eprintln!("[ipc-socket] save_document sent, no ack");
            Ok(true)
        }
        Err(e) => Err(e),
    }
}

/// Send committed content to the editor when the binary has just repaired a
/// stale post-commit working tree back to HEAD.
///
/// The expected hash/length describe the stale editor content that is safe to
/// replace. The plugin must reject the message if the live document changed
/// before it applies the refresh.
pub fn send_refresh_content(
    project_root: &Path,
    file: &str,
    content: &str,
    expected_content_hash: &str,
    expected_content_len: usize,
) -> Result<bool> {
    let message = serde_json::json!({
        "type": "refresh_content",
        "file": file,
        "content": content,
        "expected_content_hash": expected_content_hash,
        "expected_content_len": expected_content_len,
    });

    send_message(project_root, &message).map(|_| true)
}

/// Ask the live editor to republish its current visible-buffer sidecar for
/// `file` without changing the document.
pub fn send_publish_live_buffer(project_root: &Path, file: &str) -> Result<bool> {
    let message = serde_json::json!({
        "type": "publish_live_buffer",
        "file": file,
    });

    send_message(project_root, &message).map(|_| true)
}

/// Write a VS Code-style file IPC signal asking the editor to republish its
/// current visible-buffer sidecar for `file` without changing the document.
pub fn send_publish_live_buffer_file_signal(project_root: &Path, file: &str) -> Result<bool> {
    let patches_dir = project_root.join(".agent-doc").join("patches");
    std::fs::create_dir_all(&patches_dir)
        .with_context(|| format!("failed to create {}", patches_dir.display()))?;
    let signal_file = patches_dir.join("publish-live-buffer.signal");
    let tmp_file = patches_dir.join(format!(
        "publish-live-buffer.signal.{}.tmp",
        std::process::id()
    ));
    let payload = serde_json::json!({
        "type": "publish_live_buffer",
        "file": file,
    });
    std::fs::write(&tmp_file, serde_json::to_vec(&payload)?)
        .with_context(|| format!("failed to write {}", tmp_file.display()))?;
    match std::fs::rename(&tmp_file, &signal_file) {
        Ok(()) => Ok(true),
        Err(first_err) => {
            let _ = std::fs::remove_file(&signal_file);
            std::fs::rename(&tmp_file, &signal_file).with_context(|| {
                format!(
                    "failed to replace {} after initial rename error: {}",
                    signal_file.display(),
                    first_err
                )
            })?;
            Ok(true)
        }
    }
}

/// Write a VS Code-style file IPC signal asking the editor to save the current
/// visible buffer for `file` and publish ack-content for `patch_id`.
pub fn send_save_document_file_signal(
    project_root: &Path,
    file: &str,
    patch_id: &str,
) -> Result<bool> {
    let patches_dir = project_root.join(".agent-doc").join("patches");
    std::fs::create_dir_all(&patches_dir)
        .with_context(|| format!("failed to create {}", patches_dir.display()))?;
    let signal_file = patches_dir.join("save-document.signal");
    let tmp_file = patches_dir.join(format!("save-document.signal.{}.tmp", std::process::id()));
    let payload = serde_json::json!({
        "type": "save_document",
        "file": file,
        "patch_id": patch_id,
    });
    std::fs::write(&tmp_file, serde_json::to_vec(&payload)?)
        .with_context(|| format!("failed to write {}", tmp_file.display()))?;
    match std::fs::rename(&tmp_file, &signal_file) {
        Ok(()) => Ok(true),
        Err(first_err) => {
            let _ = std::fs::remove_file(&signal_file);
            std::fs::rename(&tmp_file, &signal_file).with_context(|| {
                format!(
                    "failed to replace {} after initial rename error: {}",
                    signal_file.display(),
                    first_err
                )
            })?;
            Ok(true)
        }
    }
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
    F: Fn(&str) -> Option<String> + Send + Sync + 'static,
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

    // Handle each connection on its own thread so a slow/blocking apply handler
    // can never stall the accept loop and pile up connections in the socket
    // backlog (the "22 unaccepted connections" wedge, #jbacceptwedge). The
    // handler only captures an `extern "C" fn` pointer (Send + Sync), so sharing
    // it across threads via Arc is sound.
    let handler = std::sync::Arc::new(handler);
    let root_buf = project_root.to_path_buf();

    loop {
        match listener.accept() {
            Ok(stream) => {
                let handler = std::sync::Arc::clone(&handler);
                let root_buf = root_buf.clone();
                // #jbacceptwedge: count and log the fresh handler thread
                // BEFORE spawning, so the inflight count reported in the
                // marker reflects the post-increment state of this accept.
                let inflight = INFLIGHT_CONNECTION_HANDLERS
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                crate::ops_log::log_op(&root_buf, &ipc_accept_thread_ops_marker(inflight));
                std::thread::spawn(move || {
                    let _inflight_guard = InflightConnectionGuard;
                    let (reader_half, mut writer_half) = stream.split();
                    let mut reader = BufReader::new(reader_half);
                    let mut line = String::new();

                    while reader.read_line(&mut line).unwrap_or(0) > 0 {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            // Early-ack: if the sender opted in, emit a `pending` ack
                            // the instant we receive the patch — before the blocking
                            // apply handler runs — so the sender's liveness probe is
                            // decoupled from apply latency. The terminal ack still
                            // follows. Senders that do not opt in get only the
                            // terminal ack, exactly as before.
                            if message_requests_early_ack(trimmed) {
                                let mut early = early_ack_line().to_string();
                                early.push('\n');
                                if let Err(e) = writer_half.write_all(early.as_bytes()) {
                                    eprintln!("[ipc-socket] early-ack write error: {}", e);
                                } else if let Err(e) = writer_half.flush() {
                                    eprintln!("[ipc-socket] early-ack flush error: {}", e);
                                } else {
                                    // #saev prove/disprove: a successful early-ack emit was
                                    // previously silent (only failures logged), so a live
                                    // EARLY_ACK_ENABLED run had no grep-able proof the
                                    // `pending` ack actually went out before the blocking
                                    // apply. Emit a positive marker on the success path.
                                    eprintln!(
                                        "[ipc-socket] early-ack pending emitted before apply"
                                    );
                                    // #x9ds: the stderr line above is invisible to the
                                    // ops.log gate-verify scan, and its hyphenated text does
                                    // not contain the `early_ack_pending` predicate token.
                                    // Also record the marker to ops.log (derived from the
                                    // listener's project root) so the #saev gate is provable
                                    // from ops.log once EARLY_ACK_ENABLED is driven live.
                                    crate::ops_log::log_op(&root_buf, early_ack_ops_marker());
                                }
                            }
                            if let Some(response) = handler(trimmed) {
                                let mut resp = response;
                                resp.push('\n');
                                if let Err(e) = writer_half.write_all(resp.as_bytes()) {
                                    eprintln!("[ipc-socket] handler write error: {}", e);
                                }
                                if let Err(e) = writer_half.flush() {
                                    eprintln!("[ipc-socket] handler flush error: {}", e);
                                }
                            }
                        }
                        line.clear();
                    }
                });
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
    use std::time::{Duration, Instant};

    #[test]
    fn slow_handler_does_not_block_concurrent_connections() {
        // Regression for #jbacceptwedge: the accept loop used to block on the
        // (potentially slow) apply handler, piling connections up in the socket
        // backlog ("22 unaccepted connections"). Now each connection is handled
        // on its own thread, so N concurrent sends to a slow handler complete in
        // ~one handler-duration rather than ~N. Sequential handling of three
        // 600ms runs would take ~1.8s+; the parallel bound below is generous
        // enough for CI variance while still failing under the old loop.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, move |msg| {
                thread::sleep(Duration::from_millis(600));
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                Some(serde_json::json!({"type":"ack","id":v["type"]}).to_string())
            })
            .ok()
        });
        thread::sleep(Duration::from_millis(120));

        let start = Instant::now();
        let inflight_sampler = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let sampler_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sampler_inflight = inflight_sampler.clone();
        let sampler_stop_clone = sampler_stop.clone();
        let _sampler = thread::spawn(move || {
            while !sampler_stop_clone.load(std::sync::atomic::Ordering::Relaxed) {
                let seen = inflight_connection_handlers();
                if seen > sampler_inflight.load(std::sync::atomic::Ordering::Relaxed) {
                    sampler_inflight.store(seen, std::sync::atomic::Ordering::Relaxed);
                }
                thread::sleep(Duration::from_millis(25));
            }
        });
        let handles: Vec<_> = (0..3)
            .map(|i| {
                let root = root.clone();
                thread::spawn(move || {
                    send_message(&root, &serde_json::json!({"type": format!("m{}", i)}))
                })
            })
            .collect();
        for h in handles {
            let r = h.join().unwrap().unwrap();
            assert!(r.is_some(), "concurrent send should still receive an ack");
        }
        sampler_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = _sampler.join();
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(1500),
            "3 concurrent sends to a 600ms handler took {:?} (expected parallel < 1.5s; \
              sequential would be ~1.8s+)",
            elapsed
        );
        let peak_inflight = inflight_sampler.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            peak_inflight >= 2,
            "#jbacceptwedge regression: expected the listener to reach inflight>=2 while \
             servicing 3 concurrent slow sends (proves per-connection-thread path exercised; \
             old single-threaded loop could never exceed 1), observed peak={}",
            peak_inflight
        );

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

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
    fn send_save_document_sends_typed_message_with_patch_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                *captured_clone.lock().unwrap() = Some(v);
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        let ok = send_save_document(&root, "/tmp/plan.md", "save-pid-123").unwrap();
        assert!(ok, "save_document should succeed on an ok ack");

        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("listener saw a message");
        assert_eq!(msg["type"], "save_document");
        assert_eq!(msg["file"], "/tmp/plan.md");
        assert_eq!(msg["patch_id"], "save-pid-123");

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

    #[test]
    fn send_publish_live_buffer_sends_readonly_file_message() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                *captured_clone.lock().unwrap() = Some(v);
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        let ok = send_publish_live_buffer(&root, "/tmp/plan.md").unwrap();
        assert!(ok, "publish_live_buffer should succeed on an ok ack");

        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("listener saw a message");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], "/tmp/plan.md");
        assert!(
            msg.get("content").is_none() && msg.get("patches").is_none(),
            "publish_live_buffer must not carry document mutation payload: {msg}"
        );

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

    #[test]
    fn send_publish_live_buffer_file_signal_writes_readonly_payload() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        let ok = send_publish_live_buffer_file_signal(&root, "/tmp/plan.md").unwrap();
        assert!(ok, "publish-live-buffer file signal should be written");

        let signal = root
            .join(".agent-doc")
            .join("patches")
            .join("publish-live-buffer.signal");
        let msg: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(signal).unwrap()).unwrap();
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], "/tmp/plan.md");
        assert!(
            msg.get("content").is_none() && msg.get("patches").is_none(),
            "publish-live-buffer signal must not carry document mutation payload: {msg}"
        );
    }

    #[test]
    fn send_save_document_file_signal_writes_typed_payload_with_patch_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        let ok = send_save_document_file_signal(&root, "/tmp/plan.md", "save-signal-123").unwrap();
        assert!(ok, "save-document file signal should be written");

        let signal = root
            .join(".agent-doc")
            .join("patches")
            .join("save-document.signal");
        let msg: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(signal).unwrap()).unwrap();
        assert_eq!(msg["type"], "save_document");
        assert_eq!(msg["file"], "/tmp/plan.md");
        assert_eq!(msg["patch_id"], "save-signal-123");
        assert!(
            msg.get("content").is_none() && msg.get("patches").is_none(),
            "save-document signal must not carry document replacement payload: {msg}"
        );
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
    fn send_message_handles_early_then_terminal_ack() {
        // Full two-phase roundtrip: a flagged patch makes the listener emit a
        // `pending` ack on receipt, then the terminal ack after the handler runs.
        // send_message must skip the pending ack and return the terminal result.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, |msg| {
                // Prove the early ack already went out before this (apply) runs.
                assert!(message_requests_early_ack(msg));
                thread::sleep(Duration::from_millis(50));
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        // Manually flagged so the listener early-acks independent of sender
        // auto-injection.
        let msg = serde_json::json!({"type": "patch", "early_ack": true});
        let result = send_message(&root, &msg).unwrap();
        let ack: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(
            ack["status"], "ok",
            "terminal ack must be returned, not pending"
        );

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
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

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
//! - `{"type": "publish_live_buffer", "file": "...", "early_receipt": true}` —
//!   ask the editor to republish its current visible-buffer proof without mutating the document
//! - `{"type": "vcs_refresh"}` — trigger VCS refresh
//! - `{"type": "receipt", "status": "applied"}` — terminal plugin receipt
//!
//! VS Code does not run the socket listener. For read-only live-buffer proof
//! refreshes it consumes `.agent-doc/patches/publish-live-buffer.signal` with the
//! same `{type,file}` payload; for editor-owned save recovery it consumes
//! `.agent-doc/patches/save-document.signal` with the same
//! `{type,file,patch_id}` payload as the socket `save_document` message.

use agent_doc_ipc_protocol::{
    SocketReceiptClassification, classify_socket_receipt, early_receipt_line,
    early_receipt_ops_marker, early_receipt_tagged_message, ipc_accept_thread_ops_marker,
    message_requests_early_receipt, patch_message, publish_live_buffer_message,
    queue_convergence_message, refresh_content_message, reposition_message, save_document_message,
    vcs_refresh_message, vcs_refresh_probe_message,
};
use anyhow::{Context, Result};
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, ToFsName,
    traits::{Listener as _, Stream as _},
};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub mod editor_target;

/// Socket filename within `.agent-doc/` directory.
const SOCKET_FILENAME: &str = "ipc.sock";

/// Best-effort transport marker logger used by the listener.
pub type OpsLogger = fn(&Path, &str);

fn noop_ops_logger(_: &Path, _: &str) {}

/// How long the sender waits for the plugin's delivery receipt before treating the
/// socket as timed out (`#ipc-receipt-timeout-align`).
///
/// `send_message` connects first, so a dead listener fails fast at
/// `try_connect`; this budget only applies to a *connected but slow* plugin.
/// A connected IDE may still be busy applying the patch on its UI/application
/// thread before returning a receipt. A 2s budget was below that legitimate
/// apply window and could vote toward the de-wedge degrade latch, so keep this
/// timeout high enough to distinguish a slow apply from a wedged listener.
const IPC_RECEIPT_TIMEOUT_SECS: u64 = 6;
/// Bound the blocking `connect_sync` (`#af88` F). interprocess `ConnectOptions`
/// exposes no native connect deadline, so we run the connect on a watchdog thread
/// and give up after this budget rather than parking the caller forever on a
/// socket whose listener never completes the handshake. Generous relative to a
/// live listener's near-instant accept, so it only fires on a genuinely wedged peer.
const IPC_CONNECT_TIMEOUT_SECS: u64 = 3;
/// Per-connection read timeout on the listener side (`#af88` B/D): a half-open
/// client that connects but never sends a request line must not park its handler
/// thread (and its fd) forever. A recv timeout converts the stalled read into a
/// clean EOF-style handler exit that releases the inflight slot.
const IPC_LISTENER_READ_TIMEOUT_SECS: u64 = 30;
const IPC_LISTENER_MAX_INFLIGHT_HANDLERS: u64 = 64;
const IPC_LISTENER_RESOURCE_BACKOFF: Duration = Duration::from_millis(250);

/// Early-receipt opt-in for the sender (`#ipc-early-receipt` / `#saev`, Phase 2).
///
/// When `true`, [`send_message`] tags outgoing `patch` messages with
/// `early_receipt: true`, which makes an early-receipt-aware [`start_listener`] emit an
/// `accepted` receipt the instant it receives the patch (before the blocking apply),
/// then the terminal receipt as usual. The sender's [`send_message`] read loop
/// already understands the two-phase sequence regardless of this flag, so the
/// protocol is fully wired and unit-tested; only the auto-injection of the
/// opt-in flag onto live closeout patches is gated here.
///
/// Activated (`#saevon`, 2026-06-09): the sender auto-tags live closeout `patch`
/// messages with `early_receipt: true`, so an early-receipt-aware listener emits an
/// `accepted` receipt on receipt (before the blocking apply) and the sender's
/// liveness probe is decoupled from plugin apply latency. The protocol was
/// landed dormant and unit-tested before this flip; activation is verified live
/// under real typing load (`#xkpf` / `#lvb-run`) by grepping `ops.log` for
/// `[ipc-socket] early receipt accepted emitted before apply` with a paired terminal
/// receipt and no `receipt_timeout` / `false_success`. Older listeners that
/// still emit legacy ACK lines are rejected as incompatible.
const EARLY_RECEIPT_ENABLED: bool = true;

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
    try_connect_with_timeout(project_root, Duration::from_secs(IPC_CONNECT_TIMEOUT_SECS))
}

/// Connect with a bounded deadline (`#af88` F). `connect_sync` is blocking with
/// no native timeout, so run it on a watchdog thread and fail closed after
/// `connect_timeout` instead of hanging the caller on a wedged peer.
fn try_connect_with_timeout(
    project_root: &Path,
    connect_timeout: Duration,
) -> Result<interprocess::local_socket::Stream> {
    let path = socket_path(project_root);
    let path_for_thread = path.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let name = path_for_thread.to_fs_name::<GenericFilePath>()?;
            interprocess::local_socket::ConnectOptions::new()
                .name(name)
                .connect_sync()
                .context("failed to connect to IPC socket")
        })();
        // Receiver may already have timed out and dropped rx; that is fine, the
        // orphaned connect result is simply discarded.
        let _ = tx.send(result);
    });
    match rx.recv_timeout(connect_timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(anyhow::anyhow!(
            "IPC connect timeout ({}ms) for {}",
            connect_timeout.as_millis(),
            path.display()
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(anyhow::anyhow!("IPC connect thread disconnected"))
        }
    }
}

/// Send a JSON message to the plugin via socket IPC.
/// Returns Ok(response) if the plugin returns a terminal receipt, Err if socket unavailable.
pub fn send_message(project_root: &Path, message: &serde_json::Value) -> Result<Option<String>> {
    send_message_with_timeout(
        project_root,
        message,
        Duration::from_secs(IPC_RECEIPT_TIMEOUT_SECS),
    )
}

/// Send a JSON message with an explicit receipt timeout.
///
/// Most production sends use [`send_message`]. The explicit-timeout variant is
/// reserved for liveness probes that must not clear a degraded-socket latch
/// just because the OS accepted a connection while the plugin accept/apply loop
/// is no longer returning receipts.
pub fn send_message_with_timeout(
    project_root: &Path,
    message: &serde_json::Value,
    receipt_timeout: Duration,
) -> Result<Option<String>> {
    send_message_with_timeout_inner(
        project_root,
        message,
        receipt_timeout,
        PendingReceiptMode::WaitForTerminal,
    )
}

#[derive(Clone, Copy)]
enum PendingReceiptMode {
    WaitForTerminal,
}

fn send_message_with_timeout_inner(
    project_root: &Path,
    message: &serde_json::Value,
    receipt_timeout: Duration,
    pending_mode: PendingReceiptMode,
) -> Result<Option<String>> {
    let stream = try_connect(project_root)?;

    // Bound the outbound write (wedge A): a plugin that accepted the connection
    // but stopped draining its recv buffer would otherwise block `write_all`
    // forever - before the bounded receipt-read below is ever reached - on a patch
    // payload larger than the socket buffer. With a send timeout the write fails
    // closed on timeout and the degraded-socket circuit breaker takes over.
    if let Err(e) = stream.set_send_timeout(Some(receipt_timeout)) {
        eprintln!("[ipc-socket] warning: failed to set IPC send timeout: {e}");
    }
    if let Err(e) = stream.set_recv_timeout(Some(receipt_timeout)) {
        eprintln!("[ipc-socket] warning: failed to set IPC receipt timeout: {e}");
    }

    // interprocess Stream implements Read + Write via halves
    let (reader_half, mut writer_half) = stream.split();

    // Send NDJSON message. When early receipt is enabled, tag outgoing `patch`
    // messages so an early-receipt-aware listener emits an `accepted` receipt
    // before apply. Non-patch messages (queue convergence, etc.) are sent
    // verbatim.
    let outgoing = early_receipt_tagged_message(message, EARLY_RECEIPT_ENABLED);
    let mut msg = serde_json::to_string(&outgoing)?;
    msg.push('\n');
    writer_half.write_all(msg.as_bytes())?;
    writer_half.flush()?;

    // Each phase gets its own receipt-timeout budget: an early `accepted`
    // receipt proves liveness and lets the binary keep waiting for the terminal
    // receipt instead of declaring a false timeout while the plugin is still
    // applying.
    let mut reader = BufReader::new(reader_half);
    loop {
        let mut receipt_line = String::new();
        match reader.read_line(&mut receipt_line) {
            Ok(0) => {
                return Err(anyhow::anyhow!(
                    "IPC receipt: plugin closed connection without responding"
                ));
            }
            Ok(_) => {
                let receipt = receipt_line.trim().to_string();
                match classify_socket_receipt(&receipt) {
                    // Liveness-only: listener received the message but has not
                    // applied it yet. Callers keep waiting for the terminal
                    // receipt so receipt success means the plugin-side action
                    // actually completed.
                    SocketReceiptClassification::Pending => match pending_mode {
                        PendingReceiptMode::WaitForTerminal => continue,
                    },
                    SocketReceiptClassification::Applied => return Ok(Some(receipt)),
                    SocketReceiptClassification::AlreadyApplied => {
                        return Err(anyhow::anyhow!("IPC receipt already_applied: {}", receipt));
                    }
                    SocketReceiptClassification::Rejected => {
                        return Err(anyhow::anyhow!("IPC receipt rejected: {}", receipt));
                    }
                    SocketReceiptClassification::Unsupported => {
                        return Err(anyhow::anyhow!(
                            "IPC receipt unsupported legacy response: {}; update/reinstall the editor plugin/native library so it publishes lazily transport receipts",
                            receipt
                        ));
                    }
                }
            }
            Err(e) if ipc_read_error_is_timeout(&e) => {
                return Err(anyhow::anyhow!(
                    "IPC receipt timeout ({}ms)",
                    receipt_timeout.as_millis()
                ));
            }
            Err(e) => return Err(anyhow::anyhow!("IPC receipt read error: {}", e)),
        }
    }
}

/// Probe whether the socket listener can accept and receipt a lightweight message.
pub fn probe_listener_receipt(project_root: &Path, timeout: Duration) -> Result<bool> {
    let message = vcs_refresh_probe_message("ipc_degraded_self_heal");
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
static INFLIGHT_PUBLISH_LIVE_BUFFER: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// RAII guard decrements [`INFLIGHT_CONNECTION_HANDLERS`] on drop so a
/// panicking handler still releases its slot.
struct InflightConnectionGuard;

impl Drop for InflightConnectionGuard {
    fn drop(&mut self) {
        INFLIGHT_CONNECTION_HANDLERS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// `#jbacceptwedge`: current in-flight handler-thread count. Exposed for
/// tests so the regression can assert the listener actually reached a
/// concurrent state (rather than only asserting wall-clock timing).
pub fn inflight_connection_handlers() -> u64 {
    INFLIGHT_CONNECTION_HANDLERS.load(std::sync::atomic::Ordering::SeqCst)
}

enum PublishLiveBufferAdmission {
    Admitted(Option<PublishLiveBufferGuard>),
    Duplicate { key: String },
}

struct PublishLiveBufferGuard {
    key: String,
}

impl Drop for PublishLiveBufferGuard {
    fn drop(&mut self) {
        if let Some(projection) = INFLIGHT_PUBLISH_LIVE_BUFFER.get()
            && let Ok(mut keys) = projection.lock()
        {
            keys.remove(&self.key);
        }
    }
}

fn publish_live_buffer_projection() -> &'static Mutex<HashSet<String>> {
    INFLIGHT_PUBLISH_LIVE_BUFFER.get_or_init(|| Mutex::new(HashSet::new()))
}

fn publish_live_buffer_projection_key(project_root: &Path, message: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(message).ok()?;
    if parsed.get("type").and_then(|value| value.as_str()) != Some("publish_live_buffer") {
        return None;
    }
    let file = parsed.get("file").and_then(|value| value.as_str())?;
    Some(format!("{}::{file}", project_root.display()))
}

fn begin_publish_live_buffer_projection(
    project_root: &Path,
    message: &str,
) -> PublishLiveBufferAdmission {
    let Some(key) = publish_live_buffer_projection_key(project_root, message) else {
        return PublishLiveBufferAdmission::Admitted(None);
    };
    let mut keys = publish_live_buffer_projection()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if keys.contains(&key) {
        PublishLiveBufferAdmission::Duplicate { key }
    } else {
        keys.insert(key.clone());
        PublishLiveBufferAdmission::Admitted(Some(PublishLiveBufferGuard { key }))
    }
}

fn duplicate_publish_live_buffer_receipt() -> String {
    serde_json::json!({
        "type": "receipt",
        "status": "applied",
        "duplicate": true,
        "reason": "publish_live_buffer_duplicate"
    })
    .to_string()
}

fn ipc_read_error_is_timeout(err: &std::io::Error) -> bool {
    matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
        || err.to_string().contains("timed out")
}

fn ipc_accept_error_is_resource_exhaustion(err: &std::io::Error) -> bool {
    matches!(err.raw_os_error(), Some(23 | 24))
        || err.kind() == ErrorKind::OutOfMemory
        || err.to_string().contains("Too many open files")
}

/// Send a patch message to the plugin.
pub fn send_patch(
    project_root: &Path,
    file: &str,
    patches_json: &str,
    frontmatter_yaml: Option<&str>,
) -> Result<bool> {
    let patches = serde_json::from_str::<serde_json::Value>(patches_json)?;
    let message = patch_message(file, patches, frontmatter_yaml);

    match send_message(project_root, &message) {
        Ok(Some(receipt)) => {
            eprintln!("[ipc-socket] patch sent, receipt: {}", receipt);
            Ok(true)
        }
        Ok(None) => {
            eprintln!("[ipc-socket] patch sent, no receipt");
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
    let message = queue_convergence_message(file, queue_auto, frontmatter_yaml, queue_body);

    match send_message(project_root, &message) {
        Ok(Some(receipt)) => {
            eprintln!("[ipc-socket] queue convergence sent, receipt: {}", receipt);
            Ok(true)
        }
        Ok(None) => {
            eprintln!("[ipc-socket] queue convergence sent, no receipt");
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
    let message = reposition_message(file, boundary_id, preserve_head);

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
/// flushing the buffer to disk AND clearing the editor's dirty flag, then
/// publishes the saved buffer through the lazily visible-write receipt bridge
/// keyed by `patch_id` so the binary can read exactly what was persisted and
/// adopt it as a clean on-disk snapshot.
pub fn send_save_document(project_root: &Path, file: &str, patch_id: &str) -> Result<bool> {
    let message = save_document_message(file, patch_id);

    match send_message(project_root, &message) {
        Ok(Some(receipt)) => {
            eprintln!("[ipc-socket] save_document sent, receipt: {}", receipt);
            Ok(true)
        }
        Ok(None) => {
            eprintln!("[ipc-socket] save_document sent, no receipt");
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
    let message =
        refresh_content_message(file, content, expected_content_hash, expected_content_len);

    send_message(project_root, &message).map(|_| true)
}

/// Ask the live editor to republish its current visible-buffer sidecar for
/// `file` without changing the document.
pub fn send_publish_live_buffer(project_root: &Path, file: &str) -> Result<bool> {
    let message = publish_live_buffer_message(file);

    // This is a synchronization point for the CRDT relay, not a mere editor
    // liveness probe. Returning on the early `accepted` receipt lets the caller
    // poll the relay before the plugin has registered/refreshed its replica.
    send_message_with_timeout(
        project_root,
        &message,
        Duration::from_secs(IPC_RECEIPT_TIMEOUT_SECS),
    )
    .map(|_| true)
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
    let payload = publish_live_buffer_message(file);
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
/// visible buffer for `file` and publish a lazily visible-write receipt for
/// `patch_id`.
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
    let payload = save_document_message(file, patch_id);
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
    let message = vcs_refresh_message();

    send_message(project_root, &message).map(|_| true)
}

/// Start a socket listener (for use by the FFI library / plugin).
/// This blocks the calling thread — run it on a background thread.
#[allow(unreachable_code)]
pub fn start_listener<F>(project_root: &Path, handler: F) -> Result<()>
where
    F: Fn(&str) -> Option<String> + Send + Sync + 'static,
{
    start_listener_with_logger(project_root, handler, noop_ops_logger)
}

/// Start a socket listener with an injected best-effort ops logger.
///
/// This keeps the socket transport independent of orchestration while allowing
/// production callers to persist accept/early-receipt markers to `.agent-doc/logs`.
#[allow(unreachable_code)]
pub fn start_listener_with_logger<F>(
    project_root: &Path,
    handler: F,
    ops_logger: OpsLogger,
) -> Result<()>
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

    let mut resource_exhaustion_logged = false;
    loop {
        match listener.accept() {
            Ok(stream) => {
                resource_exhaustion_logged = false;
                let current_inflight =
                    INFLIGHT_CONNECTION_HANDLERS.load(std::sync::atomic::Ordering::SeqCst);
                if current_inflight >= IPC_LISTENER_MAX_INFLIGHT_HANDLERS {
                    ops_logger(
                        &root_buf,
                        &format!("ipc_accept_dropped_inflight_limit inflight={current_inflight}"),
                    );
                    drop(stream);
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                // #af88 B/D: bound the per-connection read so a half-open client
                // that connects but never sends a request line cannot park its
                // handler thread (and fd) forever. Set before `split()` since the
                // halves borrow the stream; a recv timeout surfaces as an error on
                // `read_line`, which the `.unwrap_or(0)` below treats as EOF and
                // exits the handler cleanly (releasing the inflight slot).
                if let Err(e) = stream
                    .set_recv_timeout(Some(Duration::from_secs(IPC_LISTENER_READ_TIMEOUT_SECS)))
                {
                    ops_logger(
                        &root_buf,
                        &format!("ipc_listener_set_recv_timeout_failed error={e}"),
                    );
                }
                let handler = std::sync::Arc::clone(&handler);
                let handler_root_buf = root_buf.clone();
                // #jbacceptwedge: count and log the fresh handler thread
                // BEFORE spawning, so the inflight count reported in the
                // marker reflects the post-increment state of this accept.
                let inflight = INFLIGHT_CONNECTION_HANDLERS
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                ops_logger(&handler_root_buf, &ipc_accept_thread_ops_marker(inflight));
                if let Err(e) = std::thread::Builder::new()
                    .name("agent-doc-ipc-conn".to_string())
                    .spawn(move || {
                        let _inflight_guard = InflightConnectionGuard;
                        let (reader_half, mut writer_half) = stream.split();
                        let mut reader = BufReader::new(reader_half);
                        let mut line = String::new();

                        while reader.read_line(&mut line).unwrap_or(0) > 0 {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                match begin_publish_live_buffer_projection(
                                    &handler_root_buf,
                                    trimmed,
                                ) {
                                    PublishLiveBufferAdmission::Duplicate { key } => {
                                        ops_logger(
                                            &handler_root_buf,
                                            &format!(
                                                "ipc_publish_live_buffer_duplicate_suppressed key={key}"
                                            ),
                                        );
                                        let mut resp = duplicate_publish_live_buffer_receipt();
                                        resp.push('\n');
                                        if let Err(e) = writer_half.write_all(resp.as_bytes()) {
                                            eprintln!(
                                                "[ipc-socket] duplicate receipt write error: {}",
                                                e
                                            );
                                        }
                                        if let Err(e) = writer_half.flush() {
                                            eprintln!(
                                                "[ipc-socket] duplicate receipt flush error: {}",
                                                e
                                            );
                                        }
                                    }
                                    PublishLiveBufferAdmission::Admitted(_publish_guard) => {
                                        // Early receipt: if the sender opted in, emit an `accepted`
                                        // receipt before the blocking apply handler runs, so the
                                        // sender's liveness probe is decoupled from apply latency.
                                        // The terminal receipt still follows.
                                        if message_requests_early_receipt(trimmed) {
                                            let mut early = early_receipt_line().to_string();
                                            early.push('\n');
                                            if let Err(e) =
                                                writer_half.write_all(early.as_bytes())
                                            {
                                                eprintln!(
                                                    "[ipc-socket] early receipt write error: {}",
                                                    e
                                                );
                                            } else if let Err(e) = writer_half.flush() {
                                                eprintln!(
                                                    "[ipc-socket] early receipt flush error: {}",
                                                    e
                                                );
                                            } else {
                                                // #saev prove/disprove: a successful early receipt emit
                                                // must leave grep-able proof that the `accepted` receipt
                                                // went out before the blocking apply.
                                                eprintln!(
                                                    "[ipc-socket] early receipt accepted emitted before apply"
                                                );
                                                // Also record the marker to ops.log (derived from the
                                                // listener's project root) so the #saev gate is provable.
                                                ops_logger(
                                                    &handler_root_buf,
                                                    early_receipt_ops_marker(),
                                                );
                                            }
                                        }
                                        if let Some(response) = handler(trimmed) {
                                            let mut resp = response;
                                            resp.push('\n');
                                            if let Err(e) = writer_half.write_all(resp.as_bytes()) {
                                                eprintln!(
                                                    "[ipc-socket] handler write error: {}",
                                                    e
                                                );
                                            }
                                            if let Err(e) = writer_half.flush() {
                                                eprintln!(
                                                    "[ipc-socket] handler flush error: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            line.clear();
                        }
                    })
                {
                    INFLIGHT_CONNECTION_HANDLERS
                        .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    ops_logger(
                        &root_buf,
                        &format!("ipc_accept_thread_spawn_failed error={e}"),
                    );
                }
            }
            Err(e) => {
                if ipc_accept_error_is_resource_exhaustion(&e) {
                    if !resource_exhaustion_logged {
                        eprintln!("[ipc-socket] accept resource exhaustion: {e}; backing off");
                        ops_logger(
                            &root_buf,
                            &format!("ipc_accept_resource_exhaustion error={e}"),
                        );
                        resource_exhaustion_logged = true;
                    }
                    std::thread::sleep(IPC_LISTENER_RESOURCE_BACKOFF);
                } else {
                    resource_exhaustion_logged = false;
                    eprintln!("[ipc-socket] accept error: {}", e);
                }
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
                Some(
                    serde_json::json!({"type":"receipt","status":"applied","id":v["type"]})
                        .to_string(),
                )
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
            assert!(
                r.is_some(),
                "concurrent send should still receive a receipt"
            );
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
    fn listener_suppresses_duplicate_publish_live_buffer_while_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let file = root.join("plan.md").to_string_lossy().to_string();

        let handler_calls = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let release =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));

        let root_clone = root.clone();
        let file_for_listener = file.clone();
        let handler_calls_for_listener = handler_calls.clone();
        let release_for_listener = release.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                if v.get("type").and_then(|value| value.as_str()) != Some("publish_live_buffer") {
                    return Some(
                        serde_json::json!({"type":"receipt","status":"rejected"}).to_string(),
                    );
                }
                assert_eq!(v["file"], file_for_listener);
                handler_calls_for_listener.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = started_tx.send(());

                let (lock, condvar) = &*release_for_listener;
                let mut released = lock.lock().unwrap();
                while !*released {
                    let wait = condvar
                        .wait_timeout(released, Duration::from_secs(2))
                        .unwrap();
                    released = wait.0;
                    if wait.1.timed_out() {
                        return Some(
                            serde_json::json!({
                                "type":"receipt",
                                "status":"rejected",
                                "reason":"test_timeout"
                            })
                            .to_string(),
                        );
                    }
                }
                Some(serde_json::json!({"type":"receipt","status":"applied"}).to_string())
            })
            .ok()
        });
        thread::sleep(Duration::from_millis(100));

        let root_for_first = root.clone();
        let file_for_first = file.clone();
        let first =
            thread::spawn(move || send_publish_live_buffer(&root_for_first, &file_for_first));
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first publish should enter the handler and hold the projection");

        let second = send_message_with_timeout(
            &root,
            &publish_live_buffer_message(&file),
            Duration::from_secs(1),
        )
        .expect("duplicate publish should receive a synthetic applied receipt")
        .expect("duplicate publish should return a receipt");
        let second_receipt: serde_json::Value = serde_json::from_str(&second).unwrap();
        assert_eq!(second_receipt["status"], "applied");
        assert_eq!(second_receipt["duplicate"], true);
        assert_eq!(
            handler_calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "duplicate publish_live_buffer should not invoke the plugin handler"
        );

        let (lock, condvar) = &*release;
        *lock.lock().unwrap() = true;
        condvar.notify_all();
        assert!(first.join().unwrap().unwrap());

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
                Some(
                    serde_json::json!({"type": "receipt", "status": "applied", "id": v["type"]})
                        .to_string(),
                )
            })
            .ok();
        });

        // Give the server time to start
        thread::sleep(Duration::from_millis(100));

        // Send a message
        let msg = serde_json::json!({"type": "vcs_refresh"});
        let result = send_message(&root, &msg).unwrap();
        assert!(result.is_some());
        let receipt: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(receipt["type"], "receipt");
        assert_eq!(receipt["status"], "applied");
        assert_eq!(receipt["id"], "vcs_refresh");

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
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        let ok = send_save_document(&root, "/tmp/plan.md", "save-pid-123").unwrap();
        assert!(ok, "save_document should succeed on an applied receipt");

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

        let (captured_tx, captured_rx) = std::sync::mpsc::channel();
        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let _ = captured_tx.send(v);
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        let ok = send_publish_live_buffer(&root, "/tmp/plan.md").unwrap();
        assert!(
            ok,
            "publish_live_buffer should succeed on an applied receipt"
        );

        let msg = captured_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("listener saw a message");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], "/tmp/plan.md");
        assert_eq!(msg["early_receipt"], true);
        assert!(message_requests_early_receipt(&msg.to_string()));
        assert!(
            msg.get("content").is_none() && msg.get("patches").is_none(),
            "publish_live_buffer must not carry document mutation payload: {msg}"
        );

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

    #[test]
    fn send_publish_live_buffer_waits_for_terminal_applied_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let handler_started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler_started_for_listener = handler_started.clone();
        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, move |msg| {
                assert!(message_requests_early_receipt(msg));
                handler_started_for_listener.store(true, std::sync::atomic::Ordering::SeqCst);
                thread::sleep(Duration::from_millis(1500));
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        let start = Instant::now();
        let ok = send_publish_live_buffer(&root, "/tmp/plan.md").unwrap();
        let elapsed = start.elapsed();
        assert!(
            ok,
            "publish_live_buffer should succeed on terminal applied receipt"
        );
        assert!(
            elapsed >= Duration::from_millis(1200),
            "publish_live_buffer returned before terminal apply: elapsed={elapsed:?}"
        );

        for _ in 0..50 {
            if handler_started.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            handler_started.load(std::sync::atomic::Ordering::SeqCst),
            "listener handler should have run before send_publish_live_buffer returned"
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
        assert!(msg["issued_at_ms"].as_u64().unwrap() > 0);
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
    fn socket_rejected_receipt_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, |_msg| {
                Some(serde_json::json!({"type": "receipt", "status": "rejected"}).to_string())
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        let msg = serde_json::json!({"type": "patch"});
        let err = send_message(&root, &msg).unwrap_err().to_string();
        assert!(
            err.contains("IPC receipt rejected"),
            "rejected receipt should fail the socket IPC send, got: {err}"
        );

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

    #[test]
    fn send_message_handles_early_then_terminal_receipt() {
        // Full two-phase roundtrip: a flagged patch makes the listener emit a
        // `accepted` receipt before apply, then the terminal receipt after the
        // handler runs. send_message must skip the accepted receipt and return
        // the terminal result.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, |msg| {
                // Prove the early receipt already went out before this apply runs.
                assert!(message_requests_early_receipt(msg));
                thread::sleep(Duration::from_millis(50));
                Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
            })
            .ok();
        });

        thread::sleep(Duration::from_millis(100));

        // Manually flagged so the listener early-receipts independent of sender
        // auto-injection.
        let msg = serde_json::json!({"type": "patch", "early_receipt": true});
        let result = send_message(&root, &msg).unwrap();
        let receipt: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(
            receipt["status"], "applied",
            "terminal receipt must be returned, not accepted"
        );

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }
}

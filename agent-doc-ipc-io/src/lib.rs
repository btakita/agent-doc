//! Socket-based IPC for editor plugin communication.
//!
//! Uses Unix domain sockets (Linux/macOS) or Windows named pipes via the
//! `interprocess` crate. The socket replaces the file-based IPC mechanism
//! (NIO WatchService + patch files) for lower latency and no inotify issues.
//!
//! ## Architecture
//!
//! - **Listener** (plugin side): each editor process starts a socket listener
//!   at `.agent-doc/ipc-<pid>.sock`. The pid is the same identity carried by the
//!   Lazily editor registration, so multi-head delivery cannot hit another editor.
//! - **Sender** (CLI side): The `agent-doc write` command connects to the socket
//!   and sends patch JSON to that registered editor endpoint. An unavailable
//!   endpoint retains the state-machine transition for retry; no patch file is emitted.
//!
//! ## Protocol
//!
//! Messages are newline-delimited JSON (NDJSON). Each message is a single line
//! terminated by `\n`. Every connection starts with `ipc_hello` /
//! `ipc_hello_ack` build-and-protocol negotiation. No editor intent reaches the
//! plugin callback until that handshake succeeds.
//!
//! Message types:
//! - `{"type": "apply_canonical", "file": "...", "patches": [...], "frontmatter": "..."}` — apply canonical deltas
//! - `{"type": "reposition", "file": "...", "boundary_id": "..."}` — reposition
//!   boundary marker; `boundary_id` is optional and lets the plugin reuse the
//!   already-committed marker instead of generating a fresh boundary-only diff
//! - `{"type": "refresh_content", "file": "...", "content": "...",
//!   "expected_content_hash": "...", "expected_content_len": N}` — replace a
//!   stale editor buffer with committed content after a HEAD-authoritative repair
//! - `{"type": "observe_lazily_current", "file": "...", "early_receipt": true}` —
//!   ask the editor to observe Lazily's current value without mutating the document
//! - `{"type": "refresh_vcs"}` — trigger VCS refresh
//! - `{"type": "receipt", "status": "applied"}` — terminal plugin receipt
//!
//! VS Code and JetBrains continuously observe Lazily current and receive the
//! same typed socket messages; neither plugin consumes filesystem delivery signals.

use agent_doc_ipc_protocol::{
    IPC_PROTOCOL_VERSION, IpcHandshakeError, IpcPeerIdentity, SocketReceiptClassification,
    classify_socket_receipt, early_receipt_line, early_receipt_ops_marker,
    early_receipt_tagged_message, ipc_accept_thread_ops_marker, ipc_handshake_rejection,
    ipc_hello_ack_message, ipc_hello_message, message_is_reload_library,
    message_requests_early_receipt, observe_lazily_current_message, persist_current_message,
    refresh_content_message, reload_lib_message, validate_ipc_hello, validate_ipc_hello_ack,
    vcs_refresh_message,
};
use anyhow::{Context, Result};
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, ToFsName,
    traits::{Listener as _, Stream as _},
};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub mod editor_target;

const SOCKET_FILENAME_PREFIX: &str = "ipc";

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
/// Dedicated receipt budget for the `reload_library` generation handoff
/// (`#replicarefusalstorm` / `#editorendpointzero`). The handoff drains in-flight
/// native calls, unmaps the old cdylib, proves the mapping is gone, loads the
/// replacement, and re-registers open projects — easily 10s+ under load, and the
/// reason `reload-lib` reported endpoints "failed" while the editor listener was
/// demonstrably alive: the generic 6s receipt timeout fired mid-handoff, so the
/// reload was delivered but never acknowledged, leaving the editor stranded on the
/// stale cdylib. This budget is independent of the generic receipt timeout so a
/// slow generation swap is not misread as a wedged listener.
const IPC_RELOAD_LIBRARY_RECEIPT_TIMEOUT_SECS: u64 = 90;
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
static LOCAL_IPC_BUILD_ID: OnceLock<String> = OnceLock::new();

/// Inject the top-level binary/native-library build identity used by the IPC
/// handshake. Repeating the same identity is idempotent; changing it inside one
/// loaded process is rejected because that would make compatibility depend on
/// call order.
pub fn set_local_build_id(build_id: &str) -> Result<()> {
    if build_id.trim().is_empty() {
        return Err(anyhow::anyhow!("IPC build id must not be empty"));
    }
    if let Some(existing) = LOCAL_IPC_BUILD_ID.get() {
        if existing == build_id {
            return Ok(());
        }
        return Err(anyhow::anyhow!(
            "IPC build id already initialized as {existing}, refusing {build_id}"
        ));
    }
    LOCAL_IPC_BUILD_ID
        .set(build_id.to_string())
        .map_err(|_| anyhow::anyhow!("IPC build id initialization raced"))
}

fn local_ipc_identity() -> IpcPeerIdentity {
    IpcPeerIdentity::new(
        IPC_PROTOCOL_VERSION,
        LOCAL_IPC_BUILD_ID
            .get()
            .cloned()
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
    )
}

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

/// Get the endpoint for the current process. Delivery code should prefer
/// [`socket_path_for_pid`] with the pid from the selected Lazily registration.
pub fn socket_path(project_root: &Path) -> PathBuf {
    socket_path_for_pid(project_root, u64::from(std::process::id()))
}

pub fn socket_path_for_pid(project_root: &Path, pid: u64) -> PathBuf {
    project_root
        .join(".agent-doc")
        .join(format!("{SOCKET_FILENAME_PREFIX}-{pid}.sock"))
}

/// Check if a socket listener is active.
pub fn is_listener_active(project_root: &Path) -> bool {
    is_listener_active_for_pid(project_root, u64::from(std::process::id()))
}

/// Discover live editor endpoints from the socket files themselves, independent
/// of the reliable-sync registration record (`#editorendpointzero`).
///
/// Endpoint fan-out normally enumerates `reliable_sync_status().registrations`.
/// When that record is empty the fan-out reports `0/0` and gives up — even though
/// the editor may be alive and *actively listening* on its PID-scoped socket. That
/// is the unrecoverable wedge: `admin reload-lib` becomes a no-op with nothing to
/// deliver to, `admin recycle` does not touch the editor, and only an operator
/// reopening the tab or restarting the IDE clears it.
///
/// The socket path is deterministic (`.agent-doc/<prefix>-<pid>.sock`) and
/// [`is_listener_active_for_pid`] proves liveness by actually connecting (and
/// reaps the file when it cannot), so discovery is sound: every returned pid has a
/// live listener behind it as of this call. The caller's own pid is excluded so a
/// process never fans out to itself.
pub fn discover_listening_editor_pids(project_root: &Path) -> Vec<u64> {
    let dir = project_root.join(".agent-doc");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        // Never swallow: a project root without a readable .agent-doc cannot be
        // fanned out to, and silently returning empty looks like "no editors".
        Err(err) => {
            eprintln!(
                "[ipc] endpoint discovery skipped for {}: {}",
                dir.display(),
                err
            );
            return Vec::new();
        }
    };

    let own_pid = u64::from(std::process::id());
    let prefix = format!("{SOCKET_FILENAME_PREFIX}-");
    let mut pids: Vec<u64> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let pid = name.strip_prefix(&prefix)?.strip_suffix(".sock")?;
            pid.parse::<u64>().ok()
        })
        .filter(|pid| *pid != own_pid)
        .filter(|pid| is_listener_active_for_pid(project_root, *pid))
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

pub fn is_listener_active_for_pid(project_root: &Path, pid: u64) -> bool {
    let sock = socket_path_for_pid(project_root, pid);
    if !sock.exists() {
        return false;
    }
    // Try connecting — if it succeeds, the listener is active
    match try_connect_for_pid(project_root, pid) {
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
    try_connect_for_pid(project_root, u64::from(std::process::id()))
}

fn try_connect_for_pid(
    project_root: &Path,
    pid: u64,
) -> Result<interprocess::local_socket::Stream> {
    try_connect_with_timeout_for_pid(
        project_root,
        pid,
        Duration::from_secs(IPC_CONNECT_TIMEOUT_SECS),
    )
}

/// Connect with a bounded deadline (`#af88` F). `connect_sync` is blocking with
/// no native timeout, so run it on a watchdog thread and fail closed after
/// `connect_timeout` instead of hanging the caller on a wedged peer.
fn try_connect_with_timeout_for_pid(
    project_root: &Path,
    pid: u64,
    connect_timeout: Duration,
) -> Result<interprocess::local_socket::Stream> {
    let path = socket_path_for_pid(project_root, pid);
    let path_for_thread = path.clone();
    run_connect_with_timeout(&path, connect_timeout, move || {
        let name = path_for_thread.to_fs_name::<GenericFilePath>()?;
        interprocess::local_socket::ConnectOptions::new()
            .name(name)
            .connect_sync()
            .context("failed to connect to IPC socket")
    })
}

fn run_connect_with_timeout<T, F>(path: &Path, connect_timeout: Duration, connect: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = connect();
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

pub fn send_message_to_pid(
    project_root: &Path,
    pid: u64,
    message: &serde_json::Value,
) -> Result<Option<String>> {
    send_message_to_pid_with_timeout(
        project_root,
        pid,
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
        None,
        message,
        receipt_timeout,
        PendingReceiptMode::WaitForTerminal,
    )
}

pub fn send_message_to_pid_with_timeout(
    project_root: &Path,
    pid: u64,
    message: &serde_json::Value,
    receipt_timeout: Duration,
) -> Result<Option<String>> {
    send_message_with_timeout_inner(
        project_root,
        Some(pid),
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
    target_pid: Option<u64>,
    message: &serde_json::Value,
    receipt_timeout: Duration,
    pending_mode: PendingReceiptMode,
) -> Result<Option<String>> {
    send_message_with_timeout_inner_with_identity(
        project_root,
        target_pid,
        message,
        receipt_timeout,
        pending_mode,
        &local_ipc_identity(),
    )
}

fn send_message_with_timeout_inner_with_identity(
    project_root: &Path,
    target_pid: Option<u64>,
    message: &serde_json::Value,
    receipt_timeout: Duration,
    pending_mode: PendingReceiptMode,
    client_identity: &IpcPeerIdentity,
) -> Result<Option<String>> {
    let stream = match target_pid {
        Some(pid) => try_connect_for_pid(project_root, pid)?,
        None => try_connect(project_root)?,
    };

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
    let mut reader = BufReader::new(reader_half);

    perform_client_handshake(
        &mut reader,
        &mut writer_half,
        client_identity,
        receipt_timeout,
    )?;

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

fn perform_client_handshake<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    client_identity: &IpcPeerIdentity,
    timeout: Duration,
) -> Result<()> {
    let mut hello = serde_json::to_string(&ipc_hello_message(client_identity))?;
    hello.push('\n');
    writer
        .write_all(hello.as_bytes())
        .context("IPC handshake write error")?;
    writer.flush().context("IPC handshake flush error")?;

    let mut ack = String::new();
    match reader.read_line(&mut ack) {
        Ok(0) => Err(anyhow::anyhow!(
            "IPC handshake: plugin closed connection without negotiating"
        )),
        Ok(_) => validate_ipc_hello_ack(ack.trim(), client_identity)
            .map_err(anyhow::Error::new)
            .context("IPC handshake rejected"),
        Err(error) if ipc_read_error_is_timeout(&error) => Err(anyhow::anyhow!(
            "IPC handshake timeout ({}ms)",
            timeout.as_millis()
        )),
        Err(error) => Err(anyhow::anyhow!("IPC handshake read error: {error}")),
    }
}

fn is_ipc_handshake_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<IpcHandshakeError>().is_some())
        || error.to_string().starts_with("IPC handshake")
}

/// Whether an IPC failure proves that the sender and editor listener are
/// running different builds.
///
/// Recovery policy lives above this transport crate: a stale editor can use
/// the reload-only compatibility path, while a stale project controller must
/// recycle itself. Exposing the typed distinction prevents callers from
/// blindly reloading the wrong side of the connection.
pub fn is_ipc_build_mismatch_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<IpcHandshakeError>(),
            Some(IpcHandshakeError::BuildMismatch { .. })
        )
    })
}

fn send_legacy_reload_to_pid(
    project_root: &Path,
    pid: u64,
    message: &serde_json::Value,
    receipt_timeout: Duration,
) -> Result<Option<String>> {
    if !message_is_reload_library(&message.to_string()) {
        return Err(anyhow::anyhow!(
            "pre-handshake compatibility path accepts reload_library only"
        ));
    }
    let stream = try_connect_for_pid(project_root, pid)?;
    if let Err(error) = stream.set_send_timeout(Some(receipt_timeout)) {
        eprintln!("[ipc-socket] warning: failed to set legacy reload send timeout: {error}");
    }
    if let Err(error) = stream.set_recv_timeout(Some(receipt_timeout)) {
        eprintln!("[ipc-socket] warning: failed to set legacy reload receipt timeout: {error}");
    }
    let (reader_half, mut writer_half) = stream.split();
    let mut payload = serde_json::to_string(message)?;
    payload.push('\n');
    writer_half.write_all(payload.as_bytes())?;
    writer_half.flush()?;

    let mut reader = BufReader::new(reader_half);
    let mut receipt = String::new();
    match reader.read_line(&mut receipt) {
        Ok(0) => Err(anyhow::anyhow!(
            "legacy reload: plugin closed connection without responding"
        )),
        Ok(_) => {
            let receipt = receipt.trim().to_string();
            match classify_socket_receipt(&receipt) {
                SocketReceiptClassification::Applied => Ok(Some(receipt)),
                SocketReceiptClassification::AlreadyApplied => Ok(Some(receipt)),
                SocketReceiptClassification::Pending => Err(anyhow::anyhow!(
                    "legacy reload returned non-terminal receipt: {receipt}"
                )),
                SocketReceiptClassification::Rejected
                | SocketReceiptClassification::Unsupported => Err(anyhow::anyhow!(
                    "legacy reload rejected or unsupported: {receipt}"
                )),
            }
        }
        Err(error) if ipc_read_error_is_timeout(&error) => Err(anyhow::anyhow!(
            "legacy reload receipt timeout ({}ms)",
            receipt_timeout.as_millis()
        )),
        Err(error) => Err(anyhow::anyhow!("legacy reload receipt read error: {error}")),
    }
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
static INFLIGHT_OBSERVE_LAZILY_CURRENT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

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

enum ObserveLazilyCurrentAdmission {
    Admitted(Option<ObserveLazilyCurrentGuard>),
    Duplicate { key: String },
}

struct ObserveLazilyCurrentGuard {
    key: String,
}

impl Drop for ObserveLazilyCurrentGuard {
    fn drop(&mut self) {
        if let Some(projection) = INFLIGHT_OBSERVE_LAZILY_CURRENT.get() {
            projection.lock().remove(&self.key);
        }
    }
}

fn observe_lazily_current_projection() -> &'static Mutex<HashSet<String>> {
    INFLIGHT_OBSERVE_LAZILY_CURRENT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn observe_lazily_current_projection_key(project_root: &Path, message: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(message).ok()?;
    if parsed.get("type").and_then(|value| value.as_str()) != Some("observe_lazily_current") {
        return None;
    }
    let file = parsed.get("file").and_then(|value| value.as_str())?;
    Some(format!("{}::{file}", project_root.display()))
}

fn begin_observe_lazily_current_projection(
    project_root: &Path,
    message: &str,
) -> ObserveLazilyCurrentAdmission {
    let Some(key) = observe_lazily_current_projection_key(project_root, message) else {
        return ObserveLazilyCurrentAdmission::Admitted(None);
    };
    let mut keys = observe_lazily_current_projection().lock();
    if keys.contains(&key) {
        ObserveLazilyCurrentAdmission::Duplicate { key }
    } else {
        keys.insert(key.clone());
        ObserveLazilyCurrentAdmission::Admitted(Some(ObserveLazilyCurrentGuard { key }))
    }
}

fn duplicate_observe_lazily_current_receipt() -> String {
    serde_json::json!({
        "type": "receipt",
        "status": "applied",
        "duplicate": true,
        "reason": "observe_lazily_current_duplicate"
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
fn send_terminal_protocol_stable_message_with_listener_build_retry(
    project_root: &Path,
    editor_pid: u64,
    message: &serde_json::Value,
    operation: &str,
) -> Result<Option<String>> {
    let send = |identity: &IpcPeerIdentity| {
        send_message_with_timeout_inner_with_identity(
            project_root,
            Some(editor_pid),
            message,
            Duration::from_secs(IPC_RECEIPT_TIMEOUT_SECS),
            PendingReceiptMode::WaitForTerminal,
            identity,
        )
    };

    let receipt = match send(&local_ipc_identity()) {
        Ok(receipt) => receipt,
        Err(error) => {
            let Some(IpcHandshakeError::BuildMismatch { received, .. }) =
                error.downcast_ref::<IpcHandshakeError>()
            else {
                return Err(error);
            };
            eprintln!(
                "[ipc-socket] {operation} build compatibility retry: listener_build={received}"
            );
            send(&IpcPeerIdentity::new(
                IPC_PROTOCOL_VERSION,
                received.clone(),
            ))
            .with_context(|| {
                format!(
                    "{operation} IPC compatibility retry failed after build mismatch: {error:#}"
                )
            })?
        }
    };

    Ok(receipt)
}

/// Send committed content to the editor when the binary has just repaired a
/// stale post-commit working tree back to HEAD.
///
/// The expected hash/length describe the stale editor content that is safe to
/// replace. The plugin must reject the message if the live document changed
/// before it applies the refresh.
pub fn send_refresh_content_to_editor(
    project_root: &Path,
    editor_pid: u64,
    editor_id: &str,
    file: &str,
    content: &str,
    expected_content_hash: &str,
    expected_content_len: usize,
) -> Result<bool> {
    let mut message =
        refresh_content_message(file, content, expected_content_hash, expected_content_len);
    message["editor_id"] = serde_json::Value::String(editor_id.to_string());
    message["editor_pid"] = serde_json::Value::from(editor_pid);

    send_message_to_pid(project_root, editor_pid, &message).map(|_| true)
}

/// Ask the live editor to observe Lazily current for `file` without changing
/// the document.
pub fn send_observe_lazily_current_to_editor(
    project_root: &Path,
    editor_pid: u64,
    editor_id: &str,
    file: &str,
) -> Result<bool> {
    send_observe_lazily_current_to_editor_with_timeout(
        project_root,
        editor_pid,
        editor_id,
        file,
        Duration::from_secs(IPC_RECEIPT_TIMEOUT_SECS),
    )
}

/// Ask the editor to observe Lazily current, bounded by the caller's
/// authority-recovery budget rather than the generic IPC receipt timeout.
pub fn send_observe_lazily_current_to_editor_with_timeout(
    project_root: &Path,
    editor_pid: u64,
    editor_id: &str,
    file: &str,
    timeout: Duration,
) -> Result<bool> {
    let mut message = observe_lazily_current_message(file);
    message["editor_id"] = serde_json::Value::String(editor_id.to_string());
    message["editor_pid"] = serde_json::Value::from(editor_pid);

    // This is a synchronization point for the CRDT relay, not a mere editor
    // liveness probe. Returning on the early `accepted` receipt lets the caller
    // poll the relay before the plugin has registered/refreshed its replica.
    send_message_to_pid_with_timeout(project_root, editor_pid, &message, timeout).map(|_| true)
}

/// Ask one explicitly registered editor endpoint to persist the exact visible
/// revision through its native save lifecycle. A terminal receipt is required:
/// the early/accepted receipt used by observation would race disk projection.
pub fn send_persist_current_to_editor(
    project_root: &Path,
    editor_pid: u64,
    editor_id: &str,
    file: &str,
    expected_content_hash: &str,
    expected_content_len: usize,
) -> Result<bool> {
    let mut message = persist_current_message(file, expected_content_hash, expected_content_len);
    message["editor_id"] = serde_json::Value::String(editor_id.to_string());
    message["editor_pid"] = serde_json::Value::from(editor_pid);
    send_message_to_pid(project_root, editor_pid, &message).map(|_| true)
}

/// Re-register the editor's current document immediately after that same
/// endpoint returned an applied save-only receipt.
///
/// A freshly installed CLI may be one build ahead of an already-running editor
/// listener. This narrowly scoped post-save observation may therefore retry a
/// same-protocol listener using its reported build identity. Callers must not
/// use this as a general build-fence bypass: the preceding editor save is the
/// proof that makes the visible baseline safe to register and lets any retained
/// semantic write resume over it.
pub fn send_observe_lazily_current_after_editor_save_to_editor(
    project_root: &Path,
    editor_pid: u64,
    editor_id: &str,
    file: &str,
) -> Result<bool> {
    let mut message = observe_lazily_current_message(file);
    message["editor_id"] = serde_json::Value::String(editor_id.to_string());
    message["editor_pid"] = serde_json::Value::from(editor_pid);

    let receipt = send_terminal_protocol_stable_message_with_listener_build_retry(
        project_root,
        editor_pid,
        &message,
        "post-save observe",
    )?;
    match receipt {
        Some(receipt) => {
            eprintln!("[ipc-socket] post-save observe_lazily_current applied, receipt: {receipt}");
            Ok(true)
        }
        None => {
            eprintln!("[ipc-socket] post-save observe_lazily_current had no terminal receipt");
            Ok(false)
        }
    }
}

/// Send a VCS refresh signal.
pub fn send_vcs_refresh_to_editor(
    project_root: &Path,
    editor_pid: u64,
    editor_id: &str,
    file: &str,
) -> Result<bool> {
    let mut message = vcs_refresh_message(file);
    message["editor_id"] = serde_json::Value::String(editor_id.to_string());
    message["editor_pid"] = serde_json::Value::from(editor_pid);

    send_message_to_pid(project_root, editor_pid, &message).map(|_| true)
}

/// Ask one registered editor process to reload the installed native library.
///
/// Reload is a typed, PID-scoped control intent. It never falls back to a
/// broadcast file, so a stale or unrelated editor process cannot consume it.
pub fn send_reload_library_to_editor(
    project_root: &Path,
    editor_pid: u64,
    editor_id: &str,
    lib_version: &str,
) -> Result<bool> {
    let mut message = reload_lib_message(lib_version);
    message["editor_id"] = serde_json::Value::String(editor_id.to_string());
    message["editor_pid"] = serde_json::Value::from(editor_pid);

    match send_message_to_pid(project_root, editor_pid, &message) {
        Ok(_) => Ok(true),
        Err(handshake_error) if is_ipc_handshake_error(&handshake_error) => {
            eprintln!(
                "[ipc-socket] incompatible listener handshake; attempting reload-only compatibility path: {handshake_error:#}"
            );
            send_legacy_reload_to_pid(
                project_root,
                editor_pid,
                &message,
                Duration::from_secs(IPC_RELOAD_LIBRARY_RECEIPT_TIMEOUT_SECS),
            )
            .with_context(|| {
                format!(
                    "IPC listener version mismatch and reload-only compatibility request failed; original negotiation error: {handshake_error:#}"
                )
            })
            .map(|_| true)
        }
        Err(error) => Err(error),
    }
}

/// Outcome of sending an editor intent across a rolling native-library build
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMismatchSendOutcome {
    /// The original editor intent passed normal IPC negotiation.
    Delivered,
    /// The original intent was rejected by build negotiation, but the
    /// reload-only compatibility request was accepted. The editor will
    /// re-register and pull any retained CRDT work after the native handoff.
    ReloadRequested,
}

/// Send an editor intent and recover a typed IPC build mismatch by requesting
/// the listener's reload-only compatibility path.
///
/// Ordinary mutation intents remain fail-closed across mismatched builds. This
/// helper does not replay the rejected intent: it asks the editor to reload and
/// relies on the caller's retained CRDT frontier to be pulled after
/// re-registration. If reload delivery fails, the returned error preserves the
/// original typed build mismatch in its source chain.
pub fn send_message_to_pid_recovering_build_mismatch(
    project_root: &Path,
    editor_pid: u64,
    editor_id: &str,
    message: &serde_json::Value,
    lib_version: &str,
) -> Result<BuildMismatchSendOutcome> {
    match send_message_to_pid(project_root, editor_pid, message) {
        Ok(_) => Ok(BuildMismatchSendOutcome::Delivered),
        Err(handshake_error) if is_ipc_build_mismatch_error(&handshake_error) => {
            match send_reload_library_to_editor(project_root, editor_pid, editor_id, lib_version) {
                Ok(true) => Ok(BuildMismatchSendOutcome::ReloadRequested),
                Ok(false) => Err(handshake_error
                    .context("IPC build mismatch recovery did not deliver reload_library")),
                Err(reload_error) => Err(handshake_error.context(format!(
                    "IPC build mismatch recovery failed to deliver reload_library: {reload_error:#}"
                ))),
            }
        }
        Err(error) => Err(error),
    }
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
    start_listener_with_logger_and_read_timeout(
        project_root,
        handler,
        ops_logger,
        Duration::from_secs(IPC_LISTENER_READ_TIMEOUT_SECS),
        None,
        local_ipc_identity(),
    )
}

/// Start a listener that can be quiesced and joined by its owner.
///
/// Setting `shutdown` to `true` does not itself wake a blocked `accept`; call
/// [`wake_listener`] after setting it. Once woken, the accept loop stops taking
/// work, joins every bounded per-connection handler, removes the socket, and
/// returns. This is the native-library generation handoff boundary used by
/// reloadable editor adapters.
pub fn start_listener_with_logger_until<F>(
    project_root: &Path,
    handler: F,
    ops_logger: OpsLogger,
    shutdown: Arc<AtomicBool>,
) -> Result<()>
where
    F: Fn(&str) -> Option<String> + Send + Sync + 'static,
{
    start_listener_with_logger_and_read_timeout(
        project_root,
        handler,
        ops_logger,
        Duration::from_secs(IPC_LISTENER_READ_TIMEOUT_SECS),
        Some(shutdown),
        local_ipc_identity(),
    )
}

/// Wake a listener blocked in `accept` so it can observe its shutdown token.
pub fn wake_listener(project_root: &Path) -> Result<()> {
    drop(try_connect(project_root)?);
    Ok(())
}

fn start_listener_with_logger_and_read_timeout<F>(
    project_root: &Path,
    handler: F,
    ops_logger: OpsLogger,
    listener_read_timeout: Duration,
    shutdown: Option<Arc<AtomicBool>>,
    listener_identity: IpcPeerIdentity,
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

    let name = sock_path.clone().to_fs_name::<GenericFilePath>()?;
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
    let mut connection_threads = Vec::new();
    loop {
        if shutdown
            .as_ref()
            .is_some_and(|token| token.load(Ordering::SeqCst))
        {
            break;
        }
        match listener.accept() {
            Ok(stream) => {
                if shutdown
                    .as_ref()
                    .is_some_and(|token| token.load(Ordering::SeqCst))
                {
                    drop(stream);
                    break;
                }
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
                if let Err(e) = stream.set_recv_timeout(Some(listener_read_timeout)) {
                    ops_logger(
                        &root_buf,
                        &format!("ipc_listener_set_recv_timeout_failed error={e}"),
                    );
                }
                let handler = std::sync::Arc::clone(&handler);
                let handler_root_buf = root_buf.clone();
                let listener_identity = listener_identity.clone();
                // #jbacceptwedge: count and log the fresh handler thread
                // BEFORE spawning, so the inflight count reported in the
                // marker reflects the post-increment state of this accept.
                let inflight = INFLIGHT_CONNECTION_HANDLERS
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                ops_logger(&handler_root_buf, &ipc_accept_thread_ops_marker(inflight));
                match std::thread::Builder::new()
                    .name("agent-doc-ipc-conn".to_string())
                    .spawn(move || {
                        let _inflight_guard = InflightConnectionGuard;
                    let (reader_half, mut writer_half) = stream.split();
                    let mut reader = BufReader::new(reader_half);
                    let mut line = String::new();
                    let mut handshake_complete = false;

                    while reader.read_line(&mut line).unwrap_or(0) > 0 {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            if !handshake_complete {
                                // Reload is the sole pre-handshake exception. It is a
                                // read-only generation handoff used to replace a peer
                                // too old to speak the current handshake.
                                if message_is_reload_library(trimmed) {
                                    if let Some(response) = handler(trimmed) {
                                        let mut response = response;
                                        response.push('\n');
                                        if let Err(error) =
                                            writer_half.write_all(response.as_bytes())
                                        {
                                            eprintln!(
                                                "[ipc-socket] reload compatibility receipt write error: {error}"
                                            );
                                        }
                                        if let Err(error) = writer_half.flush() {
                                            eprintln!(
                                                "[ipc-socket] reload compatibility receipt flush error: {error}"
                                            );
                                        }
                                    }
                                    line.clear();
                                    continue;
                                }
                                match validate_ipc_hello(trimmed, &listener_identity) {
                                    Ok(()) => {
                                        let mut ack =
                                            ipc_hello_ack_message(&listener_identity).to_string();
                                        ack.push('\n');
                                        if let Err(error) = writer_half.write_all(ack.as_bytes()) {
                                            eprintln!(
                                                "[ipc-socket] handshake ack write error: {error}"
                                            );
                                            break;
                                        }
                                        if let Err(error) = writer_half.flush() {
                                            eprintln!(
                                                "[ipc-socket] handshake ack flush error: {error}"
                                            );
                                            break;
                                        }
                                        handshake_complete = true;
                                    }
                                    Err(error) => {
                                        ops_logger(
                                            &handler_root_buf,
                                            &format!(
                                                "ipc_handshake_rejected reason={} detail={error}",
                                                error.reason()
                                            ),
                                        );
                                        let mut rejection =
                                            ipc_handshake_rejection(&error, &listener_identity);
                                        rejection.push('\n');
                                        if let Err(write_error) =
                                            writer_half.write_all(rejection.as_bytes())
                                        {
                                            eprintln!(
                                                "[ipc-socket] handshake rejection write error: {write_error}"
                                            );
                                        }
                                        if let Err(flush_error) = writer_half.flush() {
                                            eprintln!(
                                                "[ipc-socket] handshake rejection flush error: {flush_error}"
                                            );
                                        }
                                        break;
                                    }
                                }
                                line.clear();
                                continue;
                            }
                            match begin_observe_lazily_current_projection(
                                &handler_root_buf,
                                trimmed,
                                ) {
                                    ObserveLazilyCurrentAdmission::Duplicate { key } => {
                                        ops_logger(
                                            &handler_root_buf,
                                            &format!(
                                                "ipc_observe_lazily_current_duplicate_suppressed key={key}"
                                            ),
                                        );
                                        let mut resp = duplicate_observe_lazily_current_receipt();
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
                                    ObserveLazilyCurrentAdmission::Admitted(_publish_guard) => {
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
                    }) {
                    Ok(handle) => connection_threads.push(handle),
                    Err(e) => {
                        INFLIGHT_CONNECTION_HANDLERS
                            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                        ops_logger(
                            &root_buf,
                            &format!("ipc_accept_thread_spawn_failed error={e}"),
                        );
                    }
                }
                let mut still_running = Vec::new();
                for handle in connection_threads.drain(..) {
                    if handle.is_finished() {
                        let _ = handle.join();
                    } else {
                        still_running.push(handle);
                    }
                }
                connection_threads = still_running;
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

    for handle in connection_threads {
        let _ = handle.join();
    }
    if let Err(error) = std::fs::remove_file(&sock_path)
        && error.kind() != ErrorKind::NotFound
    {
        return Err(error).with_context(|| format!("remove listener socket {sock_path:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn connect_watchdog_returns_before_a_wedged_connect_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wedged-connect.sock");
        let started = Instant::now();
        let err = run_connect_with_timeout(&path, Duration::from_millis(100), || {
            thread::sleep(Duration::from_secs(1));
            Ok(())
        })
        .expect_err("a connect attempt beyond its deadline must fail closed");

        assert!(
            err.to_string().contains("IPC connect timeout (100ms)"),
            "unexpected timeout error: {err:#}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "watchdog waited for the wedged connect worker to finish"
        );
    }

    #[test]
    fn half_open_listener_connection_closes_after_read_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener_with_logger_and_read_timeout(
                &root_clone,
                |_| panic!("a half-open client must not reach the message handler"),
                noop_ops_logger,
                Duration::from_millis(100),
                None,
                local_ipc_identity(),
            )
            .ok()
        });
        thread::sleep(Duration::from_millis(100));

        let stream = try_connect_with_timeout_for_pid(
            &root,
            u64::from(std::process::id()),
            Duration::from_secs(1),
        )
        .expect("connect to test listener");
        stream
            .set_recv_timeout(Some(Duration::from_secs(2)))
            .expect("set client-side test backstop");
        let (mut reader, writer) = stream.split();
        let started = Instant::now();
        let mut byte = [0u8; 1];
        let read = reader
            .read(&mut byte)
            .expect("listener should close the half-open connection cleanly");
        assert_eq!(read, 0, "listener should close without a response body");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "listener did not enforce its per-connection read timeout"
        );

        drop(writer);
        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

    #[test]
    fn listener_rejects_legacy_mutation_before_plugin_callback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let callback_reached = Arc::new(AtomicBool::new(false));
        let listener_identity = IpcPeerIdentity::new(IPC_PROTOCOL_VERSION, "listener-build");

        let root_clone = root.clone();
        let shutdown_clone = Arc::clone(&shutdown);
        let callback_reached_clone = Arc::clone(&callback_reached);
        let server = thread::spawn(move || {
            start_listener_with_logger_and_read_timeout(
                &root_clone,
                move |_| {
                    callback_reached_clone.store(true, Ordering::SeqCst);
                    Some(r#"{"type":"receipt","status":"applied"}"#.to_string())
                },
                noop_ops_logger,
                Duration::from_secs(1),
                Some(shutdown_clone),
                listener_identity,
            )
        });
        wait_for_test_listener(&root);

        let stream = try_connect_with_timeout_for_pid(
            &root,
            u64::from(std::process::id()),
            Duration::from_secs(1),
        )
        .expect("connect to test listener");
        stream
            .set_recv_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let (reader_half, mut writer_half) = stream.split();
        writer_half
            .write_all(
                br#"{"type":"apply_canonical","file":"/tmp/plan.md","patches":[]}
"#,
            )
            .unwrap();
        writer_half.flush().unwrap();
        let mut reader = BufReader::new(reader_half);
        let mut rejection = String::new();
        reader.read_line(&mut rejection).unwrap();
        let rejection: serde_json::Value = serde_json::from_str(rejection.trim()).unwrap();

        assert_eq!(rejection["status"], "rejected");
        assert_eq!(rejection["reason"], "ipc_handshake_required");
        assert!(
            !callback_reached.load(Ordering::SeqCst),
            "legacy mutation reached the plugin callback before negotiation"
        );

        stop_test_listener(&root, shutdown, server);
    }

    #[test]
    fn build_skew_blocks_mutation_but_reload_compatibility_remains_available() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mutation_reached = Arc::new(AtomicBool::new(false));
        let reload_reached = Arc::new(AtomicBool::new(false));
        let listener_identity = IpcPeerIdentity::new(IPC_PROTOCOL_VERSION, "stale-listener-build");

        let root_clone = root.clone();
        let shutdown_clone = Arc::clone(&shutdown);
        let mutation_reached_clone = Arc::clone(&mutation_reached);
        let reload_reached_clone = Arc::clone(&reload_reached);
        let server = thread::spawn(move || {
            start_listener_with_logger_and_read_timeout(
                &root_clone,
                move |message| {
                    if message_is_reload_library(message) {
                        reload_reached_clone.store(true, Ordering::SeqCst);
                    } else {
                        mutation_reached_clone.store(true, Ordering::SeqCst);
                    }
                    Some(r#"{"type":"receipt","status":"applied"}"#.to_string())
                },
                noop_ops_logger,
                Duration::from_secs(2),
                Some(shutdown_clone),
                listener_identity,
            )
        });
        wait_for_test_listener(&root);

        let error = send_message_with_timeout_inner_with_identity(
            &root,
            Some(u64::from(std::process::id())),
            &serde_json::json!({"type": "apply_canonical", "file": "/tmp/plan.md"}),
            Duration::from_secs(1),
            PendingReceiptMode::WaitForTerminal,
            &IpcPeerIdentity::new(IPC_PROTOCOL_VERSION, "new-client-build"),
        )
        .expect_err("build skew must reject before mutation");
        assert!(
            format!("{error:#}").contains("IPC build mismatch"),
            "unexpected skew error: {error:#}"
        );
        assert!(
            is_ipc_build_mismatch_error(&error),
            "rolling-upgrade callers must receive the typed mismatch, not parse display text"
        );
        assert!(!mutation_reached.load(Ordering::SeqCst));

        assert_eq!(
            send_message_to_pid_recovering_build_mismatch(
                &root,
                u64::from(std::process::id()),
                "test-editor",
                &serde_json::json!({"type": "apply_canonical", "file": "/tmp/plan.md"}),
                env!("CARGO_PKG_VERSION"),
            )
            .expect("build mismatch should request the reload-only compatibility path"),
            BuildMismatchSendOutcome::ReloadRequested,
        );
        assert!(reload_reached.load(Ordering::SeqCst));
        assert!(!mutation_reached.load(Ordering::SeqCst));

        stop_test_listener(&root, shutdown, server);
    }

    fn wait_for_test_listener(root: &Path) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !socket_path(root).exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            socket_path(root).exists(),
            "listener socket was not published"
        );
    }

    fn stop_test_listener(
        root: &Path,
        shutdown: Arc<AtomicBool>,
        server: thread::JoinHandle<Result<()>>,
    ) {
        shutdown.store(true, Ordering::SeqCst);
        if let Err(error) = wake_listener(root) {
            assert!(
                !socket_path(root).exists(),
                "failed to wake a still-present test listener: {error:#}"
            );
        }
        server
            .join()
            .expect("listener thread should join")
            .expect("listener shutdown should succeed");
    }

    #[test]
    fn quiesced_listener_joins_connection_workers_and_removes_socket() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));

        let root_clone = root.clone();
        let shutdown_clone = Arc::clone(&shutdown);
        let server = thread::spawn(move || {
            start_listener_with_logger_until(
                &root_clone,
                |_| Some(r#"{"ok":true}"#.to_string()),
                noop_ops_logger,
                shutdown_clone,
            )
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while !socket_path(&root).exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            socket_path(&root).exists(),
            "listener socket was not published"
        );

        let mut stream = try_connect_with_timeout_for_pid(
            &root,
            u64::from(std::process::id()),
            Duration::from_secs(1),
        )
        .expect("connect to cancellable listener");
        stream
            .set_recv_timeout(Some(Duration::from_secs(2)))
            .expect("set client-side test backstop");
        stream.write_all(b"ping\n").expect("write request");
        drop(stream);

        shutdown.store(true, Ordering::SeqCst);
        // The listener may observe the token and unlink before this wake races
        // in; either outcome is a successful shutdown transition.
        let _ = wake_listener(&root);
        server
            .join()
            .expect("listener thread should join")
            .expect("listener shutdown should succeed");

        assert!(
            !socket_path(&root).exists(),
            "joined listener must remove its socket"
        );
    }

    /// `#editorendpointzero`: discovery must find a live listener that the
    /// reliable-sync registration record knows nothing about, and must reap a
    /// stale socket file rather than reporting it as an endpoint.
    #[test]
    fn discover_listening_editor_pids_finds_live_listeners_and_skips_stale_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        // A socket file with nothing behind it — the shape left by a dead editor.
        let stale_pid = 999_000_001u64;
        let stale = socket_path_for_pid(root, stale_pid);
        std::fs::write(&stale, b"").unwrap();

        // Nothing is listening, so discovery must return empty and reap the file.
        assert!(
            discover_listening_editor_pids(root).is_empty(),
            "a stale socket file is not a live endpoint"
        );
        assert!(
            !stale.exists(),
            "is_listener_active_for_pid must reap the stale socket file"
        );

        // Non-socket files and foreign names are ignored rather than parsed.
        std::fs::write(root.join(".agent-doc/controller.sock"), b"").unwrap();
        std::fs::write(root.join(".agent-doc/notes.md"), b"").unwrap();
        assert!(discover_listening_editor_pids(root).is_empty());
    }

    #[test]
    fn discover_listening_editor_pids_excludes_the_calling_process() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let own = u64::from(std::process::id());
        std::fs::write(socket_path_for_pid(root, own), b"").unwrap();
        assert!(
            discover_listening_editor_pids(root).is_empty(),
            "a process must never fan out a reload intent to itself"
        );
    }

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
    fn reload_library_receipt_outlives_a_slow_generation_handoff() {
        // Regression for `#replicarefusalstorm`: `reload-lib` reported endpoints
        // "failed" while the editor listener was demonstrably alive because the
        // native generation handoff (drain/unmap/load) exceeded the generic 6s
        // receipt timeout, so the reload was delivered but never acknowledged and
        // the editor stayed stranded on the stale cdylib. The reload_library
        // receipt now uses a dedicated budget larger than the generic timeout,
        // and `send_legacy_reload_to_pid` honors it against a slow-acking peer.
        assert!(
            IPC_RELOAD_LIBRARY_RECEIPT_TIMEOUT_SECS > IPC_RECEIPT_TIMEOUT_SECS,
            "reload_library receipt budget must outlive the generic receipt timeout"
        );
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, move |msg| {
                // Simulate a generation handoff that would exceed a tight timeout.
                thread::sleep(Duration::from_millis(800));
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                Some(
                    serde_json::json!({"type":"receipt","status":"applied","id":v["type"]})
                        .to_string(),
                )
            })
            .ok()
        });
        thread::sleep(Duration::from_millis(150));

        let pid = u64::from(std::process::id());
        let message = reload_lib_message("0.0.0-test");
        // The handoff-budgeted timeout the production reload path uses must
        // survive a slow handoff and return the terminal receipt (a generic
        // sub-second receipt timeout would have timed out mid-handoff).
        let result = send_legacy_reload_to_pid(
            &root,
            pid,
            &message,
            Duration::from_secs(IPC_RELOAD_LIBRARY_RECEIPT_TIMEOUT_SECS),
        );
        assert!(
            result.is_ok(),
            "handoff-budgeted reload receipt must survive the slow handoff: {result:?}"
        );
        drop(server);
    }

    #[test]
    fn listener_suppresses_duplicate_observe_lazily_current_while_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let file = root.join("plan.md").to_string_lossy().to_string();

        let handler_calls = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let release = std::sync::Arc::new((Mutex::new(false), parking_lot::Condvar::new()));

        let root_clone = root.clone();
        let file_for_listener = file.clone();
        let handler_calls_for_listener = handler_calls.clone();
        let release_for_listener = release.clone();
        let server = thread::spawn(move || {
            start_listener(&root_clone, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                if v.get("type").and_then(|value| value.as_str()) != Some("observe_lazily_current")
                {
                    return Some(
                        serde_json::json!({"type":"receipt","status":"rejected"}).to_string(),
                    );
                }
                assert_eq!(v["file"], file_for_listener);
                handler_calls_for_listener.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = started_tx.send(());

                let (lock, condvar) = &*release_for_listener;
                let mut released = lock.lock();
                while !*released {
                    let wait = condvar.wait_for(&mut released, Duration::from_secs(2));
                    if wait.timed_out() {
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
        let first = thread::spawn(move || {
            send_observe_lazily_current_to_editor(
                &root_for_first,
                u64::from(std::process::id()),
                "test-editor",
                &file_for_first,
            )
        });
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first publish should enter the handler and hold the projection");

        let second = send_message_with_timeout(
            &root,
            &observe_lazily_current_message(&file),
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
            "duplicate observe_lazily_current should not invoke the plugin handler"
        );

        let (lock, condvar) = &*release;
        *lock.lock() = true;
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
        let msg = serde_json::json!({"type": "refresh_vcs"});
        let result = send_message(&root, &msg).unwrap();
        assert!(result.is_some());
        let receipt: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(receipt["type"], "receipt");
        assert_eq!(receipt["status"], "applied");
        assert_eq!(receipt["id"], "refresh_vcs");

        // Clean up — remove socket to stop listener
        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

    #[test]
    fn post_save_observe_retries_a_same_protocol_older_build_listener() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let captured = std::sync::Arc::new(Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let shutdown = std::sync::Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let root_clone = root.clone();
        let server = thread::spawn(move || {
            start_listener_with_logger_and_read_timeout(
                &root_clone,
                move |msg| {
                    let value: serde_json::Value = serde_json::from_str(msg).ok()?;
                    *captured_clone.lock() = Some(value);
                    Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string())
                },
                noop_ops_logger,
                Duration::from_secs(1),
                Some(server_shutdown),
                IpcPeerIdentity::new(IPC_PROTOCOL_VERSION, "older-compatible-build"),
            )
            .unwrap();
        });

        thread::sleep(Duration::from_millis(100));

        let ok = send_observe_lazily_current_after_editor_save_to_editor(
            &root,
            u64::from(std::process::id()),
            "test-editor",
            "/tmp/plan.md",
        )
        .unwrap();
        assert!(ok);
        let message = captured.lock().clone().expect("listener saw a message");
        assert_eq!(message["type"], "observe_lazily_current");
        assert_eq!(message["file"], "/tmp/plan.md");
        assert_eq!(message["early_receipt"], true);
        assert!(message.get("content").is_none());
        assert!(message.get("patches").is_none());

        shutdown.store(true, Ordering::SeqCst);
        wake_listener(&root).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn send_observe_lazily_current_sends_readonly_file_message() {
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

        let ok = send_observe_lazily_current_to_editor(
            &root,
            u64::from(std::process::id()),
            "test-editor",
            "/tmp/plan.md",
        )
        .unwrap();
        assert!(
            ok,
            "observe_lazily_current should succeed on an applied receipt"
        );

        let msg = captured_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("listener saw a message");
        assert_eq!(msg["type"], "observe_lazily_current");
        assert_eq!(msg["file"], "/tmp/plan.md");
        assert_eq!(msg["early_receipt"], true);
        assert!(message_requests_early_receipt(&msg.to_string()));
        assert!(
            msg.get("content").is_none() && msg.get("patches").is_none(),
            "observe_lazily_current must not carry document mutation payload: {msg}"
        );

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
    }

    #[test]
    fn send_observe_lazily_current_waits_for_terminal_applied_receipt() {
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
        let ok = send_observe_lazily_current_to_editor(
            &root,
            u64::from(std::process::id()),
            "test-editor",
            "/tmp/plan.md",
        )
        .unwrap();
        let elapsed = start.elapsed();
        assert!(
            ok,
            "observe_lazily_current should succeed on terminal applied receipt"
        );
        assert!(
            elapsed >= Duration::from_millis(1200),
            "observe_lazily_current returned before terminal apply: elapsed={elapsed:?}"
        );

        for _ in 0..50 {
            if handler_started.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            handler_started.load(std::sync::atomic::Ordering::SeqCst),
            "listener handler should have run before send_observe_lazily_current returned"
        );

        let _ = std::fs::remove_file(socket_path(&root));
        drop(server);
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

        let msg = serde_json::json!({"type": "apply_canonical"});
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
        let msg = serde_json::json!({"type": "apply_canonical", "early_receipt": true});
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

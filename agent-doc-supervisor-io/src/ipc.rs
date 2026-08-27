//! # Module: supervisor::ipc
//!
//! Per-session Unix-domain socket for supervisor lifecycle control.
//!
//! ## Spec
//! See `src/agent-doc/specs/supervisor.md` § IPC Socket.
//!
//! ## Architecture
//!
//! Each supervisor instance owns a socket at
//! `.agent-doc/supervisor/<session-uuid>.sock`. Route/runtime and CLI control
//! paths connect and send JSON commands for supervisor lifecycle/state/inject
//! operations. Editor document authority and CRDT replica traffic stay on the
//! CP/project-controller route, not this per-session socket.
//!
//! ## Protocol
//!
//! Newline-delimited JSON (NDJSON), same framing as `ipc_socket.rs`. Each
//! message is a single JSON object terminated by `\n`. The server reads one
//! line, dispatches to the handler, and writes one JSON response line.
//!
//! ## Methods
//!
//! | Method    | Request fields                          | Response                                     |
//! |-----------|-----------------------------------------|----------------------------------------------|
//! | `restart` | `{ "method": "restart", "mode": "..." }`| `{ "ok": true, "pid": <u32> }`               |
//! | `inject`  | `{ "method": "inject", "bytes": "..." }`| `{ "ok": true, "n": <usize> }`               |
//! | `state`   | `{ "method": "state" }`                 | `{ "ok": true, "data": { ... } }`            |
//! | `pid`     | `{ "method": "pid" }`                   | `{ "ok": true, "pid": <u32?> }`              |
//! | `stop`    | `{ "method": "stop", "graceful": bool }`| `{ "ok": true }`                              |
//!
//! ## Scope boundary
//!
//! This module handles socket lifecycle (create, accept, cleanup) and message
//! framing. The actual command handling (restart, inject, etc.) is delegated
//! to a caller-supplied handler function, keeping `ipc.rs` decoupled from
//! `pty.rs` and `state.rs`.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use agent_doc_supervisor::input::normalize_supervisor_inject_bytes;
use agent_doc_supervisor::ipc_protocol::{IpcMethod, IpcResponse};
use anyhow::{Context, Result};
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, ToFsName,
    traits::{Listener as _, Stream as _},
};

/// Subdirectory within `.agent-doc/` for supervisor sockets.
const SUPERVISOR_DIR: &str = "supervisor";

const SUPERVISOR_IPC_QUERY_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
const SUPERVISOR_IPC_EFFECT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
const SUPERVISOR_IPC_ACCEPT_READ_TIMEOUT: Duration = Duration::from_secs(5);
const SUPERVISOR_IPC_RESOURCE_BACKOFF: Duration = Duration::from_millis(250);
const SUPERVISOR_IPC_MAX_INFLIGHT_HANDLERS: u64 = 64;

/// Typed client-side evidence for deciding whether a failed supervisor command
/// may be safely retried through dead-supervisor recovery.
///
/// A connect failure proves the supervisor never accepted the command. A response
/// timeout proves the opposite boundary: the socket accepted the command, but the
/// effect receipt did not arrive before the client budget expired. Mutating
/// commands must never be replayed merely because that receipt was late.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorCommandFailureKind {
    Connect,
    ResponseTimeout,
}

#[derive(Debug)]
struct SupervisorCommandFailure {
    kind: SupervisorCommandFailureKind,
    message: String,
}

impl std::fmt::Display for SupervisorCommandFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SupervisorCommandFailure {}

fn supervisor_command_failure(
    kind: SupervisorCommandFailureKind,
    message: impl Into<String>,
) -> anyhow::Error {
    anyhow::Error::new(SupervisorCommandFailure {
        kind,
        message: message.into(),
    })
}

/// Recover typed supervisor-command failure evidence through arbitrary anyhow
/// context layers added by higher-level session commands.
pub fn supervisor_command_failure_kind(
    err: &anyhow::Error,
) -> Option<SupervisorCommandFailureKind> {
    err.chain().find_map(|cause| {
        cause
            .downcast_ref::<SupervisorCommandFailure>()
            .map(|failure| failure.kind)
    })
}

/// Read-only queries should fail fast. Effectful methods may synchronously cross
/// tmux/PTY delivery and lifecycle-receipt boundaries, which regularly exceed two
/// seconds under load; keep their bounded budget long enough for the receipt.
fn supervisor_response_timeout(method: &IpcMethod) -> Duration {
    match method {
        IpcMethod::State | IpcMethod::Pid => SUPERVISOR_IPC_QUERY_RESPONSE_TIMEOUT,
        IpcMethod::Restart { .. }
        | IpcMethod::Inject { .. }
        | IpcMethod::Clear { .. }
        | IpcMethod::Stop { .. }
        | IpcMethod::StopAgent { .. } => SUPERVISOR_IPC_EFFECT_RESPONSE_TIMEOUT,
    }
}

/// Maximum byte length for a Unix domain socket path (`sun_path` is 108 bytes
/// including the NUL terminator on Linux).
const SUN_PATH_MAX: usize = 107;

/// Compute the socket path for a given session.
///
/// Prefers `.agent-doc/supervisor/<uuid>.sock` inside the project. When that
/// path exceeds the `sun_path` limit (108 bytes on Linux), falls back to
/// `$XDG_RUNTIME_DIR/agent-doc/<hash>-<short>.sock` (Linux) or
/// `$TMPDIR/agent-doc/<hash>-<short>.sock` (macOS/other), where `<hash>` is a
/// truncated SHA-256 of the project root and `<short>` is the first 8 chars of
/// the session UUID. The fallback is deterministic — same inputs always produce
/// the same path.
pub fn socket_path(project_root: &Path, session_uuid: &str) -> PathBuf {
    let preferred = project_root
        .join(".agent-doc")
        .join(SUPERVISOR_DIR)
        .join(format!("{session_uuid}.sock"));

    if preferred.as_os_str().len() <= SUN_PATH_MAX {
        return preferred;
    }

    // Fallback: short deterministic path in a runtime directory
    let hash = agent_doc_hash::bytes_hash(project_root.as_os_str().as_encoded_bytes());
    let short_hash = &hash[..12];
    let short_uuid = &session_uuid[..session_uuid.len().min(8)];

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    PathBuf::from(runtime_dir)
        .join("agent-doc")
        .join(format!("{short_hash}-{short_uuid}.sock"))
}

pub fn active_supervisor_pids(project_root: &Path) -> Vec<(String, u32)> {
    let supervisor_dir = project_root.join(".agent-doc").join(SUPERVISOR_DIR);
    let entries = match std::fs::read_dir(&supervisor_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut active = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("sock") {
            continue;
        }
        let Some(session_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let response = match send_command(&path, &IpcMethod::Pid) {
            Ok(response) => response,
            Err(_) => continue,
        };
        let Some(pid) = response
            .data
            .as_ref()
            .and_then(|data| data.get("pid"))
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        if response.ok && pid > 0 {
            active.push((session_id.to_string(), pid));
        }
    }

    active
}

/// Serializable supervisor state projected for the `state` IPC method.
///
/// The concrete supervisor runtime owns actor stores, PTY writers, and process
/// handles. This focused IPC layer only needs the JSON-facing facts.
#[derive(Debug, Clone)]
pub struct SupervisorIpcStateSnapshot {
    pub running: bool,
    pub state: String,
    pub actor_state: Option<String>,
    pub actor_session_id: Option<String>,
    pub actor_pane_id: Option<String>,
    pub actor_generation: Option<u64>,
    pub current_harness: String,
    pub editor_sync: Option<serde_json::Value>,
    pub restart_count: u32,
    pub cwd_source: &'static str,
    pub supervisor_pid: u32,
    pub supervisor_instance_id: String,
    pub child_pid: u32,
}

pub trait SupervisorIpcSnapshotState {
    fn supervisor_running(&self) -> bool;
    fn supervisor_state_label(&self) -> String;
    fn actor_state_label(&self) -> Option<String>;
    fn actor_session_id(&self) -> Option<String>;
    fn actor_pane_id(&self) -> Option<String>;
    fn actor_generation(&self) -> Option<u64>;
    fn current_harness(&self) -> String;
    fn actor_file(&self) -> Option<String>;
    /// Lazily current-authority facts supplied by the runtime layer that owns
    /// the editor/controller dependency. Keeping this seam on the state
    /// adapter avoids pulling editor I/O into the supervisor persistence crate.
    fn editor_authority_snapshot(&self) -> Option<serde_json::Value>;
    fn restart_count(&self) -> u32;
    fn cwd_source(&self) -> &'static str;
    fn supervisor_pid(&self) -> u32;
    fn supervisor_instance_id(&self) -> String;
    fn child_pid(&self) -> u32;
}

pub fn supervisor_ipc_state_snapshot<S>(state: &S) -> SupervisorIpcStateSnapshot
where
    S: SupervisorIpcSnapshotState + ?Sized,
{
    let editor_sync = state.editor_authority_snapshot();
    SupervisorIpcStateSnapshot {
        running: state.supervisor_running(),
        state: state.supervisor_state_label(),
        actor_state: state.actor_state_label(),
        actor_session_id: state.actor_session_id(),
        actor_pane_id: state.actor_pane_id(),
        actor_generation: state.actor_generation(),
        current_harness: state.current_harness(),
        editor_sync,
        restart_count: state.restart_count(),
        cwd_source: state.cwd_source(),
        supervisor_pid: state.supervisor_pid(),
        supervisor_instance_id: state.supervisor_instance_id(),
        child_pid: state.child_pid(),
    }
}

/// Effect boundary for handling supervisor IPC methods.
pub trait SupervisorIpcHandlerState: SupervisorIpcSnapshotState {
    fn capability_dispatch_blocker(&self) -> Option<String>;
}

pub trait SupervisorInjectDeliveryState {
    fn inject_pane_id(&self) -> Option<String>;
    /// Current harness identity (owned snapshot). Owned so the backing store can be
    /// updated on an in-loop harness switch (`#actor-harness-switch-writeback`).
    fn harness_binary(&self) -> String;
    fn write_child_pty(&self, bytes: &[u8]) -> Result<(), String>;
    fn begin_prompt_dispatch(&self, source: &str, bytes: &str) -> PromptDispatchAdmission;
    fn clear_prompt_dispatch_on_failure(&self, key: &str);
}

pub trait SupervisorIpcLifecycleState {
    fn actor_waiting_input(&self) -> bool;
    fn transition_actor_busy(&self, caller: &str, reason: &str);
    fn transition_actor_waiting_input(&self, caller: &str, reason: &str);
    fn set_restart_mode(&self, mode: String);
    fn set_restart_requested(&self, requested: bool);
    fn binary_stale(&self) -> bool;
    fn set_restart_reexec(&self, reexec: bool);
    fn set_stop_requested(&self, requested: bool);
    fn set_stop_agent_requested(&self, requested: bool);
    fn kill_child_for_ipc(&self);
    /// Submit the empty line that releases the supervisor's own blocking
    /// restart/quit prompt. This is used only when `actor_waiting_input()` was
    /// true before the lifecycle transition.
    fn wake_restart_prompt(&self) -> Result<(), String>;
    /// True when a document cycle is open for this supervisor's document.
    ///
    /// `#haivendupsession`: the restart path needs the same fact the recycle
    /// path gates on, so both refuse to replace a child that still owns a turn.
    fn agent_doc_cycle_open(&self) -> bool;
    /// True when the current harness child process is still running. A restart
    /// after the child is gone has nothing to double up and must still spawn.
    fn child_alive(&self) -> bool;
}

pub fn mark_supervisor_inject_dispatched<S>(state: &S)
where
    S: SupervisorIpcLifecycleState + ?Sized,
{
    state.transition_actor_busy("dispatch", "ipc_inject");
}

pub fn mark_supervisor_clear_dispatched<S>(state: &S)
where
    S: SupervisorIpcLifecycleState + ?Sized,
{
    state.transition_actor_busy("operator", "ipc_clear");
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptDispatchAdmission {
    Accepted { key: String },
    Duplicate { key: String },
    Untracked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorInjectDeliveryOutcome {
    Delivered,
    DuplicateSuppressed,
}

const RESTART_AGENT_MODE_PREFIX: &str = "agent:";

pub fn decode_restart_intent(mode: String) -> (String, bool) {
    match mode.strip_prefix(RESTART_AGENT_MODE_PREFIX) {
        Some(mode) => (mode.to_string(), true),
        None => (mode, false),
    }
}

pub fn request_supervisor_restart<S>(state: &S, mode: String) -> Result<(), String>
where
    S: SupervisorIpcLifecycleState + ?Sized,
{
    let (mode, restart_agent) = decode_restart_intent(mode);
    // `#haivendupsession`: refuse to spawn a replacement while the current child
    // still owns an open cycle. The recycle path has always deferred here; the
    // restart path did not, and with a keep-alive reap policy the un-reaped old
    // child kept rendering through the same pane's PTY proxy underneath the new
    // one. Fail loudly rather than silently dropping the operator's request —
    // a silent no-op is the failure mode this whole area keeps reproducing.
    if agent_doc_supervisor::lifecycle::supervisor_restart_admission(
        state.agent_doc_cycle_open(),
        state.child_alive(),
    ) == agent_doc_supervisor::lifecycle::SupervisorRestartAdmission::DeferCycleOpen
    {
        return Err(
            "supervisor restart deferred: a document cycle is open and the current harness \
             child still owns it, so spawning now would leave two children on this pane. \
             Retry once the turn reaches its boundary (#haivendupsession)"
                .to_string(),
        );
    }
    let waiting_input = state.actor_waiting_input();
    state.transition_actor_busy("supervisor", "ipc_restart_requested");
    // Preserve the explicit Restart Agent intent through the child-exit
    // boundary. The supervisor loop needs it after the current child is gone
    // so it can re-resolve both `agent:` and the document's exact `resume:`
    // binding. Normalizing this to plain `continue` here made that boundary
    // indistinguishable from crash recovery.
    state.set_restart_mode(if restart_agent {
        format!("{RESTART_AGENT_MODE_PREFIX}{mode}")
    } else {
        mode
    });
    state.set_restart_requested(true);
    // A controller-only recycle may preserve the live harness child across an
    // in-place re-exec. An explicit Restart Agent intent may not: it exists to
    // re-resolve frontmatter (including an `agent:` harness switch) and replace
    // that child even when the serving supervisor binary is stale.
    let reexec = state.binary_stale() && !restart_agent;
    state.set_restart_reexec(reexec);
    if !reexec {
        state.kill_child_for_ipc();
    }
    if waiting_input && let Err(err) = state.wake_restart_prompt() {
        state.set_restart_requested(false);
        state.set_restart_reexec(false);
        state.transition_actor_waiting_input("supervisor", "ipc_restart_prompt_wake_failed");
        return Err(err);
    }
    Ok(())
}

pub fn request_supervisor_stop<S>(state: &S)
where
    S: SupervisorIpcLifecycleState + ?Sized,
{
    state.set_stop_requested(true);
    state.kill_child_for_ipc();
}

pub fn request_supervisor_stop_agent<S>(state: &S)
where
    S: SupervisorIpcLifecycleState + ?Sized,
{
    state.transition_actor_waiting_input("supervisor", "ipc_stop_agent_requested");
    state.set_stop_agent_requested(true);
    state.kill_child_for_ipc();
}

pub fn deliver_supervisor_inject<S>(
    state: &S,
    bytes: &str,
    diag_op: &str,
) -> Result<SupervisorInjectDeliveryOutcome, String>
where
    S: SupervisorInjectDeliveryState + ?Sized,
{
    let admission = if diag_op == "ipc_inject" {
        state.begin_prompt_dispatch(diag_op, bytes)
    } else {
        PromptDispatchAdmission::Untracked
    };
    let admission_key = match admission {
        PromptDispatchAdmission::Accepted { key } => Some(key),
        PromptDispatchAdmission::Duplicate { .. } => {
            return Ok(SupervisorInjectDeliveryOutcome::DuplicateSuppressed);
        }
        PromptDispatchAdmission::Untracked => None,
    };
    let harness = state.harness_binary();
    let harness = harness.as_str();
    let source = format!("supervisor.{diag_op}");
    let result = if let Some(pane_id) = state.inject_pane_id() {
        let profile = agent_doc_tmux_commands::tmux_submit_profile_for_harness(harness);
        agent_doc_tmux_io::input_diag::log_text_submit(
            agent_doc_tmux_io::input_diag::InputDiagSink::new(None, noop_input_diag_log),
            &source,
            &format!("pane:{pane_id}"),
            bytes,
            Some(harness),
            profile.transform(),
            profile.submit_key(),
        );
        let tmux = tmux_router::Tmux::default_server();
        agent_doc_tmux_io::send_submitted_text_for_harness_logged(
            &tmux,
            &pane_id,
            bytes,
            harness,
            agent_doc_tmux_io::input_diag::InputDiagSink::new(None, noop_input_diag_log),
            "sessions.send_submitted_text_for_harness",
        )
        .map_err(|err| err.to_string())
    } else {
        let normalized = normalize_supervisor_inject_bytes(bytes);
        agent_doc_tmux_io::input_diag::log_transform_event(
            agent_doc_tmux_io::input_diag::InputDiagSink::new(None, noop_input_diag_log),
            &source,
            "child_pty",
            "normalize_lf_to_cr",
            bytes.as_bytes(),
            &normalized,
            Some(harness),
        );
        state.write_child_pty(&normalized)
    };
    match result {
        Ok(()) => Ok(SupervisorInjectDeliveryOutcome::Delivered),
        Err(err) => {
            if let Some(key) = admission_key.as_deref() {
                state.clear_prompt_dispatch_on_failure(key);
            }
            Err(err)
        }
    }
}

fn noop_input_diag_log(_file: &Path, _message: &str) {}

/// Handle one decoded supervisor IPC method against a concrete supervisor
/// runtime state adapter.
pub fn handle_supervisor_ipc<S>(method: IpcMethod, state: &S) -> IpcResponse
where
    S: SupervisorIpcHandlerState
        + SupervisorIpcLifecycleState
        + SupervisorInjectDeliveryState
        + ?Sized,
{
    if agent_doc_supervisor::ipc_protocol::ipc_method_requires_capability_gate(&method)
        && let Some(reason) = state.capability_dispatch_blocker()
    {
        return IpcResponse::err(reason);
    }
    match method {
        IpcMethod::State => {
            let snapshot = supervisor_ipc_state_snapshot(state);
            IpcResponse::ok(serde_json::json!({
                "running": snapshot.running,
                "state": snapshot.state,
                "actor_state": snapshot.actor_state,
                "actor_session_id": snapshot.actor_session_id,
                "actor_pane_id": snapshot.actor_pane_id,
                "actor_generation": snapshot.actor_generation,
                "current_harness": snapshot.current_harness,
                "editor_sync": snapshot.editor_sync,
                "restart_count": snapshot.restart_count,
                "cwd_source": snapshot.cwd_source,
                "supervisor_pid": snapshot.supervisor_pid,
                "supervisor_instance_id": snapshot.supervisor_instance_id,
                "child_pid": snapshot.child_pid,
            }))
        }
        IpcMethod::Pid => {
            let snapshot = supervisor_ipc_state_snapshot(state);
            if snapshot.supervisor_pid > 0 {
                IpcResponse::ok(serde_json::json!({
                    "pid": snapshot.supervisor_pid,
                    "supervisor_instance_id": snapshot.supervisor_instance_id,
                }))
            } else {
                IpcResponse::ok(serde_json::json!({ "pid": null }))
            }
        }
        IpcMethod::Inject { bytes } => match deliver_supervisor_inject(state, &bytes, "ipc_inject")
        {
            Ok(SupervisorInjectDeliveryOutcome::Delivered) => {
                mark_supervisor_inject_dispatched(state);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            Ok(SupervisorInjectDeliveryOutcome::DuplicateSuppressed) => IpcResponse::ok(
                serde_json::json!({ "n": 0, "duplicate": true, "reason": "prompt_dispatch_duplicate" }),
            ),
            Err(err) => IpcResponse::err(err),
        },
        IpcMethod::Clear { bytes: _ } if state.actor_waiting_input() => {
            match request_supervisor_restart(state, "fresh".to_string()) {
                Ok(()) => IpcResponse::ok(serde_json::json!({
                    "n": 0,
                    "restart_fresh": true,
                    "reason": "supervisor_waiting_input"
                })),
                Err(err) => IpcResponse::err(err),
            }
        }
        IpcMethod::Clear { bytes } => match deliver_supervisor_inject(state, &bytes, "ipc_clear") {
            Ok(SupervisorInjectDeliveryOutcome::Delivered) => {
                mark_supervisor_clear_dispatched(state);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            Ok(SupervisorInjectDeliveryOutcome::DuplicateSuppressed) => IpcResponse::ok(
                serde_json::json!({ "n": 0, "duplicate": true, "reason": "prompt_dispatch_duplicate" }),
            ),
            Err(err) => IpcResponse::err(err),
        },
        IpcMethod::Restart { mode } => match request_supervisor_restart(state, mode) {
            Ok(()) => IpcResponse::ok_empty(),
            Err(err) => IpcResponse::err(err),
        },
        IpcMethod::Stop { graceful: _ } => {
            request_supervisor_stop(state);
            IpcResponse::ok_empty()
        }
        IpcMethod::StopAgent { reason: _ } => {
            request_supervisor_stop_agent(state);
            IpcResponse::ok_empty()
        }
    }
}

/// Count of in-flight per-connection supervisor-IPC handler threads. Mirrors the
/// editor-IPC listener (`ipc_socket.rs`, `#jbacceptwedge`): the accept loop
/// spawns one short-lived thread per connection so a slow/half-open client can
/// never wedge the whole accept loop (which would freeze ALL supervisor command
/// dispatch — the route/finalize IPC wedge this guards against).
static INFLIGHT_SUPERVISOR_HANDLERS: AtomicU64 = AtomicU64::new(0);

/// RAII guard decrementing [`INFLIGHT_SUPERVISOR_HANDLERS`] on drop so a
/// panicking handler thread still releases its slot.
struct InflightSupervisorGuard;

impl Drop for InflightSupervisorGuard {
    fn drop(&mut self) {
        INFLIGHT_SUPERVISOR_HANDLERS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Current number of in-flight supervisor-IPC handler threads (observability).
pub fn inflight_supervisor_handler_count() -> u64 {
    INFLIGHT_SUPERVISOR_HANDLERS.load(Ordering::SeqCst)
}

fn supervisor_accept_error_is_resource_exhaustion(err: &std::io::Error) -> bool {
    matches!(err.raw_os_error(), Some(23 | 24))
        || err.kind() == ErrorKind::OutOfMemory
        || err.to_string().contains("Too many open files")
}

fn supervisor_read_error_is_timeout(err: &std::io::Error) -> bool {
    matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
        || err.to_string().contains("timed out")
}

/// Running supervisor IPC listener. Owns the accept thread and cleans up
/// the socket file on drop.
pub struct SupervisorIpc {
    socket_path: PathBuf,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SupervisorIpc {
    /// Start the IPC listener.
    ///
    /// Creates the supervisor socket directory if needed, binds the socket
    /// with mode 0600, and spawns an accept thread that dispatches incoming
    /// commands to `handler`.
    ///
    /// The handler receives an [`IpcMethod`] and returns an [`IpcResponse`].
    /// It runs on a short-lived per-connection thread (not the accept thread),
    /// so a slow or half-open client cannot wedge the accept loop and freeze all
    /// supervisor command dispatch. Because connections are handled concurrently,
    /// the handler must be `Sync` and touch shared state (PtySession, CrashPolicy)
    /// through interior mutability (`Arc<Mutex<...>>`).
    pub fn start<F>(project_root: &Path, session_uuid: &str, handler: F) -> Result<Self>
    where
        F: Fn(IpcMethod) -> IpcResponse + Send + Sync + 'static,
    {
        let sock = socket_path(project_root, session_uuid);

        // Ensure parent directory exists
        if let Some(parent) = sock.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create supervisor dir: {}", parent.display()))?;
        }

        // Clean up stale socket
        if sock.exists() {
            let _ = std::fs::remove_file(&sock);
        }

        let name = sock.clone().to_fs_name::<GenericFilePath>()?;
        let opts = ListenerOptions::new().name(name);
        let listener = opts
            .create_sync()
            .with_context(|| format!("bind supervisor socket: {}", sock.display()))?;

        // Set socket permissions to 0600 (owner-only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&sock, perms);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let sock_clone = sock.clone();
        // Share the handler across per-connection threads. `Sync` is required by
        // `start`'s bound; shared state lives behind interior mutability.
        let handler = Arc::new(handler);

        let handle = thread::Builder::new()
            .name("supervisor-ipc".into())
            .spawn(move || {
                let mut resource_exhaustion_logged = false;
                loop {
                    if stop_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    match listener.accept() {
                        Ok(stream) => {
                            resource_exhaustion_logged = false;
                            let inflight =
                                INFLIGHT_SUPERVISOR_HANDLERS.load(Ordering::SeqCst);
                            if inflight >= SUPERVISOR_IPC_MAX_INFLIGHT_HANDLERS {
                                eprintln!(
                                    "[supervisor::ipc] warning: dropping connection because {inflight} handlers are already in flight"
                                );
                                drop(stream);
                                thread::sleep(Duration::from_millis(10));
                                continue;
                            }
                            if let Err(e) =
                                stream.set_recv_timeout(Some(SUPERVISOR_IPC_ACCEPT_READ_TIMEOUT))
                            {
                                eprintln!(
                                    "[supervisor::ipc] warning: failed to set connection read timeout ({e}); continuing"
                                );
                            }
                            // Handle each connection on its own short-lived thread
                            // so a slow/half-open client cannot wedge the accept
                            // loop and freeze all supervisor command dispatch.
                            // Mirrors the editor-IPC listener (#jbacceptwedge).
                            let handler = Arc::clone(&handler);
                            INFLIGHT_SUPERVISOR_HANDLERS.fetch_add(1, Ordering::SeqCst);
                            if let Err(e) = thread::Builder::new()
                                .name("supervisor-ipc-conn".into())
                                .spawn(move || {
                                    let _guard = InflightSupervisorGuard;
                                    serve_supervisor_connection(stream, handler.as_ref());
                                })
                            {
                                // Spawn failed (thread exhaustion / OOM — rare).
                                // Release the slot and drop the connection; the
                                // client's bounded recv timeout surfaces it as a
                                // fail-closed timeout rather than a wedge.
                                INFLIGHT_SUPERVISOR_HANDLERS.fetch_sub(1, Ordering::SeqCst);
                                eprintln!(
                                    "[supervisor::ipc] warning: failed to spawn connection thread ({e}); dropping connection"
                                );
                            }
                        }
                        Err(e) => {
                            if stop_clone.load(Ordering::Relaxed) {
                                break;
                            }
                            if supervisor_accept_error_is_resource_exhaustion(&e) {
                                if !resource_exhaustion_logged {
                                    eprintln!(
                                        "[supervisor::ipc] accept resource exhaustion: {e}; backing off"
                                    );
                                    resource_exhaustion_logged = true;
                                }
                                thread::sleep(SUPERVISOR_IPC_RESOURCE_BACKOFF);
                            } else {
                                resource_exhaustion_logged = false;
                                eprintln!("[supervisor::ipc] accept error: {e}");
                            }
                        }
                    }
                }
                // Clean up socket on thread exit
                let _ = std::fs::remove_file(&sock_clone);
            })
            .context("spawn supervisor-ipc thread")?;

        Ok(Self {
            socket_path: sock,
            stop,
            handle: Some(handle),
        })
    }

    /// Stop the IPC listener and clean up the socket file.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        // Connect to the socket to unblock the accept() call, then the
        // thread will see the stop flag and exit.
        if self.socket_path.exists() {
            let _ = try_connect(&self.socket_path);
        }

        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }

        // Ensure socket is removed
        let _ = std::fs::remove_file(&self.socket_path);
    }

    /// Path to the socket file.
    #[allow(dead_code)] // API surface — used by tests
    pub fn path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for SupervisorIpc {
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.stop();
        }
    }
}

// --- Client side ---

/// Serve one supervisor-IPC request-response on an accepted connection. The
/// client (`send_command`) connects, sends one NDJSON line, reads one response
/// line, and drops the stream. Runs on a dedicated per-connection thread so a
/// slow/half-open client only blocks its own thread, never the accept loop. A
/// dead client surfaces as EOF (`read_line` returns 0), so the thread exits
/// promptly instead of leaking.
fn serve_supervisor_connection<F>(stream: interprocess::local_socket::Stream, handler: &F)
where
    F: Fn(IpcMethod) -> IpcResponse,
{
    let (reader_half, mut writer_half) = stream.split();
    let mut reader = BufReader::new(reader_half);
    let mut line = String::new();

    if reader.read_line(&mut line).unwrap_or(0) > 0 {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            let response = match serde_json::from_str::<IpcMethod>(trimmed) {
                Ok(method) => handler(method),
                Err(e) => IpcResponse::err(format!("parse error: {e}")),
            };

            let mut resp_json = serde_json::to_string(&response)
                .unwrap_or_else(|e| format!(r#"{{"ok":false,"error":"serialize error: {e}"}}"#));
            resp_json.push('\n');

            if let Err(e) = writer_half.write_all(resp_json.as_bytes()) {
                eprintln!("[supervisor::ipc] write error: {e}");
            }
            let _ = writer_half.flush();
        }
    }
}

fn try_connect(sock: &Path) -> Result<interprocess::local_socket::Stream> {
    let name = sock.to_fs_name::<GenericFilePath>()?;
    let opts = interprocess::local_socket::ConnectOptions::new().name(name);
    let stream = opts
        .connect_sync()
        .context("failed to connect to supervisor socket")?;
    Ok(stream)
}

/// Liveness classification of a supervisor socket, used by recovery commands
/// (`session restart-supervisor`, `admin recycle`) to distinguish a fully-DEAD
/// supervisor (only a stale socket file remains, or no file at all — no listener
/// to accept a connection) from a LIVE supervisor (`connect()` succeeds) so the
/// dead case can cold-start a fresh supervisor instead of surfacing a raw
/// `Connection refused (os error 111)` (#supdead-coldstart-fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketLiveness {
    /// `connect()` succeeded — a supervisor is listening (it may still be busy /
    /// slow to ack, but the process is alive). In-place restart/recycle applies.
    Live,
    /// No listener: the socket file is missing, or it exists but `connect()` was
    /// actively refused (`ECONNREFUSED` — a stale socket left by a dead process).
    /// This is the cold-start case.
    Dead,
}

/// Returns whether a connect error is the dead-supervisor signature
/// (`ECONNREFUSED` — the kernel actively refused because no process is listening
/// on the AF_UNIX socket path). Any other connect failure (permission, name
/// resolution) is treated as NOT-dead so we never cold-start over an ambiguous
/// error.
fn connect_error_is_econnrefused(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(io) = cause.downcast_ref::<std::io::Error>()
            && io.kind() == std::io::ErrorKind::ConnectionRefused
        {
            return true;
        }
        // interprocess wraps the OS error; fall back to the os-error code in the
        // rendered chain when the concrete io::Error is not directly downcastable.
        if cause.to_string().contains("os error 111") {
            return true;
        }
    }
    false
}

/// Probe a supervisor socket and classify it as [`SocketLiveness::Live`] or
/// [`SocketLiveness::Dead`]. A missing socket file is `Dead`; an existing file
/// that refuses the connection (`ECONNREFUSED`) is `Dead`; a successful connect
/// is `Live`. Any other connect error is conservatively reported as `Live` (we
/// do not have positive proof the process is gone) so the caller falls back to
/// the existing in-place path rather than risking a duplicate cold-start.
pub fn probe_socket(sock: &Path) -> SocketLiveness {
    if !sock.exists() {
        return SocketLiveness::Dead;
    }
    match try_connect(sock) {
        Ok(_) => SocketLiveness::Live,
        Err(err) if connect_error_is_econnrefused(&err) => SocketLiveness::Dead,
        // Ambiguous error (e.g. permission): do not assert death — keep the
        // in-place path so we never cold-start over a live-but-unreachable peer.
        Err(_) => SocketLiveness::Live,
    }
}

/// Send a command to a supervisor and read the response.
///
/// Connects to the socket, sends the command as NDJSON, and reads one response
/// line with a method-specific bounded timeout. Queries fail fast; effectful
/// commands receive a longer budget for their delivery receipt.
#[allow(dead_code)] // API surface — used by tests and future IPC clients
pub fn send_command(sock: &Path, method: &IpcMethod) -> Result<IpcResponse> {
    send_command_with_response_timeout(sock, method, supervisor_response_timeout(method))
}

fn send_command_with_response_timeout(
    sock: &Path,
    method: &IpcMethod,
    response_timeout: Duration,
) -> Result<IpcResponse> {
    let stream = try_connect(sock).map_err(|err| {
        supervisor_command_failure(SupervisorCommandFailureKind::Connect, format!("{err:#}"))
    })?;
    stream
        .set_recv_timeout(Some(response_timeout))
        .context("failed to set supervisor response timeout")?;
    let (reader_half, mut writer_half) = stream.split();

    // Send NDJSON
    let mut msg = serde_json::to_string(method)?;
    msg.push('\n');
    writer_half.write_all(msg.as_bytes())?;
    writer_half.flush()?;

    let mut reader = BufReader::new(reader_half);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => anyhow::bail!("supervisor closed connection without responding"),
        Ok(_) => {
            let resp: IpcResponse =
                serde_json::from_str(line.trim()).context("failed to parse supervisor response")?;
            Ok(resp)
        }
        Err(e) if supervisor_read_error_is_timeout(&e) => Err(supervisor_command_failure(
            SupervisorCommandFailureKind::ResponseTimeout,
            format!(
                "supervisor response timeout ({:.1}s)",
                response_timeout.as_secs_f32()
            ),
        )),
        Err(e) => anyhow::bail!("supervisor read error: {e}"),
    }
}

/// Check if a supervisor socket is active for the given session.
#[allow(dead_code)] // API surface — used by tests and future callers
pub fn is_active(project_root: &Path, session_uuid: &str) -> bool {
    let sock = socket_path(project_root, session_uuid);
    if !sock.exists() {
        return false;
    }
    try_connect(&sock).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU32;

    struct RestartLifecycleState {
        waiting_input: bool,
        binary_stale: bool,
        restart_requested: AtomicBool,
        restart_reexec: AtomicBool,
        child_killed: AtomicBool,
        prompt_woken: AtomicBool,
        restart_mode: Mutex<String>,
        /// `#haivendupsession`: default false so every pre-existing restart test
        /// keeps its original meaning — the gate only engages when a test
        /// explicitly sets up an open cycle over a live child.
        cycle_open: bool,
        child_alive: bool,
    }

    /// `#haivendupsession`: an operator `session_restart` arriving while the
    /// current child still owns an open cycle must be refused, not spawned. The
    /// controller logs `ipc_accepted_deferred reason=live_supervisor_owns_drain`
    /// for this case, but that only means the CONTROLLER declined to escalate —
    /// the request had already been accepted here, and this path spawned anyway,
    /// leaving two children on one pane under a keep-alive reap policy.
    #[test]
    fn restart_over_a_live_child_mid_cycle_is_refused_not_spawned() {
        let state = RestartLifecycleState {
            cycle_open: true,
            child_alive: true,
            waiting_input: false,
            binary_stale: false,
            restart_requested: AtomicBool::new(false),
            restart_reexec: AtomicBool::new(false),
            child_killed: AtomicBool::new(false),
            prompt_woken: AtomicBool::new(false),
            restart_mode: Mutex::new(String::new()),
        };
        let result = request_supervisor_restart(&state, "continue".to_string());
        assert!(
            result.is_err(),
            "an open cycle over a live child must refuse"
        );
        let message = result.unwrap_err();
        assert!(
            message.contains("two children"),
            "the refusal must say why, not just decline: {message}"
        );
        // Nothing may be armed — a half-applied restart is how the second child
        // appeared in the first place.
        assert!(!state.restart_requested.load(Ordering::Relaxed));
        assert!(!state.child_killed.load(Ordering::Relaxed));
    }

    /// The same request with the child already gone must still spawn, otherwise
    /// a crashed harness could never be restarted.
    #[test]
    fn restart_after_the_child_died_still_arms_mid_cycle() {
        let state = RestartLifecycleState {
            cycle_open: true,
            child_alive: false,
            waiting_input: false,
            binary_stale: false,
            restart_requested: AtomicBool::new(false),
            restart_reexec: AtomicBool::new(false),
            child_killed: AtomicBool::new(false),
            prompt_woken: AtomicBool::new(false),
            restart_mode: Mutex::new(String::new()),
        };
        request_supervisor_restart(&state, "continue".to_string())
            .expect("a dead child must not block the restart");
        assert!(state.restart_requested.load(Ordering::Relaxed));
    }

    impl SupervisorIpcLifecycleState for RestartLifecycleState {
        fn actor_waiting_input(&self) -> bool {
            self.waiting_input
        }

        fn agent_doc_cycle_open(&self) -> bool {
            self.cycle_open
        }

        fn child_alive(&self) -> bool {
            self.child_alive
        }

        fn transition_actor_busy(&self, _caller: &str, _reason: &str) {}
        fn transition_actor_waiting_input(&self, _caller: &str, _reason: &str) {}
        fn set_restart_mode(&self, mode: String) {
            *self.restart_mode.lock().unwrap() = mode;
        }
        fn set_restart_requested(&self, requested: bool) {
            self.restart_requested.store(requested, Ordering::Relaxed);
        }
        fn binary_stale(&self) -> bool {
            self.binary_stale
        }
        fn set_restart_reexec(&self, reexec: bool) {
            self.restart_reexec.store(reexec, Ordering::Relaxed);
        }
        fn set_stop_requested(&self, _requested: bool) {}
        fn set_stop_agent_requested(&self, _requested: bool) {}
        fn kill_child_for_ipc(&self) {
            self.child_killed.store(true, Ordering::Relaxed);
        }
        fn wake_restart_prompt(&self) -> Result<(), String> {
            self.prompt_woken.store(true, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn restart_request_wakes_waiting_supervisor_prompt() {
        let state = RestartLifecycleState {
            cycle_open: false,
            child_alive: false,
            waiting_input: true,
            binary_stale: false,
            restart_requested: AtomicBool::new(false),
            restart_reexec: AtomicBool::new(false),
            child_killed: AtomicBool::new(false),
            prompt_woken: AtomicBool::new(false),
            restart_mode: Mutex::new(String::new()),
        };

        request_supervisor_restart(&state, "fresh".to_string()).unwrap();

        assert!(state.restart_requested.load(Ordering::Relaxed));
        assert!(state.prompt_woken.load(Ordering::Relaxed));
    }

    #[test]
    fn restart_agent_replaces_child_even_when_supervisor_binary_is_stale() {
        let state = RestartLifecycleState {
            cycle_open: false,
            child_alive: false,
            waiting_input: false,
            binary_stale: true,
            restart_requested: AtomicBool::new(false),
            restart_reexec: AtomicBool::new(false),
            child_killed: AtomicBool::new(false),
            prompt_woken: AtomicBool::new(false),
            restart_mode: Mutex::new(String::new()),
        };

        request_supervisor_restart(&state, "agent:continue".to_string()).unwrap();

        assert!(state.restart_requested.load(Ordering::Relaxed));
        assert!(!state.restart_reexec.load(Ordering::Relaxed));
        assert!(state.child_killed.load(Ordering::Relaxed));
        assert_eq!(*state.restart_mode.lock().unwrap(), "agent:continue");
    }

    #[test]
    fn controller_recycle_preserves_child_during_stale_binary_reexec() {
        let state = RestartLifecycleState {
            cycle_open: false,
            child_alive: false,
            waiting_input: false,
            binary_stale: true,
            restart_requested: AtomicBool::new(false),
            restart_reexec: AtomicBool::new(false),
            child_killed: AtomicBool::new(false),
            prompt_woken: AtomicBool::new(false),
            restart_mode: Mutex::new(String::new()),
        };

        request_supervisor_restart(&state, "continue".to_string()).unwrap();

        assert!(state.restart_reexec.load(Ordering::Relaxed));
        assert!(!state.child_killed.load(Ordering::Relaxed));
    }

    fn start_echo_handler(root: &Path, uuid: &str) -> SupervisorIpc {
        SupervisorIpc::start(root, uuid, |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "restart_count": 0,
            })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            IpcMethod::StopAgent { reason } => {
                IpcResponse::ok(serde_json::json!({ "reason": reason }))
            }
            IpcMethod::Restart { mode } => {
                IpcResponse::ok(serde_json::json!({ "pid": 99999, "mode": mode }))
            }
            IpcMethod::Inject { bytes } => IpcResponse::ok(serde_json::json!({ "n": bytes.len() })),
            IpcMethod::Clear { bytes } => IpcResponse::ok(serde_json::json!({ "n": bytes.len() })),
        })
        .expect("start test handler")
    }

    #[test]
    fn roundtrip_state_query() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let mut ipc = start_echo_handler(root, "test-state");
        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-state");
        let resp = send_command(&sock, &IpcMethod::State).unwrap();
        assert!(resp.ok);
        let data = resp.data.unwrap();
        assert_eq!(data["state"], "healthy");
        assert_eq!(data["running"], true);

        ipc.stop();
    }

    #[test]
    fn effectful_commands_have_a_longer_receipt_budget_than_queries() {
        assert_eq!(
            supervisor_response_timeout(&IpcMethod::State),
            SUPERVISOR_IPC_QUERY_RESPONSE_TIMEOUT
        );
        assert_eq!(
            supervisor_response_timeout(&IpcMethod::Pid),
            SUPERVISOR_IPC_QUERY_RESPONSE_TIMEOUT
        );
        assert_eq!(
            supervisor_response_timeout(&IpcMethod::Clear {
                bytes: "/clear".to_string(),
            }),
            SUPERVISOR_IPC_EFFECT_RESPONSE_TIMEOUT
        );
        assert!(SUPERVISOR_IPC_EFFECT_RESPONSE_TIMEOUT > SUPERVISOR_IPC_QUERY_RESPONSE_TIMEOUT);
    }

    #[test]
    fn late_effect_receipt_is_typed_as_ambiguous_not_disconnected() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let mut ipc = SupervisorIpc::start(root, "test-late-receipt", |_method| {
            std::thread::sleep(Duration::from_millis(75));
            IpcResponse::ok_empty()
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(25));

        let sock = socket_path(root, "test-late-receipt");
        let err = send_command_with_response_timeout(
            &sock,
            &IpcMethod::Clear {
                bytes: "/clear".to_string(),
            },
            Duration::from_millis(10),
        )
        .unwrap_err();
        assert_eq!(
            supervisor_command_failure_kind(&err),
            Some(SupervisorCommandFailureKind::ResponseTimeout)
        );

        ipc.stop();
    }

    #[test]
    fn roundtrip_pid() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let mut ipc = start_echo_handler(root, "test-pid");
        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-pid");
        let resp = send_command(&sock, &IpcMethod::Pid).unwrap();
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap()["pid"], 12345);

        ipc.stop();
    }

    #[test]
    fn roundtrip_restart() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let mut ipc = start_echo_handler(root, "test-restart");
        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-restart");
        let resp = send_command(
            &sock,
            &IpcMethod::Restart {
                mode: "fresh".to_string(),
            },
        )
        .unwrap();
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap()["mode"], "fresh");

        ipc.stop();
    }

    // --- `#supdead-coldstart-fallback` socket liveness probe ---

    #[test]
    fn probe_socket_reports_live_for_listening_supervisor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let mut ipc = start_echo_handler(root, "test-probe-live");
        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-probe-live");
        assert_eq!(probe_socket(&sock), SocketLiveness::Live);

        ipc.stop();
    }

    #[test]
    fn probe_socket_reports_dead_for_missing_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("nonexistent.sock");
        assert_eq!(probe_socket(&sock), SocketLiveness::Dead);
    }

    #[test]
    fn probe_socket_reports_dead_for_stale_socket_no_listener() {
        // A stale socket file with no listening process is the exact
        // dead-supervisor signature: the file exists but `connect()` is refused.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/supervisor")).unwrap();
        let sock = socket_path(root, "test-probe-stale");

        // Bind+drop a listener to materialize a real socket file, then ensure no
        // process is listening on it (drop closes the listener but on some
        // platforms leaves the path); fall back to creating a plain file so the
        // path exists with no listener either way.
        {
            let mut ipc = start_echo_handler(root, "test-probe-stale");
            std::thread::sleep(Duration::from_millis(30));
            ipc.stop();
        }
        if !sock.exists() {
            std::fs::write(&sock, b"").unwrap();
        }
        assert!(
            sock.exists(),
            "stale socket path should exist for the probe"
        );
        assert_eq!(probe_socket(&sock), SocketLiveness::Dead);
    }

    #[test]
    fn roundtrip_stop_agent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let mut ipc = start_echo_handler(root, "test-stop-agent");
        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-stop-agent");
        let resp = send_command(
            &sock,
            &IpcMethod::StopAgent {
                reason: Some("operator menu".to_string()),
            },
        )
        .unwrap();
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap()["reason"], "operator menu");

        ipc.stop();
    }

    #[test]
    fn roundtrip_inject() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let mut ipc = start_echo_handler(root, "test-inject");
        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-inject");
        let resp = send_command(
            &sock,
            &IpcMethod::Inject {
                bytes: "/agent-doc plan.md\r".to_string(),
            },
        )
        .unwrap();
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap()["n"], 19);

        ipc.stop();
    }

    #[test]
    fn malformed_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let mut ipc = start_echo_handler(root, "test-malformed");
        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-malformed");
        // Send raw malformed JSON via low-level connect
        let stream = try_connect(&sock).unwrap();
        let (reader_half, mut writer_half) = stream.split();
        writer_half
            .write_all(b"{\"not_a_method\": true}\n")
            .unwrap();
        writer_half.flush().unwrap();

        let mut reader = BufReader::new(reader_half);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let resp: IpcResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(!resp.ok);
        assert!(resp.error.unwrap().contains("parse error"));

        ipc.stop();
    }

    #[cfg(unix)]
    #[test]
    fn socket_permissions_are_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let mut ipc = start_echo_handler(root, "test-perms");
        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-perms");
        let meta = std::fs::metadata(&sock).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        // Socket permissions on Linux may be filtered by umask. We set 0600
        // explicitly, so the owner bits should have rw.
        assert_eq!(
            mode & 0o600,
            0o600,
            "owner should have rw, got mode: {mode:o}"
        );
        assert_eq!(
            mode & 0o077,
            0,
            "group/other should have no access, got mode: {mode:o}"
        );

        ipc.stop();
    }

    #[test]
    fn stale_socket_cleaned_on_start() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let supervisor_dir = root.join(".agent-doc").join("supervisor");
        std::fs::create_dir_all(&supervisor_dir).unwrap();

        // Pre-create a stale socket file
        let sock = socket_path(root, "test-stale");
        std::fs::write(&sock, "stale").unwrap();
        assert!(sock.exists());

        // Start should succeed despite stale file
        let mut ipc = start_echo_handler(root, "test-stale");
        std::thread::sleep(Duration::from_millis(50));

        // Verify it works
        let resp = send_command(&sock, &IpcMethod::Pid).unwrap();
        assert!(resp.ok);

        ipc.stop();
    }

    #[test]
    fn concurrent_clients() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let call_count = Arc::new(AtomicU32::new(0));
        let active_count = Arc::new(AtomicU32::new(0));
        let max_active_count = Arc::new(AtomicU32::new(0));
        let count_clone = call_count.clone();
        let active_clone = active_count.clone();
        let max_active_clone = max_active_count.clone();

        let mut ipc = SupervisorIpc::start(root, "test-concurrent", move |method| {
            count_clone.fetch_add(1, Ordering::Relaxed);
            let active = active_clone.fetch_add(1, Ordering::SeqCst) + 1;
            max_active_clone.fetch_max(active, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(100));
            active_clone.fetch_sub(1, Ordering::SeqCst);
            match method {
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 1 })),
                _ => IpcResponse::ok_empty(),
            }
        })
        .unwrap();

        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-concurrent");
        let start_barrier = Arc::new(std::sync::Barrier::new(6));
        let mut handles = Vec::new();
        for _ in 0..5 {
            let s = sock.clone();
            let barrier = start_barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                send_command(&s, &IpcMethod::Pid).unwrap()
            }));
        }
        start_barrier.wait();

        for h in handles {
            let resp = h.join().unwrap();
            assert!(resp.ok);
        }

        assert!(call_count.load(Ordering::Relaxed) >= 5);
        assert!(max_active_count.load(Ordering::SeqCst) > 1);

        ipc.stop();
    }

    #[test]
    fn is_active_detects_running_listener() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        assert!(!is_active(root, "test-active"));

        let mut ipc = start_echo_handler(root, "test-active");
        std::thread::sleep(Duration::from_millis(50));

        assert!(is_active(root, "test-active"));

        ipc.stop();
        // After stop, socket is removed
        assert!(!is_active(root, "test-active"));
    }

    #[test]
    fn stop_removes_socket_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();

        let mut ipc = start_echo_handler(root, "test-cleanup");
        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-cleanup");
        assert!(sock.exists());

        ipc.stop();
        assert!(!sock.exists(), "socket should be removed after stop");
    }

    #[test]
    fn socket_path_falls_back_for_long_paths() {
        // Construct a project root long enough to push the socket path over 107 bytes
        let long_root = PathBuf::from("/tmp").join("a".repeat(80)).join("nested");
        let uuid = "12345678-abcd-ef01-2345-6789abcdef01";

        let path = socket_path(&long_root, uuid);

        // Preferred path would be ~160 bytes — must fall back
        assert!(
            path.as_os_str().len() <= SUN_PATH_MAX,
            "fallback path {} is {} bytes, exceeds {SUN_PATH_MAX}",
            path.display(),
            path.as_os_str().len()
        );
        assert!(
            path.to_string_lossy().contains("agent-doc"),
            "fallback path should contain agent-doc: {}",
            path.display()
        );

        // Deterministic: same inputs → same path
        let path2 = socket_path(&long_root, uuid);
        assert_eq!(path, path2);
    }

    #[test]
    fn socket_path_prefers_project_dir_for_short_paths() {
        let short_root = PathBuf::from("/tmp/proj");
        let uuid = "abcd1234";

        let path = socket_path(&short_root, uuid);
        assert_eq!(
            path,
            short_root
                .join(".agent-doc")
                .join("supervisor")
                .join("abcd1234.sock")
        );
    }

    #[test]
    fn long_path_fallback_binds_successfully() {
        let long_root = PathBuf::from("/tmp").join("a".repeat(80)).join("nested");
        let uuid = "test-long-path";

        let sock = socket_path(&long_root, uuid);
        // Ensure parent dir exists for the fallback path
        if let Some(parent) = sock.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }

        // Should bind without sun_path overflow
        let mut ipc = SupervisorIpc::start(&long_root, uuid, |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({"running": true})),
            _ => IpcResponse::err("not implemented"),
        })
        .expect("should bind despite long project root");

        std::thread::sleep(Duration::from_millis(50));
        assert!(is_active(&long_root, uuid));

        ipc.stop();
        assert!(!is_active(&long_root, uuid));
    }
}

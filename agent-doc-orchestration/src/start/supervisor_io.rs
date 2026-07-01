//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn ipc_method_requires_capability_gate(method: &IpcMethod) -> bool {
    matches!(method, IpcMethod::Inject { .. })
}

/// Shared delivery for injected text (pane submit or PTY write). Used by both
/// the gated [`IpcMethod::Inject`] path and the gate-exempt
/// [`IpcMethod::Clear`] path; the gate decision is made by the caller.
pub(crate) fn deliver_ipc_inject(
    shared: &SupervisorShared,
    bytes: &str,
    diag_op: &str,
) -> Result<(), String> {
    if let Some(pane_id) = shared.inject_pane.as_deref() {
        let profile =
            agent_doc_tmux_commands::tmux_submit_profile_for_harness(&shared.harness_binary);
        crate::input_diag::log_text_submit(
            None,
            &format!("supervisor.{diag_op}"),
            &format!("pane:{pane_id}"),
            bytes,
            Some(&shared.harness_binary),
            profile.transform(),
            profile.submit_key(),
        );
        dispatch_submit_text_to_pane(pane_id, bytes, &shared.harness_binary)
            .map_err(|e| e.to_string())
    } else {
        let guard = shared.inject_writer.lock().unwrap();
        match guard.as_ref() {
            Some(writer_arc) => {
                let mut w = writer_arc.lock().unwrap();
                let normalized = normalize_supervisor_inject_bytes(bytes);
                crate::input_diag::log_transform_event(
                    None,
                    &format!("supervisor.{diag_op}"),
                    "child_pty",
                    "normalize_lf_to_cr",
                    bytes.as_bytes(),
                    &normalized,
                    Some(&shared.harness_binary),
                );
                w.write_all_blocking(&normalized)
                    .map_err(|e| format!("write error: {e}"))
            }
            None => Err("no active session".to_string()),
        }
    }
}

pub(crate) fn handle_ipc(method: IpcMethod, shared: &SupervisorShared) -> IpcResponse {
    // Central dispatch gate: only real prompt dispatch is gated behind the
    // managed-capability proof. Operator/read-only methods are gate-exempt.
    if ipc_method_requires_capability_gate(&method)
        && let Some(reason) = shared.capability_dispatch_blocker()
    {
        return IpcResponse::err(reason);
    }
    match method {
        IpcMethod::State => {
            let state = shared.supervisor_state.lock().unwrap();
            let actor_state = shared
                .actor_state
                .lock()
                .unwrap()
                .map(|state| state.as_str().to_string());
            let actor_session_id = shared
                .actor_runtime
                .as_ref()
                .map(|runtime| runtime.session_id.clone());
            let actor_pane_id = shared
                .actor_runtime
                .as_ref()
                .map(|runtime| runtime.pane_id.clone());
            let actor_generation = shared
                .actor_runtime
                .as_ref()
                .map(|runtime| runtime.generation);
            let editor_sync = shared.actor_runtime.as_ref().map(|runtime| {
                let file = runtime.file.display().to_string();
                let statuses = agent_doc_debounce::editor_sync_statuses(&file);
                let in_flight = statuses.iter().any(|status| status.in_flight);
                serde_json::json!({
                    "file": file,
                    "in_flight": in_flight,
                    "statuses": statuses,
                })
            });
            IpcResponse::ok(serde_json::json!({
                "running": shared.running.load(Ordering::Relaxed),
                "state": state.as_str(),
                "actor_state": actor_state,
                "actor_session_id": actor_session_id,
                "actor_pane_id": actor_pane_id,
                "actor_generation": actor_generation,
                "editor_sync": editor_sync,
                "restart_count": shared.restart_count.load(Ordering::Relaxed),
                "cwd_source": shared.cwd_source,
                "supervisor_pid": shared.supervisor_pid,
                "supervisor_instance_id": shared.supervisor_instance_id,
                "child_pid": shared.child_pid.load(Ordering::Relaxed),
            }))
        }
        IpcMethod::Pid => {
            if shared.supervisor_pid > 0 {
                IpcResponse::ok(serde_json::json!({
                    "pid": shared.supervisor_pid,
                    "supervisor_instance_id": shared.supervisor_instance_id,
                }))
            } else {
                IpcResponse::ok(serde_json::json!({ "pid": null }))
            }
        }
        IpcMethod::Inject { bytes } => {
            // `Inject` is a real prompt dispatch; the central gate above already
            // refused it when the managed-capability proof failed.
            match deliver_ipc_inject(shared, &bytes, "ipc_inject") {
                Ok(()) => {
                    shared.transition_actor_state(
                        agent_doc_sqlite::state_store::ActorState::Busy,
                        "dispatch",
                        "ipc_inject",
                    );
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                Err(err) => IpcResponse::err(err),
            }
        }
        IpcMethod::Clear { bytes } => {
            // Gate-exempt operator control channel: clearing a session is a
            // recovery action, not a dispatch, so it must succeed even when the
            // capability proof has failed (#codex-capability-proof-unrecoverable).
            match deliver_ipc_inject(shared, &bytes, "ipc_clear") {
                Ok(()) => {
                    shared.transition_actor_state(
                        agent_doc_sqlite::state_store::ActorState::Busy,
                        "operator",
                        "ipc_clear",
                    );
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                Err(err) => IpcResponse::err(err),
            }
        }
        IpcMethod::Restart { mode } => {
            shared.transition_actor_state(
                agent_doc_sqlite::state_store::ActorState::Busy,
                "supervisor",
                "ipc_restart_requested",
            );
            *shared.restart_mode.lock().unwrap() = mode;
            shared.restart_requested.store(true, Ordering::Relaxed);
            // `#supkill-bg` — blue/green drain-and-supersede. When the supervisor's own
            // binary is stale (the `restart-supervisor … generation closed` / `#fcc0`
            // case), do NOT kill the child here: route the restart through the
            // idle-watch in-place `execve` reexec so it drains the in-flight turn, then
            // hot-reloads onto the fresh binary preserving the live child + pane. The
            // in-process host loop honors this flag and defers its restart-kill. A
            // fresh binary has nothing to upgrade, so it keeps the immediate
            // kill-child → relaunch path.
            let reexec = shared.binary_stale.load(Ordering::Relaxed);
            shared.restart_reexec.store(reexec, Ordering::Relaxed);
            if !reexec {
                shared.kill_child();
            }
            IpcResponse::ok_empty()
        }
        IpcMethod::Stop { graceful: _ } => {
            shared.stop_requested.store(true, Ordering::Relaxed);
            shared.kill_child();
            IpcResponse::ok_empty()
        }
        IpcMethod::StopAgent { reason: _ } => {
            // "Stop Agent": kill the harness child but keep the supervisor alive.
            // Unlike `Stop`, this must NOT set `stop_requested` (which exits the
            // supervisor) and must NOT set `restart_requested` (which auto-restarts).
            // The run loop observes `stop_agent_requested` after the child exits and
            // lands on the restart-or-quit keepalive prompt so the operator can
            // restart manually.
            shared.transition_actor_state(
                agent_doc_sqlite::state_store::ActorState::WaitingInput,
                "supervisor",
                "ipc_stop_agent_requested",
            );
            shared.stop_agent_requested.store(true, Ordering::Relaxed);
            shared.kill_child();
            IpcResponse::ok_empty()
        }
        IpcMethod::ReplicaRegister { file, identity } => handle_replica_register(&file, &identity),
        IpcMethod::ReplicaDeregister { file, identity } => {
            handle_replica_deregister(&file, &identity)
        }
        IpcMethod::ReplicaUpdate {
            file,
            identity,
            update_b64,
        } => handle_replica_update(&file, &identity, &update_b64),
        IpcMethod::ReplicaPull { file, identity } => handle_replica_pull(&file, &identity),
        IpcMethod::ReplicaAck {
            file,
            identity,
            patch_id,
            generation,
        } => handle_replica_ack(&file, &identity, &patch_id, generation),
        IpcMethod::ReplicaAwareness {
            file,
            identity,
            awareness_b64,
        } => handle_replica_awareness(&file, &identity, &awareness_b64),
    }
}

// --- CRDT live multi-editor delta fan-out IPC handlers (`#crdtauth5`) ---------
//
// Each handler routes the new editor-replica IPC family through the per-document
// `crdt_relay_host` hub registry. The hub-host functions resolve the document's
// `CrdtAuthority` first and refuse (return `None`/`false`, allocate no hub) when
// the document has no live editor (Detached / `GitAuthoritative`), so this whole
// family is inert on the headless control-plane path. Per-document isolation is
// structural: the hub is keyed by the document hash, never shared across docs.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

fn handle_replica_register(file: &str, identity: &str) -> IpcResponse {
    match crate::crdt_relay_host::register_replica_for_file(std::path::Path::new(file), identity) {
        Ok(Some((client_id, bootstrap))) => IpcResponse::ok(serde_json::json!({
            "client_id": client_id,
            "bootstrap_b64": BASE64_STANDARD.encode(&bootstrap),
        })),
        // Detached / no live editor: refuse so a headless document never spins up
        // a multi-replica session. NOT an error — the editor falls back to the
        // existing patch-file path.
        Ok(None) => {
            IpcResponse::err("crdt replica register refused: document is not editor-attached")
        }
        Err(e) => IpcResponse::err(format!("crdt replica register failed: {e}")),
    }
}

fn handle_replica_deregister(file: &str, identity: &str) -> IpcResponse {
    match crate::crdt_relay_host::deregister_replica_for_file(std::path::Path::new(file), identity)
    {
        Ok(removed) => IpcResponse::ok(serde_json::json!({ "removed": removed })),
        Err(e) => IpcResponse::err(format!("crdt replica deregister failed: {e}")),
    }
}

fn handle_replica_update(file: &str, identity: &str, update_b64: &str) -> IpcResponse {
    let update = match BASE64_STANDARD.decode(update_b64) {
        Ok(bytes) => bytes,
        Err(e) => return IpcResponse::err(format!("crdt replica update: bad base64: {e}")),
    };
    match crate::crdt_relay_host::relay_replica_update_for_file(
        std::path::Path::new(file),
        identity,
        &update,
    ) {
        Ok(Some(fan_out)) => {
            // The per-target deltas are relayed back so the requester (or the
            // supervisor's socket fan-out) can deliver them to the peers' FFI
            // nodes. The hub already applied them to the hub-side mirrors.
            let targets: Vec<serde_json::Value> = fan_out
                .targets
                .iter()
                .map(|target| {
                    serde_json::json!({
                        "client_id": target,
                        "update_b64": BASE64_STANDARD.encode(&fan_out.update),
                    })
                })
                .collect();
            IpcResponse::ok(serde_json::json!({
                "origin": fan_out.origin,
                "canonical_len": fan_out.canonical_len,
                "targets": targets,
            }))
        }
        Ok(None) => {
            IpcResponse::err("crdt replica update refused: document is not editor-attached")
        }
        Err(e) => IpcResponse::err(format!("crdt replica update failed: {e}")),
    }
}

fn handle_replica_pull(file: &str, identity: &str) -> IpcResponse {
    match crate::crdt_relay_host::pull_replica_updates_for_file(
        std::path::Path::new(file),
        identity,
    ) {
        Ok(Some(pull)) => {
            let updates: Vec<serde_json::Value> = pull
                .updates
                .iter()
                .map(|update| {
                    serde_json::json!({
                        "patch_id": update.patch_id,
                        "origin": update.origin,
                        "target": update.target,
                        "generation": update.generation,
                        "update_b64": BASE64_STANDARD.encode(&update.update),
                    })
                })
                .collect();
            IpcResponse::ok(serde_json::json!({
                "client_id": pull.client_id,
                "updates": updates,
                "current_generation": pull.delivery.current_generation,
                "last_ack_generation": pull.delivery.last_ack_generation,
                "pending_updates": pull.delivery.pending_updates,
            }))
        }
        Ok(None) => IpcResponse::err("crdt replica pull refused: document is not editor-attached"),
        Err(e) => IpcResponse::err(format!("crdt replica pull failed: {e}")),
    }
}

fn handle_replica_ack(file: &str, identity: &str, patch_id: &str, generation: u64) -> IpcResponse {
    match crate::crdt_relay_host::ack_replica_update_for_file(
        std::path::Path::new(file),
        identity,
        patch_id,
        generation,
    ) {
        Ok(Some(acknowledged)) => IpcResponse::ok(serde_json::json!({
            "acknowledged": acknowledged,
        })),
        Ok(None) => IpcResponse::err("crdt replica ack refused: document is not editor-attached"),
        Err(e) => IpcResponse::err(format!("crdt replica ack failed: {e}")),
    }
}

fn handle_replica_awareness(file: &str, identity: &str, awareness_b64: &str) -> IpcResponse {
    let json = match BASE64_STANDARD.decode(awareness_b64) {
        Ok(bytes) => bytes,
        Err(e) => return IpcResponse::err(format!("crdt awareness: bad base64: {e}")),
    };
    let state: crate::crdt_relay::AwarenessState = match serde_json::from_slice(&json) {
        Ok(state) => state,
        Err(e) => return IpcResponse::err(format!("crdt awareness: bad json: {e}")),
    };
    match crate::crdt_relay_host::set_replica_awareness_for_file(
        std::path::Path::new(file),
        identity,
        state,
    ) {
        Ok(Some(snapshot)) => {
            let presence: Vec<serde_json::Value> = snapshot
                .iter()
                .map(|(client_id, state)| {
                    serde_json::json!({
                        "client_id": client_id,
                        "awareness_b64": BASE64_STANDARD
                            .encode(serde_json::to_vec(state).unwrap_or_default()),
                    })
                })
                .collect();
            IpcResponse::ok(serde_json::json!({ "presence": presence }))
        }
        Ok(None) => IpcResponse::err("crdt awareness refused: document is not editor-attached"),
        Err(e) => IpcResponse::err(format!("crdt awareness failed: {e}")),
    }
}

/// Spawn the master→stdout forwarding thread with escape sequence filtering.
pub(crate) fn spawn_reader_thread(
    shared: Arc<SupervisorShared>,
    harness: agent_doc_harness::HarnessConfig,
    mut reader: Box<dyn std::io::Read + Send>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("pty->stdout".into())
        .spawn(move || {
            let mut buf = [0u8; 8192];
            let mut filtered = Vec::with_capacity(8192);
            let stdout = std::io::stdout();
            let debug_filter = std::env::var("AGENT_DOC_DEBUG_FILTER").is_ok();
            // Stateful filter — carries partial escape sequences across reads.
            let mut pty_filter = crate::supervisor::pty::PtyFilter::for_harness(&harness);
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if debug_filter {
                            // Log raw bytes, showing escape sequences as hex
                            let raw = &buf[..n];
                            let mut display = String::new();
                            for &b in raw {
                                if b == 0x1b {
                                    display.push_str("\\x1b");
                                } else if b.is_ascii_graphic() || b == b' ' {
                                    display.push(b as char);
                                } else {
                                    display.push_str(&format!("\\x{b:02x}"));
                                }
                            }
                            eprintln!("[pty-filter] raw ({n} bytes): {display}");
                        }
                        filtered.clear();
                        pty_filter.filter(&buf[..n], &mut filtered);
                        if debug_filter {
                            let mut display = String::new();
                            for &b in &filtered {
                                if b == 0x1b {
                                    display.push_str("\\x1b");
                                } else if b.is_ascii_graphic() || b == b' ' {
                                    display.push(b as char);
                                } else {
                                    display.push_str(&format!("\\x{b:02x}"));
                                }
                            }
                            eprintln!(
                                "[pty-filter] filtered ({} bytes): {display}",
                                filtered.len()
                            );
                        }
                        if filtered.is_empty() {
                            continue;
                        }
                        record_terminal_screen(&shared, &filtered);
                        record_recent_output(&shared, &filtered);
                        if current_child_prompt_visible(&shared, &harness) {
                            if prompt_visible_requires_ready_transition(&shared) {
                                shared.transition_actor_state(
                                    agent_doc_sqlite::state_store::ActorState::Ready,
                                    "supervisor",
                                    "prompt_ready",
                                );
                            }
                            shared
                                .suppress_stale_ctrl_d_until_prompt
                                .store(false, Ordering::Relaxed);
                        }
                        let mut lock = stdout.lock();
                        if lock.write_all(&filtered).is_err() || lock.flush().is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("spawn pty->stdout thread")
}

/// Spawn the stdin→master forwarding thread using a shared writer.
///
/// Uses `poll()` on stdin + a stop pipe so the thread can be interrupted
/// cleanly before the supervisor needs stdin for the restart prompt.
#[cfg(unix)]
pub(crate) fn spawn_writer_thread(
    shared: Arc<SupervisorShared>,
    harness: agent_doc_harness::HarnessConfig,
    writer: Arc<Mutex<SharedPtyWriter>>,
    stop_fd: std::os::unix::io::RawFd,
    stop: Arc<AtomicBool>,
    ctrl_c_flag: Option<Arc<AtomicBool>>,
    ctrl_d_flag: Option<Arc<AtomicBool>>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("stdin->pty".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            let debug = std::env::var("AGENT_DOC_DEBUG_STDIN").is_ok();
            if debug {
                eprintln!("[stdin->pty] thread started");
            }
            loop {
                // Poll stdin (fd 0) and the stop pipe
                let mut fds = [
                    libc::pollfd {
                        fd: libc::STDIN_FILENO,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: stop_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                let ret = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
                if ret <= 0 {
                    if debug {
                        eprintln!("[stdin->pty] poll returned {ret}, exiting");
                    }
                    break; // poll error or signal interrupt
                }
                // Stop signal received
                if fds[1].revents & libc::POLLIN != 0 {
                    if debug {
                        eprintln!("[stdin->pty] stop signal received, exiting");
                    }
                    break;
                }
                // stdin ready
                if fds[0].revents & libc::POLLIN != 0 {
                    let n = unsafe {
                        libc::read(
                            libc::STDIN_FILENO,
                            buf.as_mut_ptr() as *mut libc::c_void,
                            buf.len(),
                        )
                    };
                    if n <= 0 {
                        if debug {
                            eprintln!("[stdin->pty] read returned {n}, exiting");
                        }
                        break; // EOF or error
                    }
                    let data = &buf[..n as usize];
                    let maybe_filtered = strip_stale_ctrl_d_before_prompt(
                        data,
                        shared
                            .suppress_stale_ctrl_d_until_prompt
                            .load(Ordering::Relaxed),
                        shared.prompt_visible_once.load(Ordering::Relaxed),
                    );
                    if let Some(filtered) = maybe_filtered.as_deref() {
                        crate::input_diag::log_transform_event(
                            None,
                            "supervisor.stdin",
                            "child_pty",
                            "drop_stale_ctrl_d_before_prompt",
                            data,
                            filtered,
                            Some(&harness.binary),
                        );
                    }
                    let data = maybe_filtered.as_deref().unwrap_or(data);
                    if data.is_empty() {
                        if debug {
                            eprintln!(
                                "[stdin->pty] suppressed stale Ctrl+D before keepalive prompt"
                            );
                        }
                        continue;
                    }
                    let maybe_translated =
                        normalize_stdin_for_harness_permission_prompt(&shared, &harness, data);
                    if let Some(translated) = maybe_translated.as_deref() {
                        crate::input_diag::log_prompt_detection(
                            None,
                            "supervisor.stdin",
                            "child_pty",
                            &harness.binary,
                            "active permission prompt",
                            "active",
                        );
                        crate::input_diag::log_transform_event(
                            None,
                            "supervisor.stdin",
                            "child_pty",
                            "opencode_permission_arrow_translation",
                            data,
                            translated,
                            Some(&harness.binary),
                        );
                    }
                    let data = maybe_translated.as_deref().unwrap_or(data);
                    if agent_doc_tmux_commands::input_diag::verbose_enabled() {
                        crate::input_diag::log_byte_events(
                            None,
                            "supervisor.stdin",
                            "child_pty",
                            "raw_forward",
                            data,
                            Some(&harness.binary),
                        );
                    }
                    // Detect Ctrl+D (\x04) — in raw mode this is a byte, not EOF.
                    // The pty slave's line discipline interprets it as EOF for the child.
                    if let Some(ref flag) = ctrl_d_flag
                        && data.contains(&0x04)
                    {
                        if debug {
                            eprintln!("[stdin->pty] Ctrl+D (\\x04) detected in forwarded data");
                        }
                        flag.store(true, Ordering::Relaxed);
                    }
                    if let Some(ref flag) = ctrl_c_flag
                        && data.contains(&0x03)
                    {
                        if debug {
                            eprintln!("[stdin->pty] Ctrl+C (\\x03) detected in forwarded data");
                        }
                        flag.store(true, Ordering::Relaxed);
                    }
                    let Some(mut w) = lock_writer_interruptibly(&writer, stop.as_ref()) else {
                        if debug {
                            eprintln!("[stdin->pty] stop requested while waiting for writer");
                        }
                        break;
                    };
                    if let Err(err) = w.write_all_interruptibly(data, stop.as_ref()) {
                        if debug {
                            eprintln!("[stdin->pty] pty write failed, exiting: {err}");
                        }
                        break;
                    }
                }
                // stdin hangup/error
                if fds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                    if debug {
                        eprintln!(
                            "[stdin->pty] stdin hangup/error (revents=0x{:x}), exiting",
                            fds[0].revents
                        );
                    }
                    break;
                }
            }
            if debug {
                eprintln!("[stdin->pty] thread exiting");
            }
        })
        .expect("spawn stdin->pty thread")
}

/// Non-Unix fallback: blocking stdin read (no stop signal support).
#[cfg(not(unix))]
pub(crate) fn spawn_writer_thread(
    _shared: Arc<SupervisorShared>,
    _harness: agent_doc_harness::HarnessConfig,
    writer: Arc<Mutex<SharedPtyWriter>>,
    _stop_fd: (),
    stop: Arc<AtomicBool>,
    ctrl_c_flag: Option<Arc<AtomicBool>>,
    ctrl_d_flag: Option<Arc<AtomicBool>>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("stdin->pty".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            let stdin = std::io::stdin();
            loop {
                let mut lock = stdin.lock();
                match std::io::Read::read(&mut lock, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        drop(lock);
                        if let Some(ref flag) = ctrl_d_flag {
                            if buf[..n].contains(&0x04) {
                                flag.store(true, Ordering::Relaxed);
                            }
                        }
                        if let Some(ref flag) = ctrl_c_flag {
                            if buf[..n].contains(&0x03) {
                                flag.store(true, Ordering::Relaxed);
                            }
                        }
                        if agent_doc_tmux_commands::input_diag::verbose_enabled() {
                            crate::input_diag::log_byte_events(
                                None,
                                "supervisor.stdin",
                                "child_pty",
                                "raw_forward",
                                &buf[..n],
                                None,
                            );
                        }
                        let Some(mut w) = lock_writer_interruptibly(&writer, stop.as_ref()) else {
                            break;
                        };
                        if w.write_all_interruptibly(&buf[..n], stop.as_ref()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("spawn stdin->pty thread")
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::hooks::fire_doc_hooks;
    use agent_doc_config::Config;
    use agent_doc_frontmatter::frontmatter::Frontmatter;
    use agent_doc_project_config_io as project_config_io;
    use std::collections::HashMap;
    use tempfile::TempDir;
    use tmux_router::IsolatedTmux;
    // --- `#crdtauth5` end-to-end fan-out over the NEW IPC path -------------------

    /// A throwaway tracked document under a temp project root so `doc_hash` /
    /// authority lease resolution work against a real path.
    fn crdt_temp_doc(name: &str) -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, format!("# {name}\n\nbody\n")).unwrap();
        (dir, path)
    }

    /// Send a `#crdtauth5` replica IPC method over a REAL supervisor socket and
    /// return the parsed response — the production handler routes it through the
    /// per-document `crdt_relay_host` hub.
    fn crdt_send(
        sock: &std::path::Path,
        method: &IpcMethod,
    ) -> crate::supervisor::ipc::IpcResponse {
        crate::supervisor::ipc::send_command(sock, method).expect("send crdt ipc")
    }

    #[test]
    fn crdtauth5_end_to_end_fan_out_over_the_ipc_path() {
        use agent_doc_merge::crdt_sync::ReplicaState;
        use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

        let (_dir, doc) = crdt_temp_doc("fanout.md");
        let project_root = doc.parent().unwrap().to_path_buf();
        let file_str = doc.display().to_string();

        // Make the document editor-attached (MultiReplica): seed a live owner
        // lease for the CURRENT pid so `authority_for_file` resolves MultiReplica.
        crate::test_support::seed_live_plugin_owner_lease(&file_str);
        assert!(
            crate::crdt_authority::authority_for_file(&file_str).editor_attached(),
            "test setup: the document must be editor-attached"
        );

        // Stand up the REAL supervisor IPC socket with the production handler.
        let shared = Arc::new(SupervisorShared::new(
            "test",
            "crdtauth5-instance".to_string(),
        ));
        let shared_for_ipc = shared.clone();
        let session_id = "crdtauth5-session";
        let mut ipc = crate::supervisor::ipc::SupervisorIpc::start(
            &project_root,
            session_id,
            move |method| handle_ipc(method, &shared_for_ipc),
        )
        .expect("start supervisor ipc");
        std::thread::sleep(std::time::Duration::from_millis(50));
        let sock = crate::supervisor::ipc::socket_path(&project_root, session_id);

        // Editor A and Editor B each register over the socket. The supervisor
        // hub mints their client-ids and returns the canonical bootstrap state.
        let reg_a = crdt_send(
            &sock,
            &IpcMethod::ReplicaRegister {
                file: file_str.clone(),
                identity: "intellij:A".into(),
            },
        );
        assert!(reg_a.ok, "register A: {reg_a:?}");
        let a_id = reg_a.data.as_ref().unwrap()["client_id"].as_u64().unwrap();
        let a_bootstrap = B64
            .decode(
                reg_a.data.as_ref().unwrap()["bootstrap_b64"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();

        let reg_b = crdt_send(
            &sock,
            &IpcMethod::ReplicaRegister {
                file: file_str.clone(),
                identity: "vscode:B".into(),
            },
        );
        assert!(reg_b.ok, "register B: {reg_b:?}");
        let b_id = reg_b.data.as_ref().unwrap()["client_id"].as_u64().unwrap();
        assert_ne!(a_id, b_id, "distinct editors mint distinct client-ids");

        // Editor A's FFI node (a real ReplicaState bound to the minted id) makes a
        // LOCAL edit and encodes the delta against the canonical bootstrap state.
        let editor_a = ReplicaState::from_encoded(a_id, &a_bootstrap).unwrap();
        editor_a.apply_local_edit(0, 0, "FROM-A");
        let a_delta = editor_a.diff(&ReplicaState::new(0).state_vector()).unwrap();

        // Editor A broadcasts its update OVER THE IPC PATH. The supervisor hub
        // integrates canonical and fans the delta out to editor B's hub-side mirror.
        let upd = crdt_send(
            &sock,
            &IpcMethod::ReplicaUpdate {
                file: file_str.clone(),
                identity: "intellij:A".into(),
                update_b64: B64.encode(&a_delta),
            },
        );
        assert!(upd.ok, "replica update: {upd:?}");
        let data = upd.data.as_ref().unwrap();
        assert_eq!(data["origin"].as_u64().unwrap(), a_id);
        let targets = data["targets"].as_array().unwrap();
        assert_eq!(
            targets.len(),
            1,
            "the update fans out to the one other replica (B)"
        );
        assert_eq!(targets[0]["client_id"].as_u64().unwrap(), b_id);

        // Until B applies + ACKs the queued fan-out delivery, the live delivery cut
        // is not converged and materialization must not be considered safe.
        assert!(
            !crate::crdt_relay_host::commit_barrier_for_file(&doc),
            "unacked live fan-out delivery blocks the materialization barrier"
        );

        // B pulls its own pending delivery, applies it to its FFI node, then ACKs it.
        let editor_b = ReplicaState::from_encoded(b_id, &a_bootstrap).unwrap();
        let pull = crdt_send(
            &sock,
            &IpcMethod::ReplicaPull {
                file: file_str.clone(),
                identity: "vscode:B".into(),
            },
        );
        assert!(pull.ok, "replica pull: {pull:?}");
        let pulled = pull.data.as_ref().unwrap()["updates"].as_array().unwrap();
        assert_eq!(pulled.len(), 1, "B owns one pending delivery");
        assert_eq!(pulled[0]["target"].as_u64().unwrap(), b_id);
        let patch_id = pulled[0]["patch_id"].as_str().unwrap().to_string();
        let generation = pulled[0]["generation"].as_u64().unwrap();
        let to_b = B64
            .decode(pulled[0]["update_b64"].as_str().unwrap())
            .unwrap();
        editor_b.apply_update(&to_b).unwrap();
        assert!(
            editor_b.text().contains("FROM-A"),
            "replica B received A's op over the IPC fan-out path: {:?}",
            editor_b.text()
        );
        let ack = crdt_send(
            &sock,
            &IpcMethod::ReplicaAck {
                file: file_str.clone(),
                identity: "vscode:B".into(),
                patch_id,
                generation,
            },
        );
        assert!(ack.ok, "replica ack: {ack:?}");
        assert!(
            ack.data.as_ref().unwrap()["acknowledged"]
                .as_bool()
                .unwrap(),
            "the target ack clears the pending delivery"
        );

        // The commit barrier then captures a consistent cut INCLUDING the fanned-out
        // ops: the canonical replica holds A's edit and every live delivery is ACKed.
        assert!(crate::crdt_relay_host::commit_barrier_for_file(&doc));
        crate::crdt_relay_host::with_hub(&doc, |hub| {
            assert!(
                hub.canonical_text().contains("FROM-A"),
                "the commit barrier cut holds the fanned-out op"
            );
        })
        .unwrap();

        // Deregister B over the socket; the hub drops its mirror.
        let dereg = crdt_send(
            &sock,
            &IpcMethod::ReplicaDeregister {
                file: file_str.clone(),
                identity: "vscode:B".into(),
            },
        );
        assert!(dereg.ok && dereg.data.as_ref().unwrap()["removed"].as_bool().unwrap());

        ipc.stop();
    }

    #[test]
    fn crdtauth5_detached_path_refuses_replica_register_and_allocates_no_hub() {
        // A document with NO live editor (Detached / GitAuthoritative) must refuse
        // the new replica family and allocate no hub — the headless control-plane
        // path is unchanged.
        let (_dir, doc) = crdt_temp_doc("detached.md");
        let project_root = doc.parent().unwrap().to_path_buf();
        let file_str = doc.display().to_string();
        // No lease seeded → authority is GitAuthoritative.
        assert!(
            !crate::crdt_authority::authority_for_file(&file_str).editor_attached(),
            "test setup: the document must be detached"
        );

        let shared = Arc::new(SupervisorShared::new(
            "test",
            "detached-instance".to_string(),
        ));
        let shared_for_ipc = shared.clone();
        let session_id = "crdtauth5-detached";
        let mut ipc = crate::supervisor::ipc::SupervisorIpc::start(
            &project_root,
            session_id,
            move |method| handle_ipc(method, &shared_for_ipc),
        )
        .expect("start supervisor ipc");
        std::thread::sleep(std::time::Duration::from_millis(50));
        let sock = crate::supervisor::ipc::socket_path(&project_root, session_id);

        let reg = crdt_send(
            &sock,
            &IpcMethod::ReplicaRegister {
                file: file_str.clone(),
                identity: "intellij:detached".into(),
            },
        );
        assert!(!reg.ok, "the detached path refuses replica register");
        assert!(
            reg.error
                .as_deref()
                .unwrap_or_default()
                .contains("not editor-attached"),
            "{reg:?}"
        );
        // No hub was allocated for the detached document.
        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let allocated = crate::crdt_relay_host::hub_is_allocated_for_test(&hash);
        assert!(
            !allocated,
            "the detached path must not allocate a relay hub"
        );

        ipc.stop();
    }

    #[test]
    fn handle_ipc_inject_normalizes_submit_newline_before_writing() {
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));

        let response = handle_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            &shared,
        );

        assert!(response.ok);
        assert_eq!(
            written.lock().unwrap().as_slice(),
            b"agent-doc tasks/software/tsift.md\r"
        );
    }

    #[test]
    fn handle_ipc_state_includes_editor_sync_for_actor_file() {
        let (_dir, doc) = crdt_temp_doc("state-editor-sync.md");
        let project_root = doc.parent().unwrap().to_path_buf();
        let file_str = doc.display().to_string();
        agent_doc_debounce::document_changed_with_content_for_editor(
            &file_str,
            "disk plus unsaved editor text",
            Some("jetbrains:state"),
        );
        let runtime = SessionActorRuntime {
            project_root,
            file: doc.clone(),
            session_id: "state-session".to_string(),
            pane_id: "%1".to_string(),
            generation: 7,
        };
        let shared = Arc::new(SupervisorShared::with_actor_runtime(
            "test",
            "state-instance".to_string(),
            "claude",
            Some(runtime),
            Some(agent_doc_sqlite::state_store::ActorState::Ready),
            None,
        ));

        let response = handle_ipc(IpcMethod::State, &shared);
        assert!(response.ok, "{response:?}");
        let data = response.data.expect("state data");
        let sync = data.get("editor_sync").expect("editor_sync field");
        assert_eq!(sync["file"], file_str);
        assert_eq!(sync["statuses"][0]["edit_epoch"], 1);
        assert_eq!(sync["statuses"][0]["in_flight"], true);
    }

    #[test]
    fn handle_ipc_inject_allows_pending_capability_proof() {
        // `#capproofbg`: a *pending* managed-capability proof no longer blocks
        // dispatch. The `Inject` is delivered immediately (here to a recording PTY
        // writer) while the proof runs in the background; only a proven FAILURE
        // gates the inject (`handle_ipc_inject_rejects_failed_capability_proof`).
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(CapabilityProofGate::Pending, None);
        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));
        let response = handle_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            &shared,
        );

        assert!(response.ok, "{response:?}");
        assert_eq!(
            written.lock().unwrap().as_slice(),
            b"agent-doc tasks/software/tsift.md\r"
        );
    }
    #[test]
    fn handle_ipc_inject_rejects_failed_capability_proof() {
        // The dispatch gate still fails closed on a proven proof FAILURE — a
        // failed proof must remain visible and block dispatch (`#tsiftmdcrash`).
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(
            CapabilityProofGate::Failed,
            Some("network denied".to_string()),
        );
        let response = handle_ipc(
            IpcMethod::Inject {
                bytes: "agent-doc tasks/software/tsift.md\n".to_string(),
            },
            &shared,
        );

        assert!(!response.ok);
        assert!(
            response
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("capability proof failed"),
            "{response:?}"
        );
    }
    #[test]
    fn ipc_method_gate_classification_only_gates_inject() {
        // Only a real prompt dispatch is gated; operator/read-only methods are
        // gate-exempt so a proof-failed session stays recoverable.
        assert!(ipc_method_requires_capability_gate(&IpcMethod::Inject {
            bytes: "x".to_string(),
        }));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::Clear {
            bytes: "/clear".to_string(),
        }));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::Stop {
            graceful: false,
        }));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::Restart {
            mode: "continue".to_string(),
        }));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::State));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::Pid));
    }
    #[test]
    fn handle_ipc_clear_bypasses_failed_capability_proof() {
        // #codex-capability-proof-unrecoverable: with the gate `Failed`, an
        // `Inject` is refused by the dispatch gate but `Clear` is delivered
        // (here to a recording PTY writer) without the gate error.
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(
            CapabilityProofGate::Failed,
            Some("network denied".to_string()),
        );

        let inject = handle_ipc(
            IpcMethod::Inject {
                bytes: "/clear".to_string(),
            },
            &shared,
        );
        assert!(!inject.ok);
        assert!(
            inject
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("capability proof failed"),
            "{inject:?}"
        );

        let written = Arc::new(Mutex::new(Vec::new()));
        *shared.inject_writer.lock().unwrap() = Some(Arc::new(Mutex::new(SharedPtyWriter::new(
            Box::new(RecordingWriter(written.clone())),
        ))));
        let clear = handle_ipc(
            IpcMethod::Clear {
                bytes: "/clear".to_string(),
            },
            &shared,
        );
        assert!(clear.ok, "clear must bypass the dispatch gate: {clear:?}");
        // Delivery matches the Inject path: trailing-newline normalization only,
        // no spurious CR added when the control text has none.
        assert_eq!(written.lock().unwrap().as_slice(), b"/clear");
    }
    #[test]
    fn handle_ipc_stop_bypasses_failed_capability_proof() {
        // Stopping a session is recovery, not dispatch: it must succeed even
        // when the capability proof failed.
        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        shared.set_capability_proof_gate(
            CapabilityProofGate::Failed,
            Some("network denied".to_string()),
        );
        let response = handle_ipc(IpcMethod::Stop { graceful: false }, &shared);
        assert!(response.ok, "{response:?}");
        assert!(shared.stop_requested.load(Ordering::Relaxed));
    }
    #[cfg(unix)]
    #[test]
    fn writer_thread_exits_on_stop_signal() {
        // Create a pipe to act as the "pty writer" — we just need something
        // that accepts writes without blocking
        let mut pty_fds = [0i32; 2];
        unsafe { libc::pipe(pty_fds.as_mut_ptr()) };
        let pty_write_fd = pty_fds[1];

        // Wrap the write end in a Box<dyn Write + Send> for spawn_writer_thread
        struct FdWriter(i32);
        impl Write for FdWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let n =
                    unsafe { libc::write(self.0, buf.as_ptr() as *const libc::c_void, buf.len()) };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let writer: Box<dyn Write + Send> = Box::new(FdWriter(pty_write_fd));
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::new(writer)));

        let stop = StopSignal::new().unwrap();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(SupervisorShared::new("test", "writer-stop".to_string()));
        let handle = spawn_writer_thread(
            shared,
            agent_doc_harness::HarnessConfig::codex(),
            writer_arc,
            stop.read_fd(),
            stop_flag.clone(),
            None,
            None,
        );

        // Writer thread should be alive, blocked in poll()
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Signal stop — thread should exit promptly
        stop_flag.store(true, Ordering::Relaxed);
        stop.signal();
        let result = handle.join();
        assert!(
            result.is_ok(),
            "writer thread should exit cleanly on stop signal"
        );

        // Clean up pipe fds
        unsafe {
            libc::close(pty_fds[0]);
            libc::close(pty_fds[1]);
        }
    }
    #[cfg(unix)]
    #[test]
    fn writer_thread_exits_on_pty_write_failure() {
        // Create a pipe as the "pty writer", then close the read end so
        // writes fail with EPIPE — simulating Claude exit closing the PTY
        let mut pty_fds = [0i32; 2];
        unsafe { libc::pipe(pty_fds.as_mut_ptr()) };
        // Close read end immediately so writes produce EPIPE
        unsafe { libc::close(pty_fds[0]) };

        struct FdWriter(i32);
        impl Write for FdWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let n =
                    unsafe { libc::write(self.0, buf.as_ptr() as *const libc::c_void, buf.len()) };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let writer: Box<dyn Write + Send> = Box::new(FdWriter(pty_fds[1]));
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::new(writer)));

        let stop = StopSignal::new().unwrap();
        let stop_fd = stop.read_fd();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(SupervisorShared::new("test", "writer-epipe".to_string()));
        let handle = spawn_writer_thread(
            shared,
            agent_doc_harness::HarnessConfig::codex(),
            writer_arc,
            stop_fd,
            stop_flag.clone(),
            None,
            None,
        );

        // Inject a byte into stdin to trigger a write attempt.
        // The write will fail (EPIPE) and the thread should exit.
        // We use the stop signal as a fallback timeout.
        std::thread::sleep(std::time::Duration::from_millis(50));
        stop_flag.store(true, Ordering::Relaxed);
        stop.signal();

        let result = handle.join();
        assert!(
            result.is_ok(),
            "writer thread should exit on write failure or stop"
        );

        unsafe { libc::close(pty_fds[1]) };
    }
    #[cfg(unix)]
    #[test]
    fn reader_thread_exits_on_eof() {
        // Create a pipe as mock pty reader. Closing the write end
        // should cause the reader thread to see EOF and exit.
        let mut fds = [0i32; 2];
        unsafe { libc::pipe(fds.as_mut_ptr()) };

        struct FdReader(i32);
        impl std::io::Read for FdReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n =
                    unsafe { libc::read(self.0, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }
        }

        let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
        let reader: Box<dyn std::io::Read + Send> = Box::new(FdReader(fds[0]));
        let handle = spawn_reader_thread(shared, agent_doc_harness::HarnessConfig::codex(), reader);

        // Close the write end → reader sees EOF → thread exits
        unsafe { libc::close(fds[1]) };

        let result = handle.join();
        assert!(result.is_ok(), "reader thread should exit cleanly on EOF");

        unsafe { libc::close(fds[0]) };
    }
}

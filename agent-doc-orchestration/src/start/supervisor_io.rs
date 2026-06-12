//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn ipc_method_requires_capability_gate(method: &IpcMethod) -> bool {
    matches!(method, IpcMethod::Inject { .. })
}

/// Shared delivery for injected text (pane submit or PTY write). Used by both
/// the gated [`IpcMethod::Inject`] path and the gate-exempt
/// [`IpcMethod::Clear`] path; the gate decision is made by the caller.
pub(crate) fn deliver_ipc_inject(shared: &SupervisorShared, bytes: &str, diag_op: &str) -> Result<(), String> {
    if let Some(pane_id) = shared.inject_pane.as_deref() {
        crate::input_diag::log_text_submit(
            None,
            &format!("supervisor.{diag_op}"),
            &format!("pane:{pane_id}"),
            bytes,
            Some(&shared.harness_binary),
            if shared.harness_binary == "opencode" {
                "ipc_inject_kitty_return"
            } else {
                "ipc_inject_enter"
            },
            if shared.harness_binary == "opencode" {
                "KittyReturn"
            } else {
                "Enter"
            },
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
            IpcResponse::ok(serde_json::json!({
                "running": shared.running.load(Ordering::Relaxed),
                "state": state.as_str(),
                "actor_state": actor_state,
                "actor_session_id": actor_session_id,
                "actor_pane_id": actor_pane_id,
                "actor_generation": actor_generation,
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
                        crate::session_actor::ActorState::Busy,
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
                        crate::session_actor::ActorState::Busy,
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
                crate::session_actor::ActorState::Busy,
                "supervisor",
                "ipc_restart_requested",
            );
            *shared.restart_mode.lock().unwrap() = mode;
            shared.restart_requested.store(true, Ordering::Relaxed);
            shared.kill_child();
            IpcResponse::ok_empty()
        }
        IpcMethod::Stop { graceful: _ } => {
            shared.stop_requested.store(true, Ordering::Relaxed);
            shared.kill_child();
            IpcResponse::ok_empty()
        }
    }
}

/// Spawn the master→stdout forwarding thread with escape sequence filtering.
pub(crate) fn spawn_reader_thread(
    shared: Arc<SupervisorShared>,
    harness: crate::harness::HarnessConfig,
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
                                    crate::session_actor::ActorState::Ready,
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
    harness: crate::harness::HarnessConfig,
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
                    if crate::input_diag::verbose_enabled() {
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
    _harness: crate::harness::HarnessConfig,
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
                        if crate::input_diag::verbose_enabled() {
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
use crate::config::Config;
use crate::frontmatter::Frontmatter;
use crate::hooks::fire_doc_hooks;
use crate::project_config;
use crate::sessions::IsolatedTmux;
use std::collections::HashMap;
use tempfile::TempDir;
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
fn handle_ipc_inject_rejects_pending_capability_proof() {
    let shared = Arc::new(SupervisorShared::new("test", "test-instance".to_string()));
    shared.set_capability_proof_gate(CapabilityProofGate::Pending, None);
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
            .contains("capability proof is still pending"),
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
        crate::harness::HarnessConfig::codex(),
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
        crate::harness::HarnessConfig::codex(),
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
    let handle = spawn_reader_thread(shared, crate::harness::HarnessConfig::codex(), reader);

    // Close the write end → reader sees EOF → thread exits
    unsafe { libc::close(fds[1]) };

    let result = handle.join();
    assert!(result.is_ok(), "reader thread should exit cleanly on EOF");

    unsafe { libc::close(fds[0]) };
}
}

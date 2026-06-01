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
//! `.agent-doc/supervisor/<session-uuid>.sock`. External processes (editor
//! plugins, `/agent-doc` routing, CLI tools) connect and send JSON commands
//! to control the supervised claude process.
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

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, ToFsName,
    traits::{Listener as _, Stream as _},
};
use serde::{Deserialize, Serialize};

/// Subdirectory within `.agent-doc/` for supervisor sockets.
const SUPERVISOR_DIR: &str = "supervisor";

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
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(project_root.as_os_str().as_encoded_bytes());
    let hash = format!("{:x}", hasher.finalize());
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

/// IPC command sent by a client to the supervisor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum IpcMethod {
    Restart {
        #[serde(default = "default_restart_mode")]
        mode: String,
    },
    Inject {
        bytes: String,
    },
    /// Gate-exempt operator control injection (e.g. `/clear`). Unlike
    /// [`IpcMethod::Inject`], this bypasses the managed-capability dispatch gate
    /// so an operator can stop/clear a session whose capability proof failed
    /// without `kill -9`. Delivery is otherwise identical to `Inject`.
    Clear {
        bytes: String,
    },
    State,
    Pid,
    Stop {
        #[serde(default)]
        graceful: bool,
    },
}

fn default_restart_mode() -> String {
    "continue".to_string()
}

/// Normalize a single-line harness command before handing it to a submit path.
///
/// Tmux-backed submissions should use this normalized text directly and let the
/// pane submit helper add Enter. Raw PTY fallbacks should feed the normalized
/// text into `submit_bytes`.
pub fn normalize_submit_text(text: &str) -> String {
    text.trim_end_matches(['\r', '\n']).to_string()
}

/// Build the canonical raw-PTY submit bytes for a single-line harness input.
pub fn submit_bytes(text: &str) -> String {
    let payload = normalize_submit_text(text);
    format!("{payload}\r")
}

/// Response from the supervisor to a client command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn ok_empty() -> Self {
        Self {
            ok: true,
            data: None,
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
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
    /// It runs on the accept thread — callers that need to touch shared
    /// state (PtySession, CrashPolicy) should use interior mutability
    /// (Arc<Mutex<...>>).
    pub fn start<F>(project_root: &Path, session_uuid: &str, handler: F) -> Result<Self>
    where
        F: Fn(IpcMethod) -> IpcResponse + Send + 'static,
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

        let handle = thread::Builder::new()
            .name("supervisor-ipc".into())
            .spawn(move || {
                loop {
                    if stop_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    match listener.accept() {
                        Ok(stream) => {
                            // One request-response per connection. The client
                            // (send_command) connects, sends one NDJSON line,
                            // reads one response line, and drops the stream.
                            let (reader_half, mut writer_half) = stream.split();
                            let mut reader = BufReader::new(reader_half);
                            let mut line = String::new();

                            if reader.read_line(&mut line).unwrap_or(0) > 0 {
                                let trimmed = line.trim();
                                if !trimmed.is_empty() {
                                    let response = match serde_json::from_str::<IpcMethod>(trimmed)
                                    {
                                        Ok(method) => handler(method),
                                        Err(e) => IpcResponse::err(format!("parse error: {e}")),
                                    };

                                    let mut resp_json = serde_json::to_string(&response)
                                        .unwrap_or_else(|e| {
                                            format!(
                                                r#"{{"ok":false,"error":"serialize error: {e}"}}"#
                                            )
                                        });
                                    resp_json.push('\n');

                                    if let Err(e) = writer_half.write_all(resp_json.as_bytes()) {
                                        eprintln!("[supervisor::ipc] write error: {e}");
                                    }
                                    let _ = writer_half.flush();
                                }
                            }
                        }
                        Err(e) => {
                            if stop_clone.load(Ordering::Relaxed) {
                                break;
                            }
                            eprintln!("[supervisor::ipc] accept error: {e}");
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

/// Connect to a supervisor socket.
fn try_connect(sock: &Path) -> Result<interprocess::local_socket::Stream> {
    let name = sock.to_fs_name::<GenericFilePath>()?;
    let opts = interprocess::local_socket::ConnectOptions::new().name(name);
    let stream = opts
        .connect_sync()
        .context("failed to connect to supervisor socket")?;
    Ok(stream)
}

/// Send a command to a supervisor and read the response.
///
/// Connects to the socket, sends the command as NDJSON, and reads one
/// response line with a 2-second timeout.
#[allow(dead_code)] // API surface — used by tests and future IPC clients
pub fn send_command(sock: &Path, method: &IpcMethod) -> Result<IpcResponse> {
    let stream = try_connect(sock)?;
    let (reader_half, mut writer_half) = stream.split();

    // Send NDJSON
    let mut msg = serde_json::to_string(method)?;
    msg.push('\n');
    writer_half.write_all(msg.as_bytes())?;
    writer_half.flush()?;

    // Read response with timeout
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader_half);
        let mut line = String::new();
        let result = reader.read_line(&mut line);
        let _ = tx.send((result, line));
    });

    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok((Ok(0), _)) => anyhow::bail!("supervisor closed connection without responding"),
        Ok((Ok(_), line)) => {
            let resp: IpcResponse =
                serde_json::from_str(line.trim()).context("failed to parse supervisor response")?;
            Ok(resp)
        }
        Ok((Err(e), _)) => anyhow::bail!("supervisor read error: {e}"),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            anyhow::bail!("supervisor response timeout (2s)")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            anyhow::bail!("supervisor reader thread disconnected")
        }
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
    use std::sync::atomic::AtomicU32;

    fn start_echo_handler(root: &Path, uuid: &str) -> SupervisorIpc {
        SupervisorIpc::start(root, uuid, |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "restart_count": 0,
            })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
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
    fn submit_bytes_uses_single_carriage_return_submit() {
        assert_eq!(submit_bytes("/clear"), "/clear\r");
        assert_eq!(submit_bytes("/clear\n"), "/clear\r");
        assert_eq!(submit_bytes("/clear\r\n"), "/clear\r");
    }

    #[test]
    fn normalize_submit_text_strips_trailing_line_endings() {
        assert_eq!(normalize_submit_text("/clear"), "/clear");
        assert_eq!(normalize_submit_text("/clear\n"), "/clear");
        assert_eq!(normalize_submit_text("/clear\r\n"), "/clear");
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
        let count_clone = call_count.clone();

        let mut ipc = SupervisorIpc::start(root, "test-concurrent", move |method| {
            count_clone.fetch_add(1, Ordering::Relaxed);
            match method {
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 1 })),
                _ => IpcResponse::ok_empty(),
            }
        })
        .unwrap();

        std::thread::sleep(Duration::from_millis(50));

        let sock = socket_path(root, "test-concurrent");
        let mut handles = Vec::new();
        for _ in 0..5 {
            let s = sock.clone();
            handles.push(std::thread::spawn(move || {
                send_command(&s, &IpcMethod::Pid).unwrap()
            }));
        }

        for h in handles {
            let resp = h.join().unwrap();
            assert!(resp.ok);
        }

        // Note: concurrent clients are serialized by the accept loop, but
        // all should complete successfully.
        assert!(call_count.load(Ordering::Relaxed) >= 5);

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

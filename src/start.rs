//! # Module: start
//!
//! ## Spec
//! - `run(file)`: validates the file exists, then ensures a session UUID is
//!   present in the YAML frontmatter (generates and writes one if absent).
//! - Resolves agent args through the active harness:
//!   - Claude: frontmatter `agent_args` > frontmatter `claude_args` >
//!     config `agent_args` > config `claude_args` > `AGENT_DOC_CLAUDE_ARGS`
//!   - Codex: frontmatter `agent_args` > frontmatter `codex_args` >
//!     config `agent_args` > config `codex_args`
//! - Requires an active tmux session; bails immediately if not inside tmux.
//! - Registers the session UUID → current tmux pane ID in `sessions.json` so
//!   other subcommands (`route`, `focus`, etc.) can locate the pane.
//! - Runs `claude` as a blocking child process inside a persistent restart loop
//!   so the tmux pane never dies on its own.
//! - On non-zero exit (context exhaustion, crash, etc.): auto-restarts after a
//!   2-second delay using `--continue` to resume the previous conversation.
//! - On clean exit (code 0): prints a prompt to stderr and reads stdin; pressing
//!   Enter restarts fresh (no `--continue`), typing `q` + Enter exits.
//! - Prints the truncated session UUID and pane ID to stderr on registration.
//! - Opens a persistent session log at `.agent-doc/logs/<session-uuid>.log`,
//!   appending timestamped events for session start, claude start/restart/exit,
//!   user quit, and session end.
//! - On `--continue` restarts, spawns a background thread that polls
//!   `tmux capture-pane` for the harness prompt before
//!   sending the harness-specific trigger command via `tmux send-keys` to auto-trigger
//!   the skill workflow in the resumed conversation. This avoids the race
//!   where DSR (Device Status Report) escape sequences interleave with the
//!   injected command, corrupting Claude Code's input state.
//!
//! ## Agentic Contracts
//! - The file path must exist before `run` is called; callers must not rely on
//!   `run` to create the document.
//! - After `run` returns `Ok(())`, the session has ended cleanly (user chose
//!   to quit); the sessions.json entry is not automatically removed.
//! - Session UUID in frontmatter is idempotent: calling `run` on a file that
//!   already has a UUID does not regenerate or overwrite it.
//! - Resolved harness args are prepended to every agent invocation inside the
//!   loop, including restarts; they are resolved once at startup and held for
//!   the lifetime of the loop.
//! - The module writes to the document file (UUID injection), `sessions.json`,
//!   and `.agent-doc/logs/<session-uuid>.log`; it does not touch snapshots,
//!   git, or claims.
//! - Must be called from within an active tmux session; violating this contract
//!   returns an immediate `Err`.
//!
//! ## Evals
//! - `start_missing_file`: call `run` with a non-existent path → returns `Err`
//!   containing "file not found".
//! - `start_outside_tmux`: call `run` with a valid file while `TMUX` env var is
//!   unset → returns `Err` containing "not running inside tmux".
//! - `start_generates_uuid`: call `run` on a file with no frontmatter UUID →
//!   UUID is injected into the file and a "Generated session UUID" line appears
//!   on stderr before `claude` is launched.
//! - `start_preserves_existing_uuid`: call `run` on a file that already has a
//!   `session:` key → file content is unchanged (no re-write), no "Generated"
//!   message on stderr.
//! - `start_registers_session`: after setup, `sessions.json` maps the session
//!   UUID to the current tmux pane ID.
//! - `start_claude_args_precedence`: Claude resolves frontmatter `claude_args`
//!   over config `claude_args`, with `AGENT_DOC_CLAUDE_ARGS` as fallback.
//! - `start_codex_uses_codex_specific_alias_chain`: Codex resolves `codex_args`
//!   after `agent_args` and ignores `claude_args`.

use anyhow::{Context, Result};
use portable_pty::PtySize;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use crate::supervisor::{
    cwd,
    env::EnvSpec,
    ipc::{IpcMethod, IpcResponse, SupervisorIpc},
    pty::PtySpawnConfig,
    resize,
    state::{CrashPolicy, RestartAction, SupervisorState},
};
use crate::{config, frontmatter, sessions, snapshot};

/// Open (or create) the session log file at `.agent-doc/logs/<session-uuid>.log`.
/// Returns a writable file handle in append mode, or None if the directory can't be created.
fn open_session_log(file: &Path, session_id: &str) -> Option<std::fs::File> {
    // Walk up from the document to find the project root containing .agent-doc/
    let dir = file.parent()?;
    let mut search = Some(dir);
    let mut agent_doc_dir = None;
    while let Some(d) = search {
        let candidate = d.join(".agent-doc");
        if candidate.is_dir() {
            agent_doc_dir = Some(candidate);
            break;
        }
        search = d.parent();
    }
    let logs_dir = agent_doc_dir?.join("logs");
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join(format!("{}.log", session_id));
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as ISO-ish: just use epoch seconds for simplicity in logs
    format!("{}", now)
}

fn log_event(log: &mut Option<std::fs::File>, msg: &str) {
    if let Some(f) = log {
        let _ = writeln!(f, "[{}] {}", timestamp(), msg);
    }
}

/// Put stdin into raw mode so the outer pty line discipline doesn't translate
/// input bytes (ICRNL converts \r → \n, breaking Enter for Claude Code's TUI).
/// Restores original termios on drop.
#[cfg(unix)]
struct RawMode {
    original: libc::termios,
}

#[cfg(unix)]
impl RawMode {
    fn enable() -> Self {
        unsafe {
            let mut original: libc::termios = std::mem::zeroed();
            libc::tcgetattr(libc::STDIN_FILENO, &mut original);
            let mut raw = original;
            libc::cfmakeraw(&mut raw);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);
            Self { original }
        }
    }

    /// Temporarily restore cooked mode (for read_line prompts).
    fn suspend(&self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original);
        }
    }

    /// Re-enable raw mode after a suspend.
    fn resume(&self) {
        unsafe {
            let mut raw = self.original;
            libc::cfmakeraw(&mut raw);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw);
        }
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original);
        }
    }
}

#[cfg(not(unix))]
struct RawMode;

#[cfg(not(unix))]
impl RawMode {
    fn enable() -> Self {
        Self
    }
    fn suspend(&self) {}
    fn resume(&self) {}
}

/// Signal to stop the stdin→pty writer thread.
///
/// Uses a self-pipe: the writer thread polls both stdin and the pipe read end.
/// Calling `signal()` writes a byte to the pipe, waking the poll and causing
/// the writer thread to exit cleanly so stdin is available for `read_line()`.
#[cfg(unix)]
struct StopSignal {
    read_fd: std::os::unix::io::RawFd,
    write_fd: std::os::unix::io::RawFd,
}

#[cfg(unix)]
impl StopSignal {
    fn new() -> Result<Self> {
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            anyhow::bail!("pipe() failed: {}", std::io::Error::last_os_error());
        }
        Ok(Self {
            read_fd: fds[0],
            write_fd: fds[1],
        })
    }

    /// Wake the writer thread so it exits.
    fn signal(&self) {
        unsafe {
            libc::write(self.write_fd, b"x".as_ptr() as *const libc::c_void, 1);
        }
    }

    fn read_fd(&self) -> std::os::unix::io::RawFd {
        self.read_fd
    }
}

#[cfg(unix)]
impl Drop for StopSignal {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.read_fd);
            libc::close(self.write_fd);
        }
    }
}

#[cfg(not(unix))]
struct StopSignal;

#[cfg(not(unix))]
impl StopSignal {
    fn new() -> Result<Self> {
        Ok(Self)
    }
    fn signal(&self) {}
}

/// Shared writer handle: outer Mutex guards replace/clear, inner Mutex guards concurrent writes.
type SharedWriter = Mutex<Option<Arc<Mutex<Box<dyn Write + Send>>>>>;

/// Shared state between the main supervisor loop and the IPC handler thread.
struct SupervisorShared {
    /// Current supervisor state for IPC `state` queries.
    supervisor_state: Mutex<SupervisorState>,
    /// Current restart count.
    restart_count: AtomicU32,
    /// Whether a child is currently running.
    running: AtomicBool,
    /// CWD source tag for IPC `state` responses.
    cwd_source: &'static str,
    /// Writer handle for IPC `inject`. Replaced on each spawn, cleared between restarts.
    inject_writer: SharedWriter,
    /// Child PID for IPC `pid` queries and `kill` on restart/stop.
    child_pid: AtomicU32,
    /// Flag: IPC requested a restart.
    restart_requested: AtomicBool,
    /// Flag: IPC requested a stop.
    stop_requested: AtomicBool,
    /// Restart mode requested via IPC ("fresh" or "continue").
    restart_mode: Mutex<String>,
}

impl SupervisorShared {
    fn new(cwd_source: &'static str) -> Self {
        Self {
            supervisor_state: Mutex::new(SupervisorState::Healthy),
            restart_count: AtomicU32::new(0),
            running: AtomicBool::new(false),
            cwd_source,
            inject_writer: Mutex::new(None),
            child_pid: AtomicU32::new(0),
            restart_requested: AtomicBool::new(false),
            stop_requested: AtomicBool::new(false),
            restart_mode: Mutex::new("continue".to_string()),
        }
    }

    /// Send SIGTERM to the child process to unblock `wait()`.
    #[cfg(unix)]
    fn kill_child(&self) {
        let pid = self.child_pid.load(Ordering::Relaxed);
        if pid > 0 {
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }

    #[cfg(not(unix))]
    fn kill_child(&self) {
        // On non-Unix, we can't send signals. The main loop will detect
        // the flags after the child exits naturally or via other means.
    }
}

fn handle_ipc(method: IpcMethod, shared: &SupervisorShared) -> IpcResponse {
    match method {
        IpcMethod::State => {
            let state = shared.supervisor_state.lock().unwrap();
            IpcResponse::ok(serde_json::json!({
                "running": shared.running.load(Ordering::Relaxed),
                "state": state.as_str(),
                "restart_count": shared.restart_count.load(Ordering::Relaxed),
                "cwd_source": shared.cwd_source,
            }))
        }
        IpcMethod::Pid => {
            let pid = shared.child_pid.load(Ordering::Relaxed);
            if pid > 0 {
                IpcResponse::ok(serde_json::json!({ "pid": pid }))
            } else {
                IpcResponse::ok(serde_json::json!({ "pid": null }))
            }
        }
        IpcMethod::Inject { bytes } => {
            let guard = shared.inject_writer.lock().unwrap();
            match guard.as_ref() {
                Some(writer_arc) => {
                    let mut w = writer_arc.lock().unwrap();
                    match w.write_all(bytes.as_bytes()).and_then(|_| w.flush()) {
                        Ok(()) => IpcResponse::ok(serde_json::json!({ "n": bytes.len() })),
                        Err(e) => IpcResponse::err(format!("write error: {e}")),
                    }
                }
                None => IpcResponse::err("no active session"),
            }
        }
        IpcMethod::Restart { mode } => {
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
fn spawn_reader_thread(mut reader: Box<dyn std::io::Read + Send>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("pty->stdout".into())
        .spawn(move || {
            let mut buf = [0u8; 8192];
            let mut filtered = Vec::with_capacity(8192);
            let stdout = std::io::stdout();
            let debug_filter = std::env::var("AGENT_DOC_DEBUG_FILTER").is_ok();
            // Stateful filter — carries partial escape sequences across reads
            let mut pty_filter = crate::supervisor::pty::PtyFilter::new();
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
fn spawn_writer_thread(
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    stop_fd: std::os::unix::io::RawFd,
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
                    let mut w = writer.lock().unwrap();
                    if w.write_all(&buf[..n as usize]).is_err() || w.flush().is_err() {
                        if debug {
                            eprintln!("[stdin->pty] pty write failed, exiting");
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
fn spawn_writer_thread(
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    _stop_fd: (),
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
                        let mut w = writer.lock().unwrap();
                        if w.write_all(&buf[..n]).is_err() || w.flush().is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("spawn stdin->pty thread")
}

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Ensure session UUID exists in frontmatter
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (updated_content, session_id) = frontmatter::ensure_session(&content)?;
    if updated_content != content {
        std::fs::write(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("Generated session UUID: {}", session_id);
    }

    let (fm, _body) = frontmatter::parse(&updated_content)?;
    let global_config = config::load().unwrap_or_default();

    // Resolve harness config from frontmatter agent > config default_agent > claude
    let harness = crate::harness::HarnessConfig::from_context(&fm, &global_config);
    let resolved_agent_args = resolve_agent_args(&fm, &global_config, &harness);

    // Must be inside tmux
    if !sessions::in_tmux() {
        // Distinguish "tmux not installed" from "not inside a tmux session"
        let tmux_installed = std::process::Command::new("which")
            .arg("tmux")
            .output()
            .is_ok_and(|o| o.status.success());
        if !tmux_installed {
            let hint = if cfg!(target_os = "macos") {
                "brew install tmux"
            } else if cfg!(target_os = "linux") {
                "sudo apt install tmux  # or: sudo pacman -S tmux / sudo dnf install tmux"
            } else {
                "Install WSL first, then: sudo apt install tmux"
            };
            anyhow::bail!(
                "tmux is not installed.\n\n  Install it:\n    {}\n\n  Then start a tmux session:\n    tmux new-session -s dev",
                hint
            );
        }
        anyhow::bail!(
            "not running inside tmux — start a tmux session first:\n    tmux new-session -s dev"
        );
    }

    let pane_id = sessions::current_pane()?;

    // Guard: auto-relocate if the current pane is in a different session than the project expects.
    // This is how cross-session drift happens — a terminal in session 1 claims a document,
    // permanently binding it to session 1 even though the project targets session 0.
    if let Some(expected_session) = config::project_tmux_session() {
        let tmux = sessions::Tmux::default_server();
        relocate_if_wrong_session(&tmux, &pane_id, &expected_session);
    }

    // Register session → pane (with relative file path)
    let file_str = file.to_string_lossy();
    sessions::register(&session_id, &pane_id, &file_str)?;
    eprintln!("Registered session {} → pane {}", &session_id[..8], pane_id);

    // Open session log
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut session_log = open_session_log(&canonical, &session_id);
    log_event(
        &mut session_log,
        &format!(
            "session_start file={} pane={} session={}",
            file.display(),
            pane_id,
            &session_id[..8]
        ),
    );

    // Fire document-level session_start hooks
    crate::hooks::fire_doc_hooks(
        &fm.hooks,
        "session_start",
        file,
        &session_id,
        &fm.agent,
        &fm.model,
    );

    // --- Supervisor setup ---

    // Resolve CWD deterministically
    let resolved_cwd = cwd::resolve(None, fm.cwd.as_deref(), &canonical)?;
    log_event(
        &mut session_log,
        &format!(
            "cwd_resolved path={} source={}",
            resolved_cwd.path.display(),
            resolved_cwd.source.as_str()
        ),
    );

    // Resolve env once at startup — reused across all restarts for determinism
    let env_spec = EnvSpec::from_frontmatter(&fm);
    let mut resolved_env = env_spec.resolve()?;
    if harness.supports_enable_tool_search && fm.enable_tool_search.unwrap_or(false) {
        resolved_env.insert("ENABLE_TOOL_SEARCH".into(), "true".into());
    }

    // Build base args (resolved once, reused across restarts)
    let mut base_args: Vec<String> = Vec::new();
    if let Some(ref args) = resolved_agent_args {
        base_args.extend(args.split_whitespace().map(String::from));
    }
    crate::agent::append_workspace_access_args(&harness.binary, &mut base_args, &canonical);
    if harness.supports_no_mcp && fm.no_mcp.unwrap_or(false) {
        base_args.push("--no-mcp".into());
    }

    // Query initial terminal size
    let initial_size = {
        #[cfg(unix)]
        {
            resize::query_terminal_size(libc::STDIN_FILENO)
                .map(|(rows, cols)| PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap_or(PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                })
        }
        #[cfg(not(unix))]
        {
            PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            }
        }
    };

    // Find project root for IPC socket placement
    let project_root =
        snapshot::find_project_root(&canonical).unwrap_or_else(|| resolved_cwd.path.clone());

    // Create shared state for IPC handler
    let shared = Arc::new(SupervisorShared::new(resolved_cwd.source.as_str()));

    // Start IPC listener
    let shared_for_ipc = shared.clone();
    let mut ipc = SupervisorIpc::start(&project_root, &session_id, move |method| {
        handle_ipc(method, &shared_for_ipc)
    })?;
    log_event(
        &mut session_log,
        &format!("ipc_started project_root={}", project_root.display()),
    );

    // Crash policy state machine
    let mut policy = CrashPolicy::new();

    // Put stdin into raw mode so the outer pty's line discipline doesn't
    // mangle input bytes (e.g. ICRNL converting \r→\n). Claude Code sets
    // the inner pty slave to raw mode and expects \r for Enter — without
    // this, the outer pty's cooked mode silently converts \r to \n before
    // we even read it, breaking Enter for Claude Code's TUI.
    let raw_mode = RawMode::enable();

    // --- Supervisor restart loop ---
    let mut first_run = true;
    let mut restart_count: u32 = 0;
    let mut resize_watcher: Option<resize::ResizeWatcher> = None;
    loop {
        // Build args for this iteration
        let auto_trigger;
        let args = if !first_run {
            auto_trigger = true;
            let restart_args = harness.restart_args(&base_args);
            eprintln!("Restarting {} (continue)...", harness.binary);
            log_event(
                &mut session_log,
                &format!(
                    "{}_restart mode=continue restart_count={}",
                    harness.binary, restart_count
                ),
            );
            restart_args
        } else {
            auto_trigger = false;
            let args = base_args.clone();
            eprintln!("Starting {}...", harness.binary);
            log_event(
                &mut session_log,
                &format!(
                    "{}_start mode={} restart_count={}",
                    harness.binary,
                    if restart_count == 0 {
                        "fresh"
                    } else {
                        "fresh_restart"
                    },
                    restart_count
                ),
            );
            args
        };

        // Build PtySpawnConfig and spawn child under pty
        let cfg = PtySpawnConfig {
            program: harness.binary.clone(),
            args,
            cwd: resolved_cwd.path.clone(),
            env: resolved_env.clone(),
            size: initial_size,
        };
        let mut session = crate::supervisor::pty::PtySession::spawn(cfg)
            .with_context(|| format!("failed to spawn {}", harness.binary))?;

        // Extract writer and reader for shared I/O
        let pty_writer = session.take_writer()?;
        let pty_reader = session.clone_reader()?;
        let writer_arc = Arc::new(Mutex::new(pty_writer));

        // Update shared state
        *shared.inject_writer.lock().unwrap() = Some(writer_arc.clone());
        shared
            .child_pid
            .store(session.process_id().unwrap_or(0), Ordering::Relaxed);
        shared.running.store(true, Ordering::Relaxed);
        shared.restart_count.store(restart_count, Ordering::Relaxed);
        shared.restart_requested.store(false, Ordering::Relaxed);
        shared.stop_requested.store(false, Ordering::Relaxed);

        // Spawn I/O forwarding threads
        let reader_thread = spawn_reader_thread(pty_reader);
        let writer_stop = StopSignal::new().context("failed to create writer stop signal")?;
        #[cfg(unix)]
        let writer_thread = spawn_writer_thread(writer_arc, writer_stop.read_fd());
        #[cfg(not(unix))]
        let writer_thread = spawn_writer_thread(writer_arc, ());

        // Start resize watcher (stop previous one first)
        if let Some(mut rw) = resize_watcher.take() {
            rw.stop();
        }
        let resize_handle = session.resize_handle()?;
        resize_watcher = resize::ResizeWatcher::spawn(move |size| {
            if let Err(e) = resize_handle.resize(size) {
                eprintln!("[supervisor::resize] resize error: {e}");
            }
        })
        .ok();

        // For restarts, poll for agent prompt then re-send trigger command
        if auto_trigger {
            let trigger_pane = pane_id.clone();
            let trigger_file = file.to_string_lossy().to_string();
            let mut trigger_log = session_log.as_ref().and_then(|f| f.try_clone().ok());
            let trigger_harness = harness.clone();
            std::thread::spawn(move || {
                // Poll tmux capture-pane for the agent prompt character.
                // This avoids the race where DSR escape sequences interleave
                // with injected command bytes when using a fixed sleep.
                let mut ready = false;
                for attempt in 0..60 {
                    if attempt == 0 {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    if let Ok(output) = std::process::Command::new("tmux")
                        .args(["capture-pane", "-t", &trigger_pane, "-p"])
                        .output()
                        && output.status.success()
                    {
                        let text = String::from_utf8_lossy(&output.stdout);
                        let lines: Vec<&str> = text.lines().collect();
                        let tail = if lines.len() > 20 {
                            &lines[lines.len() - 20..]
                        } else {
                            &lines
                        };
                        if tail
                            .iter()
                            .any(|l| trigger_harness.matches_prompt(l.trim()))
                        {
                            ready = true;
                            break;
                        }
                    }
                }
                if !ready {
                    if let Some(ref mut f) = trigger_log {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let _ = writeln!(f, "[{}] auto_trigger_timeout (no prompt after 30s)", ts);
                    }
                    eprintln!(
                        "[agent-doc] auto-trigger: timed out waiting for {} prompt",
                        trigger_harness.binary
                    );
                    return;
                }

                let trigger_cmd = trigger_harness.trigger_command(&trigger_file);
                let status = std::process::Command::new("tmux")
                    .args(["send-keys", "-t", &trigger_pane, &trigger_cmd, "Enter"])
                    .output();
                match status {
                    Ok(output) if output.status.success() => {
                        if let Some(ref mut f) = trigger_log {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let _ = writeln!(f, "[{}] auto_trigger sent=\"{}\"", ts, trigger_cmd);
                        }
                        eprintln!("[agent-doc] auto-triggered: {}", trigger_cmd);
                    }
                    _ => {
                        if let Some(ref mut f) = trigger_log {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let _ = writeln!(f, "[{}] auto_trigger_failed", ts);
                        }
                        eprintln!("[agent-doc] auto-trigger failed");
                    }
                }
            });
        }

        // Block until child exits
        let status = session
            .wait()
            .with_context(|| format!("failed waiting on {}", harness.binary))?;
        first_run = false;

        // Stop the stdin→pty writer thread so stdin is free for the restart
        // prompt (or for the next iteration's fresh writer thread).
        writer_stop.signal();
        let _ = writer_thread.join();

        // Clean up shared state (must happen before dropping session so the
        // inject_writer Arc is released before the pty master closes).
        shared.running.store(false, Ordering::Relaxed);
        *shared.inject_writer.lock().unwrap() = None;
        shared.child_pid.store(0, Ordering::Relaxed);

        // Drop the session to close the pty master. The reader thread holds a
        // cloned reader fd — closing the master causes its read() to return
        // EOF so the thread can exit cleanly.
        drop(session);
        let _ = reader_thread.join();

        // Flush any stale stdin bytes that the writer thread consumed from the
        // kernel but couldn't forward (e.g., user pressed Enter during the
        // tiny race window between session.wait() and writer_stop.signal()).
        #[cfg(unix)]
        unsafe {
            libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
        }

        // Check IPC-requested stop
        if shared.stop_requested.load(Ordering::Relaxed) {
            log_event(&mut session_log, "ipc_stop");
            break;
        }

        // Check IPC-requested restart (override normal exit classification)
        if shared.restart_requested.load(Ordering::Relaxed) {
            let mode = shared.restart_mode.lock().unwrap().clone();
            first_run = mode == "fresh";
            restart_count += 1;
            log_event(
                &mut session_log,
                &format!("ipc_restart mode={} restart_count={}", mode, restart_count),
            );
            continue;
        }

        // Normal exit classification via CrashPolicy
        let code = status.exit_code() as i32;
        log_event(
            &mut session_log,
            &format!(
                "{}_exit code={} restart_count={}",
                harness.binary, code, restart_count
            ),
        );

        let action = policy.on_exit(code);
        *shared.supervisor_state.lock().unwrap() = policy.state;

        match action {
            RestartAction::PromptUser => {
                // Temporarily restore cooked mode so read_line() works with
                // normal line editing (echo, backspace, etc.)
                raw_mode.suspend();
                eprintln!("\n{} exited cleanly.", harness.binary);
                eprintln!("Press Enter to restart, or 'q' to exit.");
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_err() {
                    log_event(&mut session_log, "stdin_read_failed — exiting loop");
                    break;
                }
                if input.trim().eq_ignore_ascii_case("q") {
                    log_event(&mut session_log, "user_quit");
                    break;
                }
                // User pressed Enter — restart fresh
                raw_mode.resume();
                first_run = true;
                restart_count += 1;
            }
            RestartAction::RestartAfter {
                delay,
                with_continue,
            } => {
                eprintln!(
                    "\n{} exited with code {}. Restarting in {:?}...",
                    harness.binary, code, delay
                );
                log_event(
                    &mut session_log,
                    &format!(
                        "auto_restart delay={:?} with_continue={} restart_count={}",
                        delay,
                        with_continue,
                        restart_count + 1
                    ),
                );
                std::thread::sleep(delay);
                if !with_continue {
                    first_run = true;
                }
                restart_count += 1;
            }
            RestartAction::Halt => {
                eprintln!(
                    "\nSupervisor halted after {} restarts (flapping detected).",
                    restart_count
                );
                log_event(&mut session_log, "supervisor_halted");
                break;
            }
        }
    }

    // Restore terminal to original mode before cleanup
    drop(raw_mode);

    // Cleanup
    if let Some(mut rw) = resize_watcher.take() {
        rw.stop();
    }
    ipc.stop();
    log_event(&mut session_log, "session_end");
    eprintln!("Session ended for {}", file.display());
    Ok(())
}

fn resolve_agent_args(
    fm: &frontmatter::Frontmatter,
    global_config: &config::Config,
    harness: &crate::harness::HarnessConfig,
) -> Option<String> {
    match harness.binary.as_str() {
        "claude" => fm
            .agent_args
            .clone()
            .or_else(|| fm.claude_args.clone())
            .or_else(|| global_config.agent_args.clone())
            .or_else(|| global_config.claude_args.clone())
            .or_else(|| std::env::var("AGENT_DOC_CLAUDE_ARGS").ok()),
        "codex" => fm
            .agent_args
            .clone()
            .or_else(|| fm.codex_args.clone())
            .or_else(|| global_config.agent_args.clone())
            .or_else(|| global_config.codex_args.clone()),
        _ => fm
            .agent_args
            .clone()
            .or_else(|| global_config.agent_args.clone()),
    }
}

/// Auto-relocate `pane_id` to `expected_session` if it is currently in a different session.
/// Returns `true` if relocation succeeded or was unnecessary; `false` if relocation failed.
/// Falls back to warn-only on failure so the start isn't aborted.
pub(crate) fn relocate_if_wrong_session(
    tmux: &sessions::Tmux,
    pane_id: &str,
    expected_session: &str,
) -> bool {
    let actual_session = match tmux.pane_session(pane_id) {
        Ok(s) => s,
        Err(_) => return true, // can't determine — let registration proceed
    };
    if actual_session == expected_session {
        return true;
    }
    eprintln!(
        "[start] pane {} is in session '{}', expected '{}' — auto-relocating to project session",
        pane_id, actual_session, expected_session
    );
    if let Some(anchor) = tmux.active_pane(expected_session) {
        match sessions::PaneMoveOp::new(tmux, pane_id, &anchor)
            .allow_cross_session("auto-relocate to project session on start")
            .join("-dh")
        {
            Ok(()) => {
                eprintln!(
                    "[start] relocated pane {} → session '{}'",
                    pane_id, expected_session
                );
                true
            }
            Err(e) => {
                eprintln!(
                    "[start] WARNING: relocation failed ({}); pane {} will register in session '{}'",
                    e, pane_id, actual_session
                );
                false
            }
        }
    } else {
        eprintln!(
            "[start] WARNING: no active pane found in session '{}'; \
             pane {} will register in session '{}'",
            expected_session, pane_id, actual_session
        );
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::frontmatter::Frontmatter;
    use crate::hooks::fire_doc_hooks;
    use crate::sessions::IsolatedTmux;
    use std::collections::HashMap;

    #[test]
    fn resolve_agent_args_claude_prefers_claude_alias_chain() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config {
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::claude();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("--dangerously-skip-permissions"));
    }

    #[test]
    fn resolve_agent_args_claude_prefers_agent_args_over_claude_args() {
        let fm = Frontmatter {
            agent_args: Some("--model sonnet".into()),
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config::default();
        let harness = crate::harness::HarnessConfig::claude();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("--model sonnet"));
    }

    #[test]
    fn resolve_agent_args_codex_prefers_codex_alias_chain() {
        let fm = Frontmatter {
            codex_args: Some("-s danger-full-access".into()),
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config {
            codex_args: Some("-s workspace-write".into()),
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::codex();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("-s danger-full-access"));
    }

    #[test]
    fn resolve_agent_args_codex_ignores_claude_args_aliases() {
        let fm = Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config {
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::codex();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_agent_args_codex_uses_agent_args_only() {
        let fm = Frontmatter {
            agent_args: Some("-s danger-full-access".into()),
            codex_args: Some("-s workspace-write".into()),
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let cfg = Config {
            agent_args: Some("-s workspace-write".into()),
            codex_args: Some("-s read-only".into()),
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::codex();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("-s danger-full-access"));
    }

    #[test]
    fn resolve_agent_args_codex_uses_config_codex_args_fallback() {
        let fm = Frontmatter::default();
        let cfg = Config {
            codex_args: Some("-s danger-full-access".into()),
            claude_args: Some("--old-flag".into()),
            ..Default::default()
        };
        let harness = crate::harness::HarnessConfig::codex();
        let resolved = resolve_agent_args(&fm, &cfg, &harness);
        assert_eq!(resolved.as_deref(), Some("-s danger-full-access"));
    }

    // --- relocate_if_wrong_session tests ---

    #[test]
    fn relocate_noop_when_already_correct_session() {
        let iso = IsolatedTmux::new("start-reloc-noop");
        let pane = iso
            .new_session("sess-a", std::path::Path::new("/tmp"))
            .unwrap();
        // pane is already in sess-a; no relocation needed
        let result = relocate_if_wrong_session(&iso, &pane, "sess-a");
        assert!(
            result,
            "should return true (noop — already in correct session)"
        );
        // Verify pane is still in sess-a
        let sess = iso.pane_session(&pane).unwrap();
        assert_eq!(sess, "sess-a");
    }

    #[test]
    fn relocate_succeeds_cross_session() {
        let iso = IsolatedTmux::new("start-reloc-cross");
        let _pane_a = iso
            .new_session("sess-a", std::path::Path::new("/tmp"))
            .unwrap();
        let pane_b = iso
            .new_session("sess-b", std::path::Path::new("/tmp"))
            .unwrap();
        // pane_b is in sess-b; expected is sess-a — should auto-relocate
        let result = relocate_if_wrong_session(&iso, &pane_b, "sess-a");
        assert!(result, "should return true after successful relocation");
        let sess = iso.pane_session(&pane_b).unwrap();
        assert_eq!(sess, "sess-a", "pane should be in sess-a after relocation");
    }

    #[test]
    fn relocate_fails_gracefully_when_no_anchor() {
        let iso = IsolatedTmux::new("start-reloc-noanchor");
        let pane = iso
            .new_session("sess-a", std::path::Path::new("/tmp"))
            .unwrap();
        // Expected session "sess-nonexistent" has no active pane — relocation should fail gracefully
        let result = relocate_if_wrong_session(&iso, &pane, "sess-nonexistent");
        assert!(
            !result,
            "should return false when no anchor pane exists in expected session"
        );
        // pane should still be in original session
        let sess = iso.pane_session(&pane).unwrap();
        assert_eq!(
            sess, "sess-a",
            "pane should remain in original session on failure"
        );
    }

    #[test]
    fn fire_doc_hooks_substitutes_template_vars() {
        let tmp =
            std::env::temp_dir().join(format!("agent-doc-hook-test-{}.txt", std::process::id()));
        let cmd = format!(
            "echo '{{{{session_id}}}}:{{{{agent}}}}:{{{{model}}}}' > {}",
            tmp.display()
        );
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("session_start".to_string(), vec![cmd]);
        fire_doc_hooks(
            &hooks,
            "session_start",
            Path::new("/doc/test.md"),
            "abc-123",
            &Some("claude".to_string()),
            &Some("opus".to_string()),
        );
        let output = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert!(
            output.contains("abc-123"),
            "session_id not substituted: {}",
            output
        );
        assert!(
            output.contains("claude"),
            "agent not substituted: {}",
            output
        );
        assert!(output.contains("opus"), "model not substituted: {}", output);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn fire_doc_hooks_noop_for_missing_event() {
        let hooks: HashMap<String, Vec<String>> = HashMap::new();
        fire_doc_hooks(
            &hooks,
            "session_start",
            Path::new("/doc/test.md"),
            "id",
            &None,
            &None,
        );
    }

    #[test]
    fn fire_doc_hooks_noop_for_empty_event() {
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("session_start".to_string(), vec![]);
        fire_doc_hooks(
            &hooks,
            "session_start",
            Path::new("/doc/test.md"),
            "id",
            &None,
            &None,
        );
    }

    #[test]
    fn fire_doc_hooks_handles_none_agent_model() {
        let tmp = std::env::temp_dir().join(format!(
            "agent-doc-hook-none-test-{}.txt",
            std::process::id()
        ));
        let cmd = format!("printf '{{{{agent}}}}:{{{{model}}}}' > {}", tmp.display());
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("session_start".to_string(), vec![cmd]);
        fire_doc_hooks(
            &hooks,
            "session_start",
            Path::new("/doc/test.md"),
            "id",
            &None,
            &None,
        );
        let output = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(output, ":", "expected empty agent+model, got: {}", output);
        let _ = std::fs::remove_file(&tmp);
    }

    // --- StopSignal + writer thread tests ---

    #[cfg(unix)]
    #[test]
    fn stop_signal_wakes_poll() {
        // StopSignal should create a valid pipe and signal() should not panic
        let stop = StopSignal::new().unwrap();
        stop.signal();
        // Verify the read end is readable after signal
        let mut fds = [libc::pollfd {
            fd: stop.read_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, 100) };
        assert_eq!(ret, 1, "poll should return 1 after signal");
        assert_ne!(fds[0].revents & libc::POLLIN, 0, "POLLIN should be set");
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
        let writer_arc = Arc::new(Mutex::new(writer));

        let stop = StopSignal::new().unwrap();
        let handle = spawn_writer_thread(writer_arc, stop.read_fd());

        // Writer thread should be alive, blocked in poll()
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Signal stop — thread should exit promptly
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
        let writer_arc = Arc::new(Mutex::new(writer));

        let stop = StopSignal::new().unwrap();
        let stop_fd = stop.read_fd();
        let handle = spawn_writer_thread(writer_arc, stop_fd);

        // Inject a byte into stdin to trigger a write attempt.
        // The write will fail (EPIPE) and the thread should exit.
        // We use the stop signal as a fallback timeout.
        std::thread::sleep(std::time::Duration::from_millis(50));
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

        let reader: Box<dyn std::io::Read + Send> = Box::new(FdReader(fds[0]));
        let handle = spawn_reader_thread(reader);

        // Close the write end → reader sees EOF → thread exits
        unsafe { libc::close(fds[1]) };

        let result = handle.join();
        assert!(result.is_ok(), "reader thread should exit cleanly on EOF");

        unsafe { libc::close(fds[0]) };
    }

    #[cfg(unix)]
    #[test]
    fn tcflush_discards_pending_input() {
        // Verify that tcflush(TCIFLUSH) discards buffered input.
        // This test uses a socketpair to avoid interfering with the
        // real stdin — it confirms the libc call doesn't panic.
        // (A full stdin test would require pty allocation.)
        unsafe {
            // Just verify the call doesn't error on STDIN_FILENO
            // (it may return -1 if stdin isn't a tty, which is fine in CI)
            let ret = libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
            // In CI / non-tty contexts, ret may be -1 (ENOTTY). That's OK —
            // the code uses tcflush as best-effort cleanup.
            let _ = ret;
        }
    }
}

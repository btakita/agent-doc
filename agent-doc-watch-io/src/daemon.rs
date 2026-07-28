//! # Module: watch
//!
//! ## Spec
//! - `start(config, WatchConfig)` runs the watch daemon. Acquires a PID file
//!   (`.agent-doc/watch.pid`) to prevent duplicate daemons; bails if one is already alive.
//! - `agent_doc_watch_io::is_running()` checks the PID file and `/proc/<pid>` for daemon liveness.
//! - `ensure_running()` lazily starts the daemon if it is not running; spawns the
//!   `agent-doc watch` subprocess from the project root. Returns `Ok(true)` if started,
//!   `Ok(false)` if already running.
//! - `stop()` signals shutdown by removing the PID file; the daemon detects this on its
//!   next loop tick and exits cleanly.
//! - `status()` prints daemon running state and PID to stdout.
//! - The daemon resolves the project root (directory containing `.agent-doc/`) at startup
//!   and `chdir`s there so relative paths (PID file, durable registry) are always correct.
//! - Session discovery: `discover_entries()` reads the durable registry and classifies each
//!   document by frontmatter mode:
//!   - `FileWatch` — append/template mode: monitored via `notify` file-system watcher.
//!   - `StreamCapture` — CRDT mode: polled by capturing the associated tmux pane.
//! - File-watch path: events are debounced (`debounce_ms`). After debounce, the
//!   controller-owned document watcher gate routes the change through the
//!   per-document session actor. It records the change; it no longer starts the
//!   legacy run/preflight loop directly.
//! - Loop prevention for file-watch: agent-triggered changes (within `debounce * 3` of
//!   last run) increment a per-file cycle counter; hard cap at `max_cycles`. Content hash
//!   equality stops the loop early (convergence detection).
//! - Stream-capture path: every 500 ms poll tick, `agent_doc_tmux_io::capture_pane()` is called;
//!   new lines (from `agent_doc_turn_executor::capture::capture_delta`) are
//!   flushed to the document via `stream::flush_to_document()`.
//! - CRDT documents also get reactive file-watching with zero debounce (tracked in
//!   `reactive_paths: HashSet<PathBuf>`).
//! - Watched documents keep a previous markdown projection and log
//!   `document_node_events` batches with node-keyed item insert/remove/replace/
//!   move/strike/unstrike events on each file change.
//! - Session registry is rescanned every 10 s to pick up newly registered documents.
//! - Dead stream panes (pane no longer alive in tmux) are pruned on rescan.
//! - Idle timeout: daemon exits automatically after 60 s with no active sessions.
//! - PID file removal (external `stop` or daemon crash) triggers clean shutdown on next tick.
//! - `agent_doc_turn_executor::capture::capture_delta(old, new)` finds the first
//!   diverging line between two captures and returns all new lines from that point
//!   onward.
//!
//! ## Agentic Contracts
//! - `start()`, `stop()`, `status()`, and `ensure_running()` are the
//!   public API surface; all loop internals are private.
//! - `ensure_running()` is safe to call from any subcommand needing the daemon; it
//!   is idempotent and returns promptly when the daemon is already live.
//! - Capture-delta policy is pure and allocation-only; no I/O or side effects.
//! - The daemon never panics on individual file submit errors; errors are logged to
//!   stderr and the loop continues.
//!
//! ## Evals
//! - pid_file_roundtrip: write_pid → read_pid matches process id; remove_pid → read_pid returns None
//! - pid_alive_self: current process PID → true
//! - pid_alive_nonexistent: PID 4294967295 → false
//! - discover_empty_registry: no durable registry → empty vec
//! - hash_deterministic: same file content read twice → identical hash
//! - hash_changes_with_content: file content changed → different hash
//! - loop_prevention_counter: increment then reset cycle_count → correct values
//! - convergence_detection: last_hash set → value preserved in state
//! - capture_delta_appended: new has extra lines at end → returns only new lines
//! - capture_delta_modified: diverges at line 2 → returns from divergence point onward
//! - capture_delta_identical: same content → empty string
//! - capture_delta_empty_old: old empty → all new lines returned
//! - capture_delta_empty_new: new empty → empty string
//! - stream_state_tracks_capture: two captures → incremental new content extracted correctly
//! - doc_mode_eq: FileWatch == FileWatch; StreamCapture != FileWatch

use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use agent_doc_document::watch_projection::{
    document_node_events_payload, project_watch_node_events, watch_content_hash,
};
use agent_doc_document_realtime::watch_authority::{RawWatchEvent, WatchDelivery};
use agent_doc_turn::cycle_phase_freshly_in_flight;
use notify::{EventKind, RecursiveMode, Watcher};

use agent_doc_config::Config;
use agent_doc_frontmatter::frontmatter;
use agent_doc_turn_executor::capture::{capture_delta, limit_capture_lines};

/// Default idle timeout before daemon auto-exits (seconds).
const IDLE_TIMEOUT_SECS: u64 = 60;

/// Minimum window for detecting agent-triggered changes (ms).
/// Reactive (zero-debounce) paths would collapse `debounce * 3` to 0, making
/// every change look agent-triggered. This floor ensures a usable detection
/// window regardless of debounce setting.
const MIN_AGENT_CHANGE_WINDOW_MS: u64 = 500;

/// Configuration for the watch daemon.
pub struct WatchConfig {
    pub debounce_ms: u64,
    pub max_cycles: u32,
}

/// Boundary effects the daemon cannot own while orchestration still owns stream
/// document writes and controller/session-actor routing.
pub trait WatchDaemonEffects {
    fn flush_stream_to_document(
        &mut self,
        file: &Path,
        text: &str,
        target: &str,
        baseline: &str,
    ) -> Result<()>;

    fn route_file_change(
        &mut self,
        base_dir: &Path,
        doc_id: &str,
        file: &str,
        raw: &RawWatchEvent,
        current_content: &str,
    ) -> Result<WatchDelivery>;

    fn on_file_change(&mut self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn on_config_change(&mut self) -> Result<usize> {
        Ok(0)
    }

    fn on_stream_dead(&mut self, _path: &Path) -> Result<()> {
        Ok(())
    }
}

/// Per-file state for loop prevention (file-watch mode).
struct FileState {
    last_run: Option<Instant>,
    cycle_count: u32,
    last_hash: Option<String>,
}

impl FileState {
    fn new() -> Self {
        Self {
            last_run: None,
            cycle_count: 0,
            last_hash: None,
        }
    }
}

fn seed_node_snapshot(path: &Path, snapshots: &mut HashMap<PathBuf, String>) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    snapshots.insert(path.to_path_buf(), content);
    Ok(())
}

fn refresh_node_snapshot_and_log(
    path: &Path,
    snapshots: &mut HashMap<PathBuf, String>,
) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let previous = snapshots.insert(path.to_path_buf(), content.clone());
    let events = project_watch_node_events(previous.as_deref(), &content);
    if !events.is_empty() {
        let payload = document_node_events_payload(&path.display().to_string(), &events);
        agent_doc_ops_log_io::log_op(path, &format!("document_node_events {payload}"));
    }
    Ok(())
}

/// Per-file state for stream-mode capture polling.
struct StreamState {
    pane: String,
    last_capture: String,
    target: String,
    max_lines: usize,
}

/// Entry discovered from sessions registry with mode info.
struct WatchEntry {
    path: PathBuf,
    pane: String,
    mode: DocMode,
    target: String,
    max_lines: usize,
    /// Reactive mode: skip debounce for stream-mode documents.
    /// CRDT merge handles concurrent edits, so no debounce is needed.
    reactive: bool,
}

/// Document mode determines how the watch daemon handles the file.
#[derive(Debug, PartialEq)]
enum DocMode {
    /// append/template — use notify-based file watching, submit on change
    FileWatch,
    /// stream — poll tmux pane, flush new output to document
    StreamCapture,
}

/// Ensure the watch daemon is running. If not, spawn it in the background.
///
/// Called from claim/pre-flight to implement lazy start.
/// Returns Ok(true) if daemon was started, Ok(false) if already running.
pub fn ensure_running() -> Result<bool> {
    if crate::is_running() {
        return Ok(false);
    }

    // Resolve project root (where .agent-doc/ lives)
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_root = agent_doc_project_root_io::project_root_containing(&cwd)
        .context("could not find .agent-doc/ directory — not in an agent-doc project")?;

    // Spawn daemon in background from project root
    let exe = std::env::current_exe().context("failed to resolve agent-doc binary path")?;
    let child = std::process::Command::new(exe)
        .arg("watch")
        .current_dir(&project_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn watch daemon")?;
    // Reap the detached watch daemon instead of dropping the handle so it can
    // never linger as a `<defunct>` zombie under a long-lived launcher
    // (`#zombiereap`).
    agent_doc_supervisor_process::detached_child::reap_detached(child);

    // Wait briefly for daemon to write PID file
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(50));
        if crate::is_running() {
            return Ok(true);
        }
    }

    // Best-effort: daemon may still be starting
    Ok(true)
}

/// Start the watch daemon.
///
/// Watches files registered in the durable registry for changes. On file change
/// (after debounce), records the event through the controller-owned document
/// watcher instead of launching the legacy run/preflight loop directly.
/// For stream-mode documents, polls tmux panes and flushes new output.
///
/// Loop prevention:
/// - Changes within the debounce window after a submit are treated as agent-triggered.
/// - Agent-triggered changes increment a cycle counter.
/// - If content hash matches previous submit, stop (convergence).
/// - Hard cap at `max_cycles` agent-triggered cycles per file.
///
/// Idle timeout:
/// - If no active sessions remain for 60s, daemon auto-exits.
pub fn start<E: WatchDaemonEffects>(
    config: &Config,
    watch_config: WatchConfig,
    effects: &mut E,
) -> Result<()> {
    // Resolve project root and cd there (critical for finding .agent-doc/)
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Some(root) = agent_doc_project_root_io::project_root_containing(&cwd)
        && root != cwd
    {
        std::env::set_current_dir(&root)
            .with_context(|| format!("failed to cd to project root {}", root.display()))?;
        eprintln!("Resolved project root: {}", root.display());
    }

    // Check if already running
    if let Some(pid) = crate::read_pid() {
        if crate::pid_alive(pid) {
            bail!("watch daemon already running (PID {})", pid);
        }
        // Stale PID file — clean up
        crate::remove_pid();
    }

    crate::write_current_pid()?;
    eprintln!("Watch daemon started (PID {})", std::process::id());

    // Install signal handler for clean shutdown
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    {
        let running = running.clone();
        ctrlc_handler(move || {
            running.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    let result = run_event_loop(config, &watch_config, &running, effects);

    crate::remove_pid();
    eprintln!("Watch daemon stopped.");
    result
}

/// Simple signal handler registration (best-effort).
fn ctrlc_handler<F: Fn() + Send + 'static>(f: F) {
    std::thread::spawn(move || {
        signal_wait();
        f();
    });
}

/// Wait for SIGTERM or SIGINT (Linux-specific, best-effort).
fn signal_wait() {
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

/// The main event loop.
fn run_event_loop(
    _config: &Config,
    watch_config: &WatchConfig,
    running: &std::sync::atomic::AtomicBool,
    effects: &mut impl WatchDaemonEffects,
) -> Result<()> {
    let debounce = Duration::from_millis(watch_config.debounce_ms);
    let idle_timeout = Duration::from_secs(IDLE_TIMEOUT_SECS);
    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .context("failed to create file watcher")?;

    // Discover files from sessions registry (with mode detection)
    let entries = discover_entries()?;
    let mut watched_files: Vec<PathBuf> = Vec::new();
    let mut reactive_paths: HashSet<PathBuf> = HashSet::new();
    let mut stream_states: HashMap<PathBuf, StreamState> = HashMap::new();
    let mut node_snapshots: HashMap<PathBuf, String> = HashMap::new();

    for entry in &entries {
        match entry.mode {
            DocMode::FileWatch => {
                if let Err(e) = watcher.watch(&entry.path, RecursiveMode::NonRecursive) {
                    eprintln!("Warning: could not watch {}: {}", entry.path.display(), e);
                } else {
                    watched_files.push(entry.path.clone());
                    if let Err(e) = seed_node_snapshot(&entry.path, &mut node_snapshots) {
                        eprintln!(
                            "[watch] could not seed node-event snapshot for {}: {}",
                            entry.path.display(),
                            e
                        );
                    }
                }
            }
            DocMode::StreamCapture => {
                stream_states.insert(
                    entry.path.clone(),
                    StreamState {
                        pane: entry.pane.clone(),
                        last_capture: String::new(),
                        target: entry.target.clone(),
                        max_lines: entry.max_lines,
                    },
                );
                if entry.reactive {
                    if let Err(e) = watcher.watch(&entry.path, RecursiveMode::NonRecursive) {
                        eprintln!("Warning: could not watch {}: {}", entry.path.display(), e);
                    } else {
                        watched_files.push(entry.path.clone());
                        reactive_paths.insert(entry.path.clone());
                        if let Err(e) = seed_node_snapshot(&entry.path, &mut node_snapshots) {
                            eprintln!(
                                "[watch] could not seed node-event snapshot for {}: {}",
                                entry.path.display(),
                                e
                            );
                        }
                    }
                }
            }
        }
    }

    let file_count = watched_files.len();
    let stream_count = stream_states.len();

    if file_count == 0 && stream_count == 0 {
        eprintln!("No session files found. Watching for new sessions...");
    } else {
        eprintln!(
            "Watching {} file(s), {} stream(s)",
            file_count, stream_count
        );
    }

    let mut states: HashMap<PathBuf, FileState> = HashMap::new();
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    let mut last_rescan = Instant::now();
    let mut idle_since: Option<Instant> = None;

    let tmux = tmux_router::Tmux::default_server();

    let config_toml_path = std::env::current_dir()
        .ok()
        .and_then(|d| agent_doc_project_root_io::project_root_containing(&d))
        .map(|r| r.join(".agent-doc").join("config.toml"));
    let base_dir = std::env::current_dir().context("resolve watch daemon project root")?;

    if let Some(ref cp) = config_toml_path
        && cp.exists()
        && let Err(e) = watcher.watch(cp, RecursiveMode::NonRecursive)
    {
        eprintln!("Warning: could not watch config {}: {}", cp.display(), e);
    }

    let mut config_changed = false;

    while running.load(std::sync::atomic::Ordering::Relaxed) {
        // Check PID file still exists (external stop)
        if !Path::new(crate::PID_FILE).exists() {
            eprintln!("PID file removed — shutting down.");
            break;
        }

        // Idle timeout: exit if no active sessions for IDLE_TIMEOUT_SECS
        let has_active = !watched_files.is_empty() || !stream_states.is_empty();
        if has_active {
            idle_since = None;
        } else {
            let idle_start = *idle_since.get_or_insert_with(Instant::now);
            if Instant::now().duration_since(idle_start) >= idle_timeout {
                eprintln!(
                    "No active sessions for {}s — shutting down.",
                    IDLE_TIMEOUT_SECS
                );
                break;
            }
        }

        // Rescan for new files periodically (every 10s)
        if last_rescan.elapsed() > Duration::from_secs(10) {
            let new_entries = discover_entries().unwrap_or_default();
            for entry in &new_entries {
                match entry.mode {
                    DocMode::FileWatch => {
                        if !watched_files.contains(&entry.path) {
                            if let Err(e) = watcher.watch(&entry.path, RecursiveMode::NonRecursive)
                            {
                                eprintln!(
                                    "Warning: could not watch {}: {}",
                                    entry.path.display(),
                                    e
                                );
                            } else {
                                eprintln!("Now watching {}", entry.path.display());
                                watched_files.push(entry.path.clone());
                                if let Err(e) = seed_node_snapshot(&entry.path, &mut node_snapshots)
                                {
                                    eprintln!(
                                        "[watch] could not seed node-event snapshot for {}: {}",
                                        entry.path.display(),
                                        e
                                    );
                                }
                            }
                        }
                    }
                    DocMode::StreamCapture => {
                        if !stream_states.contains_key(&entry.path) {
                            eprintln!("Now streaming {}", entry.path.display());
                            stream_states.insert(
                                entry.path.clone(),
                                StreamState {
                                    pane: entry.pane.clone(),
                                    last_capture: String::new(),
                                    target: entry.target.clone(),
                                    max_lines: entry.max_lines,
                                },
                            );
                        }
                        // Add reactive file-watch for stream-mode docs
                        if entry.reactive && !reactive_paths.contains(&entry.path) {
                            if !watched_files.contains(&entry.path) {
                                if let Err(e) =
                                    watcher.watch(&entry.path, RecursiveMode::NonRecursive)
                                {
                                    eprintln!(
                                        "Warning: could not watch {}: {}",
                                        entry.path.display(),
                                        e
                                    );
                                } else {
                                    eprintln!("Now watching {} (reactive)", entry.path.display());
                                    watched_files.push(entry.path.clone());
                                    if let Err(e) =
                                        seed_node_snapshot(&entry.path, &mut node_snapshots)
                                    {
                                        eprintln!(
                                            "[watch] could not seed node-event snapshot for {}: {}",
                                            entry.path.display(),
                                            e
                                        );
                                    }
                                }
                            }
                            reactive_paths.insert(entry.path.clone());
                        }
                    }
                }
            }

            // Prune dead stream entries (pane no longer alive)
            let dead_streams: Vec<PathBuf> = stream_states
                .iter()
                .filter(|(_, ss)| !tmux.pane_alive(&ss.pane))
                .map(|(p, _)| p.clone())
                .collect();
            for path in dead_streams {
                eprintln!("Stream pane dead for {} — removing", path.display());
                stream_states.remove(&path);
                if let Err(e) = effects.on_stream_dead(&path) {
                    eprintln!(
                        "[watch-stream] stream-dead callback failed for {}: {}",
                        path.display(),
                        e
                    );
                }
            }

            last_rescan = Instant::now();
        }

        // Poll stream-mode documents (tmux capture)
        for (path, ss) in &mut stream_states {
            match agent_doc_tmux_io::capture_pane(&tmux, &ss.pane) {
                Ok(captured) => {
                    if captured != ss.last_capture {
                        // Extract new lines since last capture, limited to last 50 lines
                        // to prevent the console component from growing indefinitely
                        let new_content = capture_delta(&ss.last_capture, &captured);
                        let limited = limit_capture_lines(&new_content, ss.max_lines);
                        if !limited.is_empty() {
                            match effects.flush_stream_to_document(path, &limited, &ss.target, "") {
                                Ok(()) => {
                                    eprint!(".");
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[watch-stream] flush error for {}: {}",
                                        path.display(),
                                        e
                                    );
                                }
                            }
                        }
                        ss.last_capture = captured;
                    }
                }
                Err(e) => {
                    eprintln!("[watch-stream] capture error for {}: {}", path.display(), e);
                }
            }
        }

        // Receive file-change events with timeout
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    for path in event.paths {
                        let canonical = path.canonicalize().unwrap_or(path);
                        if let Some(ref cp) = config_toml_path
                            && canonical == *cp
                        {
                            config_changed = true;
                            eprintln!("[watch] config change detected");
                            continue;
                        }
                        if watched_files
                            .iter()
                            .any(|w| w.canonicalize().unwrap_or_else(|_| w.clone()) == canonical)
                        {
                            pending.insert(canonical, Instant::now());
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if config_changed {
            config_changed = false;
            match effects.on_config_change() {
                Ok(count) => eprintln!(
                    "[watch] invalidated {} actor context(s) after config change",
                    count
                ),
                Err(e) => eprintln!("[watch] config-change callback failed: {e}"),
            }
        }

        // Process debounced file-change events (reactive paths skip debounce)
        let now = Instant::now();
        let ready: Vec<PathBuf> = pending
            .iter()
            .filter(|(path, when)| {
                let effective_debounce = if reactive_paths.contains(*path) {
                    Duration::ZERO
                } else {
                    debounce
                };
                now.duration_since(**when) >= effective_debounce
            })
            .map(|(path, _)| path.clone())
            .collect();

        for path in ready {
            pending.remove(&path);

            let state = states.entry(path.clone()).or_insert_with(FileState::new);

            // Check if this is an agent-triggered change
            let agent_change_window = std::cmp::max(
                debounce * 3,
                Duration::from_millis(MIN_AGENT_CHANGE_WINDOW_MS),
            );
            let is_agent_change = state
                .last_run
                .is_some_and(|t| now.duration_since(t) < agent_change_window);

            if is_agent_change {
                state.cycle_count += 1;

                // Check cycle limit
                if state.cycle_count > watch_config.max_cycles {
                    eprintln!(
                        "Max cycles ({}) reached for {} — skipping",
                        watch_config.max_cycles,
                        path.display()
                    );
                    continue;
                }

                // Check convergence
                let current_hash = std::fs::read_to_string(&path)
                    .ok()
                    .map(|content| watch_content_hash(&content));
                if current_hash.is_some() && current_hash == state.last_hash {
                    eprintln!("Converged for {} — skipping", path.display());
                    state.cycle_count = 0;
                    continue;
                }
                state.last_hash = current_hash;
            } else {
                // User change — reset cycle counter
                state.cycle_count = 0;
                state.last_hash = std::fs::read_to_string(&path)
                    .ok()
                    .map(|content| watch_content_hash(&content));
            }

            match refresh_node_snapshot_and_log(&path, &mut node_snapshots) {
                Ok(()) => {}
                Err(e) => eprintln!(
                    "[watch] node-event snapshot failed for {}: {}",
                    path.display(),
                    e
                ),
            }

            // Skip while the durable turn state machine has a fresh cycle in
            // flight. This replaces the former cross-process typing/busy sidecar;
            // a crashed cycle ages out and preflight owns its recovery.
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if cycle_freshly_in_flight(&path, now_secs) {
                eprintln!(
                    "[watch] skipping {} — agent-doc cycle in flight",
                    path.display()
                );
                continue;
            }

            // Route the settled change through the controller-owned document
            // watcher. The legacy direct submit/preflight loop is disabled for
            // realtime cutover; admission/closeout owns follow-up work.
            let file_str = path.to_string_lossy().to_string();
            eprintln!("Change detected: {}", path.display());
            if let Err(e) = effects.on_file_change(&path) {
                eprintln!(
                    "[watch] file-change callback failed for {}: {}",
                    path.display(),
                    e
                );
            }
            let current_content = match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    eprintln!(
                        "[watch] could not read {} for controller route: {}",
                        path.display(),
                        e
                    );
                    continue;
                }
            };
            let raw = RawWatchEvent::modify(path.clone());
            match effects.route_file_change(&base_dir, &file_str, &file_str, &raw, &current_content)
            {
                Ok(WatchDelivery::Change { .. }) => {
                    state.last_run = Some(Instant::now());
                    eprintln!("Change routed: {}", path.display());
                }
                Ok(delivery) => {
                    eprintln!(
                        "[watch] routed {} without submit: {:?}",
                        path.display(),
                        delivery
                    );
                }
                Err(e) => {
                    eprintln!("Submit failed for {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(())
}

/// Discover files from sessions registry with mode detection.
///
/// Reads each document's frontmatter to determine whether it's
/// file-watched (append/template) or stream-captured (stream mode).
fn discover_entries() -> Result<Vec<WatchEntry>> {
    discover_entries_in(&std::env::current_dir()?)
}

fn discover_entries_in(base_dir: &Path) -> Result<Vec<WatchEntry>> {
    let registry = agent_doc_session_registry_io::load_in(base_dir)?;
    let mut entries = Vec::new();
    for entry in registry.values() {
        let path = PathBuf::from(&entry.file);
        if !path.exists() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or(path);

        // Detect mode from frontmatter
        let (mode, target, reactive, max_lines) = match std::fs::read_to_string(&canonical) {
            Ok(content) => match frontmatter::parse(&content) {
                Ok((fm, _)) => {
                    let resolved = fm.resolve_mode();
                    if resolved.is_crdt() {
                        let target = fm
                            .stream_config
                            .as_ref()
                            .and_then(|sc| sc.target.clone())
                            .unwrap_or_else(|| "console".to_string());
                        let max_lines = fm
                            .stream_config
                            .as_ref()
                            .and_then(|sc| sc.max_lines)
                            .unwrap_or(50);
                        (DocMode::StreamCapture, target, true, max_lines)
                    } else {
                        (DocMode::FileWatch, String::new(), false, 50)
                    }
                }
                Err(_) => (DocMode::FileWatch, String::new(), false, 50),
            },
            Err(_) => (DocMode::FileWatch, String::new(), false, 50),
        };

        entries.push(WatchEntry {
            path: canonical,
            pane: entry.pane.clone(),
            mode,
            target,
            max_lines,
            reactive,
        });
    }
    Ok(entries)
}

/// Stop the watch daemon by removing the PID file.
pub fn stop() -> Result<()> {
    match crate::read_pid() {
        Some(pid) => {
            if crate::pid_alive(pid) {
                crate::remove_pid();
                eprintln!("Signaled watch daemon (PID {}) to stop.", pid);
            } else {
                crate::remove_pid();
                eprintln!(
                    "Watch daemon (PID {}) was not running. Cleaned up PID file.",
                    pid
                );
            }
        }
        None => {
            eprintln!("No watch daemon running.");
        }
    }
    Ok(())
}

/// Check the status of the watch daemon.
pub fn status() -> Result<()> {
    match crate::read_pid() {
        Some(pid) => {
            if crate::pid_alive(pid) {
                println!("Watch daemon running (PID {})", pid);
            } else {
                println!("Watch daemon not running (stale PID file: {})", pid);
            }
        }
        None => {
            println!("Watch daemon not running.");
        }
    }
    Ok(())
}

/// Whether the reactive watch should skip re-triggering for `path` because a
/// fresh agent-doc cycle is in flight.
///
/// The cross-process busy-status file used by `is_busy` goes stale after 30s
/// (`debounce::get_status_via_file`), but agent response-composition routinely
/// takes longer — so without this check a zero-debounce reactive watch would
/// re-trigger on a mid-turn editor edit and churn the queue (`#queediturn`).
/// `cycle_state` is the durable, whole-turn source of truth. Bound by
/// freshness (`agent_doc_turn::WATCH_CYCLE_IN_FLIGHT_MAX_SECS`) so a
/// crashed/stuck cycle still lets the watch proceed to preflight, which repairs
/// stale cycles.
pub fn cycle_freshly_in_flight(path: &std::path::Path, now_secs: u64) -> bool {
    match agent_doc_cycle_state_io::load_with_closeout_projection(path) {
        Ok(Some(cs)) => cycle_phase_freshly_in_flight(cs.phase, cs.updated_at, now_secs),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    #[test]
    fn cycle_freshly_in_flight_skips_open_fresh_cycle_only() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").unwrap();

        let now = now_secs();
        // No cycle state -> watch proceeds.
        assert!(!cycle_freshly_in_flight(&doc, now));

        // Open, fresh cycle -> watch skips (agent composing response).
        let cs =
            agent_doc_cycle_state_io::start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert!(cs.is_open());
        assert!(cycle_freshly_in_flight(&doc, now));
        assert!(cycle_freshly_in_flight(&doc, now + 60));

        // Open but stale beyond the freshness bound -> watch proceeds so
        // preflight can repair the stuck cycle.
        assert!(!cycle_freshly_in_flight(
            &doc,
            now + agent_doc_turn::WATCH_CYCLE_IN_FLIGHT_MAX_SECS + 1
        ));

        agent_doc_cycle_state_io::mark_committed(&doc, "test", Some("snap"), Some("body")).unwrap();
        assert!(
            !cycle_freshly_in_flight(&doc, now + 60),
            "watch should honor the terminal state.db projection"
        );
        assert!(
            !dir.path().join(".agent-doc/state/cycles").exists(),
            "watch cycle transitions must not emit compatibility files"
        );
    }

    #[test]
    fn pid_file_roundtrip() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        crate::write_current_pid_in(dir.path()).unwrap();
        let pid = crate::read_pid_in(dir.path()).unwrap();
        assert_eq!(pid, std::process::id());

        crate::remove_pid_in(dir.path());
        assert!(crate::read_pid_in(dir.path()).is_none());
    }

    #[test]
    fn pid_alive_self() {
        assert!(crate::pid_alive(std::process::id()));
    }

    #[test]
    fn pid_alive_nonexistent() {
        assert!(!crate::pid_alive(4_294_967_295));
    }

    #[test]
    fn discover_empty_registry() {
        let dir = TempDir::new().unwrap();
        let entries = discover_entries_in(dir.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn loop_prevention_counter() {
        let mut state = FileState::new();
        assert_eq!(state.cycle_count, 0);
        state.cycle_count += 1;
        assert_eq!(state.cycle_count, 1);
        state.cycle_count = 0; // user change reset
        assert_eq!(state.cycle_count, 0);
    }

    #[test]
    fn convergence_detection() {
        let mut state = FileState::new();
        state.last_hash = Some("hash".to_string());
        assert_eq!(state.last_hash.as_deref(), Some("hash"));
    }

    #[test]
    fn stream_state_tracks_capture() {
        let mut ss = StreamState {
            pane: "%42".to_string(),
            last_capture: String::new(),
            target: "console".to_string(),
            max_lines: 50,
        };
        let capture = "claude output line 1\nclaude output line 2".to_string();
        let new_content = capture_delta(&ss.last_capture, &capture);
        assert_eq!(new_content, "claude output line 1\nclaude output line 2");
        ss.last_capture = capture;

        // Second capture with more lines
        let capture2 =
            "claude output line 1\nclaude output line 2\nclaude output line 3".to_string();
        let new_content2 = capture_delta(&ss.last_capture, &capture2);
        assert_eq!(new_content2, "claude output line 3");
        ss.last_capture = capture2;
    }

    #[test]
    fn doc_mode_eq() {
        assert_eq!(DocMode::FileWatch, DocMode::FileWatch);
        assert_eq!(DocMode::StreamCapture, DocMode::StreamCapture);
        assert_ne!(DocMode::FileWatch, DocMode::StreamCapture);
    }

    #[test]
    fn watcher_detects_change() {
        use std::time::Duration;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.md");
        std::fs::write(&path, "initial").unwrap();

        let (tx, rx) = mpsc::channel();
        let mut watcher =
            match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            }) {
                Ok(watcher) => watcher,
                Err(err) if matches!(err.kind, notify::ErrorKind::MaxFilesWatch) => return,
                Err(err) => panic!("failed to create test watcher: {err}"),
            };

        if let Err(err) = watcher.watch(&path, RecursiveMode::NonRecursive) {
            if matches!(err.kind, notify::ErrorKind::MaxFilesWatch) {
                return;
            }
            panic!("failed to watch test document: {err}");
        }

        // Give watcher time to initialize
        std::thread::sleep(Duration::from_millis(100));

        std::fs::write(&path, "modified").unwrap();

        let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!event.paths.is_empty());
    }
}

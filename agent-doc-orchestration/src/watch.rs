//! # Module: watch
//!
//! ## Spec
//! - `start(config, WatchConfig)` runs the watch daemon. Acquires a PID file
//!   (`.agent-doc/watch.pid`) to prevent duplicate daemons; bails if one is already alive.
//! - `is_running()` checks the PID file and `/proc/<pid>` for daemon liveness.
//! - `ensure_running()` lazily starts the daemon if it is not running; spawns the
//!   `agent-doc watch` subprocess from the project root. Returns `Ok(true)` if started,
//!   `Ok(false)` if already running.
//! - `stop()` signals shutdown by removing the PID file; the daemon detects this on its
//!   next loop tick and exits cleanly.
//! - `status()` prints daemon running state and PID to stdout.
//! - The daemon resolves the project root (directory containing `.agent-doc/`) at startup
//!   and `chdir`s there so relative paths (PID file, sessions.json) are always correct.
//! - Session discovery: `discover_entries()` reads `sessions.json` and classifies each
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
//! - Stream-capture path: every 500 ms poll tick, `sessions::capture_pane()` is called;
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
//! - `start()`, `stop()`, `status()`, `is_running()`, and `ensure_running()` are the
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
//! - discover_empty_registry: no sessions.json → empty vec
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
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};

use agent_doc_config::Config;
use agent_doc_frontmatter::frontmatter;
use agent_doc_markdown_ast::events::DocumentNodeEvent;
use agent_doc_turn_executor::capture::{capture_delta, limit_capture_lines};

use crate::{
    document_watcher::{RawWatchEvent, WatchDelivery},
    graph::ActorContext,
    sessions, stream,
};

const PID_FILE: &str = ".agent-doc/watch.pid";

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

/// Per-file state for loop prevention (file-watch mode).
struct FileState {
    last_run: Option<Instant>,
    cycle_count: u32,
    last_hash: Option<u64>,
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

/// Hash file content for convergence detection.
fn hash_content(path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    // Strip boundary markers before hashing to prevent feedback loops.
    // Boundary repositions change the marker ID each cycle, making the hash
    // different even when no meaningful content changed. Without this,
    // reactive-mode documents (zero debounce) enter an infinite loop:
    // IPC write → boundary change → watch detects → re-run → IPC write → ...
    let content = strip_boundaries_for_hash(&content);
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    Some(hasher.finish())
}

/// Strip boundary markers from content for hash comparison.
/// This ensures boundary-only changes don't trigger re-runs.
fn strip_boundaries_for_hash(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with(" -->"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn update_node_snapshot(
    path: &Path,
    snapshots: &mut HashMap<PathBuf, String>,
) -> Result<Vec<DocumentNodeEvent>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {} for node-event snapshot", path.display()))?;
    let previous = snapshots.insert(path.to_path_buf(), content.clone());
    Ok(previous
        .as_deref()
        .map(|before| agent_doc_markdown_ast::events::diff_node_events(before, &content))
        .unwrap_or_default())
}

fn log_node_events(path: &Path, events: &[DocumentNodeEvent]) {
    if events.is_empty() {
        return;
    }
    let payload = serde_json::json!({
        "event": "document_node_events",
        "file": path.display().to_string(),
        "events": events.iter().map(node_event_json).collect::<Vec<_>>(),
    });
    crate::ops_log::log_op(path, &format!("document_node_events {payload}"));
}

fn node_event_json(event: &DocumentNodeEvent) -> serde_json::Value {
    serde_json::json!({
        "component": &event.component,
        "node_key": &event.node_key,
        "op": event.kind.as_str(),
        "item_id": &event.item_id,
        "before_index": event.before_index,
        "after_index": event.after_index,
        "before": event.before.as_deref(),
        "after": event.after.as_deref(),
        "previous_node_key": event.previous_node_key.as_deref(),
        "next_node_key": event.next_node_key.as_deref(),
    })
}

/// Check if a PID is alive via /proc.
fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{}", pid)).exists()
}

/// Read the PID from the PID file.
fn read_pid() -> Option<u32> {
    read_pid_in(&std::env::current_dir().ok()?)
}

/// Read the PID from the PID file under `base_dir`.
fn read_pid_in(base_dir: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(base_dir.join(PID_FILE)).ok()?;
    content.trim().parse().ok()
}

/// Write our PID to the PID file.
fn write_pid() -> Result<()> {
    write_pid_in(&std::env::current_dir()?)
}

/// Write our PID to the PID file under `base_dir`.
fn write_pid_in(base_dir: &Path) -> Result<()> {
    let pid_path = base_dir.join(PID_FILE);
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(pid_path, format!("{}", std::process::id()))?;
    Ok(())
}

/// Remove the PID file.
fn remove_pid() {
    let _ = std::fs::remove_file(PID_FILE);
}

/// Remove the PID file under `base_dir`.
#[cfg(test)]
fn remove_pid_in(base_dir: &Path) {
    let _ = std::fs::remove_file(base_dir.join(PID_FILE));
}

/// Check if the watch daemon is currently running.
pub fn is_running() -> bool {
    read_pid().is_some_and(pid_alive)
}

/// Ensure the watch daemon is running. If not, spawn it in the background.
///
/// Called from claim/pre-flight to implement lazy start.
/// Returns Ok(true) if daemon was started, Ok(false) if already running.
pub fn ensure_running() -> Result<bool> {
    if is_running() {
        return Ok(false);
    }

    // Resolve project root (where .agent-doc/ lives)
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_root = agent_doc_fs::find_project_root(&cwd)
        .context("could not find .agent-doc/ directory — not in an agent-doc project")?;

    // Spawn daemon in background from project root
    let exe = std::env::current_exe().context("failed to resolve agent-doc binary path")?;
    std::process::Command::new(exe)
        .arg("watch")
        .current_dir(&project_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn watch daemon")?;

    // Wait briefly for daemon to write PID file
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(50));
        if is_running() {
            return Ok(true);
        }
    }

    // Best-effort: daemon may still be starting
    Ok(true)
}

/// Start the watch daemon.
///
/// Watches files registered in sessions.json for changes. On file change
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
pub fn start(config: &Config, watch_config: WatchConfig) -> Result<()> {
    // Resolve project root and cd there (critical for finding .agent-doc/)
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Some(root) = agent_doc_fs::find_project_root(&cwd)
        && root != cwd
    {
        std::env::set_current_dir(&root)
            .with_context(|| format!("failed to cd to project root {}", root.display()))?;
        eprintln!("Resolved project root: {}", root.display());
    }

    // Check if already running
    if let Some(pid) = read_pid() {
        if pid_alive(pid) {
            bail!("watch daemon already running (PID {})", pid);
        }
        // Stale PID file — clean up
        remove_pid();
    }

    write_pid()?;
    eprintln!("Watch daemon started (PID {})", std::process::id());

    // Install signal handler for clean shutdown
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    {
        let running = running.clone();
        ctrlc_handler(move || {
            running.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    let result = run_event_loop(config, &watch_config, &running);

    remove_pid();
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
                    if let Err(e) = update_node_snapshot(&entry.path, &mut node_snapshots) {
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
                        if let Err(e) = update_node_snapshot(&entry.path, &mut node_snapshots) {
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
    let mut actor_contexts: HashMap<PathBuf, ActorContext> = HashMap::new();
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    let mut last_rescan = Instant::now();
    let mut idle_since: Option<Instant> = None;

    let tmux = tmux_router::Tmux::default_server();

    let config_toml_path = std::env::current_dir()
        .ok()
        .and_then(|d| agent_doc_fs::find_project_root(&d))
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
        if !Path::new(PID_FILE).exists() {
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
                                if let Err(e) =
                                    update_node_snapshot(&entry.path, &mut node_snapshots)
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
                                        update_node_snapshot(&entry.path, &mut node_snapshots)
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
                actor_contexts.remove(&path);
            }

            last_rescan = Instant::now();
        }

        // Poll stream-mode documents (tmux capture)
        for (path, ss) in &mut stream_states {
            match sessions::capture_pane(&tmux, &ss.pane) {
                Ok(captured) => {
                    if captured != ss.last_capture {
                        // Extract new lines since last capture, limited to last 50 lines
                        // to prevent the console component from growing indefinitely
                        let new_content = capture_delta(&ss.last_capture, &captured);
                        let limited = limit_capture_lines(&new_content, ss.max_lines);
                        if !limited.is_empty() {
                            match stream::flush_to_document(path, &limited, &ss.target, "") {
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
            for ac in actor_contexts.values() {
                ac.on_config_change();
            }
            eprintln!(
                "[watch] invalidated {} actor context(s) after config change",
                actor_contexts.len()
            );
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
                let current_hash = hash_content(&path);
                if current_hash.is_some() && current_hash == state.last_hash {
                    eprintln!("Converged for {} — skipping", path.display());
                    state.cycle_count = 0;
                    continue;
                }
                state.last_hash = current_hash;
            } else {
                // User change — reset cycle counter
                state.cycle_count = 0;
                state.last_hash = hash_content(&path);
            }

            match update_node_snapshot(&path, &mut node_snapshots) {
                Ok(node_events) => log_node_events(&path, &node_events),
                Err(e) => eprintln!(
                    "[watch] node-event snapshot failed for {}: {}",
                    path.display(),
                    e
                ),
            }

            // Skip if file has an active agent-doc operation (prevents duplicate
            // responses from watch daemon competing with skill/stream writes)
            let file_str = path.to_string_lossy().to_string();
            if agent_doc_debounce::is_busy(&file_str) {
                eprintln!(
                    "[watch] skipping {} — busy (active operation in progress)",
                    path.display()
                );
                continue;
            }

            // Also skip when an agent-doc cycle is freshly in flight. The
            // cross-process status file used by `is_busy` goes stale after 30s,
            // but agent response-composition routinely takes longer — without
            // this guard a zero-debounce reactive watch re-triggers mid-turn and
            // churns the queue (#queediturn). Bound by freshness so a
            // crashed/stuck cycle still lets the watch proceed to preflight
            // (which repairs stale cycles).
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
            eprintln!("Change detected: {}", path.display());
            let ac = actor_contexts
                .entry(path.clone())
                .or_insert_with(|| ActorContext::new(path.clone()));
            ac.on_file_change(path.clone());
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
            match crate::document_watcher::route_event(
                &base_dir,
                &file_str,
                &file_str,
                &raw,
                &current_content,
                || Ok(()),
            ) {
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
    let registry = sessions::load_in(base_dir)?;
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
    match read_pid() {
        Some(pid) => {
            if pid_alive(pid) {
                remove_pid();
                eprintln!("Signaled watch daemon (PID {}) to stop.", pid);
            } else {
                remove_pid();
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
    match read_pid() {
        Some(pid) => {
            if pid_alive(pid) {
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
/// freshness (`WATCH_CYCLE_IN_FLIGHT_MAX_SECS`) so a crashed/stuck cycle still
/// lets the watch proceed to preflight, which repairs stale cycles.
pub(crate) const WATCH_CYCLE_IN_FLIGHT_MAX_SECS: u64 = 600;

pub(crate) fn cycle_freshly_in_flight(path: &std::path::Path, now_secs: u64) -> bool {
    match crate::cycle_state::load(path) {
        Ok(Some(cs)) if cs.is_open() => {
            now_secs.saturating_sub(cs.updated_at) < WATCH_CYCLE_IN_FLIGHT_MAX_SECS
        }
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
        let cs = crate::cycle_state::start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert!(cs.is_open());
        assert!(cycle_freshly_in_flight(&doc, now));
        assert!(cycle_freshly_in_flight(&doc, now + 60));

        // Open but stale beyond the freshness bound -> watch proceeds so
        // preflight can repair the stuck cycle.
        assert!(!cycle_freshly_in_flight(
            &doc,
            now + WATCH_CYCLE_IN_FLIGHT_MAX_SECS + 1
        ));
    }

    #[test]
    fn pid_file_roundtrip() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        write_pid_in(dir.path()).unwrap();
        let pid = read_pid_in(dir.path()).unwrap();
        assert_eq!(pid, std::process::id());

        remove_pid_in(dir.path());
        assert!(read_pid_in(dir.path()).is_none());
    }

    #[test]
    fn pid_alive_self() {
        assert!(pid_alive(std::process::id()));
    }

    #[test]
    fn pid_alive_nonexistent() {
        assert!(!pid_alive(4_294_967_295));
    }

    #[test]
    fn discover_empty_registry() {
        let dir = TempDir::new().unwrap();
        let entries = discover_entries_in(dir.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn hash_deterministic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.md");
        std::fs::write(&path, "hello world").unwrap();

        let h1 = hash_content(&path).unwrap();
        let h2 = hash_content(&path).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_changes_with_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.md");

        std::fs::write(&path, "version 1").unwrap();
        let h1 = hash_content(&path).unwrap();

        std::fs::write(&path, "version 2").unwrap();
        let h2 = hash_content(&path).unwrap();

        assert_ne!(h1, h2);
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
        state.last_hash = Some(42);
        assert_eq!(state.last_hash, Some(42));
    }

    #[test]
    fn update_node_snapshot_emits_node_keyed_events_after_seed() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("session.md");
        std::fs::write(
            &path,
            "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let mut snapshots = HashMap::new();

        assert!(
            update_node_snapshot(&path, &mut snapshots)
                .unwrap()
                .is_empty()
        );

        std::fs::write(
            &path,
            "<!-- agent:queue -->\n- do [#alpha]\n- do [#beta]\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let events = update_node_snapshot(&path, &mut snapshots).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].kind,
            agent_doc_markdown_ast::events::DocumentNodeEventKind::Insert
        );
        assert!(events[0].node_key.contains(":beta:"));
        assert!(
            events[0]
                .previous_node_key
                .as_deref()
                .is_some_and(|key| key.contains(":alpha:"))
        );
    }

    #[test]
    fn log_node_events_writes_document_node_events_payload() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let path = dir.path().join("session.md");
        let before =
            "<!-- agent:backlog -->\n1. [ ] [#task] old wording\n<!-- /agent:backlog -->\n";
        let after = "<!-- agent:backlog -->\n1. [ ] [#task] new wording\n<!-- /agent:backlog -->\n";
        std::fs::write(&path, after).unwrap();
        let events = agent_doc_markdown_ast::events::diff_node_events(before, after);

        log_node_events(&path, &events);

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("document_node_events"), "{log}");
        assert!(log.contains("\"event\":\"document_node_events\""), "{log}");
        assert!(log.contains("\"component\":\"backlog\""), "{log}");
        assert!(log.contains("\"node_key\":\"backlog:0:task:0\""), "{log}");
        assert!(log.contains("\"op\":\"replace\""), "{log}");
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
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        })
        .unwrap();

        watcher.watch(&path, RecursiveMode::NonRecursive).unwrap();

        // Give watcher time to initialize
        std::thread::sleep(Duration::from_millis(100));

        std::fs::write(&path, "modified").unwrap();

        let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!event.paths.is_empty());
    }
}

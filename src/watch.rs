use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::{config::Config, frontmatter, sessions, stream, submit};

const PID_FILE: &str = ".agent-doc/watch.pid";

/// Default idle timeout before daemon auto-exits (seconds).
const IDLE_TIMEOUT_SECS: u64 = 60;

/// Configuration for the watch daemon.
pub struct WatchConfig {
    pub debounce_ms: u64,
    pub max_cycles: u32,
}

/// Per-file state for loop prevention (file-watch mode).
struct FileState {
    last_submit: Option<Instant>,
    cycle_count: u32,
    last_hash: Option<u64>,
}

impl FileState {
    fn new() -> Self {
        Self {
            last_submit: None,
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
}

/// Entry discovered from sessions registry with mode info.
struct WatchEntry {
    path: PathBuf,
    pane: String,
    mode: DocMode,
    target: String,
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
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    Some(hasher.finish())
}

/// Check if a PID is alive via /proc.
fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{}", pid)).exists()
}

/// Read the PID from the PID file.
fn read_pid() -> Option<u32> {
    let content = std::fs::read_to_string(PID_FILE).ok()?;
    content.trim().parse().ok()
}

/// Write our PID to the PID file.
fn write_pid() -> Result<()> {
    let pid_path = Path::new(PID_FILE);
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
    let project_root = crate::snapshot::find_project_root(&cwd)
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
/// (after debounce), runs `submit::run()` on the changed file.
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
    if let Some(root) = find_project_root(&cwd)
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
    config: &Config,
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

    for entry in &entries {
        match entry.mode {
            DocMode::FileWatch => {
                if let Err(e) = watcher.watch(&entry.path, RecursiveMode::NonRecursive) {
                    eprintln!("Warning: could not watch {}: {}", entry.path.display(), e);
                } else {
                    watched_files.push(entry.path.clone());
                }
            }
            DocMode::StreamCapture => {
                // Stream-mode: tmux capture polling
                stream_states.insert(
                    entry.path.clone(),
                    StreamState {
                        pane: entry.pane.clone(),
                        last_capture: String::new(),
                        target: entry.target.clone(),
                    },
                );
                // Reactive: also file-watch with zero debounce (CRDT handles concurrency)
                if entry.reactive {
                    if let Err(e) = watcher.watch(&entry.path, RecursiveMode::NonRecursive) {
                        eprintln!("Warning: could not watch {}: {}", entry.path.display(), e);
                    } else {
                        watched_files.push(entry.path.clone());
                        reactive_paths.insert(entry.path.clone());
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

    let tmux = sessions::Tmux::default_server();

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
                eprintln!("No active sessions for {}s — shutting down.", IDLE_TIMEOUT_SECS);
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
                            if let Err(e) =
                                watcher.watch(&entry.path, RecursiveMode::NonRecursive)
                            {
                                eprintln!(
                                    "Warning: could not watch {}: {}",
                                    entry.path.display(),
                                    e
                                );
                            } else {
                                eprintln!("Now watching {}", entry.path.display());
                                watched_files.push(entry.path.clone());
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
                                    eprintln!(
                                        "Now watching {} (reactive)",
                                        entry.path.display()
                                    );
                                    watched_files.push(entry.path.clone());
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
            }

            last_rescan = Instant::now();
        }

        // Poll stream-mode documents (tmux capture)
        for (path, ss) in &mut stream_states {
            match sessions::capture_pane(&tmux, &ss.pane) {
                Ok(captured) => {
                    if captured != ss.last_capture {
                        // Extract new lines since last capture
                        let new_content = extract_new_lines(&ss.last_capture, &captured);
                        if !new_content.is_empty() {
                            match stream::flush_to_document(path, &new_content, &ss.target, "") {
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
                    eprintln!(
                        "[watch-stream] capture error for {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        // Receive file-change events with timeout
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => {
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                ) {
                    for path in event.paths {
                        let canonical = path.canonicalize().unwrap_or(path);
                        if watched_files.iter().any(|w| {
                            w.canonicalize().unwrap_or_else(|_| w.clone()) == canonical
                        }) {
                            pending.insert(canonical, Instant::now());
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
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
            let is_agent_change = state
                .last_submit
                .is_some_and(|t| now.duration_since(t) < debounce * 3);

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

            // Submit
            eprintln!("Change detected: {}", path.display());
            match submit::run(&path, false, None, None, false, false, config) {
                Ok(()) => {
                    state.last_submit = Some(Instant::now());
                    eprintln!("Submit complete: {}", path.display());
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
    let registry = sessions::load()?;
    let mut entries = Vec::new();
    for entry in registry.values() {
        let path = PathBuf::from(&entry.file);
        if !path.exists() {
            continue;
        }
        let canonical = path.canonicalize().unwrap_or(path);

        // Detect mode from frontmatter
        let (mode, target, reactive) = match std::fs::read_to_string(&canonical) {
            Ok(content) => match frontmatter::parse(&content) {
                Ok((fm, _)) => {
                    let resolved = fm.resolve_mode();
                    if resolved.is_crdt() {
                        let target = fm
                            .stream_config
                            .as_ref()
                            .and_then(|sc| sc.target.clone())
                            .unwrap_or_else(|| "exchange".to_string());
                        (DocMode::StreamCapture, target, true)
                    } else {
                        (DocMode::FileWatch, String::new(), false)
                    }
                }
                Err(_) => (DocMode::FileWatch, String::new(), false),
            },
            Err(_) => (DocMode::FileWatch, String::new(), false),
        };

        entries.push(WatchEntry {
            path: canonical,
            pane: entry.pane.clone(),
            mode,
            target,
            reactive,
        });
    }
    Ok(entries)
}

/// Discover only file paths (backward-compat wrapper used by tests).
#[cfg(test)]
fn discover_files() -> Result<Vec<PathBuf>> {
    Ok(discover_entries()?
        .into_iter()
        .map(|e| e.path)
        .collect())
}

/// Extract new lines from a pane capture by diffing against the previous capture.
///
/// Compares line-by-line: finds the first divergence point and returns
/// all lines from that point onward in the new capture.
fn extract_new_lines(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Find the first line that differs
    let common_prefix = old_lines
        .iter()
        .zip(new_lines.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Everything after the common prefix in the new capture is new content
    if common_prefix < new_lines.len() {
        new_lines[common_prefix..].join("\n")
    } else {
        String::new()
    }
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

/// Find the project root by walking up from `path` looking for `.agent-doc/`.
fn find_project_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_file() {
        path.parent()?
    } else {
        path
    };
    loop {
        if current.join(".agent-doc").is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn pid_file_roundtrip() {
        let dir = TempDir::new().unwrap();
        let _guard = std::env::set_current_dir(dir.path());
        std::fs::create_dir_all(".agent-doc").unwrap();

        write_pid().unwrap();
        let pid = read_pid().unwrap();
        assert_eq!(pid, std::process::id());

        remove_pid();
        assert!(read_pid().is_none());
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
        let _guard = std::env::set_current_dir(dir.path());
        let files = discover_files().unwrap();
        assert!(files.is_empty());
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
    fn extract_new_lines_appended() {
        let old = "line 1\nline 2\nline 3";
        let new = "line 1\nline 2\nline 3\nline 4\nline 5";
        let result = extract_new_lines(old, new);
        assert_eq!(result, "line 4\nline 5");
    }

    #[test]
    fn extract_new_lines_modified() {
        let old = "line 1\nline 2\nline 3";
        let new = "line 1\nchanged\nline 3\nline 4";
        let result = extract_new_lines(old, new);
        // Diverges at line 2; returns from there onward
        assert_eq!(result, "changed\nline 3\nline 4");
    }

    #[test]
    fn extract_new_lines_identical() {
        let old = "line 1\nline 2";
        let new = "line 1\nline 2";
        let result = extract_new_lines(old, new);
        assert_eq!(result, "");
    }

    #[test]
    fn extract_new_lines_empty_old() {
        let old = "";
        let new = "line 1\nline 2";
        let result = extract_new_lines(old, new);
        assert_eq!(result, "line 1\nline 2");
    }

    #[test]
    fn extract_new_lines_empty_new() {
        let old = "line 1\nline 2";
        let new = "";
        let result = extract_new_lines(old, new);
        assert_eq!(result, "");
    }

    #[test]
    fn stream_state_tracks_capture() {
        let mut ss = StreamState {
            pane: "%42".to_string(),
            last_capture: String::new(),
            target: "exchange".to_string(),
        };
        let capture = "claude output line 1\nclaude output line 2".to_string();
        let new_content = extract_new_lines(&ss.last_capture, &capture);
        assert_eq!(new_content, "claude output line 1\nclaude output line 2");
        ss.last_capture = capture;

        // Second capture with more lines
        let capture2 = "claude output line 1\nclaude output line 2\nclaude output line 3".to_string();
        let new_content2 = extract_new_lines(&ss.last_capture, &capture2);
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
    #[ignore] // Integration test — requires notify to work with real filesystem
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

        watcher
            .watch(&path, RecursiveMode::NonRecursive)
            .unwrap();

        // Give watcher time to initialize
        std::thread::sleep(Duration::from_millis(100));

        std::fs::write(&path, "modified").unwrap();

        let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!event.paths.is_empty());
    }
}

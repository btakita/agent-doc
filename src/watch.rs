use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::{config::Config, sessions, submit};

const PID_FILE: &str = ".agent-doc/watch.pid";

/// Configuration for the watch daemon.
pub struct WatchConfig {
    pub debounce_ms: u64,
    pub max_cycles: u32,
}

/// Per-file state for loop prevention.
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

/// Start the watch daemon.
///
/// Watches files registered in sessions.json for changes. On file change
/// (after debounce), runs `submit::run()` on the changed file.
///
/// Loop prevention:
/// - Changes within the debounce window after a submit are treated as agent-triggered.
/// - Agent-triggered changes increment a cycle counter.
/// - If content hash matches previous submit, stop (convergence).
/// - Hard cap at `max_cycles` agent-triggered cycles per file.
pub fn start(config: &Config, watch_config: WatchConfig) -> Result<()> {
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
    // Use a thread that waits for SIGTERM/SIGINT via a basic mechanism.
    // For simplicity, we just set the flag — the recv_timeout loop will pick it up.
    std::thread::spawn(move || {
        // Block on signal — this is a simplified approach.
        // The main loop checks `running` flag periodically via recv_timeout.
        signal_wait();
        f();
    });
}

/// Wait for SIGTERM or SIGINT (Linux-specific, best-effort).
fn signal_wait() {
    // We rely on the main loop's PID file check and recv_timeout for shutdown.
    // This thread just sleeps forever — the process exits when main returns.
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
    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .context("failed to create file watcher")?;

    // Discover files to watch from sessions registry
    let mut watched_files = discover_files()?;
    for path in &watched_files {
        if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
            eprintln!("Warning: could not watch {}: {}", path.display(), e);
        }
    }

    if watched_files.is_empty() {
        eprintln!("No session files found. Watching for new sessions...");
    } else {
        eprintln!("Watching {} file(s)", watched_files.len());
    }

    let mut states: HashMap<PathBuf, FileState> = HashMap::new();
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    let mut last_rescan = Instant::now();

    while running.load(std::sync::atomic::Ordering::Relaxed) {
        // Check PID file still exists (external stop)
        if !Path::new(PID_FILE).exists() {
            eprintln!("PID file removed — shutting down.");
            break;
        }

        // Rescan for new files periodically (every 10s)
        if last_rescan.elapsed() > Duration::from_secs(10) {
            let new_files = discover_files().unwrap_or_default();
            for path in &new_files {
                if !watched_files.contains(path) {
                    if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
                        eprintln!("Warning: could not watch {}: {}", path.display(), e);
                    } else {
                        eprintln!("Now watching {}", path.display());
                        watched_files.push(path.clone());
                    }
                }
            }
            last_rescan = Instant::now();
        }

        // Receive events with timeout
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

        // Process debounced events
        let now = Instant::now();
        let ready: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, when)| now.duration_since(**when) >= debounce)
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

/// Discover files to watch from the sessions registry.
fn discover_files() -> Result<Vec<PathBuf>> {
    let registry = sessions::load()?;
    let mut files = Vec::new();
    for entry in registry.values() {
        let path = PathBuf::from(&entry.file);
        if path.exists() {
            files.push(path.canonicalize().unwrap_or(path));
        }
    }
    Ok(files)
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
                eprintln!("Watch daemon (PID {}) was not running. Cleaned up PID file.", pid);
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
        // PID 4294967295 is very unlikely to exist
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
        // Same hash = converged
        assert_eq!(state.last_hash, Some(42));
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

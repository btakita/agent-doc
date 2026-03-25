//! # Module: debounce
//!
//! ## Spec
//! - Provides a shared typing-debounce mechanism used by all editor plugins (JetBrains, VS Code,
//!   Neovim, Zed) so they share identical timing logic via the agent-doc FFI layer.
//! - In-process state: a `Mutex<HashMap<PathBuf, Instant>>` (`LAST_CHANGE`) records the last
//!   edit timestamp per file path.
//! - Cross-process state: each `document_changed` call also writes a millisecond Unix timestamp
//!   to `.agent-doc/typing/<hash>` so CLI invocations running in a separate process can detect
//!   active typing. The hash is derived from the file path string via `DefaultHasher`.
//!   Cross-process writes are best-effort and never block the caller.
//! - `is_idle` / `await_idle` operate on in-process state (same process as the plugin).
//! - `is_typing_via_file` / `await_idle_via_file` operate on the file-based indicator (CLI use).
//! - Files with no recorded `document_changed` call are considered idle by `is_idle`; this
//!   prevents `await_idle` from blocking forever on untracked documents.
//! - `is_tracked` distinguishes "never seen" from "seen and idle" for non-blocking probes.
//! - `await_idle` polls every 100 ms and returns `false` if `timeout_ms` expires before idle.
//!
//! ## Agentic Contracts
//! - `document_changed(file: &str)` — records now as last-change time; writes typing indicator
//!   file (best-effort); never panics.
//! - `is_idle(file, debounce_ms) -> bool` — `true` if elapsed ≥ `debounce_ms` or file untracked.
//! - `is_tracked(file) -> bool` — `true` if at least one `document_changed` was recorded.
//! - `await_idle(file, debounce_ms, timeout_ms) -> bool` — blocks until idle or timeout; 100 ms
//!   poll interval.
//! - `is_typing_via_file(file, debounce_ms) -> bool` — reads indicator file; `false` if absent or
//!   timestamp older than `debounce_ms`.
//! - `await_idle_via_file(file, debounce_ms, timeout_ms) -> bool` — file-based blocking variant.
//!
//! ## Evals
//! - idle_no_changes: file never passed to `document_changed` → `is_idle` returns `true`
//! - not_idle_after_change: immediately after `document_changed` with 1500 ms window → `false`
//! - idle_after_debounce: 50 ms sleep with 10 ms debounce → `is_idle` returns `true`
//! - await_immediate: untracked file, `await_idle` → returns `true` in < 200 ms
//! - await_settle: `document_changed` then `await_idle` with 200 ms debounce → waits ≥ 200 ms
//! - typing_indicator_written: `document_changed` on file with `.agent-doc/typing/` dir →
//!   `is_typing_via_file` returns `true` within 2000 ms window
//! - typing_indicator_expires: 50 ms after change with 10 ms debounce →
//!   `is_typing_via_file` returns `false`
//! - no_indicator_file: nonexistent path → `is_typing_via_file` returns `false`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

/// Global state: last change timestamp per file.
static LAST_CHANGE: Mutex<Option<HashMap<PathBuf, Instant>>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut HashMap<PathBuf, Instant>) -> R) -> R {
    let mut guard = LAST_CHANGE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// Record a document change event for the given file.
///
/// Called by editor plugins on every document modification.
/// Also writes a typing indicator file for cross-process visibility.
pub fn document_changed(file: &str) {
    let path = PathBuf::from(file);
    with_state(|map| {
        map.insert(path.clone(), Instant::now());
    });
    // Write cross-process typing indicator (best-effort, never block)
    let _ = write_typing_indicator(file);
}

/// Check if the document has been idle (no changes) for at least `debounce_ms`.
///
/// Returns `true` if no recent changes (safe to run), `false` if still active.
/// For untracked files (no `document_changed` ever called), returns `true` —
/// the blocking `await_idle` relies on this to not wait forever.
pub fn is_idle(file: &str, debounce_ms: u64) -> bool {
    let path = PathBuf::from(file);
    with_state(|map| {
        match map.get(&path) {
            None => true, // No recorded changes — idle
            Some(last) => last.elapsed().as_millis() >= debounce_ms as u128,
        }
    })
}

/// Check if the document has been tracked (at least one `document_changed` call recorded).
///
/// Used by non-blocking probes to distinguish "never tracked" from "tracked and idle".
/// If a file is untracked, the probe should be conservative (assume not idle).
pub fn is_tracked(file: &str) -> bool {
    let path = PathBuf::from(file);
    with_state(|map| map.contains_key(&path))
}

/// Block until the document has been idle for `debounce_ms`, or `timeout_ms` expires.
///
/// Returns `true` if idle was reached, `false` if timed out.
///
/// Poll interval: 100ms (responsive without busy-waiting).
pub fn await_idle(file: &str, debounce_ms: u64, timeout_ms: u64) -> bool {
    let start = Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let poll_interval = std::time::Duration::from_millis(100);

    loop {
        if is_idle(file, debounce_ms) {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(poll_interval);
    }
}

// ── Cross-process typing bridge ──

/// Directory for typing indicator files, relative to project root.
const TYPING_DIR: &str = ".agent-doc/typing";

/// Write a typing indicator file for the given document path.
/// The file contains a Unix timestamp (milliseconds) of the last edit.
fn write_typing_indicator(file: &str) -> std::io::Result<()> {
    let typing_path = typing_indicator_path(file);
    if let Some(parent) = typing_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::fs::write(&typing_path, now.to_string())
}

/// Compute the typing indicator file path for a document.
fn typing_indicator_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    // Walk up to find .agent-doc/ directory
    let mut dir = PathBuf::from(file);
    loop {
        dir.pop();
        if dir.join(".agent-doc").is_dir() {
            return dir.join(TYPING_DIR).join(format!("{:016x}", hash));
        }
        if !dir.pop() {
            // Fallback: use file's parent directory
            let parent = PathBuf::from(file).parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
            return parent.join(TYPING_DIR).join(format!("{:016x}", hash));
        }
    }
}

/// Check if the document has a recent typing indicator (cross-process).
///
/// Returns `true` if the typing indicator exists and was updated within
/// `debounce_ms` milliseconds. Used by CLI preflight to detect active typing
/// from a plugin running in a different process.
pub fn is_typing_via_file(file: &str, debounce_ms: u64) -> bool {
    let path = typing_indicator_path(file);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            if let Ok(ts_ms) = content.trim().parse::<u128>() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                now.saturating_sub(ts_ms) < debounce_ms as u128
            } else {
                false
            }
        }
        Err(_) => false, // No indicator file — not typing
    }
}

/// Block until the typing indicator shows idle, or timeout.
///
/// Used by CLI preflight to wait for plugin-side typing to settle.
/// Returns `true` if idle was reached, `false` if timed out.
pub fn await_idle_via_file(file: &str, debounce_ms: u64, timeout_ms: u64) -> bool {
    let start = Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let poll_interval = std::time::Duration::from_millis(100);

    loop {
        if !is_typing_via_file(file, debounce_ms) {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(poll_interval);
    }
}

// ── Response status signal (A: file, B: FFI) ──

/// Status directory for cross-process signals (Option A).
const STATUS_DIR: &str = ".agent-doc/status";

/// In-process status (Option B: FFI).
static STATUS: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

fn with_status<R>(f: impl FnOnce(&mut HashMap<PathBuf, String>) -> R) -> R {
    let mut guard = STATUS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// Set the response status for a file.
///
/// Status values: "generating", "writing", "routing", "idle"
/// Sets both in-process state (B) and file signal (A).
pub fn set_status(file: &str, status: &str) {
    let path = PathBuf::from(file);
    with_status(|map| {
        if status == "idle" {
            map.remove(&path);
        } else {
            map.insert(path, status.to_string());
        }
    });
    let _ = write_status_file(file, status);
}

/// Get the response status for a file (in-process, Option B).
///
/// Returns "idle" if no status is set.
pub fn get_status(file: &str) -> String {
    let path = PathBuf::from(file);
    with_status(|map| {
        map.get(&path).cloned().unwrap_or_else(|| "idle".to_string())
    })
}

/// Check if any operation is in progress for a file (in-process, Option B).
///
/// Returns `true` if status is NOT "idle". Used by plugins to avoid
/// triggering routes during active operations.
pub fn is_busy(file: &str) -> bool {
    get_status(file) != "idle"
}

/// Get status from file signal (cross-process, Option A).
///
/// Returns "idle" if no status file exists or it's stale (>30s).
pub fn get_status_via_file(file: &str) -> String {
    let path = status_file_path(file);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            // Format: "status:timestamp_ms"
            let parts: Vec<&str> = content.trim().splitn(2, ':').collect();
            if parts.len() == 2 {
                if let Ok(ts) = parts[1].parse::<u128>() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    // Stale after 30s — operation probably crashed
                    if now.saturating_sub(ts) < 30_000 {
                        return parts[0].to_string();
                    }
                }
            }
            "idle".to_string()
        }
        Err(_) => "idle".to_string(),
    }
}

fn write_status_file(file: &str, status: &str) -> std::io::Result<()> {
    let path = status_file_path(file);
    if status == "idle" {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::fs::write(&path, format!("{}:{}", status, now))
}

fn status_file_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    loop {
        dir.pop();
        if dir.join(".agent-doc").is_dir() {
            return dir.join(STATUS_DIR).join(format!("{:016x}", hash));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file).parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
            return parent.join(STATUS_DIR).join(format!("{:016x}", hash));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_when_no_changes() {
        assert!(is_idle("/tmp/test-no-changes.md", 1500));
    }

    #[test]
    fn not_idle_after_change() {
        document_changed("/tmp/test-just-changed.md");
        assert!(!is_idle("/tmp/test-just-changed.md", 1500));
    }

    #[test]
    fn idle_after_debounce_period() {
        document_changed("/tmp/test-debounce.md");
        // Use a very short debounce for testing
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(is_idle("/tmp/test-debounce.md", 10));
    }

    #[test]
    fn await_idle_returns_immediately_when_idle() {
        let start = Instant::now();
        assert!(await_idle("/tmp/test-await-idle.md", 100, 5000));
        assert!(start.elapsed().as_millis() < 200);
    }

    #[test]
    fn await_idle_waits_for_settle() {
        document_changed("/tmp/test-await-settle.md");
        let start = Instant::now();
        assert!(await_idle("/tmp/test-await-settle.md", 200, 5000));
        assert!(start.elapsed().as_millis() >= 200);
    }

    #[test]
    fn typing_indicator_written_on_change() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("typing");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        let doc = tmp.path().join("test-typing.md");
        std::fs::write(&doc, "test").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        document_changed(&doc_str);

        // Should detect typing within 2000ms window
        assert!(is_typing_via_file(&doc_str, 2000));
    }

    #[test]
    fn typing_indicator_expires() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("typing");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        let doc = tmp.path().join("test-typing-expire.md");
        std::fs::write(&doc, "test").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        document_changed(&doc_str);
        std::thread::sleep(std::time::Duration::from_millis(50));

        // With a 10ms debounce, 50ms ago should NOT be typing
        assert!(!is_typing_via_file(&doc_str, 10));
    }

    #[test]
    fn no_typing_indicator_means_not_typing() {
        assert!(!is_typing_via_file("/tmp/nonexistent-file-xyz.md", 2000));
    }
}

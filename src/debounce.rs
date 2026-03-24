//! Typing debounce for editor plugins.
//!
//! Provides a centralized debounce mechanism via FFI so all editor plugins
//! (JetBrains, VS Code, Neovim, Zed) share identical timing logic.
//!
//! Plugins call `document_changed()` on every document edit, and
//! `await_idle()` before submitting to wait for typing to settle.

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
pub fn document_changed(file: &str) {
    let path = PathBuf::from(file);
    with_state(|map| {
        map.insert(path, Instant::now());
    });
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
}

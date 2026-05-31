//! # Module: debounce
//!
//! ## Spec
//! - Provides a shared typing-debounce mechanism used by all editor plugins (JetBrains, VS Code,
//!   Neovim, Zed) so they share identical timing logic via the agent-doc FFI layer.
//! - In-process state: a `Mutex<HashMap<PathBuf, Instant>>` (`LAST_CHANGE`) records the last
//!   edit timestamp per file path.
//! - Cross-process state: each `document_changed` call also writes a millisecond Unix timestamp
//!   to `.agent-doc/typing/<hash>` so CLI invocations running in a separate process can detect
//!   active typing. Editor plugins may additionally record the visible buffer digest in
//!   `.agent-doc/live-buffer/<hash>` so CLI direct-disk writes can detect idle-but-unsaved
//!   editor drift before mutating the file on disk. The hash is derived from the file path string
//!   via `DefaultHasher`.
//!   Cross-process writes are best-effort and never block the caller.
//! - Editor-authoritative buffer state: plugins report `EditorBufferState` through the FFI bridge
//!   on every document change, including monotonic version, dirty flag, last-edit timestamp,
//!   optional save timestamp, optional content hash, and optional session ID. This state is
//!   held in process memory via a `Mutex<HashMap>` (`EDITOR_BUFFER_STATES`), which is
//!   concurrency-safe within the Session Actor process. When present, `wait_for_stable_content`
//!   uses version/hash stability instead of the `extract_last_added_line` / `looks_truncated`
//!   heuristic, eliminating race classes where edits above the last inserted line are missed.
//!   If no editor state exists, the fallback truncation heuristic is used. No file sidecars
//!   are written for editor buffer state, avoiding filesystem concurrency issues.
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
//! - `document_changed_with_digest(file, len, hash)` — records typing plus the latest
//!   editor-visible buffer digest without passing full document content through FFI.
//! - `live_buffer_diverges_from_content(file, content)` — returns the latest editor-visible
//!   digest when it differs from the provided disk/expected content.
//! - `record_editor_buffer_state(state: &EditorBufferState)` — stores editor-authoritative
//!   buffer state (version, dirty, hash, timestamps) in the in-memory Session Actor map.
//! - `editor_buffer_state(file) -> Option<EditorBufferState>` — reads the latest state.
//! - `editor_buffer_stable(file, debounce_ms) -> Option<EditorBufferState>` — returns the state
//!   when the editor is idle (not dirty or debounce elapsed), `None` otherwise.
//! - `await_editor_buffer_stable(file, debounce_ms, timeout_ms) -> Option<EditorBufferState>` —
//!   blocks until stable or timeout; 100ms poll interval.
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

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

static LAST_CHANGE: Mutex<Option<HashMap<PathBuf, Instant>>> = Mutex::new(None);

static EDITOR_BUFFER_STATES: Mutex<Option<HashMap<PathBuf, EditorBufferState>>> =
    Mutex::new(None);

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
    if let Err(e) = write_typing_indicator(file) {
        eprintln!(
            "[debounce] typing indicator write failed for {:?}: {}",
            file, e
        );
    }
}

/// Record a document change and the editor-visible buffer digest.
///
/// Editor integrations should prefer this over `document_changed` when they
/// can cheaply compute a digest. The CLI uses the digest sidecar to fail closed
/// before direct disk writes when an editor buffer is idle but still unsaved.
pub fn document_changed_with_digest(file: &str, content_len: usize, content_hash: &str) {
    document_changed(file);
    if let Err(e) = record_live_buffer_digest(file, content_len, content_hash) {
        eprintln!(
            "[debounce] live buffer digest write failed for {:?}: {}",
            file, e
        );
    }
}

/// Compute the SHA-256 hex digest used for live-buffer sidecars.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
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

/// Return the number of tracked files in the debounce state.
pub fn tracked_count() -> usize {
    with_state(|map| map.len())
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

/// Directory for latest editor-visible buffer digests, relative to project root.
const LIVE_BUFFER_DIR: &str = ".agent-doc/live-buffer";

/// Latest editor-visible buffer digest for a document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveBufferSnapshot {
    pub path: String,
    pub len: usize,
    pub hash: String,
    pub timestamp_ms: u128,
}

/// Editor-authoritative buffer state reported by IDE plugins through the FFI bridge.
///
/// When an editor plugin is attached, it reports this state on every document
/// change. The debounce/stability layer uses it instead of the truncation
/// heuristic (`extract_last_added_line` + `looks_truncated`) to detect whether
/// the user is still typing. This eliminates race classes where edits above the
/// last inserted line are missed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditorBufferState {
    pub path: String,
    pub version: u64,
    pub dirty: bool,
    pub last_edit_timestamp_ms: u128,
    pub save_timestamp_ms: Option<u128>,
    pub hash: Option<String>,
    pub content_len: Option<usize>,
    pub session_id: Option<String>,
}

/// Record the latest editor buffer state for a document.
///
/// Called by IDE plugins via the FFI bridge on every document change.
/// Stores in the in-memory `EDITOR_BUFFER_STATES` map, which is
/// concurrency-safe via the Session Actor's Mutex.
pub fn record_editor_buffer_state(state: &EditorBufferState) {
    let path = PathBuf::from(&state.path);
    let mut guard = EDITOR_BUFFER_STATES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(path, state.clone());
}

/// Return the latest editor buffer state for a document, if one has been recorded.
pub fn editor_buffer_state(file: &str) -> Option<EditorBufferState> {
    let path = PathBuf::from(file);
    let guard = EDITOR_BUFFER_STATES.lock().unwrap();
    match guard.as_ref() {
        Some(map) => map.get(&path).cloned(),
        None => None,
    }
}

/// Check whether the editor buffer state indicates a stable (idle) document.
///
/// Returns `Some(stable_state)` when the editor plugin has reported a state
/// where: the buffer is not dirty, or the last edit timestamp is older than
/// `debounce_ms`, or the version/hash has been stable across consecutive reads.
///
/// Returns `None` when no editor state exists (plugin not attached) or when
/// the editor is still actively editing.
pub fn editor_buffer_stable(file: &str, debounce_ms: u64) -> Option<EditorBufferState> {
    let state = editor_buffer_state(file)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let elapsed_since_edit = now.saturating_sub(state.last_edit_timestamp_ms);
    if !state.dirty || elapsed_since_edit >= debounce_ms as u128 {
        Some(state)
    } else {
        None
    }
}

/// Block until the editor buffer state indicates stability, or timeout.
///
/// Returns `Some(EditorBufferState)` when the editor buffer becomes stable
/// within `timeout_ms`, or `None` on timeout / no editor state available.
pub fn await_editor_buffer_stable(
    file: &str,
    debounce_ms: u64,
    timeout_ms: u64,
) -> Option<EditorBufferState> {
    let start = Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let poll_interval = std::time::Duration::from_millis(100);
    loop {
        if let Some(state) = editor_buffer_stable(file, debounce_ms) {
            return Some(state);
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(poll_interval);
    }
}

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

/// Record the latest editor-visible buffer digest for the given document path.
pub fn record_live_buffer_digest(
    file: &str,
    content_len: usize,
    content_hash: &str,
) -> std::io::Result<()> {
    let live_path = live_buffer_snapshot_path(file);
    if let Some(parent) = live_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let snapshot = LiveBufferSnapshot {
        path: file.to_string(),
        len: content_len,
        hash: content_hash.to_ascii_lowercase(),
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    };
    let encoded = serde_json::to_string(&snapshot)?;
    std::fs::write(&live_path, encoded)
}

/// Return the latest editor-visible buffer digest for a document.
pub fn live_buffer_snapshot(file: &str) -> Option<LiveBufferSnapshot> {
    let path = live_buffer_snapshot_path(file);
    let content = std::fs::read_to_string(path).ok()?;
    let snapshot: LiveBufferSnapshot = serde_json::from_str(&content).ok()?;
    if snapshot.path == file {
        Some(snapshot)
    } else {
        None
    }
}

/// Return `Some(snapshot)` when the editor-visible buffer digest differs from
/// the supplied content. Returns `None` when there is no editor-visible sidecar
/// or when it matches the content.
pub fn live_buffer_diverges_from_content(file: &str, content: &str) -> Option<LiveBufferSnapshot> {
    let snapshot = live_buffer_snapshot(file)?;
    let expected_len = content.len();
    let expected_hash = content_hash(content);
    if snapshot.len == expected_len && snapshot.hash.eq_ignore_ascii_case(&expected_hash) {
        None
    } else {
        Some(snapshot)
    }
}

/// Compute the typing indicator file path for a document.
fn typing_indicator_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    // Walk up to find .agent-doc/ directory (one pop per level, no skip)
    let mut dir = PathBuf::from(file);
    dir.pop(); // Start from file's parent
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir.join(TYPING_DIR).join(format!("{:016x}", hash));
        }
        if !dir.pop() {
            // Fallback: use file's parent directory
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf();
            return parent.join(TYPING_DIR).join(format!("{:016x}", hash));
        }
    }
}

/// Compute the live-buffer snapshot file path for a document.
fn live_buffer_snapshot_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir.join(LIVE_BUFFER_DIR).join(format!("{:016x}", hash));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf();
            return parent.join(LIVE_BUFFER_DIR).join(format!("{:016x}", hash));
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

// ── Response status signal (file-based) ──

/// Status directory for cross-process signals.
const STATUS_DIR: &str = ".agent-doc/status";

/// Set the response status for a file.
///
/// Status values: "generating", "writing", "routing", "idle"
/// Writes a file signal to `.agent-doc/status/` for cross-process visibility.
pub fn set_status(file: &str, status: &str) {
    let _ = write_status_file(file, status);
}

/// Get the response status for a file.
///
/// Returns "idle" if no status file exists or it's stale (>30s).
pub fn get_status(file: &str) -> String {
    get_status_via_file(file)
}

/// Check if any operation is in progress for a file.
///
/// Returns `true` if status is NOT "idle". Used by plugins to avoid
/// triggering routes during active operations.
pub fn is_busy(file: &str) -> bool {
    get_status(file) != "idle"
}

/// Get status from file signal (cross-process).
///
/// Returns "idle" if no status file exists or it's stale (>30s).
pub fn get_status_via_file(file: &str) -> String {
    let path = status_file_path(file);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            // Format: "status:timestamp_ms"
            let parts: Vec<&str> = content.trim().splitn(2, ':').collect();
            if parts.len() == 2
                && let Ok(ts) = parts[1].parse::<u128>()
            {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                // Stale after 30s — operation probably crashed
                if now.saturating_sub(ts) < 30_000 {
                    return parts[0].to_string();
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
    dir.pop(); // Start from file's parent
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir.join(STATUS_DIR).join(format!("{:016x}", hash));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf();
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
    fn live_buffer_digest_records_visible_content() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("live-buffer");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        let doc = tmp.path().join("test-live-buffer.md");
        std::fs::write(&doc, "disk").unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let visible = "disk plus unsaved prompt";

        document_changed_with_digest(&doc_str, visible.len(), &content_hash(visible));

        let snapshot = live_buffer_snapshot(&doc_str).expect("live buffer snapshot");
        assert_eq!(snapshot.path, doc_str);
        assert_eq!(snapshot.len, visible.len());
        assert_eq!(snapshot.hash, content_hash(visible));
        assert!(live_buffer_diverges_from_content(&doc_str, "disk").is_some());
        assert!(live_buffer_diverges_from_content(&doc_str, visible).is_none());
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

    // ── GAP 1: Mtime Granularity ──
    // Route path relies on filesystem mtime which may have coarse resolution (100ms-1s).
    // Can miss rapid successive edits if they occur within mtime granularity window.

    #[test]
    fn rapid_edits_within_mtime_granularity() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("test-rapid-edits.md");
        std::fs::write(&doc, "initial").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        // Simulate rapid edits: write → is_idle check → write again
        // All within filesystem mtime granularity (e.g., 1s on some systems)
        document_changed(&doc_str);
        // This may not detect the second change on coarse-grained filesystems
        document_changed(&doc_str);

        // Should be not idle, but mtime-based detection may fail
        assert!(!is_idle(&doc_str, 500));
    }

    // ── GAP 2: Untracked File Edge Case ──
    // Untracked files return idle=true immediately, preventing await_idle from blocking forever.
    // But is_tracked() should distinguish "never-tracked" from "tracked and idle".

    #[test]
    fn is_tracked_distinguishes_untracked_from_idle() {
        let file_never_tracked = "/tmp/never-tracked.md";
        let file_tracked_idle = "/tmp/tracked-idle.md";

        // Never-tracked file
        assert!(!is_tracked(file_never_tracked));
        assert!(is_idle(file_never_tracked, 1500)); // idle=true for untracked

        // Tracked file that is now idle
        document_changed(file_tracked_idle);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(is_tracked(file_tracked_idle)); // is_tracked=true
        assert!(is_idle(file_tracked_idle, 10)); // also idle=true after debounce
    }

    #[test]
    fn await_idle_on_untracked_file_returns_immediately() {
        let start = Instant::now();
        // Untracked file should return immediately, not wait
        assert!(await_idle("/tmp/untracked-await.md", 1500, 5000));
        assert!(start.elapsed().as_millis() < 500);
    }

    #[test]
    fn await_idle_respects_tracked_state() {
        let tracked_file = "/tmp/tracked-await.md";
        document_changed(tracked_file);
        assert!(is_tracked(tracked_file));
        assert!(!await_idle(tracked_file, 500, 100));

        document_changed(tracked_file);

        // await_idle should wait for debounce even though tracked
        let start = Instant::now();
        assert!(await_idle(tracked_file, 200, 5000));
        assert!(start.elapsed().as_millis() >= 100);
        assert!(is_idle(tracked_file, 200));
    }

    // ── GAP 3: Hash Collision Risk ──
    // DefaultHasher is non-cryptographic; collision risk is low but possible.
    // Need to verify collision handling in typing indicator files.

    #[test]
    fn hash_collision_handling() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("typing");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();

        let doc1 = tmp.path().join("doc1.md");
        let doc2 = tmp.path().join("doc2.md");
        std::fs::write(&doc1, "test").unwrap();
        std::fs::write(&doc2, "test").unwrap();

        let doc1_str = doc1.to_string_lossy().to_string();
        let doc2_str = doc2.to_string_lossy().to_string();

        document_changed(&doc1_str);
        let path1 = typing_indicator_path(&doc1_str);

        document_changed(&doc2_str);
        let path2 = typing_indicator_path(&doc2_str);

        // If hashes collide, paths are identical
        // This is a low-probability event but should be documented
        if path1 == path2 {
            // Collision detected: last write wins, earlier timestamp is overwritten
            // is_typing_via_file for both returns true for the more recent change only
            assert!(is_typing_via_file(&doc2_str, 2000)); // Most recent
        } else {
            // No collision: separate files, both typing
            assert!(is_typing_via_file(&doc1_str, 2000));
            assert!(is_typing_via_file(&doc2_str, 2000));
        }
    }

    // ── GAP 4: Reactive Mode CRDT Assumption ──
    // Watch daemon reactive path (zero debounce) assumes CRDT merge always converges.
    // If CRDT merge fails or produces unexpected state, reactive mode could cause issues.
    // Note: This is tested at watch.rs level; debounce.rs cannot test CRDT semantics.

    #[test]
    fn reactive_mode_requires_zero_debounce() {
        // Reactive mode relies on zero debounce (instant idle check).
        // With debounce_ms=0, elapsed >= 0 is always true.
        let reactive_file = "/tmp/reactive.md";
        document_changed(reactive_file);

        // With zero debounce, even freshly changed files return idle=true
        // because elapsed (even nanoseconds) >= 0
        assert!(is_idle(reactive_file, 0));

        // This means reactive mode responds instantly but assumes CRDT merge
        // will handle concurrent edits correctly (see Gap 4 in SPEC.md)
    }

    // ── GAP 5: Status File Staleness (30s timeout) ──
    // Response status files expire after 30s with assumption operation crashed.
    // No recovery for long-running operations or delayed writes.

    #[test]
    fn status_file_staleness_timeout() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("status");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        let doc = tmp.path().join("test-status.md");
        std::fs::write(&doc, "test").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        set_status(&doc_str, "generating");
        assert_eq!(get_status(&doc_str), "generating");

        // get_status now delegates to get_status_via_file
        assert_eq!(get_status_via_file(&doc_str), "generating");

        // After 30s, get_status_via_file returns "idle" (assumes operation crashed)
        // This test documents the 30s assumption but cannot test actual passage of time
        // in unit tests without mocking SystemTime.
    }

    #[test]
    fn status_file_cleared_on_idle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("status");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        let doc = tmp.path().join("test-status-clear.md");
        std::fs::write(&doc, "test").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        set_status(&doc_str, "writing");
        assert!(is_busy(&doc_str));

        set_status(&doc_str, "idle");
        assert!(!is_busy(&doc_str));
        assert_eq!(get_status(&doc_str), "idle");
    }

    // ── GAP 6: Hardcoded Timing Constants ──
    // Preflight hardcodes 1500ms for typing indicator debounce (vs 500ms poll debounce).
    // Not configurable; one-size-fits-all fails for slow CI or fast typists.

    #[test]
    fn timing_constants_are_configurable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("typing");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        let doc = tmp.path().join("test-timing.md");
        std::fs::write(&doc, "test").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        document_changed(&doc_str);

        // is_typing_via_file accepts debounce_ms as parameter — good
        assert!(is_typing_via_file(&doc_str, 2000));
        assert!(is_typing_via_file(&doc_str, 100));

        // await_idle_via_file also accepts debounce_ms — configurable
        let start = Instant::now();
        let result = await_idle_via_file(&doc_str, 10, 1000);
        let elapsed = start.elapsed();

        // With 10ms debounce, should wait ~10ms then return true
        assert!(result);
        assert!(elapsed.as_millis() >= 10);

        // preflight.rs hardcodes 1500ms in is_typing_via_file call
        // This is a documentation test: ideally 1500ms should be configurable
    }

    #[test]
    fn await_idle_via_file_respects_poll_interval() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("typing");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        let doc = tmp.path().join("test-poll-interval.md");
        std::fs::write(&doc, "test").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        document_changed(&doc_str);

        let start = Instant::now();
        // With 100ms debounce, poll should check ~every 100ms
        assert!(await_idle_via_file(&doc_str, 100, 5000));
        let elapsed = start.elapsed().as_millis();

        // Should wait at least the debounce time (allowing some jitter)
        assert!(elapsed >= 100);
    }

    // ── GAP 7: Directory-walk bug (depth-1) ──
    // typing_indicator_path and status_file_path had a double-pop bug:
    // each loop iteration popped twice, skipping every other directory level.
    // Files at depth 1 from the project root (e.g. tasks/file.md) failed to
    // find .agent-doc/ and fell back to the wrong path.

    #[test]
    fn typing_indicator_found_for_file_one_level_deep() {
        let tmp = tempfile::TempDir::new().unwrap();
        // .agent-doc at project root
        let agent_doc_dir = tmp.path().join(".agent-doc").join("typing");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        // File one level deep (tasks/file.md pattern)
        let subdir = tmp.path().join("tasks");
        std::fs::create_dir_all(&subdir).unwrap();
        let doc = subdir.join("test-depth1.md");
        std::fs::write(&doc, "test").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        document_changed(&doc_str);

        // Should find .agent-doc/ at project root, not fall back to wrong path
        assert!(is_typing_via_file(&doc_str, 2000));
    }

    #[test]
    fn typing_indicator_found_for_file_two_levels_deep() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("typing");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        // File two levels deep (tasks/software/file.md pattern)
        let subdir = tmp.path().join("tasks").join("software");
        std::fs::create_dir_all(&subdir).unwrap();
        let doc = subdir.join("test-depth2.md");
        std::fs::write(&doc, "test").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        document_changed(&doc_str);

        assert!(is_typing_via_file(&doc_str, 2000));
    }

    #[test]
    fn status_found_for_file_one_level_deep() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc").join("status");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        let subdir = tmp.path().join("tasks");
        std::fs::create_dir_all(&subdir).unwrap();
        let doc = subdir.join("test-status-depth1.md");
        std::fs::write(&doc, "test").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        set_status(&doc_str, "generating");

        // get_status delegates to file-based check — must find .agent-doc at project root
        assert_eq!(get_status_via_file(&doc_str), "generating");
    }

    // ── Editor Buffer State tests ──

    #[test]
    fn editor_buffer_state_records_and_reads() {
        let doc_str = "/tmp/test-editor-state-inmem.md";

        let state = EditorBufferState {
            path: doc_str.to_string(),
            version: 1,
            dirty: true,
            last_edit_timestamp_ms: 1000,
            save_timestamp_ms: None,
            hash: Some("abc123".to_string()),
            content_len: Some(4),
            session_id: None,
        };
        record_editor_buffer_state(&state);

        let read = editor_buffer_state(doc_str).expect("should read state");
        assert_eq!(read.version, 1);
        assert!(read.dirty);
        assert_eq!(read.hash.as_deref(), Some("abc123"));
    }

    #[test]
    fn editor_buffer_state_returns_none_for_unknown() {
        assert!(editor_buffer_state("/tmp/no-such-editor-state.md").is_none());
    }

    #[test]
    fn editor_buffer_stable_when_not_dirty() {
        let doc_str = "/tmp/test-stable-inmem.md";

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let state = EditorBufferState {
            path: doc_str.to_string(),
            version: 5,
            dirty: false,
            last_edit_timestamp_ms: now,
            save_timestamp_ms: Some(now),
            hash: None,
            content_len: None,
            session_id: None,
        };
        record_editor_buffer_state(&state);

        let stable = editor_buffer_stable(doc_str, 500).expect("should be stable");
        assert_eq!(stable.version, 5);
    }

    #[test]
    fn editor_buffer_stable_when_debounce_elapsed() {
        let doc_str = "/tmp/test-stable-debounce-inmem.md";

        let old_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            - 2000;

        let state = EditorBufferState {
            path: doc_str.to_string(),
            version: 3,
            dirty: true,
            last_edit_timestamp_ms: old_ts,
            save_timestamp_ms: None,
            hash: Some("deadbeef".to_string()),
            content_len: Some(10),
            session_id: Some("jb-session-1".to_string()),
        };
        record_editor_buffer_state(&state);

        let stable = editor_buffer_stable(doc_str, 500).expect("should be stable after debounce");
        assert_eq!(stable.version, 3);
        assert!(stable.dirty);
        assert_eq!(stable.session_id.as_deref(), Some("jb-session-1"));
    }

    #[test]
    fn editor_buffer_not_stable_while_editing() {
        let doc_str = "/tmp/test-not-stable-inmem.md";

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let state = EditorBufferState {
            path: doc_str.to_string(),
            version: 2,
            dirty: true,
            last_edit_timestamp_ms: now,
            save_timestamp_ms: None,
            hash: None,
            content_len: None,
            session_id: None,
        };
        record_editor_buffer_state(&state);

        assert!(editor_buffer_stable(doc_str, 2000).is_none());
    }

    #[test]
    fn editor_buffer_stable_returns_none_when_no_state() {
        assert!(editor_buffer_stable("/tmp/no-state.md", 500).is_none());
    }

    #[test]
    fn editor_buffer_state_overwrites_on_new_version() {
        let doc_str = "/tmp/test-overwrite-inmem.md";

        let state_v1 = EditorBufferState {
            path: doc_str.to_string(),
            version: 1,
            dirty: true,
            last_edit_timestamp_ms: 1000,
            save_timestamp_ms: None,
            hash: None,
            content_len: None,
            session_id: None,
        };
        record_editor_buffer_state(&state_v1);

        let state_v2 = EditorBufferState {
            path: doc_str.to_string(),
            version: 2,
            dirty: false,
            last_edit_timestamp_ms: 2000,
            save_timestamp_ms: Some(2000),
            hash: Some("newhash".to_string()),
            content_len: Some(20),
            session_id: None,
        };
        record_editor_buffer_state(&state_v2);

        let read = editor_buffer_state(doc_str).unwrap();
        assert_eq!(read.version, 2);
        assert!(!read.dirty);
        assert_eq!(read.hash.as_deref(), Some("newhash"));
    }
}

//! # Module: debounce
//!
//! ## Spec
//! - Provides a shared typing-debounce mechanism used by all editor plugins (JetBrains, VS Code,
//!   Neovim, Zed) so they share identical timing logic via the agent-doc FFI layer.
//! - In-process state: a `Mutex<HashMap<PathBuf, Instant>>` (`LAST_CHANGE`) records the last
//!   edit timestamp per file path.
//! - Cross-process state: each `document_changed` call queues a best-effort millisecond Unix
//!   timestamp write to `.agent-doc/typing/<hash>` so CLI invocations running in a separate
//!   process can detect active typing without making editor change listeners wait on disk.
//!   Editor plugins may additionally record the visible buffer digest in
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
//! - `document_changed(file: &str)` — records now as last-change time; queues a typing indicator
//!   file write (best-effort); never panics.
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

use agent_doc_hash::content_hash;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Instant;

static LAST_CHANGE: Mutex<Option<HashMap<PathBuf, Instant>>> = Mutex::new(None);

static EDITOR_BUFFER_STATES: Mutex<Option<HashMap<PathBuf, EditorBufferState>>> = Mutex::new(None);

static TYPING_INDICATOR_TX: OnceLock<mpsc::Sender<String>> = OnceLock::new();

fn with_state<R>(f: impl FnOnce(&mut HashMap<PathBuf, Instant>) -> R) -> R {
    let mut guard = LAST_CHANGE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// Record a document change event for the given file.
///
/// Called by editor plugins on every document modification.
/// Also queues a typing indicator file write for cross-process visibility.
pub fn document_changed(file: &str) {
    let path = PathBuf::from(file);
    with_state(|map| {
        map.insert(path.clone(), Instant::now());
    });
    queue_typing_indicator_write(file);
}

/// Record a document change and the editor-visible buffer digest.
///
/// Editor integrations should prefer this over `document_changed` when they
/// can cheaply compute a digest. The CLI uses the digest sidecar to fail closed
/// before direct disk writes when an editor buffer is idle but still unsaved.
pub fn document_changed_with_digest(file: &str, content_len: usize, content_hash: &str) {
    document_changed_with_digest_for_editor(file, content_len, content_hash, None);
}

/// Record a document change and the editor-visible buffer digest for one editor
/// instance. `editor_id` keys the durable live-buffer sidecar per editor so
/// multiple open editors on the same document do not overwrite each other.
pub fn document_changed_with_digest_for_editor(
    file: &str,
    content_len: usize,
    content_hash: &str,
    editor_id: Option<&str>,
) {
    document_changed(file);
    if let Err(e) = record_live_buffer_digest_for_editor(file, content_len, content_hash, editor_id)
    {
        eprintln!(
            "[debounce] live buffer digest write failed for {:?}: {}",
            file, e
        );
    }
}

/// Record a document change and the editor-visible buffer digest WITH full
/// content (#pcp6). Editor integrations that can cheaply read the buffer text
/// prefer this so the visible-write reconcile guard can positively confirm the
/// editor buffer equals on-disk content.
pub fn document_changed_with_content(file: &str, content: &str) {
    document_changed_with_content_for_editor(file, content, None);
}

/// Record a document change plus the editor-visible full buffer content for one
/// editor instance. This is the multi-editor form of
/// [`document_changed_with_content`].
pub fn document_changed_with_content_for_editor(
    file: &str,
    content: &str,
    editor_id: Option<&str>,
) {
    document_changed(file);
    if let Err(e) = record_live_buffer_digest_content_for_editor(file, content, editor_id) {
        eprintln!(
            "[debounce] live buffer content digest write failed for {:?}: {}",
            file, e
        );
    }
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
const WRITE_PROVENANCE_DIR: &str = ".agent-doc/write-provenance";
pub const OPERATOR_TEXT_AUTHORITY_CAPABILITY: &str = "operator_text_authority_v1";

/// Latest editor-visible buffer digest for a document.
///
/// `content` (#pcp6) is the editor's full buffer text when the plugin reports it
/// (`agent_doc_document_changed_digest_content`); `None` for the len/hash-only
/// digest path. When present it lets `live_buffer_diverges_from_content`
/// positively confirm the editor buffer equals on-disk content (no unsaved edit
/// ahead of disk) instead of inferring from len/hash + the mtime/provenance
/// heuristics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveBufferSnapshot {
    pub path: String,
    pub len: usize,
    pub hash: String,
    pub timestamp_ms: u128,
    /// Monotonic per-editor edit epoch, bumped on each durable live-buffer report.
    #[serde(default)]
    pub edit_epoch: u64,
    /// Last epoch known to be synced to disk by the reporting editor. Older
    /// sidecars default to zero and are treated conservatively unless disk
    /// content proves the current epoch is already flushed.
    #[serde(default)]
    pub last_synced_epoch: u64,
    /// Optional base64-encoded yrs state vector for callers that have an attached
    /// editor replica. The epoch barrier can operate without it, but carrying it
    /// through the sidecar gives binary-side probes a state-vector exchange
    /// surface instead of requiring text comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_vector_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl LiveBufferSnapshot {
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|candidate| candidate == capability)
    }
}

/// Return whether this live-buffer sidecar still belongs to a live editor
/// process when the editor id carries process identity.
pub fn live_buffer_snapshot_editor_is_live(snapshot: &LiveBufferSnapshot) -> bool {
    match snapshot.editor_id.as_deref() {
        Some(editor_id) => editor_id_is_live_for_delivery(editor_id),
        None => true,
    }
}

struct LiveBufferSnapshotMetadata<'a> {
    editor_id: Option<&'a str>,
    editor_kind: Option<&'a str>,
    editor_version: Option<&'a str>,
    capabilities: &'a [&'a str],
}

/// Cross-process editor sync status derived from durable live-buffer sidecars.
///
/// `effective_last_synced_epoch` is promoted to `edit_epoch` when disk content
/// proves the current editor buffer is already flushed, even if the sidecar came
/// from an older plugin that did not explicitly stamp `last_synced_epoch`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditorSyncStatus {
    pub editor_id: Option<String>,
    pub edit_epoch: u64,
    pub last_synced_epoch: u64,
    pub effective_last_synced_epoch: u64,
    pub in_flight: bool,
    pub disk_matches: bool,
    pub timestamp_ms: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_vector_b64: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditorSyncBarrierKind {
    NoEditor,
    Ready,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditorSyncBarrierOutcome {
    pub kind: EditorSyncBarrierKind,
    pub statuses: Vec<EditorSyncStatus>,
    pub typing_recent: bool,
}

/// Write-provenance record for agent-doc's own most-recent disk write to a
/// document (#pcp2 / #ipc-drift-writeprovenance).
///
/// `atomic_write` stamps this on every document disk write (not on `.agent-doc/`
/// sidecars). The visible-write reconcile guard reads it to *positively* attribute
/// a foreign-looking disk change to agent-doc's own machinery instead of inferring
/// "foreign disk write vs genuine unsaved editor edit" from the
/// `LIVE_BUFFER_STALE_SKEW_MS` filesystem-mtime heuristic. `timestamp_ms` is
/// agent-doc's own write time, in the same `SystemTime` clock domain as the editor
/// plugin's `LiveBufferSnapshot.timestamp_ms`, so the two are directly comparable.
///
/// `actor` is a free-text provenance label that maps to `agent-doc-turn`'s
/// `OpActor` (today always `"agent"` for binary disk writes); kept as a string here
/// to avoid coupling the debounce layer to the core op-log crate in this phase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteProvenance {
    pub path: String,
    pub len: usize,
    pub hash: String,
    pub write_id: String,
    pub actor: String,
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

fn queue_typing_indicator_write(file: &str) {
    if let Err(e) = typing_indicator_sender().send(file.to_string()) {
        eprintln!(
            "[debounce] typing indicator enqueue failed for {:?}: {}",
            file, e
        );
    }
}

fn typing_indicator_sender() -> &'static mpsc::Sender<String> {
    TYPING_INDICATOR_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<String>();
        if let Err(e) = std::thread::Builder::new()
            .name("agent-doc-typing-indicator".to_string())
            .spawn(move || typing_indicator_worker(rx))
        {
            eprintln!("[debounce] typing indicator worker spawn failed: {e}");
        }
        tx
    })
}

fn typing_indicator_worker(rx: mpsc::Receiver<String>) {
    loop {
        let first = match rx.recv() {
            Ok(file) => file,
            Err(_) => return,
        };
        let mut pending = HashMap::new();
        pending.insert(first, ());
        while let Ok(file) = rx.try_recv() {
            pending.insert(file, ());
        }

        for file in pending.into_keys() {
            if let Err(e) = write_typing_indicator(&file) {
                eprintln!(
                    "[debounce] typing indicator write failed for {:?}: {}",
                    file, e
                );
            }
        }
    }
}

/// Record the latest editor-visible buffer digest (len/hash only) for the given
/// document path.
pub fn record_live_buffer_digest(
    file: &str,
    content_len: usize,
    content_hash: &str,
) -> std::io::Result<()> {
    record_live_buffer_digest_for_editor(file, content_len, content_hash, None)
}

/// Record the latest editor-visible buffer digest for one editor instance.
pub fn record_live_buffer_digest_for_editor(
    file: &str,
    content_len: usize,
    content_hash: &str,
    editor_id: Option<&str>,
) -> std::io::Result<()> {
    write_live_buffer_snapshot_with_metadata(
        file,
        content_len,
        content_hash.to_ascii_lowercase(),
        None,
        LiveBufferSnapshotMetadata {
            editor_id,
            editor_kind: None,
            editor_version: None,
            capabilities: &[],
        },
    )
}

/// Record the latest editor-visible buffer digest WITH the full buffer content
/// (#pcp6). The plugin reports the editor's text so the visible-write reconcile
/// guard can positively confirm the editor buffer equals on-disk content.
pub fn record_live_buffer_digest_content(file: &str, content: &str) -> std::io::Result<()> {
    record_live_buffer_digest_content_for_editor(file, content, None)
}

/// Record the latest editor-visible full buffer content for one editor
/// instance.
pub fn record_live_buffer_digest_content_for_editor(
    file: &str,
    content: &str,
    editor_id: Option<&str>,
) -> std::io::Result<()> {
    write_live_buffer_snapshot_with_metadata(
        file,
        content.len(),
        content_hash(content),
        Some(content.to_string()),
        LiveBufferSnapshotMetadata {
            editor_id,
            editor_kind: None,
            editor_version: None,
            capabilities: &[],
        },
    )
}

pub fn record_live_buffer_digest_content_for_editor_with_capabilities(
    file: &str,
    content: &str,
    editor_id: &str,
    editor_kind: &str,
    editor_version: &str,
    capabilities: &[&str],
) -> std::io::Result<()> {
    write_live_buffer_snapshot_with_metadata(
        file,
        content.len(),
        content_hash(content),
        Some(content.to_string()),
        LiveBufferSnapshotMetadata {
            editor_id: Some(editor_id),
            editor_kind: Some(editor_kind),
            editor_version: Some(editor_version),
            capabilities,
        },
    )
}

fn write_live_buffer_snapshot_with_metadata(
    file: &str,
    content_len: usize,
    hash: String,
    content: Option<String>,
    metadata: LiveBufferSnapshotMetadata<'_>,
) -> std::io::Result<()> {
    let live_path = live_buffer_snapshot_path_for_editor(file, metadata.editor_id);
    let previous = read_live_buffer_snapshot(file, &live_path);
    let edit_epoch = previous
        .as_ref()
        .map(|snapshot| snapshot.edit_epoch)
        .unwrap_or(0)
        .saturating_add(1);
    let last_synced_epoch = previous
        .as_ref()
        .map(|snapshot| snapshot.last_synced_epoch)
        .unwrap_or(0)
        .min(edit_epoch);
    let state_vector_b64 = previous
        .as_ref()
        .and_then(|snapshot| snapshot.state_vector_b64.clone());
    if let Some(parent) = live_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let snapshot = LiveBufferSnapshot {
        path: file.to_string(),
        len: content_len,
        hash,
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        edit_epoch,
        last_synced_epoch,
        state_vector_b64,
        editor_id: metadata
            .editor_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(ToString::to_string),
        editor_kind: metadata
            .editor_kind
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
            .map(ToString::to_string),
        editor_version: metadata
            .editor_version
            .map(str::trim)
            .filter(|version| !version.is_empty())
            .map(ToString::to_string),
        capabilities: metadata
            .capabilities
            .iter()
            .map(|capability| capability.trim())
            .filter(|capability| !capability.is_empty())
            .map(ToString::to_string)
            .collect(),
        content,
    };
    let encoded = serde_json::to_string(&snapshot)?;
    std::fs::write(&live_path, encoded)
}

/// Return the latest editor-visible buffer digest for a document.
pub fn live_buffer_snapshot(file: &str) -> Option<LiveBufferSnapshot> {
    live_buffer_snapshots(file)
        .into_iter()
        .max_by_key(|snapshot| snapshot.timestamp_ms)
}

/// Return every live-buffer snapshot for a document: the legacy single-editor
/// sidecar plus any per-editor sidecars (`<pathhash>.<editorid>`).
pub fn live_buffer_snapshots(file: &str) -> Vec<LiveBufferSnapshot> {
    let (dir, stem) = live_buffer_snapshot_dir_and_stem(file);
    let mut snapshots = Vec::new();
    let legacy = dir.join(&stem);
    if let Some(snapshot) = read_live_buffer_snapshot(file, &legacy) {
        snapshots.push(snapshot);
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return snapshots;
    };
    let prefix = format!("{stem}.");
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        if let Some(snapshot) = read_live_buffer_snapshot(file, &path) {
            snapshots.push(snapshot);
        }
    }
    snapshots
}

fn read_live_buffer_snapshot(file: &str, path: &std::path::Path) -> Option<LiveBufferSnapshot> {
    let content = std::fs::read_to_string(path).ok()?;
    let snapshot: LiveBufferSnapshot = serde_json::from_str(&content).ok()?;
    (snapshot.path == file).then_some(snapshot)
}

/// Return the per-editor sync status for every live-buffer sidecar of `file`.
///
/// The poll is state-vector/epoch based when the editor reports those fields and
/// uses disk len/hash only to prove the current epoch has already been flushed.
/// It never compares full text.
pub fn editor_sync_statuses(file: &str) -> Vec<EditorSyncStatus> {
    let disk = std::fs::read_to_string(file).ok();
    let disk_len = disk.as_ref().map(|content| content.len());
    let disk_hash = disk.as_ref().map(|content| content_hash(content));
    live_buffer_snapshots(file)
        .into_iter()
        .map(|snapshot| {
            let disk_matches = match (disk_len, disk_hash.as_ref()) {
                (Some(len), Some(hash)) => {
                    snapshot.len == len && snapshot.hash.eq_ignore_ascii_case(hash)
                }
                _ => false,
            };
            let effective_last_synced_epoch = if disk_matches {
                snapshot.edit_epoch
            } else {
                snapshot.last_synced_epoch
            };
            let in_flight = snapshot.edit_epoch > effective_last_synced_epoch;
            EditorSyncStatus {
                editor_id: snapshot.editor_id,
                edit_epoch: snapshot.edit_epoch,
                last_synced_epoch: snapshot.last_synced_epoch,
                effective_last_synced_epoch,
                in_flight,
                disk_matches,
                timestamp_ms: snapshot.timestamp_ms,
                state_vector_b64: snapshot.state_vector_b64,
            }
        })
        .collect()
}

pub fn editor_sync_in_flight(file: &str) -> bool {
    editor_sync_statuses(file)
        .iter()
        .any(|status| status.in_flight)
}

/// Wait briefly for the editor epoch/typing sidecars to settle.
///
/// This is a bounded barrier, not a lock: timeout returns `TimedOut` so callers
/// can fail open to the editor buffer (for example by requesting a save) instead
/// of discarding a live edit or blocking indefinitely.
pub fn await_editor_sync_barrier(
    file: &str,
    settle_ms: u64,
    timeout_ms: u64,
) -> EditorSyncBarrierOutcome {
    let start = Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let poll_interval = std::time::Duration::from_millis(10);

    loop {
        let statuses = editor_sync_statuses(file);
        let typing_recent = settle_ms > 0 && is_typing_via_file(file, settle_ms);
        let in_flight = typing_recent || statuses.iter().any(|status| status.in_flight);
        if statuses.is_empty() && !typing_recent {
            return EditorSyncBarrierOutcome {
                kind: EditorSyncBarrierKind::NoEditor,
                statuses,
                typing_recent,
            };
        }
        if !in_flight {
            return EditorSyncBarrierOutcome {
                kind: EditorSyncBarrierKind::Ready,
                statuses,
                typing_recent,
            };
        }
        if start.elapsed() >= timeout {
            return EditorSyncBarrierOutcome {
                kind: EditorSyncBarrierKind::TimedOut,
                statuses,
                typing_recent,
            };
        }
        std::thread::sleep(poll_interval);
    }
}

/// Clear the durable live-buffer sidecar for a document.
///
/// Models the editor-close lifecycle (Shared Foundation pattern): when an editor
/// closes a document there are no unsaved edits ahead of disk anymore, so the
/// cycle must fall back to the on-disk file. Removing the sidecar makes
/// [`live_buffer_diverges_from_content`] (and through it
/// `realtime_model::resolve_current_doc`) return `editor_absent` instead of
/// surfacing a stale buffer. Best-effort but not error-swallowing: a missing
/// sidecar is success; any other IO error is returned so callers log rather than
/// silently ignore it (per the no-`let _ =` rule).
pub fn clear_live_buffer(file: &str) -> std::io::Result<()> {
    clear_live_buffer_for_editor(file, None)
}

/// Clear the durable live-buffer sidecar for one editor instance. Passing
/// `None` clears the legacy sidecar and every per-editor sidecar for the file.
pub fn clear_live_buffer_for_editor(file: &str, editor_id: Option<&str>) -> std::io::Result<()> {
    if editor_id.map(str::trim).is_some_and(|id| !id.is_empty()) {
        return remove_live_buffer_snapshot_path(&live_buffer_snapshot_path_for_editor(
            file, editor_id,
        ));
    }

    let (dir, stem) = live_buffer_snapshot_dir_and_stem(file);
    remove_live_buffer_snapshot_path(&dir.join(&stem))?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };
    let prefix = format!("{stem}.");
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(&prefix) {
            remove_live_buffer_snapshot_path(&path)?;
        }
    }
    Ok(())
}

fn remove_live_buffer_snapshot_path(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Record write-provenance for agent-doc's own disk write to `file` (#pcp2).
///
/// Called from `atomic_write` after a successful document write. Best-effort and
/// must never block or fail the write — callers log on error and continue.
pub fn record_write_provenance(
    file: &str,
    content_len: usize,
    content_hash: &str,
    write_id: &str,
    actor: &str,
) -> std::io::Result<()> {
    let prov_path = write_provenance_path(file);
    if let Some(parent) = prov_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let record = WriteProvenance {
        path: file.to_string(),
        len: content_len,
        hash: content_hash.to_ascii_lowercase(),
        write_id: write_id.to_string(),
        actor: actor.to_string(),
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    };
    let encoded = serde_json::to_string(&record)?;
    std::fs::write(&prov_path, encoded)
}

/// Return the latest write-provenance record for a document, if one exists.
pub fn write_provenance(file: &str) -> Option<WriteProvenance> {
    let path = write_provenance_path(file);
    let content = std::fs::read_to_string(path).ok()?;
    let record: WriteProvenance = serde_json::from_str(&content).ok()?;
    if record.path == file {
        Some(record)
    } else {
        None
    }
}

/// Clock skew tolerance (ms) when comparing the live-buffer sidecar timestamp
/// against the on-disk file mtime. A sidecar is only treated as stale when the
/// disk was modified at least this much later than the editor last reported, so
/// an editor digest stamped right after a near-simultaneous write is never
/// misclassified as stale.
const LIVE_BUFFER_STALE_SKEW_MS: u128 = 250;

/// Disk mtime (Unix ms) for a path, or `None` if it cannot be determined.
fn file_mtime_ms(file: &str) -> Option<u128> {
    let meta = std::fs::metadata(file).ok()?;
    let mtime = meta.modified().ok()?;
    Some(
        mtime
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
}

/// Return `Some(snapshot)` when the editor-visible buffer digest differs from
/// the supplied content. Returns `None` when there is no editor-visible sidecar,
/// when it matches the content, or when the sidecar is *stale* — i.e. the file
/// on disk was modified after the editor last reported its buffer.
///
/// Staleness check (#ipc-crdt-response-drift / visible-buffer false positives):
/// the editor plugin stamps the sidecar (`agent_doc_document_changed_digest`)
/// with the editor's buffer on every modification. A `len`/`hash` mismatch
/// against current disk has two causes: (1) the editor holds genuine *unsaved*
/// edits ahead of disk — protect those; or (2) a concurrent foreign writer (or
/// agent-doc's own machinery) changed disk *after* the editor last reported, so
/// the editor digest merely *lags* a disk change it has not observed yet — that
/// is not a user edit and must not block the write. Case (2) is the
/// false-positive class the user hit ("I did not edit the document"). We
/// distinguish them by timestamp: when the disk mtime is clearly newer than the
/// sidecar's `timestamp_ms`, the editor digest describes superseded content and
/// cannot represent unsaved edits against the *current* disk, so it is ignored.
pub fn live_buffer_diverges_from_content(file: &str, content: &str) -> Option<LiveBufferSnapshot> {
    live_buffer_snapshots(file)
        .into_iter()
        .filter(|snapshot| live_buffer_snapshot_diverges_from_content(file, snapshot, content))
        .max_by_key(|snapshot| snapshot.timestamp_ms)
}

pub fn live_buffer_divergence_missing_operator_text_authority(
    file: &str,
    content: &str,
) -> Option<LiveBufferSnapshot> {
    live_buffer_diverges_from_content(file, content)
        .filter(|snapshot| !snapshot.has_capability(OPERATOR_TEXT_AUTHORITY_CAPABILITY))
}

pub fn live_buffer_delivery_missing_operator_text_authority(
    file: &str,
    content: &str,
) -> Option<LiveBufferSnapshot> {
    live_buffer_snapshots(file)
        .into_iter()
        .filter(|snapshot| live_buffer_snapshot_can_receive_delivery(file, snapshot, content))
        .filter(|snapshot| !snapshot.has_capability(OPERATOR_TEXT_AUTHORITY_CAPABILITY))
        .max_by_key(|snapshot| snapshot.timestamp_ms)
}

fn live_buffer_snapshot_can_receive_delivery(
    file: &str,
    snapshot: &LiveBufferSnapshot,
    content: &str,
) -> bool {
    if let Some(editor_id) = snapshot.editor_id.as_deref()
        && !editor_id_is_live_for_delivery(editor_id)
    {
        return false;
    }

    let expected_len = content.len();
    let expected_hash = content_hash(content);
    if snapshot.len == expected_len && snapshot.hash.eq_ignore_ascii_case(&expected_hash) {
        return true;
    }

    live_buffer_snapshot_diverges_from_content(file, snapshot, content)
}

fn editor_id_is_live_for_delivery(editor_id: &str) -> bool {
    match jetbrains_editor_id_pid_for_delivery(editor_id) {
        Some(pid) => pid_is_live(pid),
        None => true,
    }
}

#[cfg(unix)]
fn pid_is_live(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_is_live(_pid: u32) -> bool {
    true
}

fn jetbrains_editor_id_pid_for_delivery(editor_id: &str) -> Option<u32> {
    let rest = editor_id.strip_prefix("jetbrains-")?;
    let pid_str = rest.split('-').next()?;
    if pid_str.is_empty() || !pid_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid_str.parse::<u32>().ok()
}

fn live_buffer_snapshot_diverges_from_content(
    file: &str,
    snapshot: &LiveBufferSnapshot,
    content: &str,
) -> bool {
    let expected_len = content.len();
    let expected_hash = content_hash(content);
    if snapshot.len == expected_len && snapshot.hash.eq_ignore_ascii_case(&expected_hash) {
        return false;
    }

    // #pcp6 content-based positive confirmation: when the plugin reported the
    // editor's full buffer text and it exactly equals the current on-disk
    // content, the editor holds no unsaved edits *ahead* of disk. A len/hash
    // mismatch against `content` (the caller's expected/merge baseline) is then a
    // stale comparison, not a live user edit. This is definitive (full content,
    // not the len/hash + mtime heuristics) and only suppresses when the editor
    // provably matches disk, so it can never mask a genuine unsaved edit.
    if let Some(ref editor_text) = snapshot.content
        && let Ok(disk) = std::fs::read_to_string(file)
        && *editor_text == disk
    {
        // #f5d2/#pcp6 prove/disprove: record which suppression branch decided the
        // editor holds no unsaved edit ahead of disk. Previously every suppression
        // returned None silently, so a "agent-doc didn't detect my unsaved edit"
        // bug report could not tell content-match from provenance from mtime-stale.
        log_live_buffer_decision(file, "suppressed", "editor_content_equals_disk");
        return false;
    }

    // #pcp2 write-provenance positive attribution: if agent-doc recorded a disk
    // write to this document *after* the editor last reported its buffer, the
    // editor digest lags agent-doc's own write — not a user edit — so suppress.
    // This uses agent-doc's own recorded write time (same SystemTime clock domain
    // as the editor's `timestamp_ms`), a definitive signal preferred over the
    // filesystem-mtime heuristic below. A genuine unsaved edit reports *after*
    // agent-doc's last write (`snapshot.timestamp_ms > prov.timestamp_ms`) and is
    // still protected. Falls through to the mtime fallback when no provenance
    // record exists (shadow-mode additive).
    if let Some(prov) = write_provenance(file)
        && prov.timestamp_ms > snapshot.timestamp_ms
    {
        log_live_buffer_decision_with_details(
            file,
            "suppressed",
            "write_provenance_newer_than_buffer",
            &format!(
                " provenance_actor={} provenance_write_id={} provenance_timestamp_ms={} buffer_timestamp_ms={} provenance_len={} provenance_hash={} buffer_len={} buffer_hash={} stale_owner={} stale_attempt={}",
                prov.actor,
                prov.write_id,
                prov.timestamp_ms,
                snapshot.timestamp_ms,
                prov.len,
                prov.hash,
                snapshot.len,
                snapshot.hash,
                prov.actor,
                prov.write_id
            ),
        );
        return false;
    }

    // Stale-sidecar suppression (fallback): if disk changed after the editor last
    // reported, the digest is lagging a disk write the editor has not seen — not a
    // live unsaved buffer. Only suppress on a confident staleness margin so genuine
    // unsaved edits (editor newer than disk) are still protected.
    if let Some(disk_mtime_ms) = file_mtime_ms(file)
        && disk_mtime_ms
            > snapshot
                .timestamp_ms
                .saturating_add(LIVE_BUFFER_STALE_SKEW_MS)
    {
        log_live_buffer_decision(file, "suppressed", "disk_mtime_stale_vs_buffer");
        return false;
    }

    log_live_buffer_decision(file, "diverges", "unsaved_buffer_ahead_of_disk");
    true
}

/// Best-effort prove/disprove diagnostic for the live-buffer divergence
/// classifier (`#f5d2` / `#pcp6`). Records the final decision and the branch
/// reason to `.agent-doc/logs/ops.log` so a live editor test can grep exactly
/// why a buffer was (or was not) treated as a pending unsaved edit. Logging
/// only; the return value of [`live_buffer_diverges_from_content`] is unchanged.
fn log_live_buffer_decision(file: &str, decision: &str, reason: &str) {
    log_live_buffer_decision_with_details(file, decision, reason, "");
}

fn log_live_buffer_decision_with_details(file: &str, decision: &str, reason: &str, details: &str) {
    log_live_buffer_op(
        Path::new(file),
        &format!("live_buffer_classify decision={decision} reason={reason} file={file}{details}"),
    );
}

fn log_live_buffer_op(file: &Path, message: &str) {
    let _ = try_log_live_buffer_op(file, message);
}

fn try_log_live_buffer_op(file: &Path, message: &str) -> Option<()> {
    let canonical = file.canonicalize().ok()?;
    let project_root = find_project_root(&canonical)?;
    let logs_dir = project_root.join(".agent-doc/logs");
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join("ops.log");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()?;
    writeln!(
        f,
        "[{}] {}",
        agent_doc_log_time::format_log_timestamp(ts),
        message
    )
    .ok()
}

fn find_project_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    loop {
        if current.join(".agent-doc").is_dir() || current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn live_buffer_snapshot_dir_and_stem(file: &str) -> (PathBuf, String) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let stem = format!("{:016x}", hash);
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return (dir.join(LIVE_BUFFER_DIR), stem);
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf();
            return (parent.join(LIVE_BUFFER_DIR), stem);
        }
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

fn live_buffer_snapshot_path_for_editor(file: &str, editor_id: Option<&str>) -> PathBuf {
    let (dir, stem) = live_buffer_snapshot_dir_and_stem(file);
    let Some(editor_id) = editor_id
        .map(str::trim)
        .filter(|editor_id| !editor_id.is_empty())
    else {
        return dir.join(stem);
    };
    dir.join(format!(
        "{}.{}",
        stem,
        sanitize_editor_id_for_filename(editor_id)
    ))
}

fn sanitize_editor_id_for_filename(editor_id: &str) -> String {
    let sanitized: String = editor_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "editor".to_string()
    } else {
        sanitized
    }
}

/// Compute the write-provenance file path for a document (#pcp2). Mirrors
/// `live_buffer_snapshot_path` so the provenance sidecar lands in the same
/// `.agent-doc/` directory the live-buffer digest uses.
fn write_provenance_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir
                .join(WRITE_PROVENANCE_DIR)
                .join(format!("{:016x}", hash));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf();
            return parent
                .join(WRITE_PROVENANCE_DIR)
                .join(format!("{:016x}", hash));
        }
    }
}

/// Tri-state status of a document's cross-process editor typing indicator.
///
/// Distinguishes "no editor is tracking this document" (`Absent`) from "an
/// editor tracked typing and it has settled" (`Idle`), which `is_typing_via_file`
/// collapses into the same `false`. Route uses this distinction to skip the
/// redundant mtime settle when an editor already owns the typing lifecycle
/// (`#jb-run-agent-doc-double-debounce`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypingIndicatorStatus {
    /// No typing indicator file exists (or it is malformed) — no editor is
    /// tracking this document, so the indicator proves nothing.
    Absent,
    /// Indicator exists and was updated within `debounce_ms` — user is typing.
    Active,
    /// Indicator exists but is older than `debounce_ms` — the editor tracked
    /// typing and it has since settled.
    Idle,
}

/// Classify the cross-process typing indicator for a document.
///
/// Reads `.agent-doc/typing/<hash>` and compares its timestamp against the
/// debounce window. Used by callers that need to tell an idle-but-present
/// editor indicator apart from a missing one.
pub fn typing_indicator_status(file: &str, debounce_ms: u64) -> TypingIndicatorStatus {
    let path = typing_indicator_path(file);
    match std::fs::read_to_string(&path) {
        Ok(content) => match content.trim().parse::<u128>() {
            Ok(ts_ms) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                if now.saturating_sub(ts_ms) < debounce_ms as u128 {
                    TypingIndicatorStatus::Active
                } else {
                    TypingIndicatorStatus::Idle
                }
            }
            // Malformed indicator proves nothing — treat as absent.
            Err(_) => TypingIndicatorStatus::Absent,
        },
        Err(_) => TypingIndicatorStatus::Absent, // No indicator file — not tracked
    }
}

/// Check if the document has a recent typing indicator (cross-process).
///
/// Returns `true` if the typing indicator exists and was updated within
/// `debounce_ms` milliseconds. Used by CLI preflight to detect active typing
/// from a plugin running in a different process.
pub fn is_typing_via_file(file: &str, debounce_ms: u64) -> bool {
    typing_indicator_status(file, debounce_ms) == TypingIndicatorStatus::Active
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

    fn wait_for_typing_indicator(file: &str, debounce_ms: u64) {
        for _ in 0..50 {
            if is_typing_via_file(file, debounce_ms) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("typing indicator was not written for {file}");
    }

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
        wait_for_typing_indicator(&doc_str, 2000);
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
    fn live_buffer_epoch_reports_in_flight_until_disk_matches_editor_buffer() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("epoch-barrier.md");
        std::fs::write(&doc, "disk").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        document_changed_with_content_for_editor(&doc_str, "disk", Some("jetbrains:test"));
        let first = live_buffer_snapshot(&doc_str).expect("first snapshot");
        assert_eq!(first.edit_epoch, 1);

        let ready = editor_sync_statuses(&doc_str);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].edit_epoch, 1);
        assert_eq!(ready[0].effective_last_synced_epoch, 1);
        assert!(ready[0].disk_matches);
        assert!(!ready[0].in_flight);

        let unsaved = "disk plus unsaved editor text";
        document_changed_with_content_for_editor(&doc_str, unsaved, Some("jetbrains:test"));
        let second = live_buffer_snapshot(&doc_str).expect("second snapshot");
        assert_eq!(second.edit_epoch, 2);

        let blocked = editor_sync_statuses(&doc_str);
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].edit_epoch, 2);
        assert_eq!(blocked[0].effective_last_synced_epoch, 0);
        assert!(!blocked[0].disk_matches);
        assert!(blocked[0].in_flight);

        std::fs::write(&doc, unsaved).unwrap();
        let flushed = editor_sync_statuses(&doc_str);
        assert_eq!(flushed[0].effective_last_synced_epoch, 2);
        assert!(flushed[0].disk_matches);
        assert!(!flushed[0].in_flight);
    }

    #[test]
    fn editor_sync_barrier_times_out_until_editor_buffer_is_flushed() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("barrier-timeout.md");
        std::fs::write(&doc, "saved").unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let unsaved = "saved plus unsaved editor text";

        document_changed_with_content_for_editor(&doc_str, unsaved, Some("vscode:test"));

        let start = Instant::now();
        let timed_out = await_editor_sync_barrier(&doc_str, 0, 30);
        assert_eq!(timed_out.kind, EditorSyncBarrierKind::TimedOut);
        assert!(start.elapsed().as_millis() >= 30);
        assert!(timed_out.statuses.iter().any(|status| status.in_flight));

        std::fs::write(&doc, unsaved).unwrap();
        let ready = await_editor_sync_barrier(&doc_str, 0, 100);
        assert_eq!(ready.kind, EditorSyncBarrierKind::Ready);
        assert!(ready.statuses.iter().all(|status| !status.in_flight));
    }

    /// Editor-close lifecycle: `clear_live_buffer` removes the sidecar so the
    /// cycle falls back to disk, and a clear with no sidecar present is success.
    #[test]
    fn clear_live_buffer_removes_sidecar_and_is_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("clear-live-buffer.md");
        std::fs::write(&doc, "disk").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        record_live_buffer_digest_content(&doc_str, "disk plus unsaved prompt").unwrap();
        assert!(live_buffer_snapshot(&doc_str).is_some(), "sidecar recorded");

        clear_live_buffer(&doc_str).expect("clear removes the sidecar");
        assert!(
            live_buffer_snapshot(&doc_str).is_none(),
            "after clear the editor is absent and disk is the only source"
        );
        // Idempotent: clearing an already-absent sidecar is success, not an error.
        clear_live_buffer(&doc_str).expect("clear with no sidecar is a no-op success");
    }

    /// `#f5d2`/`#pcp6` prove/disprove: the divergence classifier records its
    /// decision + branch reason to ops.log so a live editor test can grep exactly
    /// why a buffer was treated as a pending unsaved edit. Drives the positive
    /// `diverges` outcome and asserts the marker lands.
    #[test]
    fn live_buffer_classify_logs_diverges_decision() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("classify-log.md");
        std::fs::write(&doc, "disk").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        // Editor reports a buffer ahead of disk (genuine unsaved edit).
        let visible = "disk plus unsaved prompt";
        document_changed_with_digest(&doc_str, visible.len(), &content_hash(visible));

        // Compare against the on-disk content: buffer is ahead → diverges.
        assert!(live_buffer_diverges_from_content(&doc_str, "disk").is_some());

        let ops_log = tmp.path().join(".agent-doc").join("logs").join("ops.log");
        let log = std::fs::read_to_string(&ops_log).expect("ops.log written");
        assert!(
            log.contains(
                "live_buffer_classify decision=diverges reason=unsaved_buffer_ahead_of_disk"
            ),
            "ops.log missing diverges marker, got: {log}"
        );
    }

    /// Regression (#ipc-crdt-response-drift / visible-buffer false positives):
    /// when the file on disk is modified *after* the editor last reported its
    /// buffer (a concurrent foreign writer, or agent-doc's own machinery), the
    /// sidecar is stale — it lags a disk change the editor never made — and must
    /// NOT be reported as a divergence. The user did not edit the document.
    #[test]
    fn live_buffer_stale_sidecar_lagging_disk_write_is_not_divergence() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("stale-sidecar.md");
        std::fs::write(&doc, "original disk content").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        // Editor reports its buffer (matching the then-current disk).
        let reported = "original disk content";
        document_changed_with_digest(&doc_str, reported.len(), &content_hash(reported));

        // A concurrent foreign writer grows the file AFTER the editor's report
        // (past the skew margin). The editor never saw this change.
        std::thread::sleep(std::time::Duration::from_millis(
            (LIVE_BUFFER_STALE_SKEW_MS as u64) + 100,
        ));
        let foreign = "original disk content\nappended by a foreign supervisor\n";
        std::fs::write(&doc, foreign).unwrap();

        // The sidecar (len/hash of `reported`) differs from current disk
        // (`foreign`), but it is stale — must be suppressed, not fired.
        assert!(
            live_buffer_diverges_from_content(&doc_str, foreign).is_none(),
            "stale sidecar lagging a foreign disk write was wrongly reported as a live divergence"
        );
    }

    /// Complement: a genuinely fresh sidecar (editor reported AFTER the last
    /// disk write) with unsaved edits ahead of disk must still be protected.
    #[test]
    fn live_buffer_fresh_sidecar_with_unsaved_edits_still_diverges() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("fresh-sidecar.md");
        std::fs::write(&doc, "saved disk content").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        // Editor reports unsaved edits ahead of disk, AFTER the disk write.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let unsaved = "saved disk content plus a real unsaved user edit";
        document_changed_with_digest(&doc_str, unsaved.len(), &content_hash(unsaved));

        // Disk still holds the saved content; the fresh sidecar diverges and
        // must be protected (Some).
        assert!(
            live_buffer_diverges_from_content(&doc_str, "saved disk content").is_some(),
            "fresh sidecar with genuine unsaved edits was wrongly suppressed"
        );
    }

    #[test]
    fn write_provenance_roundtrips() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("prov-roundtrip.md");
        std::fs::write(&doc, "body").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        assert!(write_provenance(&doc_str).is_none());
        record_write_provenance(&doc_str, 4, &content_hash("body"), "wid-1", "agent").unwrap();
        let prov = write_provenance(&doc_str).expect("provenance recorded");
        assert_eq!(prov.path, doc_str);
        assert_eq!(prov.len, 4);
        assert_eq!(prov.hash, content_hash("body"));
        assert_eq!(prov.write_id, "wid-1");
        assert_eq!(prov.actor, "agent");
    }

    /// #pcp2: write-provenance positively attributes a stale editor digest to
    /// agent-doc's own write even when the filesystem-mtime fallback would NOT
    /// suppress it. The editor reported an old buffer; agent-doc then recorded a
    /// disk write *after* that report. The disk file itself is not touched after
    /// the editor report, so the mtime heuristic alone leaves the digest firing —
    /// provenance is what makes it suppress.
    #[test]
    fn write_provenance_suppresses_stale_digest_beyond_mtime_heuristic() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("prov-stale.md");
        std::fs::write(&doc, "agent-doc wrote this").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        // Editor reports an old buffer (the disk write at creation predates this
        // report, so the mtime fallback cannot suppress: disk is older, not newer).
        std::thread::sleep(std::time::Duration::from_millis(5));
        let reported = "buffer the editor still shows";
        document_changed_with_digest(&doc_str, reported.len(), &content_hash(reported));

        // Without provenance, the digest fires (mtime fallback does not suppress).
        assert!(
            live_buffer_diverges_from_content(&doc_str, "agent-doc wrote this").is_some(),
            "precondition: without provenance the stale digest should fire"
        );

        // agent-doc records its own write AFTER the editor's report.
        std::thread::sleep(std::time::Duration::from_millis(5));
        record_write_provenance(
            &doc_str,
            "agent-doc wrote this".len(),
            &content_hash("agent-doc wrote this"),
            "wid-stale",
            "agent",
        )
        .unwrap();

        // Provenance positively attributes the stale digest to agent-doc's write.
        assert!(
            live_buffer_diverges_from_content(&doc_str, "agent-doc wrote this").is_none(),
            "write provenance should suppress a digest that predates agent-doc's own write"
        );
        let ops_log = tmp.path().join(".agent-doc").join("logs").join("ops.log");
        let log = std::fs::read_to_string(&ops_log).expect("ops.log written");
        assert!(
            log.contains("reason=write_provenance_newer_than_buffer"),
            "ops.log missing provenance suppression marker, got: {log}"
        );
        assert!(log.contains("provenance_actor=agent"), "{log}");
        assert!(log.contains("provenance_write_id=wid-stale"), "{log}");
        assert!(log.contains("stale_owner=agent"), "{log}");
        assert!(log.contains("stale_attempt=wid-stale"), "{log}");
        assert!(log.contains("buffer_timestamp_ms="), "{log}");
        assert!(log.contains("provenance_timestamp_ms="), "{log}");
    }

    /// #pcp2 complement: provenance must NOT suppress a genuine unsaved editor
    /// edit. When the editor reports its buffer AFTER agent-doc's last recorded
    /// write, the digest represents real unsaved edits ahead of disk and stays
    /// protected.
    #[test]
    fn write_provenance_protects_genuine_unsaved_edit() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("prov-fresh.md");
        std::fs::write(&doc, "saved").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        // agent-doc records its write first.
        record_write_provenance(&doc_str, 5, &content_hash("saved"), "wid-fresh", "agent").unwrap();

        // Editor reports unsaved edits AFTER agent-doc's write.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let unsaved = "saved plus a genuine unsaved edit";
        document_changed_with_digest(&doc_str, unsaved.len(), &content_hash(unsaved));

        assert!(
            live_buffer_diverges_from_content(&doc_str, "saved").is_some(),
            "genuine unsaved edit reported after agent-doc's write must stay protected"
        );
    }

    #[test]
    fn live_buffer_content_digest_roundtrips() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("content-roundtrip.md");
        std::fs::write(&doc, "hello").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        document_changed_with_content(&doc_str, "hello");
        let snap = live_buffer_snapshot(&doc_str).expect("snapshot recorded");
        assert_eq!(snap.content.as_deref(), Some("hello"));
        assert_eq!(snap.len, 5);
        assert_eq!(snap.hash, content_hash("hello"));
    }

    #[test]
    fn live_buffer_capability_metadata_roundtrips_and_legacy_report_clears_authority() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("cap-roundtrip.md");
        std::fs::write(&doc, "hello").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            "hello",
            "jetbrains-7-test",
            "jetbrains",
            "0.2.197",
            &[OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();
        let snap = live_buffer_snapshot(&doc_str).expect("snapshot recorded");
        assert_eq!(snap.editor_id.as_deref(), Some("jetbrains-7-test"));
        assert_eq!(snap.editor_kind.as_deref(), Some("jetbrains"));
        assert_eq!(snap.editor_version.as_deref(), Some("0.2.197"));
        assert!(snap.has_capability(OPERATOR_TEXT_AUTHORITY_CAPABILITY));

        record_live_buffer_digest_content_for_editor(
            &doc_str,
            "hello again",
            Some("jetbrains-7-test"),
        )
        .unwrap();
        let snap = live_buffer_snapshot(&doc_str).expect("snapshot recorded");
        assert_eq!(snap.editor_kind, None);
        assert_eq!(snap.editor_version, None);
        assert!(
            !snap.has_capability(OPERATOR_TEXT_AUTHORITY_CAPABILITY),
            "a capability-less legacy report must not inherit stale authority proof"
        );
    }

    #[test]
    fn diverged_live_buffer_without_operator_authority_capability_is_detected() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("cap-missing.md");
        std::fs::write(&doc, "saved").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        record_live_buffer_digest_content_for_editor(
            &doc_str,
            "saved plus operator text",
            Some("jetbrains-old"),
        )
        .unwrap();

        let snap = live_buffer_divergence_missing_operator_text_authority(&doc_str, "saved")
            .expect("old sidecar diverges without authority capability");
        assert_eq!(snap.editor_id.as_deref(), Some("jetbrains-old"));
        assert!(!snap.has_capability(OPERATOR_TEXT_AUTHORITY_CAPABILITY));
    }

    #[test]
    fn matching_live_buffer_without_operator_authority_capability_blocks_delivery() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("cap-missing-clean.md");
        std::fs::write(&doc, "saved").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        record_live_buffer_digest_content_for_editor(&doc_str, "saved", Some("jetbrains-old"))
            .unwrap();

        let snap = live_buffer_delivery_missing_operator_text_authority(&doc_str, "saved")
            .expect("old matching live editor sidecar still lacks safe delivery proof");
        assert_eq!(snap.editor_id.as_deref(), Some("jetbrains-old"));
        assert!(!snap.has_capability(OPERATOR_TEXT_AUTHORITY_CAPABILITY));
    }

    #[test]
    fn diverged_live_buffer_with_operator_authority_capability_is_not_missing_authority() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("cap-present.md");
        std::fs::write(&doc, "saved").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            "saved plus operator text",
            "jetbrains-new",
            "jetbrains",
            "0.2.197",
            &[OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        assert!(
            live_buffer_divergence_missing_operator_text_authority(&doc_str, "saved").is_none(),
            "capable sidecar may be merged through the realtime operator-authoritative path"
        );
    }

    /// #pcp6: when the editor reports its full buffer content and it exactly
    /// equals on-disk content, the editor holds no unsaved edits — divergence is
    /// suppressed even against a different caller `content` baseline, and even
    /// when len/hash differ from that baseline.
    #[test]
    fn live_buffer_content_matching_disk_suppresses_divergence() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("content-match.md");
        std::fs::write(&doc, "saved disk content").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        // Editor reports its buffer == current disk content.
        document_changed_with_content(&doc_str, "saved disk content");

        // The caller's expected/merge baseline differs (stale), so len/hash do not
        // match it — but the editor provably matches disk, so no live divergence.
        assert!(
            live_buffer_diverges_from_content(&doc_str, "a stale expected merge baseline")
                .is_none(),
            "editor buffer equal to disk must suppress divergence regardless of the caller baseline"
        );
    }

    /// #pcp6 complement: when the editor's reported content is ahead of disk
    /// (genuine unsaved edit), the content path must NOT suppress — the edit stays
    /// protected.
    #[test]
    fn live_buffer_content_ahead_of_disk_still_diverges() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();
        let doc = tmp.path().join("content-ahead.md");
        std::fs::write(&doc, "saved disk content").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        // Editor holds unsaved edits ahead of disk.
        document_changed_with_content(&doc_str, "saved disk content plus a real unsaved edit");

        // Disk still holds the saved content; the editor content != disk, so the
        // #pcp6 path does not fire and the genuine edit stays protected.
        assert!(
            live_buffer_diverges_from_content(&doc_str, "saved disk content").is_some(),
            "genuine unsaved edit ahead of disk must not be suppressed by the content path"
        );
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
        wait_for_typing_indicator(&doc_str, 2000);
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
        wait_for_typing_indicator(&doc1_str, 2000);

        document_changed(&doc2_str);
        let path2 = typing_indicator_path(&doc2_str);
        wait_for_typing_indicator(&doc2_str, 2000);

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
        wait_for_typing_indicator(&doc_str, 2000);

        // is_typing_via_file accepts debounce_ms as parameter — good
        assert!(is_typing_via_file(&doc_str, 2000));
        assert!(is_typing_via_file(&doc_str, 100));

        // await_idle_via_file also accepts debounce_ms — configurable
        write_typing_indicator(&doc_str).unwrap();
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
        wait_for_typing_indicator(&doc_str, 2000);
        write_typing_indicator(&doc_str).unwrap();

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
        wait_for_typing_indicator(&doc_str, 2000);

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
        wait_for_typing_indicator(&doc_str, 2000);

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

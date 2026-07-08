//! # Module: ffi
//!
//! ## Spec
//! - Exports a C ABI (`extern "C"`) surface consumed by the JetBrains plugin via JNA and the VS
//!   Code extension via Node native addons, eliminating duplicated parsing/merge logic in Kotlin
//!   and TypeScript.
//! - `agent_doc_parse_components(doc)`: parses all `<!-- agent:name -->` components and returns a
//!   JSON-encoded array with fields `name`, `attrs`, `open_start`, `open_end`, `close_start`,
//!   `close_end`, `content`.
//! - `agent_doc_visual_tokens_json(doc)`: returns a JSON-encoded array of visual token ranges used
//!   by editor plugins to highlight agent-doc-specific markdown structures consistently. Exported
//!   offsets are UTF-16 document positions so editor APIs can consume them directly.
//! - `agent_doc_apply_patch(doc, component_name, content, mode)`: applies a patch to a named
//!   component using `replace`, `append`, or `prepend` mode.
//! - `agent_doc_apply_patch_with_caret(…, caret_offset)`: caret-aware append — inserts content at
//!   the line boundary before the caret when `mode == "append"` and `caret_offset >= 0`.  Falls
//!   back to `apply_patch` otherwise.
//! - `agent_doc_apply_patch_with_boundary(…, boundary_id)`: boundary-marker-aware append — inserts
//!   content at the named boundary marker position, then falls back to `apply_patch` if the marker
//!   is not found.
//! - `agent_doc_crdt_merge(base_state, base_state_len, ours, theirs)`: 3-way CRDT merge; `base_state`
//!   may be null for the first merge.  Returns merged text and updated opaque CRDT state bytes.
//! - `agent_doc_merge_frontmatter(doc, yaml_fields)`: merges YAML key/value pairs into the
//!   document's frontmatter additively (never removes keys).
//! - `agent_doc_reposition_boundary_to_end(doc)`: removes all existing boundary markers and inserts
//!   a single fresh one at the end of the `exchange` component.
//! - `agent_doc_document_changed(file_path)`: records a file-backed typing projection.
//! - `agent_doc_document_changed_digest(file_path, len, hash)`: records typing telemetry. Current
//!   editor content is resolved through the CRDT relay, not a file-backed live-buffer sidecar.
//! - `agent_doc_report_editor_state(file_path, version, dirty, last_edit_ts, save_ts, hash, len, session_id)`:
//!   records editor-authoritative buffer state for debounce/stability. When present,
//!   `wait_for_stable_content` uses version/hash stability instead of the truncation heuristic.
//! - `agent_doc_get_editor_state(file_path)`: returns the current editor buffer state as JSON.
//! - `agent_doc_is_editor_stable(file_path, debounce_ms)`: returns whether the editor buffer is
//!   stable (not dirty or debounce elapsed).
//! - `agent_doc_is_typing_via_file(file_path, debounce_ms)`: cross-process check — reads the
//!   file-based typing indicator written by `document_changed`; `true` if updated within
//!   `debounce_ms`.  For CLI tools running in a separate process from the editor plugin.
//! - `agent_doc_await_idle_via_file(file_path, debounce_ms, timeout_ms)`: blocking variant of
//!   `is_typing_via_file`; polls until idle or `timeout_ms` expires.
//! - `agent_doc_admin_*_json(...)`: controller-backed admin/editor wrappers for inspect, queue
//!   pause/resume/drain, handoff, reap, and projection repair. They return the same JSON receipt
//!   envelopes as the CLI `--json` forms.
//! - `agent_doc_free_string(ptr)` / `agent_doc_free_state(ptr, len)`: free memory returned by any
//!   `agent_doc_*` function.  Must be called for every non-null pointer.
//!
//! ## Agentic Contracts
//! - All string parameters must be valid, non-null, NUL-terminated UTF-8; violation is UB.
//! - Every non-null `text` or `error` pointer in a result struct must be freed exactly once with
//!   `agent_doc_free_string`; CRDT `state` pointers must be freed with `agent_doc_free_state`.
//! - On parse/apply errors, `text` (or `json`) is null and `error` holds a message; callers must
//!   check nullability before use.
//! - `agent_doc_await_idle_via_file` returning `false` means the timeout expired — the caller must
//!   not proceed with the agent run.
//!
//! ## Evals
//! - parse_components_roundtrip: single `agent:status` component → JSON count=1, content="hello\n"
//! - apply_patch_replace: replace mode on `agent:output` → new content present, old content absent
//! - merge_frontmatter_adds_field: add `model: opus` to existing frontmatter → both keys present, body unchanged
//! - reposition_boundary_removes_stale: two boundary markers in exchange → exactly one marker at end
//! - crdt_merge_no_base: identical `ours`/`theirs` with null base → merged text equals input

use agent_doc_state_backbone::{EventLedger, StateEvent, StateFact};
use anyhow::Context as _;
use serde::Serialize;
use std::ffi::{CStr, CString, c_char, c_int};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cross-editor sync lock — prevents concurrent layout syncs.
static SYNC_LOCKED: AtomicBool = AtomicBool::new(false);

/// Sync debounce generation counter — only the latest scheduled sync fires.
static SYNC_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Epoch-millis when the current [`SYNC_LOCKED`] holder acquired the guard (`0` = no
/// holder). Lets a later acquire detect a guard held past the stale bound — a wedged or
/// dead holder that never called `agent_doc_sync_unlock` (`#recyclerestart` Q2) — and
/// supersede it instead of deferring forever with "another sync is already running".
static SYNC_LOCK_ACQUIRED_AT_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Default stale bound for the cross-editor sync guard. Sized above the JetBrains
/// plugin's `SYNC_PROCESS_TIMEOUT_MS` (30s) plus margin so a legitimately in-flight sync
/// is never superseded — only a guard held past this bound (a wedged/dead holder) is.
pub const DEFAULT_SYNC_LOCK_STALE_BOUND_MS: u64 = 45_000;

/// `#recyclerestart` Q2 — pure decision for acquiring the cross-editor sync guard, split
/// out so the self-heal is unit-testable without real threads/clocks. A free guard is
/// acquired; a guard held past `stale_bound_ms` by a known-age holder is superseded
/// (the prior holder wedged and never released); an unknown-age (`acquired_at_ms == 0`)
/// or still-fresh holder is deferred so a legitimately in-flight sync keeps the guard.
#[derive(Debug, PartialEq, Eq)]
pub enum SyncLockDecision {
    Acquire,
    SupersedeStaleHolder { held_ms: u64 },
    Defer,
}

pub fn sync_lock_acquire_decision(
    currently_locked: bool,
    acquired_at_ms: u64,
    now_ms: u64,
    stale_bound_ms: u64,
) -> SyncLockDecision {
    if !currently_locked {
        return SyncLockDecision::Acquire;
    }
    let held_ms = now_ms.saturating_sub(acquired_at_ms);
    if acquired_at_ms != 0 && held_ms >= stale_bound_ms {
        SyncLockDecision::SupersedeStaleHolder { held_ms }
    } else {
        SyncLockDecision::Defer
    }
}

fn sync_lock_now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sync_try_lock_with_bound(stale_bound_ms: u64) -> i32 {
    let now = sync_lock_now_epoch_ms();
    // Fast path: take a free guard.
    if SYNC_LOCKED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        SYNC_LOCK_ACQUIRED_AT_MS.store(now, Ordering::SeqCst);
        return 1;
    }
    // Held: self-heal a guard wedged past the stale bound so it can never block every
    // later sync (`#recyclerestart` Q2). The CLI keeps its own `.agent-doc/sync.lock`
    // file-lock with stale-orphan reaping, so superseding here cannot double-run a sync.
    let acquired_at = SYNC_LOCK_ACQUIRED_AT_MS.load(Ordering::SeqCst);
    match sync_lock_acquire_decision(true, acquired_at, now, stale_bound_ms) {
        SyncLockDecision::SupersedeStaleHolder { held_ms } => {
            SYNC_LOCK_ACQUIRED_AT_MS.store(now, Ordering::SeqCst);
            eprintln!(
                "[sync] sync_guard_released reason=stale_holder_superseded held_ms={held_ms} bound_ms={stale_bound_ms}"
            );
            1
        }
        SyncLockDecision::Defer => 0,
        // Raced free between the CAS and the load — take it if still free.
        SyncLockDecision::Acquire => {
            if SYNC_LOCKED
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                SYNC_LOCK_ACQUIRED_AT_MS.store(now, Ordering::SeqCst);
                1
            } else {
                0
            }
        }
    }
}

/// Result of [`agent_doc_resolve_project_path`].
#[repr(C)]
pub struct FfiProjectPath {
    /// Absolute path to the project root (nearest ancestor containing `.agent-doc/`),
    /// or null if no ancestor has `.agent-doc/`. Free with [`agent_doc_free_string`].
    pub project_root: *mut c_char,
    /// Path to the input file, relative to `project_root`. Null when `project_root`
    /// is null. Free with [`agent_doc_free_string`].
    pub relative_path: *mut c_char,
}

/// JSON result returned by controller-backed editor/admin FFI wrappers.
#[repr(C)]
pub struct FfiJsonResult {
    /// JSON object on success, or null on error. Free with [`agent_doc_free_string`].
    pub json: *mut c_char,
    /// Error message when `json` is null. Free with [`agent_doc_free_string`].
    pub error: *mut c_char,
}

fn ffi_json_ok<T: Serialize>(value: &T) -> FfiJsonResult {
    match serde_json::to_string(value) {
        Ok(json) => FfiJsonResult {
            json: CString::new(json).unwrap_or_default().into_raw(),
            error: ptr::null_mut(),
        },
        Err(err) => ffi_json_err(&format!("failed to serialize JSON result: {err}")),
    }
}

fn ffi_json_err(message: &str) -> FfiJsonResult {
    FfiJsonResult {
        json: ptr::null_mut(),
        error: CString::new(message).unwrap_or_default().into_raw(),
    }
}

fn ffi_json_from_result<T: Serialize>(result: anyhow::Result<T>) -> FfiJsonResult {
    match result {
        Ok(value) => ffi_json_ok(&value),
        Err(err) => ffi_json_err(&format!("{err:#}")),
    }
}

unsafe fn optional_ffi_string(ptr: *const c_char, name: &str) -> anyhow::Result<Option<String>> {
    if ptr.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .with_context(|| format!("{name} is not valid UTF-8"))?
        .trim();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

unsafe fn required_ffi_string(ptr: *const c_char, name: &str) -> anyhow::Result<String> {
    unsafe { optional_ffi_string(ptr, name) }?.with_context(|| format!("{name} is required"))
}

fn optional_generation(value: i64, name: &str) -> anyhow::Result<Option<u64>> {
    if value < 0 {
        return Ok(None);
    }
    u64::try_from(value)
        .map(Some)
        .with_context(|| format!("{name} is out of range"))
}

fn required_generation(value: i64, name: &str) -> anyhow::Result<u64> {
    optional_generation(value, name)?.with_context(|| format!("{name} is required"))
}

fn optional_path(value: Option<String>) -> Option<PathBuf> {
    value.map(PathBuf::from)
}

fn resolve_admin_root(
    project_root: Option<&str>,
    document: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(PathBuf::from(root));
    }
    if let Some(document) = document
        && let Some(root) = agent_doc_fs::find_project_root(document)
    {
        return Ok(root);
    }
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    agent_doc_fs::find_project_root(&cwd)
        .with_context(|| format!("no .agent-doc project root found from {}", cwd.display()))
}

/// Record a file-backed typing projection.
///
/// Plugins call this on document modifications as telemetry for Project
/// Controller-owned
/// recovery paths. It is not authoritative editor state.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_changed(file_path: *const c_char) {
    if let Ok(path) = unsafe { CStr::from_ptr(file_path) }.to_str() {
        agent_doc_debounce::document_changed(path);
    }
}

/// Record a document change event plus legacy digest arguments.
///
/// Modern editor integrations publish current content through the CRDT replica
/// relay. The legacy digest arguments are accepted for ABI compatibility, but no
/// `.agent-doc/live-buffer` sidecar is written from this path.
///
/// # Safety
///
/// `file_path` and `content_hash` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_changed_digest(
    file_path: *const c_char,
    content_len: i64,
    content_hash: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let _hash = match unsafe { CStr::from_ptr(content_hash) }.to_str() {
        Ok(hash) => hash,
        Err(_) => return,
    };
    let Ok(_len) = usize::try_from(content_len) else {
        return;
    };
    agent_doc_debounce::document_changed(path);
}

/// Record a document change plus digest for one editor instance.
///
/// # Safety
///
/// `file_path`, `content_hash`, and `editor_id` must be valid, NUL-terminated
/// UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_changed_digest_for_editor(
    file_path: *const c_char,
    content_len: i64,
    content_hash: *const c_char,
    editor_id: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let _hash = match unsafe { CStr::from_ptr(content_hash) }.to_str() {
        Ok(hash) => hash,
        Err(_) => return,
    };
    let _editor = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor) => editor,
        Err(_) => return,
    };
    let Ok(_len) = usize::try_from(content_len) else {
        return;
    };
    agent_doc_debounce::document_changed(path);
}

/// Record a document change plus legacy full-buffer content.
///
/// The buffer text is consumed only for compatibility with older editor flows.
/// Current editor authority is the CRDT relay, so this path records typing
/// telemetry and does not persist the content to `.agent-doc/live-buffer`.
///
/// # Safety
///
/// `file_path` and `content` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_changed_digest_content(
    file_path: *const c_char,
    content: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let _text = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(text) => text,
        Err(_) => return,
    };
    agent_doc_debounce::document_changed(path);
}

/// Record a document change plus legacy full visible buffer content for one
/// editor instance.
///
/// Modern editor integrations publish content by applying local edits to their
/// CRDT replica and sending `replica_update` through the Project Controller
/// relay. This ABI remains as a wake/telemetry compatibility hook only; it must
/// not write a separate editor-content channel.
///
/// # Safety
///
/// `file_path`, `content`, and `editor_id` must be valid, NUL-terminated UTF-8
/// strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_changed_digest_content_for_editor(
    file_path: *const c_char,
    content: *const c_char,
    editor_id: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let _text = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(text) => text,
        Err(_) => return,
    };
    let _editor = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor) => editor,
        Err(_) => return,
    };
    agent_doc_debounce::document_changed(path);
}

/// Record a document change plus full visible buffer content for one editor
/// instance, including frontend metadata/capabilities.
///
/// `capabilities_csv` is a comma-separated list of stable capability tokens.
/// Unknown/empty tokens are ignored. Old plugins use
/// [`agent_doc_document_changed_digest_content_for_editor`] and therefore record
/// no capability proof.
///
/// # Safety
///
/// `file_path`, `content`, `editor_id`, `editor_kind`, `editor_version`, and
/// `capabilities_csv` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_changed_digest_content_for_editor_v2(
    file_path: *const c_char,
    content: *const c_char,
    editor_id: *const c_char,
    editor_kind: *const c_char,
    editor_version: *const c_char,
    capabilities_csv: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let text = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(text) => text,
        Err(_) => return,
    };
    let editor = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor) => editor,
        Err(_) => return,
    };
    let kind = match unsafe { CStr::from_ptr(editor_kind) }.to_str() {
        Ok(kind) => kind,
        Err(_) => return,
    };
    let version = match unsafe { CStr::from_ptr(editor_version) }.to_str() {
        Ok(version) => version,
        Err(_) => return,
    };
    let capabilities_raw = match unsafe { CStr::from_ptr(capabilities_csv) }.to_str() {
        Ok(capabilities) => capabilities,
        Err(_) => return,
    };
    let capabilities: Vec<&str> = capabilities_raw
        .split(',')
        .map(str::trim)
        .filter(|capability| !capability.is_empty())
        .collect();
    let _ = agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
        path,
        text,
        editor,
        kind,
        version,
        &capabilities,
    );
    agent_doc_debounce::document_changed(path);
}

/// Record a document change plus full visible buffer content for one editor
/// instance, with capabilities AND #falsetyping-guard replica-churn provenance.
///
/// Identical to [`agent_doc_document_changed_digest_content_for_editor_v2`] but
/// carries `no_unsaved_operator_edits`: pass `1` when the editor has proven the
/// buffer holds no unsaved *local operator* edits ahead of disk (any divergence
/// is replica-driven — a `remoteCrdtApply`), so the binary re-merges on replica
/// churn instead of failing the visible-write guard closed. Pass `0` (the
/// conservative default older plugins imply via the `_v2` entrypoint) when there
/// may be unsaved operator text, which keeps operator text authoritative by
/// failing closed.
///
/// # Safety
///
/// `file_path`, `content`, `editor_id`, `editor_kind`, `editor_version`, and
/// `capabilities_csv` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_changed_digest_content_for_editor_v3(
    file_path: *const c_char,
    content: *const c_char,
    editor_id: *const c_char,
    editor_kind: *const c_char,
    editor_version: *const c_char,
    capabilities_csv: *const c_char,
    no_unsaved_operator_edits: i32,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let text = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(text) => text,
        Err(_) => return,
    };
    let editor = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor) => editor,
        Err(_) => return,
    };
    let kind = match unsafe { CStr::from_ptr(editor_kind) }.to_str() {
        Ok(kind) => kind,
        Err(_) => return,
    };
    let version = match unsafe { CStr::from_ptr(editor_version) }.to_str() {
        Ok(version) => version,
        Err(_) => return,
    };
    let capabilities_raw = match unsafe { CStr::from_ptr(capabilities_csv) }.to_str() {
        Ok(capabilities) => capabilities,
        Err(_) => return,
    };
    let capabilities: Vec<&str> = capabilities_raw
        .split(',')
        .map(str::trim)
        .filter(|capability| !capability.is_empty())
        .collect();
    let _ = agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities_v2(
        path,
        text,
        editor,
        kind,
        version,
        &capabilities,
        no_unsaved_operator_edits != 0,
    );
    agent_doc_debounce::document_changed(path);
}

/// Legacy supervisor-origin synced-buffer proof hook.
///
/// Current editor state is proven through lazily visible-write receipts and the
/// CRDT relay. This ABI remains for older plugins but no longer writes a
/// file-backed live-buffer sidecar.
///
/// # Safety
///
/// `file_path`, `content`, `editor_id`, `editor_kind`, `editor_version`, and
/// `capabilities_csv` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_synced_digest_content_for_editor_v2(
    file_path: *const c_char,
    content: *const c_char,
    editor_id: *const c_char,
    editor_kind: *const c_char,
    editor_version: *const c_char,
    capabilities_csv: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let _text = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(text) => text,
        Err(_) => return,
    };
    let _editor = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor) => editor,
        Err(_) => return,
    };
    let _kind = match unsafe { CStr::from_ptr(editor_kind) }.to_str() {
        Ok(kind) => kind,
        Err(_) => return,
    };
    let _version = match unsafe { CStr::from_ptr(editor_version) }.to_str() {
        Ok(version) => version,
        Err(_) => return,
    };
    let _capabilities_raw = match unsafe { CStr::from_ptr(capabilities_csv) }.to_str() {
        Ok(capabilities) => capabilities,
        Err(_) => return,
    };
    eprintln!(
        "[ffi] synced live-buffer sidecar ABI ignored for {path}: current editor state is resolved through the CRDT relay"
    );
}

/// Clear one editor instance's live-buffer sidecar when the editor closes the
/// document.
///
/// # Safety
///
/// `file_path` and `editor_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_closed_for_editor(
    file_path: *const c_char,
    editor_id: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return,
    };
    let editor = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor) => editor,
        Err(_) => return,
    };
    if let Err(e) = agent_doc_debounce::clear_live_buffer_for_editor(path, Some(editor)) {
        eprintln!("[ffi] clear live buffer for editor failed for {path}: {e}");
    }
}

/// `#cancel-orphans-preflight-cycle`: explicit run-cancel reclaim seam.
///
/// The JB plugin's "cancel run" action calls this so an orphaned, empty
/// `preflight_started` cycle (no response capture) is abandoned immediately and
/// the next `Run Agent Doc` starts fresh instead of waiting for the staleness
/// window. The abandon decision is fail-safe in the binary: a cycle that
/// advanced past preflight or already owns a response capture is left intact.
///
/// Returns `1` if a cycle was abandoned, `0` if nothing was reclaimed
/// (no open cycle, or the cycle is protected), and `-1` on error.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_cancel_preflight_cycle(file_path: *const c_char) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match agent_doc_repair_io::cancel_preflight_cycle(
        &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
        std::path::Path::new(path),
    ) {
        Ok(agent_doc_turn::repair::CancelOutcome::Abandoned) => 1,
        Ok(_) => 0,
        Err(e) => {
            eprintln!("[ffi] agent_doc_cancel_preflight_cycle failed for {path}: {e}");
            -1
        }
    }
}

/// Check if a plugin in another process has typed recently (cross-process).
///
/// Reads the file-based typing indicator written by `agent_doc_document_changed`.
/// Returns `true` if the indicator exists and was updated within `debounce_ms`.
/// Returns `false` if no indicator file exists (plugin not active or no edits).
///
/// Use this from CLI/Project Controller tools that run separately from the editor plugin.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_typing_via_file(
    file_path: *const c_char,
    debounce_ms: i64,
) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    agent_doc_debounce::is_typing_via_file(path, debounce_ms as u64) as i32
}

/// Block until the file-based typing indicator shows idle, or timeout expires.
///
/// Used by CLI tools to wait for an editor plugin (separate process) to stop
/// typing before running an agent. Returns `true` if idle was reached, `false`
/// if `timeout_ms` expired first.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_await_idle_via_file(
    file_path: *const c_char,
    debounce_ms: i64,
    timeout_ms: i64,
) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 1, // Invalid path — don't block
    };
    agent_doc_debounce::await_idle_via_file(path, debounce_ms as u64, timeout_ms as u64) as i32
}

/// Set the response status for a file (Option B: in-process).
///
/// Status values: "generating", "writing", "routing", "idle"
/// Also writes a file-based signal (Option A: cross-process).
///
/// # Safety
///
/// `file_path` and `status` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_set_status(file_path: *const c_char, status: *const c_char) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    let st = match unsafe { CStr::from_ptr(status) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    agent_doc_debounce::set_status(path, st);
}

/// Get the response status for a file (file-based).
///
/// Returns a NUL-terminated string: "generating", "writing", "routing", or "idle".
/// Caller must free with `agent_doc_free_string`.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_get_status(file_path: *const c_char) -> *mut c_char {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return CString::new("idle").unwrap().into_raw(),
    };
    let status = agent_doc_debounce::get_status(path);
    CString::new(status)
        .unwrap_or_else(|_| CString::new("idle").unwrap())
        .into_raw()
}

/// Get the Project Controller→plugin turn-state projection for a document, as JSON.
///
/// Returns a NUL-terminated JSON string of `TurnProjection`:
/// `{"state":"idle|awaiting_response|persisting","turn_in_flight":bool,"transition_authority":"project_controller","realtime_steering":{"state":"...","preview":"..."}}`.
/// The Project Controller owns the authoritative turn phase; the plugin observes
/// this projection to render turn-in-flight UI and to decide whether a forwarded
/// operator prompt starts a fresh turn (`turn_in_flight == false`) or would
/// collide with an in-flight response (the `live_prompt_drift` double-append
/// guard). Defaults to the idle projection when no cycle state exists or on any
/// error.
///
/// Caller must free with `agent_doc_free_string`.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_turn_projection(file_path: *const c_char) -> *mut c_char {
    fn idle_json() -> String {
        let proj = agent_doc_turn::cpc_projection::TurnProjection::from_phase(
            agent_doc_turn::CyclePhase::Committed,
        );
        serde_json::to_string(&proj).unwrap_or_else(|_| r#"{"state":"idle"}"#.to_string())
    }
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return CString::new(idle_json()).unwrap().into_raw(),
    };
    // The Project Controller/state-backbone projection is the editor-facing turn
    // source. Cycle sidecars are recovery artifacts only and are intentionally
    // not consulted here.
    let projected_phase = state_backbone_closeout_phase(std::path::Path::new(path))
        .unwrap_or(agent_doc_turn::CyclePhase::Committed);
    let phase = resolve_turn_phase(
        document_model_actor_state(std::path::Path::new(path)),
        projected_phase,
    );
    let mut proj = agent_doc_turn::cpc_projection::TurnProjection::from_phase(phase);
    if proj.turn_in_flight {
        let steering =
            agent_doc_session_check_io::realtime_steering_since_turn_baseline(Path::new(path))
                .ok()
                .map(turn_steering_projection_from_realtime)
                .unwrap_or_else(agent_doc_turn::cpc_projection::TurnSteeringProjection::none);
        proj = proj.with_realtime_steering(steering);
    }
    let json = serde_json::to_string(&proj).unwrap_or_else(|_| idle_json());
    CString::new(json)
        .unwrap_or_else(|_| CString::new(idle_json()).unwrap())
        .into_raw()
}

/// Resolve the turn phase for the projection with the Project Controller
/// document model authoritative and the lazily state-backbone closeout phase as
/// the fine-label source. Pure so the authority policy is exhaustively
/// unit-testable without a state store.
///
/// - Model `Busy`/`Blocked` → a turn is live: keep the projected phase for the
///   awaiting/persisting label, but if the projection is terminal use a generic
///   in-flight phase so a lagging terminal projection cannot hide it.
/// - Model any other state → no turn is running: `Committed` (idle).
/// - Model unavailable (`None`, cold start) → the state-backbone projection.
fn resolve_turn_phase(
    model_state: Option<agent_doc_sqlite::state_store::ActorState>,
    projected_phase: agent_doc_turn::CyclePhase,
) -> agent_doc_turn::CyclePhase {
    use agent_doc_sqlite::state_store::ActorState;
    match model_state {
        Some(ActorState::Busy | ActorState::Blocked) => {
            if projected_phase.is_open() {
                projected_phase
            } else {
                agent_doc_turn::CyclePhase::PreflightStarted
            }
        }
        Some(_) => agent_doc_turn::CyclePhase::Committed,
        None => projected_phase,
    }
}

fn state_backbone_closeout_phase(file: &Path) -> Option<agent_doc_turn::CyclePhase> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    let document_hash = agent_doc_hash::document_id_for_path(file);
    agent_doc_controller_io::project_controller::load_state_backbone_projection(&root)
        .ok()?
        .document(&document_hash)
        .and_then(|document| document.closeout.phase)
}

/// The Project Controller document model's authoritative actor state for a
/// document, read directly from the state store (a local sqlite read, NOT a
/// controller RPC, so it stays cheap and works even while the controller is
/// busy). `None` when there is no state store yet (cold start / never-registered)
/// or the row is unreadable, so the projection falls back to the state-backbone
/// closeout phase.
///
/// Reads WITHOUT creating the store (checks the path first), so a bare read from
/// the editor plugin never materializes controller state as a side effect.
fn document_model_actor_state(file: &Path) -> Option<agent_doc_sqlite::state_store::ActorState> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    let db_path = agent_doc_sqlite::state_store::state_db_path(&root);
    if !db_path.exists() {
        return None;
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root).ok()?;
    let document_id =
        agent_doc_session_actor_io::canonical_document_id_in(&root, &file.to_string_lossy());
    agent_doc_sqlite::state_store::load_actor_record_from_db(&conn, &document_id)
        .ok()
        .flatten()
        .map(|record| record.state)
}

fn turn_steering_projection_from_realtime(
    steering: agent_doc_document_realtime::baseline_comparison::RealtimeSteering,
) -> agent_doc_turn::cpc_projection::TurnSteeringProjection {
    use agent_doc_document_realtime::baseline_comparison::RealtimeSteering;
    use agent_doc_turn::cpc_projection::{TurnSteeringProjection, TurnSteeringState};

    let Some(preview) = steering.preview().map(ToOwned::to_owned) else {
        return TurnSteeringProjection::none();
    };
    let state = match steering {
        RealtimeSteering::None => return TurnSteeringProjection::none(),
        RealtimeSteering::PromptTarget { .. } => TurnSteeringState::PromptTarget,
        RealtimeSteering::ContentEdit { .. } => TurnSteeringState::ContentEdit,
        RealtimeSteering::PromptDeleted { .. } => TurnSteeringState::PromptDeleted,
        RealtimeSteering::PromptReduced { .. } => TurnSteeringState::PromptReduced,
    };
    TurnSteeringProjection::observed(state, Some(preview))
}

/// Check if any operation is in progress for a file (file-based).
///
/// Returns `true` if status is NOT "idle". Plugins should skip route
/// operations when this returns `true` to prevent cascading.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_busy(file_path: *const c_char) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    agent_doc_debounce::is_busy(path) as i32
}

/// Return the `agent:exchange` nodes of `file_path` as a JSON array
/// (`[{"id","kind","label"}, …]`), or `[]` on any error or when the document has no
/// exchange component. This is the Phase 4 read surface of the exchange document-tree
/// (`tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md`): editors use it to
/// show the exchange's distinct per-response / per-prompt structure.
///
/// Read-only by design — mutating operations go through the binary-owned
/// `agent-doc exchange {remove,add-response,add-prompt,move}` CLI so the snapshot +
/// CRDT re-baseline runs correctly (editors stay thin; the binary owns document
/// mutation).
///
/// Caller must free with `agent_doc_free_string`.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_exchange_nodes(file_path: *const c_char) -> *mut c_char {
    fn empty() -> *mut c_char {
        CString::new("[]").unwrap().into_raw()
    }
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return empty(),
    };
    let Ok(doc) = std::fs::read_to_string(path) else {
        return empty();
    };
    let Ok(components) = agent_doc_element::element::parse(&doc) else {
        return empty();
    };
    let Some(comp) = components.into_iter().find(|c| c.name == "exchange") else {
        return empty();
    };
    let nodes = agent_doc_markdown_ast::exchange_tree::list_exchange_nodes(comp.content(&doc));
    let json = serde_json::to_string(
        &nodes
            .iter()
            .map(|n| serde_json::json!({ "id": n.node_id, "kind": n.kind, "label": n.label }))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());
    CString::new(json)
        .unwrap_or_else(|_| CString::new("[]").unwrap())
        .into_raw()
}

/// Check whether a `.md` file is an opted-in agent-doc session document.
///
/// Returns `1` when the file carries agent-doc frontmatter, matches a
/// `[documents] include` glob in `.agent-doc/config.toml`, or runs under the
/// `auto_session_for_all_md` escape hatch; `0` otherwise (including unreadable
/// paths). Editor plugins call this to gate **Run Agent Doc** / SubmitAction so
/// a plain `.md` is not offered as a session. Mirrors the binary opt-in gate.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_session_document(file_path: *const c_char) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    agent_doc_frontmatter_io::session::is_agent_doc_document_for_file(
        &content,
        std::path::Path::new(path),
    ) as i32
}

/// Report the current editor buffer state for a document.
///
/// IDE plugins call this on every document change to provide editor-authoritative
/// buffer stability information. When present, `wait_for_stable_content` uses
/// this state instead of the truncation heuristic (`extract_last_added_line`).
///
/// Parameters:
/// - `file_path`: canonical document path
/// - `version`: monotonic editor document version (incremented on each edit)
/// - `dirty`: whether the buffer has unsaved changes
/// - `last_edit_timestamp_ms`: Unix epoch milliseconds of the last edit
/// - `save_timestamp_ms`: Unix epoch milliseconds of last save (0 if never saved)
/// - `content_hash`: SHA-256 hex digest of the editor-visible buffer (null if unavailable)
/// - `content_len`: byte length of the editor-visible buffer (-1 if unavailable)
/// - `session_id`: editor session identifier (null if unavailable)
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
/// `content_hash` and `session_id` may be null if unavailable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_report_editor_state(
    file_path: *const c_char,
    version: u64,
    dirty: i32,
    last_edit_timestamp_ms: u64,
    save_timestamp_ms: u64,
    content_hash: *const c_char,
    content_len: i64,
    session_id: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    let hash = if content_hash.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(content_hash) }
            .to_str()
            .ok()
            .map(|s| s.to_string())
    };
    let len = if content_len < 0 {
        None
    } else {
        usize::try_from(content_len).ok()
    };
    let sid = if session_id.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(session_id) }
            .to_str()
            .ok()
            .map(|s| s.to_string())
    };
    let save_ts = if save_timestamp_ms == 0 {
        None
    } else {
        Some(save_timestamp_ms as u128)
    };
    let state = agent_doc_debounce::EditorBufferState {
        path: path.to_string(),
        version,
        dirty: dirty != 0,
        last_edit_timestamp_ms: last_edit_timestamp_ms as u128,
        save_timestamp_ms: save_ts,
        hash,
        content_len: len,
        session_id: sid,
    };
    agent_doc_debounce::record_editor_buffer_state(&state);
}

/// Get the current editor buffer state for a document as JSON.
///
/// Returns a NUL-terminated JSON string with the editor buffer state fields,
/// or null if no editor state has been recorded. Caller must free with
/// `agent_doc_free_string`.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_get_editor_state(file_path: *const c_char) -> *mut c_char {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    match agent_doc_debounce::editor_buffer_state(path) {
        Some(state) => match serde_json::to_string(&state) {
            Ok(json) => CString::new(json).unwrap().into_raw(),
            Err(_) => ptr::null_mut(),
        },
        None => ptr::null_mut(),
    }
}

/// Check if the editor buffer state indicates a stable (idle) document.
///
/// Returns `true` if the editor has reported buffer state and the buffer is
/// stable (not dirty, or debounce elapsed). Returns `false` if still editing
/// or no editor state exists.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_editor_stable(
    file_path: *const c_char,
    debounce_ms: i64,
) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    agent_doc_debounce::editor_buffer_stable(path, debounce_ms as u64).is_some() as i32
}

/// Try to acquire the sync lock. Returns `true` if acquired, `false` if already held.
///
/// Editors call this before triggering `agent-doc sync`. If it returns `false`,
/// skip the sync (another sync is in progress). Call `agent_doc_sync_unlock()`
/// when the sync completes.
///
/// This is a cross-editor shared lock — prevents concurrent syncs from IntelliJ
/// and VS Code plugins simultaneously.
///
/// `#recyclerestart` Q2: the guard self-heals — a holder that wedges past
/// [`DEFAULT_SYNC_LOCK_STALE_BOUND_MS`] (a sync thread blocked on in-pane recovery that
/// never released) is superseded so a later `Sync Tmux Layout` can never be deferred
/// forever with "another sync is already running". A legitimately in-flight sync (held
/// under the bound) still defers as before. Use [`agent_doc_sync_try_lock_bounded`] to
/// override the bound.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_try_lock() -> i32 {
    sync_try_lock_with_bound(DEFAULT_SYNC_LOCK_STALE_BOUND_MS)
}

/// Like [`agent_doc_sync_try_lock`] but with an explicit stale bound in milliseconds
/// (`<= 0` falls back to [`DEFAULT_SYNC_LOCK_STALE_BOUND_MS`]). Lets a caller that knows
/// its sync is fast use a tighter self-heal window.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_try_lock_bounded(stale_bound_ms: i64) -> i32 {
    let bound = if stale_bound_ms <= 0 {
        DEFAULT_SYNC_LOCK_STALE_BOUND_MS
    } else {
        stale_bound_ms as u64
    };
    sync_try_lock_with_bound(bound)
}

/// Release the sync lock acquired by `agent_doc_sync_try_lock()`.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_unlock() {
    // Clear the holder timestamp before releasing so a racing acquirer that observes the
    // free guard never reads a stale acquisition time.
    SYNC_LOCK_ACQUIRED_AT_MS.store(0, Ordering::SeqCst);
    SYNC_LOCKED.store(false, Ordering::SeqCst);
}

/// `#8bfz` / `#fcconeowner`: elect a single live IntelliJ plugin consumer per
/// document. Returns `1` if THIS consumer owns the document (apply the patch +
/// `saveDocument`), `0` if a live owner already holds it (defer; leave the patch
/// for the owner to apply).
///
/// Cross-process safe: the election is a `.agent-doc/plugin-owner/<hash>.json`
/// lease claimed atomically (O_EXCL), self-healing on a stale heartbeat or a
/// dead owner pid. Fail-open — any IO error returns `1` (apply) so a
/// single-instance setup is never worse off than before the lease.
///
/// # Safety
///
/// `file_path` and `consumer_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_plugin_owner_try_acquire(
    file_path: *const c_char,
    consumer_id: *const c_char,
    pid: i64,
) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 1, // can't resolve path — fail open (apply)
    };
    let consumer = match unsafe { CStr::from_ptr(consumer_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    agent_doc_plugin_owner::try_acquire_plugin_owner(path, consumer, pid as u32) as i32
}

/// `#8bfz` / `#fcconeowner`: release the plugin-owner lease for `file_path`, but
/// only if `consumer_id` still owns it (never stomps a successor). The plugin
/// calls this when it stops watching the document (project/file close) so a
/// live successor takes over without waiting for the TTL.
///
/// # Safety
///
/// `file_path` and `consumer_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_plugin_owner_release(
    file_path: *const c_char,
    consumer_id: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    let consumer = match unsafe { CStr::from_ptr(consumer_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    agent_doc_plugin_owner::release_plugin_owner(path, consumer);
}

/// Bump the sync debounce generation. Returns the new generation number.
///
/// Editors call this when a layout change is detected. After a delay (e.g., 500ms),
/// they call `agent_doc_sync_check_generation(gen)` — if it returns `true`, the
/// generation is still current and the sync should proceed. If `false`, a newer
/// event superseded this one.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_bump_generation() -> u64 {
    SYNC_GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

/// Check if a generation is still current. Returns `true` if `generation` matches the
/// latest generation (no newer events have been scheduled).
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_check_generation(generation: u64) -> i32 {
    (SYNC_GENERATION.load(Ordering::SeqCst) == generation) as i32
}

/// Start the IPC socket listener on a background thread.
///
/// The plugin calls this on project open to start listening for socket IPC
/// messages from the CLI. The callback receives each JSON message as a
/// read-only NUL-terminated string (do NOT free it) and returns `true` if
/// the message was handled successfully, `false` on error. The listener
/// generates the socket receipt response internally based on the return value.
///
/// Returns `true` if the listener was started, `false` on error.
///
/// # Safety
///
/// - `project_root` must be a valid, NUL-terminated UTF-8 string.
/// - `callback` must be a valid function pointer that remains valid for the
///   lifetime of the listener thread. The message pointer is borrowed — the
///   callback must NOT free it or hold a reference after returning.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_start_ipc_listener(
    project_root: *const c_char,
    callback: extern "C" fn(message: *const c_char) -> i32,
) -> i32 {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return 0,
    };
    let root_path = std::path::PathBuf::from(&root_str);

    std::thread::spawn(move || {
        let result = agent_doc_ipc_io::start_listener_with_logger(
            &root_path,
            move |msg| {
                // Lend the message to the callback (no ownership transfer)
                let c_msg = match CString::new(msg) {
                    Ok(c) => c,
                    Err(_) => {
                        return Some(r#"{"type":"receipt","status":"rejected"}"#.to_string());
                    }
                };
                let success = callback(c_msg.as_ptr()) != 0;
                if success {
                    Some(r#"{"type":"receipt","status":"applied"}"#.to_string())
                } else {
                    Some(r#"{"type":"receipt","status":"rejected"}"#.to_string())
                }
            },
            agent_doc_ops_log_io::log_op,
        );
        if let Err(e) = result {
            eprintln!("[ffi] IPC listener error: {}", e);
        }
    });

    1
}

/// V2 of [`agent_doc_start_ipc_listener`] with extended receipt-result encoding.
///
/// The callback returns one of three values:
/// - `0` → receipt `{"type":"receipt","status":"rejected"}` (apply failed)
/// - `1` → receipt `{"type":"receipt","status":"applied"}` (apply succeeded)
/// - `2` → receipt `{"type":"receipt","status":"applied","reason":"already_applied"}`
///   (plugin detected the patch is already in the live buffer and chose NOT
///   to re-apply; binary skips the file-IPC fallback so a duplicate response
///   heading cannot land).
///
/// Plugins should prefer v2 when available.
///
/// Early receipt (`#ipc-early-receipt`): the underlying
/// `agent_doc_ipc_io::start_listener` transport emits an `accepted` receipt the
/// instant it receives a patch that opted in (`"early_receipt": true`), before
/// this callback's blocking apply runs, then this callback's terminal receipt as
/// usual. The early receipt is owned by the Rust transport.
///
/// Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
/// `[#ipcpluginalready]`.
///
/// # Safety
///
/// Same as [`agent_doc_start_ipc_listener`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_start_ipc_listener_v2(
    project_root: *const c_char,
    callback: extern "C" fn(message: *const c_char) -> i32,
) -> i32 {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return 0,
    };
    let root_path = std::path::PathBuf::from(&root_str);

    std::thread::spawn(move || {
        let result = agent_doc_ipc_io::start_listener_with_logger(
            &root_path,
            move |msg| {
                let c_msg = match CString::new(msg) {
                    Ok(c) => c,
                    Err(_) => {
                        return Some(r#"{"type":"receipt","status":"rejected"}"#.to_string());
                    }
                };
                match callback(c_msg.as_ptr()) {
                    1 => Some(r#"{"type":"receipt","status":"applied"}"#.to_string()),
                    2 => Some(
                        r#"{"type":"receipt","status":"applied","reason":"already_applied"}"#
                            .to_string(),
                    ),
                    _ => Some(r#"{"type":"receipt","status":"rejected"}"#.to_string()),
                }
            },
            agent_doc_ops_log_io::log_op,
        );
        if let Err(e) = result {
            eprintln!("[ffi] IPC listener v2 error: {}", e);
        }
    });

    1
}

/// Stop the IPC socket listener by removing the socket file.
///
/// The listener thread will exit on its next accept() call when the socket
/// is removed. Call this on project close / plugin disposal.
///
/// # Safety
///
/// `project_root` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_stop_ipc_listener(project_root: *const c_char) {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    let sock = agent_doc_ipc_io::socket_path(std::path::Path::new(root_str));
    if let Err(e) = std::fs::remove_file(&sock)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("[ffi] failed to remove socket {:?}: {}", sock, e);
    }
}

/// Legacy ACK-content ABI retained only to fail old editor plugins closed.
///
/// Current editor plugins must publish lazily transport receipts through
/// `agent_doc_editor_patch_applied` / `agent_doc_editor_patch_rejected` and use
/// the capability-bearing editor content bridge below.
///
/// # Safety
///
/// `project_root` and `patch_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_write_ack_content(
    project_root: *const c_char,
    patch_id: *const c_char,
    content: *const c_char,
) -> i32 {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let patch_id_str = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let _ = content;
    eprintln!(
        "[ffi] incompatible editor plugin for {root_str}: agent_doc_write_ack_content is no longer supported for patch_id {}; update/reinstall the plugin so it publishes lazily transport receipts",
        &patch_id_str[..patch_id_str.len().min(8)]
    );
    0
}

fn record_lazily_visible_write_receipt(
    file: &Path,
    patch_id_str: &str,
    content_str: &str,
    source: &str,
) -> anyhow::Result<()> {
    let proof =
        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
            file,
            patch_id_str,
            content_str,
            source,
    )?;
    eprintln!(
        "[ffi] lazily visible-write receipt recorded: patch_id {} model_revision={} candidate_hash={} source={}",
        &patch_id_str[..patch_id_str.len().min(8)],
        proof.model_revision,
        proof.commit_candidate_hash,
        source
    );
    Ok(())
}

fn record_editor_patch_receipt(
    file_path: &str,
    patch_id: &str,
    actor_generation: u64,
    rejected_reason: Option<&str>,
) -> i32 {
    let canonical = Path::new(file_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(file_path));
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let event_suffix = match rejected_reason {
        Some(reason) => format!(
            "editor-patch-rejected-{patch_id}-{actor_generation}-{}",
            agent_doc_hash::content_hash(reason)
        ),
        None => format!("editor-patch-applied-{patch_id}-{actor_generation}"),
    };
    let generation_event = StateEvent::new(
        format!("{document_hash}:editor-patch-generation-{patch_id}-{actor_generation}"),
        StateFact::OwnerGenerationChanged {
            document_hash: document_hash.clone(),
            owner: agent_doc_state_backbone::StateOwner::EditorIpcBridge,
            generation: actor_generation,
        },
    );
    let receipt_event_id = format!("{document_hash}:{event_suffix}");
    let fact = match rejected_reason {
        Some(reason) => StateFact::EditorPatchRejected {
            document_hash: document_hash.clone(),
            patch_id: patch_id.to_string(),
            actor_generation,
            reason: reason.to_string(),
        },
        None => StateFact::EditorPatchApplied {
            document_hash: document_hash.clone(),
            patch_id: patch_id.to_string(),
            actor_generation,
        },
    };
    let receipt_event = StateEvent::new(receipt_event_id, fact);
    match state_ledger().lock() {
        Ok(mut ledger) => {
            ledger.append(generation_event.clone());
            ledger.append(receipt_event.clone());
        }
        Err(err) => {
            eprintln!(
                "[state-projection] editor patch receipt rejected for {file_path}: ledger lock poisoned: {err}"
            );
            return 0;
        }
    }

    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        eprintln!(
            "[state-projection] editor patch receipt rejected for {file_path}: no agent-doc project root found"
        );
        return 0;
    };
    if let Err(err) = agent_doc_controller_io::project_controller::append_state_event(
        &project_root,
        &generation_event,
    ) {
        eprintln!(
            "[state-projection] editor patch receipt rejected for {file_path}: durable generation append failed: {err}"
        );
        return 0;
    }
    if let Err(err) = agent_doc_controller_io::project_controller::append_state_event(
        &project_root,
        &receipt_event,
    ) {
        eprintln!(
            "[state-projection] editor patch receipt rejected for {file_path}: durable receipt append failed: {err}"
        );
        return 0;
    }
    1
}

/// Record that the editor applied a queued patch as a lazily transport receipt.
///
/// # Safety
///
/// `file_path` and `patch_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_patch_applied(
    file_path: *const c_char,
    patch_id: *const c_char,
    actor_generation: u64,
) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return 0,
    };
    let patch = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(patch) => patch,
        Err(_) => return 0,
    };
    record_editor_patch_receipt(path, patch, actor_generation, None)
}

/// Record that the editor rejected a queued patch as a lazily transport receipt.
///
/// # Safety
///
/// `file_path`, `patch_id`, and `reason` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_patch_rejected(
    file_path: *const c_char,
    patch_id: *const c_char,
    actor_generation: u64,
    reason: *const c_char,
) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return 0,
    };
    let patch = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(patch) => patch,
        Err(_) => return 0,
    };
    let rejection = match unsafe { CStr::from_ptr(reason) }.to_str() {
        Ok(reason) => reason,
        Err(_) => return 0,
    };
    record_editor_patch_receipt(path, patch, actor_generation, Some(rejection))
}

/// Record editor-applied content through one lazily receipt capability-bearing
/// ABI call.
///
/// This is the preferred editor endpoint when the content came from the live
/// editor buffer. The lazily/controller receipt is authoritative; file-backed
/// live-buffer sidecars are not written from this path.
///
/// # Safety
///
/// All pointers must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_editor_content_applied_for_editor_v1(
    project_root: *const c_char,
    patch_id: *const c_char,
    file_path: *const c_char,
    content: *const c_char,
    editor_id: *const c_char,
    editor_kind: *const c_char,
    editor_version: *const c_char,
    capabilities_csv: *const c_char,
) -> i32 {
    let _root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let patch_id_str = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return 0,
    };
    let text = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(text) => text,
        Err(_) => return 0,
    };
    let _editor = match unsafe { CStr::from_ptr(editor_id) }.to_str() {
        Ok(editor) => editor,
        Err(_) => return 0,
    };
    let _kind = match unsafe { CStr::from_ptr(editor_kind) }.to_str() {
        Ok(kind) => kind,
        Err(_) => return 0,
    };
    let _version = match unsafe { CStr::from_ptr(editor_version) }.to_str() {
        Ok(version) => version,
        Err(_) => return 0,
    };
    let capabilities_raw = match unsafe { CStr::from_ptr(capabilities_csv) }.to_str() {
        Ok(capabilities) => capabilities,
        Err(_) => return 0,
    };
    let capabilities: Vec<&str> = capabilities_raw
        .split(',')
        .map(str::trim)
        .filter(|capability| !capability.is_empty())
        .collect();
    if !capabilities.contains(&agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY) {
        eprintln!(
            "[ffi] incompatible editor plugin for {path}: agent_doc_editor_content_applied_for_editor_v1 requires {}; update/reinstall the plugin so it publishes editor_patch_applied/editor_patch_rejected lazily receipts",
            agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY
        );
        return 0;
    }

    if let Err(err) = record_lazily_visible_write_receipt(
        Path::new(path),
        patch_id_str,
        text,
        "editor_patch_applied_for_editor_v2",
    ) {
        eprintln!("[ffi] consolidated lazily receipt event failed for {path}: {err}");
        return 0;
    }
    eprintln!(
        "[ffi] lazily content proof recorded via CRDT/controller authority: {} bytes for patch_id {}",
        text.len(),
        &patch_id_str[..patch_id_str.len().min(8)]
    );
    1
}

/// Legacy ACK-content ABI retained only to fail old editor plugins closed.
///
/// # Safety
///
/// All pointers must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_write_ack_content_for_editor_v2(
    _project_root: *const c_char,
    patch_id: *const c_char,
    file_path: *const c_char,
    _content: *const c_char,
    _editor_id: *const c_char,
    _editor_kind: *const c_char,
    _editor_version: *const c_char,
    _capabilities_csv: *const c_char,
) -> i32 {
    let patch_id_str = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(path) => path,
        Err(_) => return 0,
    };
    eprintln!(
        "[ffi] incompatible editor plugin for {path}: agent_doc_write_ack_content_for_editor_v2 is no longer supported for patch_id {}; update/reinstall the plugin so it publishes lazily transport receipts",
        &patch_id_str[..patch_id_str.len().min(8)]
    );
    0
}

/// Check if the CLI claimed this patch by writing a local-closeout sentinel.
/// Returns true if the sentinel `.agent-doc/claimed-patches/<patch_id>` exists.
/// The sentinel is intentionally durable for the patch id so multiple editor
/// watchers or repeated directory scans all skip the same locally-closed patch.
///
/// # Safety
///
/// Both pointers must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_claimed_by_force_disk(
    project_root: *const c_char,
    patch_id: *const c_char,
) -> i32 {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let patch_id_str = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let sentinel = std::path::Path::new(root_str)
        .join(".agent-doc/claimed-patches")
        .join(patch_id_str);

    if sentinel.exists() {
        eprintln!(
            "[ffi] patch_id {} claimed by local closeout — skipping apply",
            &patch_id_str[..patch_id_str.len().min(8)]
        );
        1
    } else {
        0
    }
}

/// Return true when the current disk file still matches committed `HEAD` and
/// that committed document already contains the incoming response patch body.
///
/// This gives editor plugins a safe late-replay gate: a stale File Cache
/// Conflict accept should no-op only when both disk and HEAD prove that the
/// response is already committed. If disk has drifted away from HEAD, the
/// plugin must not use HEAD alone as proof because the patch may be needed to
/// restore a stale editor buffer.
///
/// # Safety
///
/// `file_path` and `content` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_patch_content_already_committed(
    file_path: *const c_char,
    content: *const c_char,
) -> i32 {
    let file = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => std::path::PathBuf::from(s),
        Err(_) => return 0,
    };
    let patch_content = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let Some(head) = ffi_show_head(&file) else {
        return 0;
    };
    let Ok(current) = std::fs::read_to_string(&file) else {
        return 0;
    };
    if ffi_normalize_transient_agent_doc_markers(&current)
        != ffi_normalize_transient_agent_doc_markers(&head)
    {
        return 0;
    }
    ffi_response_already_applied(&head, patch_content) as i32
}

/// Report whether the editor plugin's own `WatchService` file-apply path is
/// demoted to read-only buffer reporting (`#dsqa` / `#pcp7` — 08b cut-over
/// complete). Always returns `1`: post-cutover the plugin must NOT apply
/// file-IPC patches it observes on disk — the controller-owned watcher + socket
/// IPC are the sole writer to the live buffer. Emits a structured
/// `plugin_watch_readonly` marker to that document's `ops.log` so the demotion
/// is verifiable from the logs (the `#q6js` / `#lvbremain` proof).
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_plugin_watch_readonly(file_path: *const c_char) -> i32 {
    let file = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    if agent_doc_document_realtime::watch_authority::plugin_watch_is_readonly() {
        agent_doc_ops_log_io::log_op(
            std::path::Path::new(file),
            "plugin_watch_readonly skipped file-apply (controller-owned watcher is sole writer) #dsqa",
        );
        1
    } else {
        0
    }
}

fn ffi_show_head(file: &std::path::Path) -> Option<String> {
    let parent = file.parent()?;
    let root = std::process::Command::new("git")
        .current_dir(parent)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| std::path::PathBuf::from(stdout.trim()))?;
    let relative = file.strip_prefix(&root).ok()?;
    let spec = format!("HEAD:{}", relative.to_string_lossy());
    std::process::Command::new("git")
        .current_dir(root)
        .args(["show", &spec])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

/// Recent prior-commit blob contents of `file` (newest first, up to `limit`
/// commits that touched the file). Used by the reconnect staleness check
/// (`#yzer`) to prove an editor buffer is stale committed content rather than
/// unsynced user edits. Best-effort: returns an empty vec on any git failure.
fn ffi_show_prior_blobs(file: &std::path::Path, limit: usize) -> Vec<String> {
    let Some(parent) = file.parent() else {
        return Vec::new();
    };
    let Some(root) = std::process::Command::new("git")
        .current_dir(parent)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| std::path::PathBuf::from(stdout.trim()))
    else {
        return Vec::new();
    };
    let Ok(relative) = file.strip_prefix(&root) else {
        return Vec::new();
    };
    let rel = relative.to_string_lossy().to_string();
    let commits = std::process::Command::new("git")
        .current_dir(&root)
        .args([
            "log",
            &format!("-n{limit}"),
            "--format=%H",
            "--",
            rel.as_str(),
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_default();
    commits
        .lines()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .filter_map(|commit| {
            let spec = format!("{commit}:{rel}");
            std::process::Command::new("git")
                .current_dir(&root)
                .args(["show", &spec])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
        })
        .collect()
}

/// Decide how an editor plugin should reconcile its buffer with disk when it
/// (re)connects its IPC listener, and return the disk content to apply when a
/// re-read is warranted (`#yzer` / `#evmhplugin`).
///
/// Returns a JSON object (free with [`agent_doc_free_string`]):
/// ```json
/// {"decision": "reread_disk" | "keep_buffer" | "in_sync", "content": "<disk>"}
/// ```
/// `content` is present only for `reread_disk`. On any error the result is
/// `{"decision":"keep_buffer"}` (fail safe — never clobber the buffer on
/// uncertainty). Emits a `reconnect_buffer_decision` marker to the document's
/// `ops.log` so the path is verifiable from the logs.
///
/// # Safety
///
/// `project_root`, `file_path`, and `buffer_content` must be valid,
/// NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reconnect_buffer_decision(
    project_root: *const c_char,
    file_path: *const c_char,
    buffer_content: *const c_char,
) -> *mut c_char {
    use agent_doc_document_realtime::write_policy::{
        ReconnectBufferDecision, decide_reconnect_buffer,
    };
    let keep = || {
        CString::new(r#"{"decision":"keep_buffer"}"#)
            .unwrap()
            .into_raw()
    };
    let (Ok(file), Ok(buffer)) = (
        unsafe { CStr::from_ptr(file_path) }.to_str(),
        unsafe { CStr::from_ptr(buffer_content) }.to_str(),
    ) else {
        return keep();
    };
    let _ = project_root; // root is derived from the file's git toplevel
    let file_path_buf = std::path::PathBuf::from(file);
    let Ok(disk) = std::fs::read_to_string(&file_path_buf) else {
        return keep();
    };

    // Trailing-whitespace tolerant comparison: editor buffers routinely differ
    // from git blobs only by a trailing newline.
    let norm = |s: &str| s.trim_end().to_string();
    let buffer_n = norm(buffer);
    let disk_n = norm(&disk);

    let buffer_matches_disk = buffer_n == disk_n;
    let head = ffi_show_head(&file_path_buf);
    let disk_is_committed_head = head.as_deref().map(norm) == Some(disk_n.clone());
    let buffer_matches_prior_commit = ffi_show_prior_blobs(&file_path_buf, 10)
        .iter()
        .any(|blob| norm(blob) == buffer_n);

    let decision = decide_reconnect_buffer(
        buffer_matches_disk,
        disk_is_committed_head,
        buffer_matches_prior_commit,
    );

    agent_doc_ops_log_io::log_op(
        &file_path_buf,
        &format!(
            "reconnect_buffer_decision decision={} buffer_matches_disk={} disk_is_head={} buffer_is_prior_commit={} #yzer",
            decision.as_str(),
            buffer_matches_disk,
            disk_is_committed_head,
            buffer_matches_prior_commit,
        ),
    );

    let json = match decision {
        ReconnectBufferDecision::RereadDisk => serde_json::json!({
            "decision": decision.as_str(),
            "content": disk,
        }),
        _ => serde_json::json!({ "decision": decision.as_str() }),
    };
    match CString::new(json.to_string()) {
        Ok(c) => c.into_raw(),
        Err(_) => keep(),
    }
}

/// Record one real editor operation for op-capture (`#qnodemerge4wire` part 2).
///
/// The FFI-first ingestion point the thin editor reporters (JetBrains
/// `DocumentListener.documentChanged`, VS Code `onDidChangeTextDocument`) call to
/// feed the editor's *real* operations to the merge instead of a Myers
/// diff-guess. A replacement is reported as a `delete` followed by an `insert`.
///
/// `op_kind` is `"insert"` or `"delete"`. For `"insert"`, `insert_text` is the
/// inserted text and `delete_len` is ignored. For `"delete"`, `delete_len` is the
/// number of bytes removed at `offset` and `insert_text` may be null. `offset`
/// and `delete_len` are byte offsets/lengths into the buffer the op was captured
/// against, whose text hashes to `base_hash` (see
/// [`agent_doc_hash::content_hash`]). A later merge trusts
/// the op only when its resolved base hashes to the same value; otherwise the op
/// is silently disqualified and the merge falls back to the diff-guess (never
/// worse than today).
///
/// Returns `1` when the op was durably recorded, `0` on any error (bad UTF-8,
/// unknown kind, negative offset/len, or I/O failure) so the reporter can fall
/// back to the diff-guess path.
///
/// # Safety
///
/// `file_path`, `base_hash`, and `op_kind` must be valid, NUL-terminated UTF-8
/// strings. `insert_text` may be null (for deletes) or a valid NUL-terminated
/// UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_record_editor_op(
    file_path: *const c_char,
    base_hash: *const c_char,
    op_kind: *const c_char,
    offset: i64,
    insert_text: *const c_char,
    delete_len: i64,
) -> i32 {
    let (Ok(file), Ok(base), Ok(kind)) = (
        unsafe { CStr::from_ptr(file_path) }.to_str(),
        unsafe { CStr::from_ptr(base_hash) }.to_str(),
        unsafe { CStr::from_ptr(op_kind) }.to_str(),
    ) else {
        eprintln!("[op-capture] agent_doc_record_editor_op: non-UTF-8 argument; ignoring op");
        return 0;
    };
    let Ok(offset) = usize::try_from(offset) else {
        eprintln!("[op-capture] agent_doc_record_editor_op: negative offset {offset}; ignoring op");
        return 0;
    };
    let file_path_buf = std::path::PathBuf::from(file);

    let op = match kind {
        "insert" => {
            if insert_text.is_null() {
                eprintln!(
                    "[op-capture] agent_doc_record_editor_op: insert with null text; ignoring op"
                );
                return 0;
            }
            let Ok(text) = unsafe { CStr::from_ptr(insert_text) }.to_str() else {
                eprintln!(
                    "[op-capture] agent_doc_record_editor_op: non-UTF-8 insert text; ignoring op"
                );
                return 0;
            };
            agent_doc_merge::crdt::EditorOp::Insert {
                offset,
                text: text.to_string(),
            }
        }
        "delete" => {
            let Ok(len) = usize::try_from(delete_len) else {
                eprintln!(
                    "[op-capture] agent_doc_record_editor_op: negative delete_len {delete_len}; ignoring op"
                );
                return 0;
            };
            agent_doc_merge::crdt::EditorOp::Delete { offset, len }
        }
        other => {
            eprintln!(
                "[op-capture] agent_doc_record_editor_op: unknown op_kind {other:?}; ignoring"
            );
            return 0;
        }
    };

    let op_log_summary = match &op {
        agent_doc_merge::crdt::EditorOp::Insert { text, .. } => format!(
            "insert_bytes={} insert_non_ascii={}",
            text.len(),
            !text.is_ascii()
        ),
        agent_doc_merge::crdt::EditorOp::Delete { len, .. } => {
            format!("delete_len={len}")
        }
    };

    match agent_doc_op_capture_io::record_editor_op(&file_path_buf, base, op) {
        Ok(()) => {
            agent_doc_ops_log_io::log_op(
                &file_path_buf,
                &format!(
                    "editor_op_recorded kind={kind} offset={offset} {op_log_summary} base={} #qnodemerge4wire",
                    base.get(..12).unwrap_or(base),
                ),
            );
            1
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                &file_path_buf,
                &format!("editor_op_record_failed kind={kind} error={e} #qnodemerge4wire"),
            );
            0
        }
    }
}

/// Compute the `base_hash` the editor op reporters must stamp on captured ops
/// (`#qnodemerge4wire`) so the write-time merge accepts them — the SHA256 hex of
/// the same CRDT merge base text [`agent_doc_merge_io::merge_contents_crdt_with_ops`]
/// resolves. The reporter calls this once per edit (cheap; the editor offsets it
/// pairs with are relative to this base) and passes the result to
/// [`agent_doc_record_editor_op`].
///
/// Returns a NUL-terminated string (the empty-text hash when no snapshot/CRDT
/// base exists yet), or null on a bad path / resolution error so the reporter
/// skips op capture for this edit and the merge falls back to the diff-guess —
/// never worse than today. Caller must free with `agent_doc_free_string`.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_base_hash(file_path: *const c_char) -> *mut c_char {
    let Ok(path) = (unsafe { CStr::from_ptr(file_path) }).to_str() else {
        eprintln!("[op-capture] agent_doc_document_base_hash: non-UTF-8 path; returning null");
        return std::ptr::null_mut();
    };
    match agent_doc_op_capture_io::current_base_hash_with(
        std::path::Path::new(path),
        |doc, baseline| {
            agent_doc_snapshot_io::crdt_merge_base_state_with(
                doc,
                baseline,
                agent_doc_op_capture_io::has_pending_editor_ops,
                agent_doc_ops_log_io::log_op,
            )
            .map(|base| base.state)
        },
    ) {
        Ok(hash) => CString::new(hash)
            .map(|c| c.into_raw())
            .unwrap_or(std::ptr::null_mut()),
        Err(e) => {
            eprintln!("[op-capture] agent_doc_document_base_hash: {e}; returning null");
            std::ptr::null_mut()
        }
    }
}

fn ffi_normalize_transient_agent_doc_markers(content: &str) -> String {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("<!-- agent:boundary:") {
                return None;
            }
            let stripped = if (trimmed.starts_with('#') || trimmed.starts_with("**"))
                && line.ends_with(" (HEAD)")
            {
                line.strip_suffix(" (HEAD)").unwrap_or(line)
            } else {
                line
            };
            Some(stripped.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ffi_response_already_applied(doc: &str, response: &str) -> bool {
    let response_lines = ffi_normalized_response_lines(response);
    if response_lines.is_empty() {
        return false;
    }
    let doc_lines = ffi_normalized_response_lines(doc);
    doc_lines
        .windows(response_lines.len())
        .any(|window| window == response_lines.as_slice())
}

fn ffi_normalized_response_lines(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(ffi_normalize_response_line)
        .collect()
}

fn ffi_normalize_response_line(line: &str) -> Option<String> {
    let raw = line.trim_end_matches('\r');
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!-- patch:")
        || trimmed.starts_with("<!-- /patch:")
        || trimmed.starts_with("<!-- agent:")
        || trimmed.starts_with("<!-- /agent:")
    {
        return None;
    }
    if let Some(stripped) = raw.strip_suffix(" (HEAD)") {
        let trimmed = stripped.trim_start();
        if trimmed.starts_with("### Re:") || trimmed.starts_with("**Re:") && trimmed.ends_with("**")
        {
            return Some(stripped.to_string());
        }
    }
    let trimmed_start = raw.trim_start();
    if let Some(stripped) = trimmed_start.strip_prefix("❯ ") {
        let indent_len = raw.len() - trimmed_start.len();
        return Some(format!("{}{}", &raw[..indent_len], stripped));
    }
    Some(raw.to_string())
}

/// Commit the document at `file_path` to git (Fix 4: plugin post-apply commit).
///
/// Defense-in-depth guarantee: editor plugins call this after successfully applying
/// a patch so the agent response is committed even when the shell-side `--commit`
/// was skipped (e.g., IPC timeout path). Uses a minimal inline git flow rather than
/// the full `git::commit` (which lives in the binary crate) — stages the document
/// file and commits with a timestamp message.
///
/// Returns `true` on success, `false` on failure (null pointer, invalid UTF-8, git error).
///
/// # Safety
///
/// `file_path` must be a valid NUL-terminated UTF-8 string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_commit(file_path: *const c_char) -> i32 {
    if file_path.is_null() {
        return 0;
    }
    let path_str = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let path = std::path::Path::new(path_str);
    ffi_git_commit(path) as i32
}

/// Minimal git commit for the FFI context (no full git module dependency).
/// Stages the snapshot if present (via hash-object + update-index), falls back
/// to `git add`, then commits. Skips hooks, HEAD markers, and ops logging — those
/// live in the binary's git::commit. This is a best-effort backstop only.
fn ffi_git_commit(file: &std::path::Path) -> bool {
    let parent = file.parent().unwrap_or(file);
    // Unset git env vars inherited from parent hooks (GIT_DIR etc.) so git
    // discovers the correct repo from current_dir rather than the outer repo.
    let git_root_out = std::process::Command::new("git")
        .current_dir(parent)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .args(["rev-parse", "--show-toplevel"])
        .output();
    let git_root = match git_root_out {
        Ok(o) if o.status.success() => {
            std::path::PathBuf::from(String::from_utf8_lossy(&o.stdout).trim().to_string())
        }
        _ => return false,
    };

    let doc_name = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    // Use unix timestamp (chrono not available in lib crate); full datetime in binary git::commit
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let msg = format!("agent-doc({}): {}", doc_name, secs);

    // Stage the document file
    let add_ok = std::process::Command::new("git")
        .current_dir(&git_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .args(["add", &file.to_string_lossy()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !add_ok {
        eprintln!(
            "[ffi] agent_doc_commit: git add failed for {}",
            file.display()
        );
        return false;
    }

    std::process::Command::new("git")
        .current_dir(&git_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .args(["commit", "-m", &msg, "--no-verify"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the agent-doc library version.
///
/// Returns a NUL-terminated string like "0.26.1".
/// Caller must free with `agent_doc_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_version() -> *mut c_char {
    CString::new(env!("CARGO_PKG_VERSION")).unwrap().into_raw()
}

/// Process-global lazily state-backbone event ledger backing the FFI
/// state-projection exports. Phase 1 of `tasks/software/plan-lazily-ffi-state-projection.md`
/// (`#lzffistate1`): plugins become thin event-reporters + projection-renderers while
/// the binary owns the durable FSMs. Default-initialized on first access.
static STATE_LEDGER: std::sync::OnceLock<std::sync::Mutex<EventLedger>> =
    std::sync::OnceLock::new();

fn state_ledger() -> &'static std::sync::Mutex<EventLedger> {
    STATE_LEDGER.get_or_init(|| std::sync::Mutex::new(EventLedger::new()))
}

/// Stamp a queue `StateFact` reported without an explicit `hosting_epoch` with
/// the document's current hosting epoch (`#xdocsuper3`). The binary owns the
/// gate so live producers stay thin: a queue fact reported during the current
/// hosting carries that epoch and is rejected as stale after a host/switch.
/// Facts that already carry an explicit `hosting_epoch` (e.g. an intentional
/// stale-replay test) are left untouched, as are non-queue facts.
fn stamp_queue_fact_hosting_epoch(ledger: &EventLedger, event: &mut StateEvent) {
    use agent_doc_state_backbone::StateFact;
    let current = ledger.document_hosting_epoch(event.document_hash());
    let Some(current) = current else {
        return;
    };
    match &mut event.fact {
        StateFact::QueueHeadSelected { hosting_epoch, .. }
        | StateFact::QueueHeadDeferred { hosting_epoch, .. }
        | StateFact::QueueHeadCompleted { hosting_epoch, .. }
        | StateFact::QueueWorklistProjected { hosting_epoch, .. }
            if hosting_epoch.is_none() =>
        {
            *hosting_epoch = Some(current);
        }
        _ => {}
    }
}

/// Read the binary's lazily-powered state projection for a document.
///
/// Returns a NUL-terminated JSON string of `DocumentStateProjection`
/// (document/queue/closeout/transport/supervisor/route/proof slices) for the given
/// `document_hash`, or `"null"` when no events have been recorded for it. The
/// projection is recomputed from the in-process event ledger on every call. Plugins
/// render this without re-deriving the durable FSMs (FFI-first rule).
///
/// Caller must free the returned pointer with `agent_doc_free_string`.
///
/// # Safety
///
/// `document_hash` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_state_projection(document_hash: *const c_char) -> *mut c_char {
    let doc_hash = match unsafe { CStr::from_ptr(document_hash) }.to_str() {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_state_projection: non-UTF-8 document_hash; returning null: {err}"
            );
            return CString::new("null").unwrap().into_raw();
        }
    };
    let json = match state_ledger().lock() {
        Ok(ledger) => match ledger.project().document(doc_hash) {
            Some(doc) => serde_json::to_string(doc).unwrap_or_else(|err| {
                eprintln!(
                    "[state-projection] agent_doc_state_projection: serialize failed for {doc_hash}: {err}"
                );
                "null".to_string()
            }),
            None => "null".to_string(),
        },
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_state_projection: ledger lock poisoned: {err}"
            );
            "null".to_string()
        }
    };
    CString::new(json)
        .unwrap_or_else(|_| CString::new("null").unwrap())
        .into_raw()
}

/// Record a lazily state-backbone event from a plugin.
///
/// `fact_json` must be a JSON object deserializable as a `StateEvent`
/// (`{ "event_id": "...", "fact": { "type": "baseline_saved", ... } }`). The event
/// is appended to the in-process ledger; the projection's `seen_event_ids` dedupe
/// absorbs replays. Returns `1` on success, `0` on a parse/lock failure (logged to
/// stderr). Plugins trigger transitions by reporting events here rather than running
/// durable FSMs in-process (FFI-first rule).
///
/// # Safety
///
/// `document_hash` and `fact_json` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_record_state_event(
    document_hash: *const c_char,
    fact_json: *const c_char,
) -> i32 {
    let doc_hash = match unsafe { CStr::from_ptr(document_hash) }.to_str() {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_record_state_event: non-UTF-8 document_hash: {err}"
            );
            return 0;
        }
    };
    let fact_str = match unsafe { CStr::from_ptr(fact_json) }.to_str() {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_record_state_event: non-UTF-8 fact_json for {doc_hash}: {err}"
            );
            return 0;
        }
    };
    let mut event: StateEvent = match serde_json::from_str(fact_str) {
        Ok(e) => e,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_record_state_event: failed to parse StateEvent JSON for {doc_hash}: {err}"
            );
            return 0;
        }
    };
    match state_ledger().lock() {
        Ok(mut ledger) => {
            // `#xdocsuper3`: the binary owns the hosting-epoch gate (FFI-first).
            // Queue facts reported by a live producer without an explicit
            // `hosting_epoch` are stamped with the document's CURRENT hosting
            // epoch, so a later host/switch makes them stale automatically and
            // a producer never has to track the epoch itself.
            stamp_queue_fact_hosting_epoch(&ledger, &mut event);
            ledger.append(event);
            1
        }
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_record_state_event: ledger lock poisoned for {doc_hash}: {err}"
            );
            0
        }
    }
}

/// Subscribe to the agent-doc state projection for a document (`#lazilystatesync2`).
///
/// `last_epoch` is the caller's last-seen lazily-spec epoch for this document.
/// The return value is a NUL-terminated JSON message in the lazily-spec wire
/// envelope:
/// - `{ "type": "snapshot", ... }` when `last_epoch == 0` (cold read) or the
///   document has no state yet — a full graph image the mirror applies once.
/// - `{ "type": "delta", "base_epoch": <last_epoch>, "epoch": <current>, "ops": [...] }`
///   when `0 < last_epoch < current_epoch` — the ordered ops the mirror applies
///   verbatim to converge from `last_epoch` to current.
/// - `{ "type": "delta", "ops": [], ... }` when the caller is already current.
///
/// The projection is a pure fold of deduped events, so delta application is
/// deterministic and idempotent: a re-emit/replay yields an empty (no-op) delta.
/// Because the ledger is append-only within a process lifetime, any
/// `last_epoch <= current_epoch` is satisfiable without a resync.
///
/// This complements [`agent_doc_state_projection`], which stays as the full
/// `DocumentStateProjection` JSON for round-trip/human reads (the cold path).
/// Plugins that want reactive updates should subscribe here and apply deltas to a
/// lazily-kt / lazily-js mirror graph instead of re-rendering the snapshot.
///
/// Caller must free the returned pointer with `agent_doc_free_string`.
///
/// # Safety
///
/// `document_hash` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_state_subscribe(
    document_hash: *const c_char,
    last_epoch: u64,
) -> *mut c_char {
    let doc_hash = match unsafe { CStr::from_ptr(document_hash) }.to_str() {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "[state-projection] agent_doc_state_subscribe: non-UTF-8 document_hash; returning null: {err}"
            );
            return CString::new("null").unwrap().into_raw();
        }
    };
    let json = match state_ledger().lock() {
        Ok(ledger) => agent_doc_state_wire::subscribe(&ledger, doc_hash, last_epoch).to_json(),
        Err(err) => {
            eprintln!("[state-projection] agent_doc_state_subscribe: ledger lock poisoned: {err}");
            "null".to_string()
        }
    };
    CString::new(json)
        .unwrap_or_else(|_| CString::new("null").unwrap())
        .into_raw()
}

/// Inspect one actor through the project controller and return the same JSON
/// shape as `agent-doc admin inspect --json`.
///
/// Null or empty `project_root`, `document_path`, `session_id`, and `pane_id`
/// values are treated as absent.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_inspect_json(
    project_root: *const c_char,
    document_path: *const c_char,
    session_id: *const c_char,
    pane_id: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            optional_path(unsafe { optional_ffi_string(document_path, "document_path") }?);
        let session_id = unsafe { optional_ffi_string(session_id, "session_id") }?;
        let pane_id = unsafe { optional_ffi_string(pane_id, "pane_id") }?;
        let root = resolve_admin_root(project_root.as_deref(), document_path.as_deref())?;
        agent_doc_controller_io::project_controller::inspect_actor(
            &root,
            document_path.as_deref(),
            session_id.as_deref(),
            pane_id.as_deref(),
        )
    })())
}

/// Return the Project Controller-owned tmux focus projection for a project.
///
/// The response reports an active document only when the configured tmux
/// session's current window is `agent-doc`; other tmux windows intentionally
/// return an inactive projection so editor selection is not recalled from a
/// stale agent-doc pane.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_tmux_focus_state_json(
    project_root: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let root = resolve_admin_root(project_root.as_deref(), None)?;
        agent_doc_controller_io::project_controller::tmux_focus_state(&root)
    })())
}

/// Focus the actor pane for a document through the Project Controller.
///
/// This centralizes pane selection behind the controller so JetBrains does not
/// run `agent-doc focus` or raw tmux commands for editor focus handoff.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_focus_document_pane_json(
    project_root: *const c_char,
    document_path: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            PathBuf::from(unsafe { required_ffi_string(document_path, "document_path") }?);
        let root = resolve_admin_root(project_root.as_deref(), Some(&document_path))?;
        agent_doc_controller_io::project_controller::focus_document_pane(&root, &document_path)
    })())
}

/// Sync the tmux layout through the Project Controller.
///
/// `columns_json` is a JSON array of column strings using the same comma-joined
/// column representation as `agent-doc sync --col`.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_sync_tmux_layout_json(
    project_root: *const c_char,
    columns_json: *const c_char,
    window: *const c_char,
    focus: *const c_char,
    no_autostart: c_int,
    exact_visible: c_int,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let columns_json = unsafe { required_ffi_string(columns_json, "columns_json") }?;
        let window = unsafe { optional_ffi_string(window, "window") }?;
        let focus = unsafe { optional_ffi_string(focus, "focus") }?;
        let columns: Vec<String> =
            serde_json::from_str(&columns_json).context("parse columns_json")?;
        let focus_path = focus.as_deref().map(Path::new);
        let root = resolve_admin_root(project_root.as_deref(), focus_path)?;
        agent_doc_controller_io::project_controller::sync_tmux_layout(
            &root,
            agent_doc_controller_io::project_controller::ControllerTmuxLayoutSyncInvocation {
                columns,
                window,
                focus,
                no_autostart: no_autostart != 0,
                exact_visible: exact_visible != 0,
            },
        )
    })())
}

/// Pause, resume, or drain queue work through the project controller and return
/// the same receipt JSON shape as `agent-doc admin queue ... --json`.
///
/// Pass `observed_generation = -1` when no generation guard is supplied.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_queue_control_json(
    project_root: *const c_char,
    document_path: *const c_char,
    action: *const c_char,
    observed_generation: i64,
    reason: *const c_char,
    item_id: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            optional_path(unsafe { optional_ffi_string(document_path, "document_path") }?);
        let action = unsafe { required_ffi_string(action, "action") }?;
        let observed_generation = optional_generation(observed_generation, "observed_generation")?;
        let reason = unsafe { optional_ffi_string(reason, "reason") }?;
        let item_id = unsafe { optional_ffi_string(item_id, "item_id") }?;
        let root = resolve_admin_root(project_root.as_deref(), document_path.as_deref())?;
        agent_doc_controller_io::project_controller::control_queue(
            &root,
            document_path.as_deref(),
            &action,
            observed_generation,
            reason.as_deref(),
            item_id.as_deref(),
        )
    })())
}

/// Reap a stale actor through the project controller and return the same
/// receipt JSON shape as `agent-doc admin reap --json`.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_reap_json(
    project_root: *const c_char,
    document_path: *const c_char,
    session_id: *const c_char,
    pane_id: *const c_char,
    observed_generation: i64,
    reason: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            optional_path(unsafe { optional_ffi_string(document_path, "document_path") }?);
        let session_id = unsafe { optional_ffi_string(session_id, "session_id") }?;
        let pane_id = unsafe { optional_ffi_string(pane_id, "pane_id") }?;
        let observed_generation = required_generation(observed_generation, "observed_generation")?;
        let reason = unsafe { required_ffi_string(reason, "reason") }?;
        let root = resolve_admin_root(project_root.as_deref(), document_path.as_deref())?;
        agent_doc_controller_io::project_controller::admin_reap(
            &root,
            document_path.as_deref(),
            session_id.as_deref(),
            pane_id.as_deref(),
            observed_generation,
            &reason,
        )
    })())
}

/// Handoff a document actor through the project controller and return the same
/// receipt JSON shape as `agent-doc admin handoff --json`.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_handoff_json(
    project_root: *const c_char,
    document_path: *const c_char,
    to_pane: *const c_char,
    observed_generation: i64,
    reason: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            PathBuf::from(unsafe { required_ffi_string(document_path, "document_path") }?);
        let to_pane = unsafe { required_ffi_string(to_pane, "to_pane") }?;
        let observed_generation = required_generation(observed_generation, "observed_generation")?;
        let reason = unsafe { required_ffi_string(reason, "reason") }?;
        let root = resolve_admin_root(project_root.as_deref(), Some(document_path.as_path()))?;
        agent_doc_controller_io::project_controller::admin_handoff(
            &root,
            &document_path,
            &to_pane,
            observed_generation,
            &reason,
        )
    })())
}

/// Repair controller compatibility projections and return the same receipt JSON
/// shape as `agent-doc admin repair-projection --json`.
///
/// Pass `observed_generation = -1` when no generation guard is supplied.
///
/// # Safety
///
/// Non-null string pointers must be NUL-terminated UTF-8. Returned pointers must
/// be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_admin_repair_projection_json(
    project_root: *const c_char,
    document_path: *const c_char,
    projection: *const c_char,
    observed_generation: i64,
    reason: *const c_char,
) -> FfiJsonResult {
    ffi_json_from_result((|| -> anyhow::Result<_> {
        let project_root = unsafe { optional_ffi_string(project_root, "project_root") }?;
        let document_path =
            optional_path(unsafe { optional_ffi_string(document_path, "document_path") }?);
        let projection = unsafe { required_ffi_string(projection, "projection") }?;
        let observed_generation = optional_generation(observed_generation, "observed_generation")?;
        let reason = unsafe { optional_ffi_string(reason, "reason") }?;
        let root = resolve_admin_root(project_root.as_deref(), document_path.as_deref())?;
        agent_doc_controller_io::project_controller::repair_projection(
            &root,
            document_path.as_deref(),
            &projection,
            observed_generation,
            reason.as_deref(),
        )
    })())
}

/// Walk up from `path` to find the nearest ancestor containing `.agent-doc/`.
/// Delegates to [`agent_doc_fs::find_project_root`].
fn find_project_root_ffi(path: &std::path::Path) -> Option<std::path::PathBuf> {
    agent_doc_fs::find_project_root(path)
}

/// Resolve a file's agent-doc project root and the path relative to that root.
///
/// Walks up from the file's parent directory looking for the nearest ancestor
/// containing a `.agent-doc/` directory. Editor plugins use this to detect when
/// a file inside a submodule belongs to its own agent-doc project (with its own
/// sessions, snapshots, etc.) rather than the enclosing workspace.
///
/// Returns `FfiProjectPath` with:
/// - `project_root`: absolute path of the nearest `.agent-doc/`-containing
///   ancestor, or null if none exists.
/// - `relative_path`: `file_path` made relative to `project_root`, or null when
///   `project_root` is null or the relative computation fails.
///
/// Both strings (when non-null) must be freed with [`agent_doc_free_string`].
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_resolve_project_path(
    file_path: *const c_char,
) -> FfiProjectPath {
    let null_result = FfiProjectPath {
        project_root: ptr::null_mut(),
        relative_path: ptr::null_mut(),
    };

    let path_str = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return null_result,
    };
    let path = std::path::Path::new(path_str);

    // Canonicalize when possible so ancestor walks resolve symlinks (submodule
    // worktrees often live inside symlinked trees). Fall back to the raw path
    // when canonicalization fails (e.g., file does not yet exist) — the walk
    // still works on lexical parents.
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    let project_root = match find_project_root_ffi(&resolved) {
        Some(root) => root,
        None => return null_result,
    };

    let rel = match resolved.strip_prefix(&project_root) {
        Ok(p) => p.to_path_buf(),
        Err(_) => return null_result,
    };

    let root_c = match CString::new(project_root.to_string_lossy().as_ref()) {
        Ok(s) => s,
        Err(_) => return null_result,
    };
    let rel_c = match CString::new(rel.to_string_lossy().as_ref()) {
        Ok(s) => s,
        Err(_) => return null_result,
    };

    FfiProjectPath {
        project_root: root_c.into_raw(),
        relative_path: rel_c.into_raw(),
    }
}

/// Free a string returned by any `agent_doc_*` function.
///
/// # Safety
///
// `agent_doc_free_string` and `agent_doc_free_state` moved to
// `agent_doc_ffi` (Wave 5 / `#k9e1` proof-of-concept). They are
// re-exported below via `pub use agent_doc_ffi::*;`. The
// `force_link_core_ffi_symbols` function below references them to
// prevent the static linker from stripping them out of the main cdylib.
pub use agent_doc_ffi::*;

#[derive(Debug, serde::Deserialize)]
struct IpcNodePatchJson {
    component: String,
    node_key: String,
    op: String,
    content: Option<String>,
    expected_content: Option<String>,
    before: Option<String>,
    after: Option<String>,
    #[serde(default)]
    order: Vec<String>,
}

fn parse_node_patch_op(
    op: &str,
) -> Result<agent_doc_markdown_ast::mutations::MutationNodePatchOp, String> {
    use agent_doc_markdown_ast::mutations::MutationNodePatchOp;
    match op {
        "insert" => Ok(MutationNodePatchOp::Insert),
        "remove" => Ok(MutationNodePatchOp::Remove),
        "replace" => Ok(MutationNodePatchOp::Replace),
        "move" => Ok(MutationNodePatchOp::Move),
        "strike" => Ok(MutationNodePatchOp::Strike),
        "unstrike" => Ok(MutationNodePatchOp::Unstrike),
        other => Err(format!("unsupported node patch op `{other}`")),
    }
}

fn parse_node_patches_json(
    raw: &str,
) -> Result<Vec<agent_doc_markdown_ast::mutations::MutationNodePatch>, String> {
    serde_json::from_str::<Vec<IpcNodePatchJson>>(raw)
        .map_err(|err| format!("invalid node_patches JSON: {err}"))?
        .into_iter()
        .map(|patch| {
            Ok(agent_doc_markdown_ast::mutations::MutationNodePatch {
                component: patch.component,
                node_key: patch.node_key,
                op: parse_node_patch_op(&patch.op)?,
                content: patch.content,
                expected_content: patch.expected_content,
                before: patch.before,
                after: patch.after,
                order: patch.order,
            })
        })
        .collect()
}

/// Apply node-keyed IPC patches to a live editor document snapshot.
///
/// # Safety
///
/// `doc` and `node_patches_json` must be non-null, NUL-terminated UTF-8 strings.
/// The returned pointers must be freed with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_apply_node_patches(
    doc: *const c_char,
    node_patches_json: *const c_char,
) -> FfiPatchResult {
    if doc.is_null() {
        return ffi_patch_err("doc pointer is null");
    }
    if node_patches_json.is_null() {
        return ffi_patch_err("node_patches_json pointer is null");
    }
    let doc = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(value) => value,
        Err(err) => return ffi_patch_err(&format!("doc is not valid UTF-8: {err}")),
    };
    let raw_patches = match unsafe { CStr::from_ptr(node_patches_json) }.to_str() {
        Ok(value) => value,
        Err(err) => {
            return ffi_patch_err(&format!("node_patches_json is not valid UTF-8: {err}"));
        }
    };
    let patches = match parse_node_patches_json(raw_patches) {
        Ok(value) => value,
        Err(err) => return ffi_patch_err(&err),
    };
    match agent_doc_markdown_ast::mutations::apply_node_patches(doc, &patches) {
        Ok(updated) => ffi_patch_ok(updated),
        Err(err) => ffi_patch_err(&err.to_string()),
    }
}

/// Prevent the static linker from stripping the `agent_doc_ffi`
/// symbols out of `libagent_doc.{so,dylib,dll}`. Called from `lib.rs`
/// via a constructor-style reference path so editor plugins
/// (JetBrains, VS Code) continue to find the symbols at runtime.
///
/// The function itself is never called — we just need the symbol
/// references to exist in the main crate's compilation unit so the
/// linker keeps them in the cdylib export table.
#[allow(dead_code)]
fn force_link_core_ffi_symbols() {
    use agent_doc_ffi::{
        FfiComponentList, FfiMergeResult, FfiPatchResult, agent_doc_apply_patch,
        agent_doc_apply_patch_with_boundary, agent_doc_apply_patch_with_caret,
        agent_doc_converge_queue_auto, agent_doc_crdt_merge, agent_doc_free_state,
        agent_doc_free_string, agent_doc_merge_crdt, agent_doc_merge_frontmatter,
        agent_doc_normalize_template_structure, agent_doc_parse_components,
        agent_doc_reposition_boundary_to_end, agent_doc_reposition_boundary_to_end_preserve_head,
        agent_doc_reposition_boundary_to_end_preserve_head_with_id,
        agent_doc_reposition_boundary_to_end_with_id, agent_doc_visual_tokens_json,
    };
    let _: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> FfiPatchResult = agent_doc_apply_patch;
    let _: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        i32,
    ) -> FfiPatchResult = agent_doc_apply_patch_with_caret;
    let _: unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> FfiPatchResult = agent_doc_apply_patch_with_boundary;
    let _: unsafe extern "C" fn(*mut c_char) = agent_doc_free_string;
    let _: unsafe extern "C" fn(*mut u8, usize) = agent_doc_free_state;
    let _: unsafe extern "C" fn(*const c_char) -> FfiComponentList = agent_doc_parse_components;
    let _: unsafe extern "C" fn(*const c_char) -> *mut c_char = agent_doc_visual_tokens_json;
    let _: unsafe extern "C" fn(*const c_char, *const c_char) -> FfiPatchResult =
        agent_doc_merge_frontmatter;
    let _: unsafe extern "C" fn(*const c_char, std::os::raw::c_int) -> FfiPatchResult =
        agent_doc_converge_queue_auto;
    let _: unsafe extern "C" fn(*const c_char) -> FfiPatchResult =
        agent_doc_normalize_template_structure;
    let _: unsafe extern "C" fn(*const u8, usize, *const c_char, *const c_char) -> FfiMergeResult =
        agent_doc_crdt_merge;
    let _: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char) -> *mut c_char =
        agent_doc_merge_crdt;
    // Editor-as-replica FFI (#crdtauth2).
    let _: unsafe extern "C" fn(u64, *const u8, usize) -> i32 = agent_doc_replica_open;
    let _: unsafe extern "C" fn(u64, u32, u32, *const c_char) -> i32 =
        agent_doc_replica_apply_local;
    let _: unsafe extern "C" fn(u64) -> *mut c_char = agent_doc_replica_text;
    let _: unsafe extern "C" fn(u64, *mut usize) -> *mut u8 = agent_doc_replica_state_vector;
    let _: unsafe extern "C" fn(u64, *const u8, usize, *mut usize) -> *mut u8 =
        agent_doc_replica_diff;
    let _: unsafe extern "C" fn(u64, *const u8, usize) -> i32 = agent_doc_replica_apply_update;
    let _: unsafe extern "C" fn(u64, *mut usize) -> *mut u8 = agent_doc_replica_encode_state;
    let _: unsafe extern "C" fn(u64) -> i32 = agent_doc_replica_close;
    let _: unsafe extern "C" fn(*const c_char) -> FfiPatchResult =
        agent_doc_reposition_boundary_to_end;
    let _: unsafe extern "C" fn(*const c_char, *const c_char) -> FfiPatchResult =
        agent_doc_reposition_boundary_to_end_with_id;
    let _: unsafe extern "C" fn(*const c_char) -> FfiPatchResult =
        agent_doc_reposition_boundary_to_end_preserve_head;
    let _: unsafe extern "C" fn(*const c_char, *const c_char) -> FfiPatchResult =
        agent_doc_reposition_boundary_to_end_preserve_head_with_id;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_turn_phase_makes_document_model_authoritative() {
        use agent_doc_sqlite::state_store::ActorState;
        use agent_doc_turn::CyclePhase;
        // Model Busy/Blocked + a terminal projection → in-flight; a lagging
        // terminal projection cannot hide a live turn.
        assert_eq!(
            resolve_turn_phase(Some(ActorState::Busy), CyclePhase::Committed),
            CyclePhase::PreflightStarted
        );
        assert_eq!(
            resolve_turn_phase(Some(ActorState::Blocked), CyclePhase::Abandoned),
            CyclePhase::PreflightStarted
        );
        // Model Busy + an open projected phase → keep the fine awaiting/persisting label.
        assert_eq!(
            resolve_turn_phase(Some(ActorState::Busy), CyclePhase::ResponseCaptured),
            CyclePhase::ResponseCaptured
        );
        // Model not-in-flight → idle regardless of a stale open projection.
        for st in [
            ActorState::Ready,
            ActorState::Closed,
            ActorState::Starting,
            ActorState::WaitingInput,
        ] {
            assert_eq!(
                resolve_turn_phase(Some(st), CyclePhase::PreflightStarted),
                CyclePhase::Committed,
                "{st:?} must not let a stale projection assert an in-flight turn"
            );
        }
        // No document model (cold start) → state-backbone projection.
        assert_eq!(
            resolve_turn_phase(None, CyclePhase::PreflightStarted),
            CyclePhase::PreflightStarted
        );
        assert_eq!(
            resolve_turn_phase(None, CyclePhase::Committed),
            CyclePhase::Committed
        );
    }

    #[test]
    fn turn_projection_uses_state_backbone_over_cycle_sidecar() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        let content = "---\nagent_doc_session: session-1\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let sidecar_path = std::fs::read_dir(tmp.path().join(".agent-doc/state/cycles"))
            .unwrap()
            .next()
            .expect("cycle state sidecar")
            .unwrap()
            .path();
        let stale_open_sidecar = std::fs::read(&sidecar_path).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        std::fs::write(&sidecar_path, stale_open_sidecar).unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::PreflightStarted
        );

        let doc_c = CString::new(doc.to_string_lossy().as_ref()).unwrap();
        let projection_ptr = unsafe { agent_doc_turn_projection(doc_c.as_ptr()) };
        let projection = unsafe { CStr::from_ptr(projection_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(projection_ptr) });
        let projection: serde_json::Value = serde_json::from_str(&projection).unwrap();

        assert_eq!(projection["turn_in_flight"], false);
        assert_eq!(projection["state"], "idle");
    }

    #[test]
    fn state_projection_ffi_round_trip() {
        use agent_doc_state_backbone::{StateEvent, StateFact};

        let doc_hash = "state_projection_ffi_round_trip_doc";
        let event = StateEvent::new(
            "evt-ffi-roundtrip-1",
            StateFact::BaselineSaved {
                document_hash: doc_hash.to_string(),
                cycle_id: "cycle-1".to_string(),
                baseline_hash: "baseline-1".to_string(),
                baseline_path: None,
            },
        );
        let fact_json = CString::new(serde_json::to_string(&event).unwrap()).unwrap();
        let doc_hash_c = CString::new(doc_hash).unwrap();

        let rc = unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), fact_json.as_ptr()) };
        assert_eq!(
            1, rc,
            "record_state_event should succeed on valid StateEvent JSON"
        );

        let proj_ptr = unsafe { agent_doc_state_projection(doc_hash_c.as_ptr()) };
        let proj = unsafe { CStr::from_ptr(proj_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(proj_ptr) });

        assert_ne!(
            "null", proj,
            "projection should exist after recording an event"
        );
        assert!(
            proj.contains(doc_hash),
            "projection should reference the document hash: {proj}"
        );

        // Replaying the same event_id still appends (returns 1); the projection's
        // seen_event_ids dedupe absorbs the duplicate on the next project() call.
        let replay_rc =
            unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), fact_json.as_ptr()) };
        assert_eq!(1, replay_rc, "replay record should still return success");

        // Unknown document_hash yields "null".
        let missing_c = CString::new("no-such-doc-hash").unwrap();
        let missing_ptr = unsafe { agent_doc_state_projection(missing_c.as_ptr()) };
        let missing = unsafe { CStr::from_ptr(missing_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(missing_ptr) });
        assert_eq!(
            "null", missing,
            "unknown document_hash should project to null"
        );
    }

    #[test]
    fn state_projection_ffi_accepts_editor_patch_applied_event() {
        let doc_hash = "state_projection_ffi_applied_doc";
        let event_json = CString::new(format!(
            r#"{{
                "event_id":"{doc_hash}:applied",
                "fact":{{
                    "type":"editor_patch_applied",
                    "document_hash":"{doc_hash}",
                    "patch_id":"patch-applied",
                    "actor_generation":1
                }}
            }}"#
        ))
        .unwrap();
        let doc_hash_c = CString::new(doc_hash).unwrap();

        let rc = unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), event_json.as_ptr()) };
        assert_eq!(1, rc, "applied lazily receipt events should be accepted");
    }

    #[test]
    fn state_projection_ffi_rejects_unknown_legacy_receipt_event() {
        let doc_hash = "state_projection_ffi_legacy_receipt_doc";
        let event_json = CString::new(format!(
            r#"{{
                "event_id":"{doc_hash}:legacy-receipt",
                "fact":{{
                    "type":"legacy_editor_receipt_observed",
                    "document_hash":"{doc_hash}",
                    "patch_id":"patch-legacy",
                    "actor_generation":1
                }}
            }}"#
        ))
        .unwrap();
        let doc_hash_c = CString::new(doc_hash).unwrap();

        let rc = unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), event_json.as_ptr()) };
        assert_eq!(0, rc, "unknown legacy receipt events must be rejected");
    }

    #[test]
    fn editor_patch_applied_ffi_records_lazily_receipt() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "hello\n").unwrap();
        let canonical = file.canonicalize().unwrap();
        let doc_hash = agent_doc_hash::document_id_for_path(&canonical);
        let file_c = CString::new(canonical.to_string_lossy().as_ref()).unwrap();
        let patch_c = CString::new("patch-applied-ffi").unwrap();

        let rc = unsafe { agent_doc_editor_patch_applied(file_c.as_ptr(), patch_c.as_ptr(), 7) };
        assert_eq!(1, rc, "applied receipt should be recorded");

        let doc_hash_c = CString::new(doc_hash.clone()).unwrap();
        let proj_ptr = unsafe { agent_doc_state_projection(doc_hash_c.as_ptr()) };
        let proj = unsafe { CStr::from_ptr(proj_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(proj_ptr) });
        assert!(
            proj.contains(r#""phase":"applied""#),
            "projection should contain applied transport receipt: {proj}"
        );

        let durable =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(tmp.path())
                .unwrap();
        let document = durable
            .document(&doc_hash)
            .expect("durable receipt should project for the document");
        assert_eq!(
            document
                .transport
                .patches
                .get("patch-applied-ffi")
                .map(|patch| patch.phase),
            Some(agent_doc_state_backbone::TransportPatchPhase::Applied)
        );
    }

    #[test]
    fn editor_patch_rejected_ffi_records_durable_lazily_receipt() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "hello\n").unwrap();
        let canonical = file.canonicalize().unwrap();
        let doc_hash = agent_doc_hash::document_id_for_path(&canonical);
        let file_c = CString::new(canonical.to_string_lossy().as_ref()).unwrap();
        let patch_c = CString::new("patch-rejected-ffi").unwrap();
        let reason_c = CString::new("file_apply_failed").unwrap();

        let rc = unsafe {
            agent_doc_editor_patch_rejected(file_c.as_ptr(), patch_c.as_ptr(), 8, reason_c.as_ptr())
        };
        assert_eq!(1, rc, "rejected receipt should be recorded durably");

        let durable =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(tmp.path())
                .unwrap();
        let document = durable
            .document(&doc_hash)
            .expect("durable rejection should project for the document");
        assert_eq!(
            document
                .transport
                .patches
                .get("patch-rejected-ffi")
                .map(|patch| patch.phase),
            Some(agent_doc_state_backbone::TransportPatchPhase::Rejected)
        );
        assert_eq!(
            document.transport.last_rejected_reason.as_deref(),
            Some("file_apply_failed")
        );
    }

    #[test]
    fn turn_projection_ffi_defaults_to_idle_for_unknown_document() {
        // No cycle state for this path → idle projection, valid JSON, not in flight.
        let path = CString::new("/nonexistent/agent-doc/turnproj.md").unwrap();
        let ptr = unsafe { agent_doc_turn_projection(path.as_ptr()) };
        let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        drop(unsafe { CString::from_raw(ptr) });

        let value: serde_json::Value = serde_json::from_str(&json).expect("valid projection JSON");
        assert_eq!(value["state"], "idle");
        assert_eq!(value["turn_in_flight"], false);
        assert_eq!(value["transition_authority"], "project_controller");
    }

    #[test]
    fn record_state_event_stamps_and_gates_queue_hosting_epoch() {
        use agent_doc_state_backbone::{StateEvent, StateFact};

        let doc_hash = "ffi_hosting_epoch_gate_doc";
        let doc_hash_c = CString::new(doc_hash).unwrap();

        let record = |event: &StateEvent| {
            let json = CString::new(serde_json::to_string(event).unwrap()).unwrap();
            let rc = unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), json.as_ptr()) };
            assert_eq!(1, rc, "record_state_event should succeed");
        };
        let projection = || {
            let ptr = unsafe { agent_doc_state_projection(doc_hash_c.as_ptr()) };
            let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
            drop(unsafe { CString::from_raw(ptr) });
            json
        };

        // Supervisor begins hosting on a pane (hosting epoch 1).
        record(&StateEvent::new(
            "ffi-host-1",
            StateFact::SupervisorHosting {
                document_hash: doc_hash.to_string(),
                pane_session: "%9:ffi-session".to_string(),
                lease_epoch: 1,
            },
        ));
        // A live producer reports a completed head WITHOUT a hosting_epoch; the
        // binary stamps it with epoch 1 so it is accepted now.
        record(&StateEvent::new(
            "ffi-done-1",
            StateFact::QueueHeadCompleted {
                document_hash: doc_hash.to_string(),
                node_key: "ffi-answered-head".to_string(),
                backlog_id: None,
                hosting_epoch: None,
            },
        ));
        assert!(
            projection().contains("ffi-answered-head"),
            "in-hosting completed head should be accepted and projected"
        );

        // Supervisor re-hosts at a higher lease epoch (switch boundary): the
        // answered-head residue is dropped by the reset.
        record(&StateEvent::new(
            "ffi-host-2",
            StateFact::SupervisorHosting {
                document_hash: doc_hash.to_string(),
                pane_session: "%9:ffi-session".to_string(),
                lease_epoch: 2,
            },
        ));
        assert!(
            !projection().contains("ffi-answered-head"),
            "fresh host must drop the prior hosting's answered-head residue"
        );

        // A stale producer replays the answered head stamped with the OLD epoch;
        // it must be rejected, not re-injected.
        record(&StateEvent::new(
            "ffi-stale-done",
            StateFact::QueueHeadCompleted {
                document_hash: doc_hash.to_string(),
                node_key: "ffi-answered-head".to_string(),
                backlog_id: None,
                hosting_epoch: Some(1),
            },
        ));
        assert!(
            !projection().contains("ffi-answered-head"),
            "stale-epoch queue replay must not re-inject the answered head"
        );
    }

    #[test]
    fn state_subscribe_ffi_emits_snapshot_then_delta() {
        use agent_doc_state_backbone::{StateEvent, StateFact};

        let doc_hash = "state_subscribe_ffi_round_trip_doc";
        let doc_hash_c = CString::new(doc_hash).unwrap();

        // Cold read with no events: snapshot, epoch 0, empty graph.
        let cold_ptr = unsafe { agent_doc_state_subscribe(doc_hash_c.as_ptr(), 0) };
        let cold = unsafe { CStr::from_ptr(cold_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(cold_ptr) });
        assert!(
            cold.contains("\"type\":\"snapshot\""),
            "cold subscribe with no state must yield a snapshot: {cold}"
        );
        assert!(
            cold.contains("\"epoch\":0"),
            "empty cold read is epoch 0: {cold}"
        );

        // Record one baseline event.
        let baseline = StateEvent::new(
            "evt-subscribe-ffi-1",
            StateFact::BaselineSaved {
                document_hash: doc_hash.to_string(),
                cycle_id: "cycle-subscribe".to_string(),
                baseline_hash: "bl-subscribe".to_string(),
                baseline_path: None,
            },
        );
        let baseline_json = CString::new(serde_json::to_string(&baseline).unwrap()).unwrap();
        let rc =
            unsafe { agent_doc_record_state_event(doc_hash_c.as_ptr(), baseline_json.as_ptr()) };
        assert_eq!(1, rc, "record_state_event should succeed");

        // Cold read now carries the baseline node at epoch 1.
        let cold_ptr = unsafe { agent_doc_state_subscribe(doc_hash_c.as_ptr(), 0) };
        let cold = unsafe { CStr::from_ptr(cold_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(cold_ptr) });
        assert!(
            cold.contains("\"epoch\":1"),
            "one accepted event bumps epoch: {cold}"
        );
        assert!(
            cold.contains("agent_doc.document.baseline"),
            "cold snapshot must include the baseline type_tag: {cold}"
        );

        // Warm read at last_epoch=1 (caller is current) yields a no-op delta.
        let delta_ptr = unsafe { agent_doc_state_subscribe(doc_hash_c.as_ptr(), 1) };
        let delta = unsafe { CStr::from_ptr(delta_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        drop(unsafe { CString::from_raw(delta_ptr) });
        assert!(
            delta.contains("\"type\":\"delta\""),
            "warm read must yield a delta: {delta}"
        );
        assert!(
            delta.contains("\"base_epoch\":1") && delta.contains("\"epoch\":1"),
            "no-op delta keeps base_epoch == epoch == current: {delta}"
        );
        assert!(
            delta.contains("\"ops\":[]"),
            "caller-current delta must be empty: {delta}"
        );
    }

    #[test]
    fn record_editor_op_ffi_writes_base_keyed_sidecar() {
        use agent_doc_op_capture_io as op_capture;
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base_text = "hello world\n";
        let base_hash = agent_doc_hash::content_hash(base_text);

        let file_c = CString::new(doc.to_str().unwrap()).unwrap();
        let base_c = CString::new(base_hash.as_str()).unwrap();
        let insert_kind = CString::new("insert").unwrap();
        let delete_kind = CString::new("delete").unwrap();
        let text_c = CString::new("!").unwrap();

        // Insert op records (returns 1).
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                insert_kind.as_ptr(),
                5,
                text_c.as_ptr(),
                0,
            )
        };
        assert_eq!(rc, 1, "valid insert op must record");

        // Delete op appends within the same epoch (null insert_text is allowed).
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                delete_kind.as_ptr(),
                0,
                std::ptr::null(),
                3,
            )
        };
        assert_eq!(rc, 1, "valid delete op must record");

        // The consumer trusts the sidecar only against the matching base, and the
        // ops round-trip in editor order.
        let ops = op_capture::editor_ops_for_base(&doc, base_text)
            .unwrap()
            .expect("ops captured against the base must be trusted");
        assert_eq!(ops.len(), 2);
        assert_eq!(
            ops[0],
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: 5,
                text: "!".into()
            }
        );
        assert_eq!(
            ops[1],
            agent_doc_merge::crdt::EditorOp::Delete { offset: 0, len: 3 }
        );

        // Unknown kind and negative offset fail closed (return 0, record nothing).
        let bad_kind = CString::new("replace").unwrap();
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                bad_kind.as_ptr(),
                0,
                text_c.as_ptr(),
                0,
            )
        };
        assert_eq!(rc, 0, "unknown op_kind must fail closed");
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                insert_kind.as_ptr(),
                -1,
                text_c.as_ptr(),
                0,
            )
        };
        assert_eq!(rc, 0, "negative offset must fail closed");
        let ops = op_capture::editor_ops_for_base(&doc, base_text)
            .unwrap()
            .unwrap();
        assert_eq!(ops.len(), 2, "failed ops must not be recorded");
    }

    #[test]
    fn record_editor_op_ffi_preserves_non_ascii_byte_offsets() {
        use agent_doc_op_capture_io as op_capture;
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base_text = "café 日本 😀\n";
        let base_hash = agent_doc_hash::content_hash(base_text);
        let offset = "café ".len() as i64;
        let delete_len = "日本".len() as i64;

        let file_c = CString::new(doc.to_str().unwrap()).unwrap();
        let base_c = CString::new(base_hash.as_str()).unwrap();
        let insert_kind = CString::new("insert").unwrap();
        let delete_kind = CString::new("delete").unwrap();
        let insert_text = CString::new("世界").unwrap();

        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                delete_kind.as_ptr(),
                offset,
                std::ptr::null(),
                delete_len,
            )
        };
        assert_eq!(rc, 1, "valid non-ASCII delete op must record");
        let rc = unsafe {
            agent_doc_record_editor_op(
                file_c.as_ptr(),
                base_c.as_ptr(),
                insert_kind.as_ptr(),
                offset,
                insert_text.as_ptr(),
                0,
            )
        };
        assert_eq!(rc, 1, "valid non-ASCII insert op must record");

        let ops = op_capture::editor_ops_for_base(&doc, base_text)
            .unwrap()
            .expect("non-ASCII ops captured against the base must be trusted");
        assert_eq!(
            ops,
            vec![
                agent_doc_merge::crdt::EditorOp::Delete { offset: 6, len: 6 },
                agent_doc_merge::crdt::EditorOp::Insert {
                    offset: 6,
                    text: "世界".into()
                },
            ]
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("editor_op_recorded kind=delete offset=6 delete_len=6"),
            "delete byte evidence missing:\n{ops_log}"
        );
        assert!(
            ops_log.contains(
                "editor_op_recorded kind=insert offset=6 insert_bytes=6 insert_non_ascii=true"
            ),
            "insert byte evidence missing:\n{ops_log}"
        );
        assert!(
            !ops_log.contains('�'),
            "ops log should not contain mojibake:\n{ops_log}"
        );
    }

    #[test]
    fn sync_lock_acquire_decision_self_heals_wedged_holder() {
        use super::{SyncLockDecision, sync_lock_acquire_decision};
        let bound = DEFAULT_SYNC_LOCK_STALE_BOUND_MS;
        // Free guard → acquire.
        assert_eq!(
            sync_lock_acquire_decision(false, 0, 1_000, bound),
            SyncLockDecision::Acquire
        );
        // Held by a legitimately in-flight sync (5s < 45s bound) → defer, do NOT supersede.
        assert_eq!(
            sync_lock_acquire_decision(true, 1_000, 6_000, bound),
            SyncLockDecision::Defer
        );
        // Held just under the bound → still defer (boundary).
        assert_eq!(
            sync_lock_acquire_decision(true, 1_000, 1_000 + bound - 1, bound),
            SyncLockDecision::Defer
        );
        // Held past the bound by a wedged/dead holder → supersede with the held duration.
        assert_eq!(
            sync_lock_acquire_decision(true, 1_000, 1_000 + bound, bound),
            SyncLockDecision::SupersedeStaleHolder { held_ms: bound }
        );
        // Locked but holder age unknown (0) → defer; never supersede an unknown-age holder.
        assert_eq!(
            sync_lock_acquire_decision(true, 0, 999_999, bound),
            SyncLockDecision::Defer
        );
    }

    #[test]
    fn sync_try_lock_supersedes_a_guard_wedged_past_the_bound() {
        // Acquire, then simulate a holder that wedged ~1 minute ago by back-dating the
        // recorded acquisition time. A 45s-bound acquire must self-heal (supersede), and
        // a fresh acquire afterward must defer (the new holder is in-flight).
        assert_eq!(agent_doc_sync_try_lock(), 1, "free guard acquires");
        SYNC_LOCK_ACQUIRED_AT_MS.store(
            sync_lock_now_epoch_ms().saturating_sub(60_000),
            Ordering::SeqCst,
        );
        assert_eq!(
            agent_doc_sync_try_lock(),
            1,
            "a guard wedged past the bound is superseded, not deferred forever"
        );
        // The superseding acquire reset the timestamp to now, so a second contender defers.
        assert_eq!(
            agent_doc_sync_try_lock(),
            0,
            "a fresh in-flight holder still serializes later syncs"
        );
        agent_doc_sync_unlock();
        assert_eq!(
            SYNC_LOCK_ACQUIRED_AT_MS.load(Ordering::SeqCst),
            0,
            "unlock clears the holder timestamp"
        );
        assert_eq!(
            agent_doc_sync_try_lock(),
            1,
            "guard is free again after unlock"
        );
        agent_doc_sync_unlock();
    }

    fn parse_visual_tokens_json(doc: &str) -> Vec<serde_json::Value> {
        let c_doc = CString::new(doc).unwrap();
        let ptr = unsafe { agent_doc_visual_tokens_json(c_doc.as_ptr()) };
        assert!(!ptr.is_null(), "visual token JSON should not be null");
        let json_str = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        unsafe { agent_doc_free_string(ptr) };
        serde_json::from_str(&json_str).unwrap()
    }

    fn ffi_json_value(result: FfiJsonResult) -> serde_json::Value {
        if !result.error.is_null() {
            let error = unsafe { CStr::from_ptr(result.error) }
                .to_str()
                .unwrap()
                .to_string();
            unsafe { agent_doc_free_string(result.error) };
            panic!("unexpected FFI error: {error}");
        }
        assert!(!result.json.is_null(), "FFI JSON should not be null");
        let json = unsafe { CStr::from_ptr(result.json) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { agent_doc_free_string(result.json) };
        serde_json::from_str(&json).unwrap()
    }

    fn utf16_len(text: &str) -> usize {
        text.encode_utf16().count()
    }

    #[test]
    fn parse_components_roundtrip() {
        let doc = "before\n<!-- agent:status -->\nhello\n<!-- /agent:status -->\nafter\n";
        let c_doc = CString::new(doc).unwrap();
        let result = unsafe { agent_doc_parse_components(c_doc.as_ptr()) };
        assert_eq!(result.count, 1);
        assert!(!result.json.is_null());
        let json_str = unsafe { CStr::from_ptr(result.json) }.to_str().unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed[0]["name"], "status");
        assert_eq!(parsed[0]["content"], "hello\n");
        unsafe { agent_doc_free_string(result.json) };
    }

    #[test]
    fn apply_node_patches_ffi_preserves_live_buffer_drift() {
        let doc = CString::new(
            "\
operator note
<!-- agent:queue -->
- do [#alpha]
- do [#beta]
- live buffer addition
<!-- /agent:queue -->
",
        )
        .unwrap();
        let patches = CString::new(
            r#"[
{"component":"queue","node_key":"queue:0:beta:0","op":"strike"},
{"component":"queue","node_key":"queue:0:gamma:0","op":"insert","content":"- do [#gamma]\n","after":"queue:0:beta:0"}
]"#,
        )
        .unwrap();

        let result = unsafe { agent_doc_apply_node_patches(doc.as_ptr(), patches.as_ptr()) };

        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("operator note\n"));
        assert!(text.contains("- live buffer addition\n"));
        assert!(text.contains("- ~~do [#beta]~~\n- do [#gamma]\n"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn apply_node_patches_ffi_rejects_unknown_op() {
        let doc =
            CString::new("<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n").unwrap();
        let patches =
            CString::new(r#"[{"component":"queue","node_key":"queue:0:alpha:0","op":"unknown"}]"#)
                .unwrap();

        let result = unsafe { agent_doc_apply_node_patches(doc.as_ptr(), patches.as_ptr()) };

        assert!(result.text.is_null());
        assert!(!result.error.is_null());
        let error = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(error.contains("unsupported node patch op"));
        unsafe { agent_doc_free_string(result.error) };
    }

    #[test]
    fn admin_queue_control_ffi_returns_typed_receipt_json() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin-ffi.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-admin-ffi\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-admin-ffi",
            "%41",
            "@1",
            1,
        )
        .unwrap();

        let root = CString::new(dir.path().to_string_lossy().to_string()).unwrap();
        let document = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let action = CString::new("pause").unwrap();
        let reason = CString::new("ffi pause").unwrap();
        let accepted = unsafe {
            agent_doc_admin_queue_control_json(
                root.as_ptr(),
                document.as_ptr(),
                action.as_ptr(),
                1,
                reason.as_ptr(),
                std::ptr::null(),
            )
        };
        let accepted = ffi_json_value(accepted);
        assert_eq!(accepted["operation_kind"], "queue_paused");
        assert_eq!(accepted["status"], "accepted");
        assert!(accepted["receipt_id"].as_u64().unwrap() > 0);

        let stale = unsafe {
            agent_doc_admin_queue_control_json(
                root.as_ptr(),
                document.as_ptr(),
                action.as_ptr(),
                0,
                reason.as_ptr(),
                std::ptr::null(),
            )
        };
        let stale = ffi_json_value(stale);
        assert_eq!(stale["status"], "rejected");
        assert_eq!(stale["failed_stage"], "stale_generation");
        assert_eq!(stale["current_generation"], 1);
    }

    #[test]
    fn admin_inspect_ffi_returns_queue_diagnostics_json() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin-inspect-ffi.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-inspect-ffi\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-inspect-ffi",
            "%42",
            "@1",
            1,
        )
        .unwrap();

        let root = CString::new(dir.path().to_string_lossy().to_string()).unwrap();
        let document = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let action = CString::new("drain").unwrap();
        let reason = CString::new("ffi drain").unwrap();
        let item_id = CString::new("next").unwrap();
        let receipt = unsafe {
            agent_doc_admin_queue_control_json(
                root.as_ptr(),
                document.as_ptr(),
                action.as_ptr(),
                1,
                reason.as_ptr(),
                item_id.as_ptr(),
            )
        };
        assert_eq!(ffi_json_value(receipt)["status"], "accepted");

        let inspection = unsafe {
            agent_doc_admin_inspect_json(
                root.as_ptr(),
                document.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        let inspection = ffi_json_value(inspection);
        assert_eq!(inspection["record"]["pane_id"], "%42");
        assert_eq!(inspection["queue_control"]["state"], "draining");
        assert!(
            inspection["admin_operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| operation["operation_kind"] == "queue_draining"
                    && operation["status"] == "accepted")
        );
    }

    #[test]
    fn visual_tokens_json_uses_utf16_document_offsets() {
        let doc = "\
é
😀
❯ do #qey0. spec-test-build-install-commit-push
### Re: café — gpt-5
- [recommended] next
";
        let tokens = parse_visual_tokens_json(doc);
        let find = |kind: &str| {
            tokens
                .iter()
                .find(|token| token["kind"] == kind)
                .unwrap_or_else(|| panic!("missing token kind {kind}"))
        };

        let prompt = find("prompt");
        assert_eq!(
            prompt["start"].as_u64().unwrap() as usize,
            utf16_len("é\n😀\n")
        );
        assert_eq!(
            prompt["end"].as_u64().unwrap() as usize,
            utf16_len("é\n😀\n❯ do #qey0. spec-test-build-install-commit-push")
        );

        let heading = find("response_heading");
        assert_eq!(
            heading["start"].as_u64().unwrap() as usize,
            utf16_len("é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n")
        );
        assert_eq!(
            heading["end"].as_u64().unwrap() as usize,
            utf16_len(
                "é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n### Re: café — gpt-5"
            )
        );

        let label = find("label_tag");
        assert_eq!(
            label["start"].as_u64().unwrap() as usize,
            utf16_len(
                "é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n### Re: café — gpt-5\n- "
            )
        );
        assert_eq!(
            label["end"].as_u64().unwrap() as usize,
            utf16_len(
                "é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n### Re: café — gpt-5\n- [recommended]"
            )
        );
    }

    #[test]
    fn apply_patch_replace() {
        let doc = "<!-- agent:output -->\nold\n<!-- /agent:output -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_name = CString::new("output").unwrap();
        let c_content = CString::new("new content\n").unwrap();
        let c_mode = CString::new("replace").unwrap();
        let result = unsafe {
            agent_doc_apply_patch(
                c_doc.as_ptr(),
                c_name.as_ptr(),
                c_content.as_ptr(),
                c_mode.as_ptr(),
            )
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("new content"));
        assert!(!text.contains("old"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn apply_patch_with_boundary_marks_new_exchange_heading_with_head() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: earlier — gpt-5\nBody.\n<!-- agent:boundary:abc12345 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_name = CString::new("exchange").unwrap();
        let c_content = CString::new("### Re: latest — gpt-5\nNew body.\n").unwrap();
        let c_mode = CString::new("append").unwrap();
        let c_boundary = CString::new("abc12345").unwrap();
        let result = unsafe {
            agent_doc_apply_patch_with_boundary(
                c_doc.as_ptr(),
                c_name.as_ptr(),
                c_content.as_ptr(),
                c_mode.as_ptr(),
                c_boundary.as_ptr(),
            )
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("### Re: earlier — gpt-5\n"));
        assert!(text.contains("### Re: latest — gpt-5 (HEAD)\n"));
        assert_eq!(text.matches("(HEAD)").count(), 1, "got:\n{text}");
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn normalize_template_structure_ffi_repairs_safe_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let c_doc = CString::new(doc).unwrap();

        let result = unsafe { agent_doc_normalize_template_structure(c_doc.as_ptr()) };

        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert_eq!(text.matches("<!-- /agent:exchange -->").count(), 1);
        assert_eq!(text.matches("<!-- agent:queue -->").count(), 1);
        assert_eq!(text.matches("<!-- agent:backlog -->").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn normalize_template_structure_ffi_rejects_mixed_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
            "user typed into duplicated shell\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        let c_doc = CString::new(doc).unwrap();

        let result = unsafe { agent_doc_normalize_template_structure(c_doc.as_ptr()) };

        assert!(result.text.is_null());
        assert!(!result.error.is_null());
        let error = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(
            error.contains("mixed duplicate scaffold"),
            "unexpected error: {error}"
        );
        unsafe { agent_doc_free_string(result.error) };
    }

    #[test]
    fn normalize_template_structure_ffi_rejects_truncated_agent_comment() {
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- a\n",
            "<!-- /agent:queue ->\n",
            "<!-- /agent:exchange --\n",
        );
        let c_doc = CString::new(doc).unwrap();

        let result = unsafe { agent_doc_normalize_template_structure(c_doc.as_ptr()) };

        assert!(result.text.is_null());
        assert!(!result.error.is_null());
        let error = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(
            error.contains("malformed_agent_comment"),
            "unexpected error: {error}"
        );
        unsafe { agent_doc_free_string(result.error) };
    }

    #[test]
    fn merge_frontmatter_adds_field() {
        let doc = "---\nagent_doc_session: abc\n---\nBody\n";
        let fields = "model: opus";
        let c_doc = CString::new(doc).unwrap();
        let c_fields = CString::new(fields).unwrap();
        let result = unsafe { agent_doc_merge_frontmatter(c_doc.as_ptr(), c_fields.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("model: opus"));
        assert!(text.contains("agent_doc_session: abc"));
        assert!(text.contains("Body"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_boundary_removes_stale() {
        let doc = "<!-- agent:exchange patch=append -->\ntext\n<!-- agent:boundary:aaaa1111 -->\nmore\n<!-- agent:boundary:bbbb2222 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let result = unsafe { agent_doc_reposition_boundary_to_end(c_doc.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        // Should have exactly one boundary marker at the end
        let boundary_count = text.matches("<!-- agent:boundary:").count();
        assert_eq!(
            boundary_count, 1,
            "should have exactly 1 boundary, got {}",
            boundary_count
        );
        // The boundary should be just before the close tag
        assert!(text.contains("more\n<!-- agent:boundary:"));
        assert!(text.contains(" -->\n<!-- /agent:exchange -->"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_boundary_with_id_reuses_requested_marker() {
        let doc = "<!-- agent:exchange patch=append -->\ntext\n<!-- agent:boundary:aaaa1111 -->\nmore\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_id = CString::new("keep-this-id").unwrap();
        let result =
            unsafe { agent_doc_reposition_boundary_to_end_with_id(c_doc.as_ptr(), c_id.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("<!-- agent:boundary:keep-this-id -->"));
        assert_eq!(text.matches("<!-- agent:boundary:").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_preserve_head_keeps_head_markers() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: topic (HEAD)\ntext\n<!-- agent:boundary:aaaa1111 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let result = unsafe { agent_doc_reposition_boundary_to_end_preserve_head(c_doc.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(
            text.contains("### Re: topic (HEAD)"),
            "preserve_head FFI must keep (HEAD); got:\n{text}"
        );
        assert!(!text.contains("aaaa1111"), "old boundary gone");
        assert_eq!(text.matches("<!-- agent:boundary:").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_preserve_head_with_id_keeps_head_and_id() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: topic (HEAD)\ntext\n<!-- agent:boundary:aaaa1111 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_id = CString::new("my-id").unwrap();
        let result = unsafe {
            agent_doc_reposition_boundary_to_end_preserve_head_with_id(
                c_doc.as_ptr(),
                c_id.as_ptr(),
            )
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(
            text.contains("### Re: topic (HEAD)"),
            "preserve_head FFI must keep (HEAD); got:\n{text}"
        );
        assert!(
            text.contains("<!-- agent:boundary:my-id -->"),
            "explicit id used; got:\n{text}"
        );
        assert_eq!(text.matches("<!-- agent:boundary:").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn resolve_project_path_finds_nested_submodule() {
        use tempfile::TempDir;
        let outer = TempDir::new().unwrap();
        // Outer project root has .agent-doc/
        std::fs::create_dir_all(outer.path().join(".agent-doc")).unwrap();
        // Nested submodule with its own .agent-doc/
        let sub = outer.path().join("src/session-share");
        std::fs::create_dir_all(sub.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(sub.join("tasks")).unwrap();
        let doc = sub.join("tasks/claudescore.md");
        std::fs::write(&doc, "# test\n").unwrap();

        let c_path = CString::new(doc.to_str().unwrap()).unwrap();
        let result = unsafe { agent_doc_resolve_project_path(c_path.as_ptr()) };
        assert!(
            !result.project_root.is_null(),
            "project_root should be non-null"
        );
        assert!(
            !result.relative_path.is_null(),
            "relative_path should be non-null"
        );

        let root = unsafe { CStr::from_ptr(result.project_root) }
            .to_str()
            .unwrap();
        let rel = unsafe { CStr::from_ptr(result.relative_path) }
            .to_str()
            .unwrap();
        // Nearest .agent-doc/ is the submodule, not the outer project.
        let expected_root = sub.canonicalize().unwrap();
        assert_eq!(std::path::Path::new(root), expected_root);
        assert_eq!(rel, "tasks/claudescore.md");

        unsafe {
            agent_doc_free_string(result.project_root);
            agent_doc_free_string(result.relative_path);
        }
    }

    #[test]
    fn resolve_project_path_prefers_nearest_ancestor() {
        use tempfile::TempDir;
        let outer = TempDir::new().unwrap();
        std::fs::create_dir_all(outer.path().join(".agent-doc")).unwrap();
        let mid = outer.path().join("mid");
        std::fs::create_dir_all(mid.join(".agent-doc")).unwrap();
        let deep = mid.join("deep/subdir");
        std::fs::create_dir_all(&deep).unwrap();
        let doc = deep.join("doc.md");
        std::fs::write(&doc, "").unwrap();

        let c_path = CString::new(doc.to_str().unwrap()).unwrap();
        let result = unsafe { agent_doc_resolve_project_path(c_path.as_ptr()) };
        let root = unsafe { CStr::from_ptr(result.project_root) }
            .to_str()
            .unwrap();
        let rel = unsafe { CStr::from_ptr(result.relative_path) }
            .to_str()
            .unwrap();
        assert_eq!(
            std::path::Path::new(root),
            mid.canonicalize().unwrap(),
            "should prefer nearest (mid) over outer"
        );
        assert_eq!(rel, "deep/subdir/doc.md");
        unsafe {
            agent_doc_free_string(result.project_root);
            agent_doc_free_string(result.relative_path);
        }
    }

    #[test]
    fn resolve_project_path_file_directly_in_root() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("plan.md");
        std::fs::write(&doc, "").unwrap();

        let c_path = CString::new(doc.to_str().unwrap()).unwrap();
        let result = unsafe { agent_doc_resolve_project_path(c_path.as_ptr()) };
        assert!(!result.project_root.is_null());
        let rel = unsafe { CStr::from_ptr(result.relative_path) }
            .to_str()
            .unwrap();
        assert_eq!(rel, "plan.md");
        unsafe {
            agent_doc_free_string(result.project_root);
            agent_doc_free_string(result.relative_path);
        }
    }

    #[test]
    fn crdt_merge_no_base() {
        let c_ours = CString::new("hello world").unwrap();
        let c_theirs = CString::new("hello world").unwrap();
        let result =
            unsafe { agent_doc_crdt_merge(ptr::null(), 0, c_ours.as_ptr(), c_theirs.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert_eq!(text, "hello world");
        unsafe {
            agent_doc_free_string(result.text);
            agent_doc_free_state(result.state, result.state_len);
        };
    }
}

#[cfg(test)]
mod ack_content_tests {
    use super::*;
    use std::ffi::CString;
    use tempfile::TempDir;

    #[test]
    fn test_write_ack_content_legacy_abi_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-123").unwrap();
        let content = CString::new("hello world").unwrap();

        let result = unsafe {
            agent_doc_write_ack_content(project_root.as_ptr(), patch_id.as_ptr(), content.as_ptr())
        };
        assert_eq!(result, 0, "legacy no-capability ABI should fail closed");

        let sidecar = tmp
            .path()
            .join(".agent-doc/ack-content/test-patch-id-123.md");
        assert!(
            !sidecar.exists(),
            "legacy ABI must not write a projection sidecar at {:?}",
            sidecar
        );
    }

    #[test]
    fn test_editor_content_applied_for_editor_v1_records_receipt_without_live_buffer_sidecar() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/live-buffer")).unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-ack-v2").unwrap();
        let file_path = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let content = CString::new("before\n### Re: done\n").unwrap();
        let editor_id = CString::new("jetbrains:test").unwrap();
        let editor_kind = CString::new("jetbrains").unwrap();
        let editor_version = CString::new("test").unwrap();
        let capabilities = CString::new(format!(
            "{},{}",
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY
        ))
        .unwrap();

        let result = unsafe {
            agent_doc_editor_content_applied_for_editor_v1(
                project_root.as_ptr(),
                patch_id.as_ptr(),
                file_path.as_ptr(),
                content.as_ptr(),
                editor_id.as_ptr(),
                editor_kind.as_ptr(),
                editor_version.as_ptr(),
                capabilities.as_ptr(),
            )
        };
        assert_eq!(result, 1, "should return 1 on success");

        assert!(
            !tmp.path()
                .join(".agent-doc/ack-content/test-patch-id-ack-v2.md")
                .exists(),
            "current lazily receipt ABI must not write an ack-content compatibility sidecar"
        );
        let proof =
            agent_doc_controller_io::project_controller::visible_write_commit_candidate_for_patch_file(
                &doc,
                "test-patch-id-ack-v2",
            )
            .expect("consolidated receipt should publish a visible-write projection");
        assert_eq!(
            proof.commit_candidate_hash,
            agent_doc_hash::content_hash(
                &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                    "before\n### Re: done\n"
                )
            )
        );
        assert_eq!(
            proof.source, "editor_patch_applied_for_editor_v2",
            "visible-write projection should record the editor ABI source"
        );

        assert!(
            agent_doc_debounce::live_buffer_snapshot(&doc.to_string_lossy()).is_none(),
            "editor-applied receipt must not record a live-buffer sidecar proof"
        );
    }

    #[test]
    fn test_document_changed_digest_content_for_editor_v3_records_live_buffer_provenance() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/live-buffer")).unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let file_path = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let content = CString::new("before\n### Re: remote\n").unwrap();
        let editor_id = CString::new("jetbrains:test").unwrap();
        let editor_kind = CString::new("jetbrains").unwrap();
        let editor_version = CString::new("test").unwrap();
        let capabilities = CString::new(format!(
            "{},{}",
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY
        ))
        .unwrap();

        unsafe {
            agent_doc_document_changed_digest_content_for_editor_v3(
                file_path.as_ptr(),
                content.as_ptr(),
                editor_id.as_ptr(),
                editor_kind.as_ptr(),
                editor_version.as_ptr(),
                capabilities.as_ptr(),
                1,
            )
        };

        let snapshot = agent_doc_debounce::live_buffer_snapshot(&doc.to_string_lossy())
            .expect("v3 full-content report should persist live-buffer sidecar");
        assert_eq!(
            snapshot.content.as_deref(),
            Some("before\n### Re: remote\n")
        );
        assert_eq!(snapshot.editor_kind.as_deref(), Some("jetbrains"));
        assert!(snapshot.has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY));
        assert!(snapshot.has_capability(agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY));
        assert!(
            snapshot.no_unsaved_operator_edits,
            "v3 provenance flag must survive the FFI bridge"
        );
    }

    #[test]
    fn test_document_changed_digest_content_for_editor_v2_records_live_buffer_without_provenance() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/live-buffer")).unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let file_path = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let content = CString::new("before\n### Re: fallback\n").unwrap();
        let editor_id = CString::new("jetbrains:test").unwrap();
        let editor_kind = CString::new("jetbrains").unwrap();
        let editor_version = CString::new("test").unwrap();
        let capabilities = CString::new(format!(
            "{},{}",
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY
        ))
        .unwrap();

        unsafe {
            agent_doc_document_changed_digest_content_for_editor_v2(
                file_path.as_ptr(),
                content.as_ptr(),
                editor_id.as_ptr(),
                editor_kind.as_ptr(),
                editor_version.as_ptr(),
                capabilities.as_ptr(),
            )
        };

        let snapshot = agent_doc_debounce::live_buffer_snapshot(&doc.to_string_lossy())
            .expect("v2 full-content report should persist live-buffer sidecar");
        assert_eq!(
            snapshot.content.as_deref(),
            Some("before\n### Re: fallback\n")
        );
        assert_eq!(snapshot.editor_kind.as_deref(), Some("jetbrains"));
        assert!(!snapshot.no_unsaved_operator_edits);
    }

    #[test]
    fn test_editor_content_applied_for_editor_v1_requires_lazily_receipt_capability() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/live-buffer")).unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-missing-cap").unwrap();
        let file_path = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let content = CString::new("before\n### Re: done\n").unwrap();
        let editor_id = CString::new("jetbrains:test").unwrap();
        let editor_kind = CString::new("jetbrains").unwrap();
        let editor_version = CString::new("test").unwrap();
        let capabilities =
            CString::new(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY).unwrap();

        let result = unsafe {
            agent_doc_editor_content_applied_for_editor_v1(
                project_root.as_ptr(),
                patch_id.as_ptr(),
                file_path.as_ptr(),
                content.as_ptr(),
                editor_id.as_ptr(),
                editor_kind.as_ptr(),
                editor_version.as_ptr(),
                capabilities.as_ptr(),
            )
        };
        assert_eq!(
            result, 0,
            "current editor bridge should reject plugins without lazily receipt support"
        );
        assert!(
            !tmp.path()
                .join(".agent-doc/ack-content/test-patch-id-missing-cap.md")
                .exists(),
            "missing-capability call must not write a projection sidecar"
        );
        assert!(
            agent_doc_debounce::live_buffer_snapshot(&doc.to_string_lossy()).is_none(),
            "missing-capability call must not record live-buffer proof"
        );
    }

    #[test]
    fn test_write_ack_content_for_editor_v2_legacy_abi_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-old-v2").unwrap();
        let file_path = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let content = CString::new("before\n### Re: done\n").unwrap();
        let editor_id = CString::new("jetbrains:test").unwrap();
        let editor_kind = CString::new("jetbrains").unwrap();
        let editor_version = CString::new("test").unwrap();
        let capabilities = CString::new(format!(
            "{},{}",
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY
        ))
        .unwrap();

        let result = unsafe {
            agent_doc_write_ack_content_for_editor_v2(
                project_root.as_ptr(),
                patch_id.as_ptr(),
                file_path.as_ptr(),
                content.as_ptr(),
                editor_id.as_ptr(),
                editor_kind.as_ptr(),
                editor_version.as_ptr(),
                capabilities.as_ptr(),
            )
        };
        assert_eq!(result, 0, "legacy receipt-named v2 ABI should fail closed");
        assert!(
            !tmp.path()
                .join(".agent-doc/ack-content/test-patch-id-old-v2.md")
                .exists(),
            "legacy receipt-named v2 ABI must not write a projection sidecar"
        );
    }

    #[test]
    fn test_is_claimed_by_force_disk_present() {
        let tmp = TempDir::new().unwrap();
        let claimed_dir = tmp.path().join(".agent-doc/claimed-patches");
        std::fs::create_dir_all(&claimed_dir).unwrap();
        std::fs::write(claimed_dir.join("test-patch-456"), "").unwrap();

        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-456").unwrap();

        let claimed =
            unsafe { agent_doc_is_claimed_by_force_disk(project_root.as_ptr(), patch_id.as_ptr()) };
        assert_eq!(claimed, 1, "should return 1 when sentinel exists");
        assert!(
            claimed_dir.join("test-patch-456").exists(),
            "sentinel should remain so repeated watcher passes skip the patch"
        );
        let claimed_again =
            unsafe { agent_doc_is_claimed_by_force_disk(project_root.as_ptr(), patch_id.as_ptr()) };
        assert_eq!(claimed_again, 1, "claimed sentinel should be durable");
    }

    #[test]
    fn plugin_watch_readonly_always_demotes_post_cutover() {
        // 08b cutover complete: the plugin WatchService file-apply path is
        // unconditionally read-only — the controller-owned watcher + socket IPC
        // are the sole writer (#dsqa / #pcp7).
        let tmp = TempDir::new().unwrap();
        let file = CString::new(tmp.path().join("plan.md").to_str().unwrap()).unwrap();
        let readonly = unsafe { agent_doc_plugin_watch_readonly(file.as_ptr()) };
        assert_eq!(
            readonly, 1,
            "post-cutover the plugin must never apply file-IPC patches"
        );
    }

    #[test]
    fn test_is_claimed_by_force_disk_absent() {
        let tmp = TempDir::new().unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("nonexistent-patch").unwrap();

        let claimed =
            unsafe { agent_doc_is_claimed_by_force_disk(project_root.as_ptr(), patch_id.as_ptr()) };
        assert_eq!(claimed, 0, "should return 0 when sentinel absent");
    }

    #[test]
    fn patch_content_already_committed_requires_disk_to_match_head() {
        use std::process::Command;
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        macro_rules! git {
            ($($arg:expr),+) => {
                Command::new("git")
                    .current_dir(root)
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_INDEX_FILE")
                    .env_remove("GIT_WORK_TREE")
                    .args([$($arg),+])
                    .output()
                    .unwrap()
            };
        }

        git!["init"];
        git!["config", "user.email", "test@test.com"];
        git!["config", "user.name", "Test"];

        let doc = root.join("doc.md");
        let committed = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: committed — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, committed).unwrap();
        git!["add", "doc.md"];
        git!["commit", "-m", "committed response", "--no-verify"];

        let file_path = CString::new(doc.to_string_lossy().as_ref()).unwrap();
        let patch_content = CString::new("### Re: committed — gpt-5\n\nDone.\n").unwrap();
        assert_eq!(
            unsafe {
                agent_doc_patch_content_already_committed(
                    file_path.as_ptr(),
                    patch_content.as_ptr(),
                )
            },
            1,
            "committed disk content should prove the patch is already present"
        );

        std::fs::write(
            &doc,
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        assert_eq!(
            unsafe {
                agent_doc_patch_content_already_committed(
                    file_path.as_ptr(),
                    patch_content.as_ptr(),
                )
            },
            0,
            "HEAD alone must not be enough when disk drifted away from HEAD"
        );
    }

    // --- Fix 4: agent_doc_commit FFI export ---

    #[test]
    fn agent_doc_commit_returns_false_for_null() {
        let result = unsafe { agent_doc_commit(std::ptr::null()) };
        assert_eq!(result, 0, "null path should return 0");
    }

    #[test]
    fn ffi_git_commit_commits_staged_file() {
        use std::process::Command;
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Helper: run git command isolated from any parent git hook env vars.
        // Pre-commit hooks set GIT_DIR/GIT_INDEX_FILE which would confuse
        // git commands targeting the temp repo.
        macro_rules! git {
            ($($arg:expr),+) => {
                Command::new("git")
                    .current_dir(root)
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_INDEX_FILE")
                    .env_remove("GIT_WORK_TREE")
                    .args([$($arg),+])
                    .output()
                    .unwrap()
            };
        }

        // Set up minimal git repo
        git!["init"];
        git!["config", "user.email", "test@test.com"];
        git!["config", "user.name", "Test"];

        // Commit initial file so HEAD exists
        let readme = root.join("README.md");
        std::fs::write(&readme, "# test\n").unwrap();
        git!["add", "README.md"];
        git!["commit", "-m", "initial", "--no-verify"];

        // Create a document file (not yet committed)
        let doc = root.join("session.md");
        std::fs::write(&doc, "# content\n").unwrap();

        // ffi_git_commit should stage + commit the doc
        let ok = ffi_git_commit(&doc);
        assert!(ok, "ffi_git_commit should succeed for a valid git repo");

        // Verify git log contains the commit
        let log = git!["log", "--oneline", "-2"];
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain agent-doc commit, got:\n{log_str}"
        );
    }
}

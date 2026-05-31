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
//! - `agent_doc_document_changed(file_path)`: records a change event for debounce tracking.
//! - `agent_doc_document_changed_digest(file_path, len, hash)`: records typing plus the latest
//!   editor-visible buffer digest so CLI direct-disk writes can detect idle unsaved drift.
//! - `agent_doc_report_editor_state(file_path, version, dirty, last_edit_ts, save_ts, hash, len, session_id)`:
//!   records editor-authoritative buffer state for debounce/stability. When present,
//!   `wait_for_stable_content` uses version/hash stability instead of the truncation heuristic.
//! - `agent_doc_get_editor_state(file_path)`: returns the current editor buffer state as JSON.
//! - `agent_doc_is_editor_stable(file_path, debounce_ms)`: returns whether the editor buffer is
//!   stable (not dirty or debounce elapsed).
//! - `agent_doc_is_tracked(file_path)`: returns whether at least one change event has been recorded
//!   for the file.
//! - `agent_doc_is_idle(file_path, debounce_ms)`: non-blocking check — returns `true` if no
//!   `document_changed` event within `debounce_ms`.  Used by the IDE plugin to defer IPC writes
//!   (e.g. boundary repositioning) while the user is actively typing.
//! - `agent_doc_await_idle(file_path, debounce_ms, timeout_ms)`: blocks until the document has been
//!   idle for `debounce_ms`, or `timeout_ms` expires.  Returns `true` on idle, `false` on timeout.
//! - `agent_doc_is_typing_via_file(file_path, debounce_ms)`: cross-process check — reads the
//!   file-based typing indicator written by `document_changed`; `true` if updated within
//!   `debounce_ms`.  For CLI tools running in a separate process from the editor plugin.
//! - `agent_doc_await_idle_via_file(file_path, debounce_ms, timeout_ms)`: blocking variant of
//!   `is_typing_via_file`; polls until idle or `timeout_ms` expires.
//! - `agent_doc_free_string(ptr)` / `agent_doc_free_state(ptr, len)`: free memory returned by any
//!   `agent_doc_*` function.  Must be called for every non-null pointer.
//!
//! ## Agentic Contracts
//! - All string parameters must be valid, non-null, NUL-terminated UTF-8; violation is UB.
//! - Every non-null `text` or `error` pointer in a result struct must be freed exactly once with
//!   `agent_doc_free_string`; CRDT `state` pointers must be freed with `agent_doc_free_state`.
//! - On parse/apply errors, `text` (or `json`) is null and `error` holds a message; callers must
//!   check nullability before use.
//! - `agent_doc_await_idle` returning `false` means the timeout expired — the caller must not
//!   proceed with the agent run.
//!
//! ## Evals
//! - parse_components_roundtrip: single `agent:status` component → JSON count=1, content="hello\n"
//! - apply_patch_replace: replace mode on `agent:output` → new content present, old content absent
//! - merge_frontmatter_adds_field: add `model: opus` to existing frontmatter → both keys present, body unchanged
//! - reposition_boundary_removes_stale: two boundary markers in exchange → exactly one marker at end
//! - crdt_merge_no_base: identical `ours`/`theirs` with null base → merged text equals input

use std::ffi::{CStr, CString, c_char};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cross-editor sync lock — prevents concurrent layout syncs.
static SYNC_LOCKED: AtomicBool = AtomicBool::new(false);

/// Sync debounce generation counter — only the latest scheduled sync fires.
static SYNC_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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

/// Record a document change event for debounce tracking.
///
/// Plugins call this on every document modification (typing, paste, undo).
/// Used by [`agent_doc_await_idle`] to determine if the user is still editing.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_document_changed(file_path: *const c_char) {
    if let Ok(path) = unsafe { CStr::from_ptr(file_path) }.to_str() {
        agent_doc_orchestration::debounce::document_changed(path);
    }
}

/// Record a document change event plus the current editor-visible buffer digest.
///
/// Plugins call this on every document modification when they can compute a
/// SHA-256 content hash. This avoids sending full document content through FFI
/// while still letting CLI direct-disk writes detect an idle unsaved editor
/// buffer before writing the file on disk.
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
    let hash = match unsafe { CStr::from_ptr(content_hash) }.to_str() {
        Ok(hash) => hash,
        Err(_) => return,
    };
    let Ok(len) = usize::try_from(content_len) else {
        return;
    };
    agent_doc_orchestration::debounce::document_changed_with_digest(path, len, hash);
}

/// Check if the document has been tracked (at least one `document_changed` call recorded).
///
/// Returns `true` if the file has been tracked, `false` if never seen.
/// Plugins use this to decide whether `await_idle` results are trustworthy:
/// an untracked file returns idle=true from `await_idle`, but that's because
/// no changes were recorded, not because the user isn't typing.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_tracked(file_path: *const c_char) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    agent_doc_orchestration::debounce::is_tracked(path) as i32
}

/// Return the number of files tracked in the debounce state.
/// Used by IDE plugins for state diagnostics.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_tracked_count() -> u32 {
    agent_doc_orchestration::debounce::tracked_count() as u32
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
    match agent_doc_orchestration::repair::cancel_preflight_cycle(std::path::Path::new(path)) {
        Ok(agent_doc_orchestration::repair::CancelOutcome::Abandoned) => 1,
        Ok(_) => 0,
        Err(e) => {
            eprintln!("[ffi] agent_doc_cancel_preflight_cycle failed for {path}: {e}");
            -1
        }
    }
}

/// Non-blocking idle check — returns `true` if no `document_changed` event
/// within `debounce_ms`.
///
/// Used by IDE plugins to defer IPC operations (boundary repositioning, patch
/// application) while the user is actively typing.  Unlike `await_idle`, this
/// returns immediately.
///
/// For untracked files (no `document_changed` ever called), returns `true`.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_idle(file_path: *const c_char, debounce_ms: i64) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 1, // Invalid path — don't block callers
    };
    let in_process_idle = agent_doc_orchestration::debounce::is_idle(path, debounce_ms as u64);
    if !in_process_idle {
        return 0;
    }
    // In-process says idle. If the file was never tracked in this process (e.g., after
    // plugin restart), also check the file-based indicator so cross-process typing state
    // from another plugin instance isn't silently lost.
    if !agent_doc_orchestration::debounce::is_tracked(path) {
        return (!agent_doc_orchestration::debounce::is_typing_via_file(path, debounce_ms as u64)) as i32;
    }
    1
}

/// Block until the document has been idle for `debounce_ms`, or `timeout_ms` expires.
///
/// Returns `true` if idle was reached (safe to run), `false` if timed out.
/// If no changes have been recorded for this file, returns `true` immediately.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_await_idle(
    file_path: *const c_char,
    debounce_ms: i64,
    timeout_ms: i64,
) -> i32 {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return 1, // Invalid path — don't block
    };
    // When the file is untracked in-process (e.g., after plugin restart), bridge to
    // file-based indicator so cross-process typing state isn't silently ignored.
    if !agent_doc_orchestration::debounce::is_tracked(path) {
        return agent_doc_orchestration::debounce::await_idle_via_file(path, debounce_ms as u64, timeout_ms as u64)
            as i32;
    }
    agent_doc_orchestration::debounce::await_idle(path, debounce_ms as u64, timeout_ms as u64) as i32
}

/// Check if a plugin in another process has typed recently (cross-process).
///
/// Reads the file-based typing indicator written by `agent_doc_document_changed`.
/// Returns `true` if the indicator exists and was updated within `debounce_ms`.
/// Returns `false` if no indicator file exists (plugin not active or no edits).
///
/// This is the cross-process complement to `agent_doc_is_idle`, which only
/// works within the same process. Use this from CLI tools that run separately
/// from the editor plugin.
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
    agent_doc_orchestration::debounce::is_typing_via_file(path, debounce_ms as u64) as i32
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
    agent_doc_orchestration::debounce::await_idle_via_file(path, debounce_ms as u64, timeout_ms as u64) as i32
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
    agent_doc_orchestration::debounce::set_status(path, st);
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
    let status = agent_doc_orchestration::debounce::get_status(path);
    CString::new(status)
        .unwrap_or_else(|_| CString::new("idle").unwrap())
        .into_raw()
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
    agent_doc_orchestration::debounce::is_busy(path) as i32
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
        unsafe { CStr::from_ptr(content_hash) }.to_str().ok().map(|s| s.to_string())
    };
    let len = if content_len < 0 {
        None
    } else {
        usize::try_from(content_len).ok()
    };
    let sid = if session_id.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(session_id) }.to_str().ok().map(|s| s.to_string())
    };
    let save_ts = if save_timestamp_ms == 0 {
        None
    } else {
        Some(save_timestamp_ms as u128)
    };
    let state = agent_doc_orchestration::debounce::EditorBufferState {
        path: path.to_string(),
        version,
        dirty: dirty != 0,
        last_edit_timestamp_ms: last_edit_timestamp_ms as u128,
        save_timestamp_ms: save_ts,
        hash,
        content_len: len,
        session_id: sid,
    };
    agent_doc_orchestration::debounce::record_editor_buffer_state(&state);
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
    match agent_doc_orchestration::debounce::editor_buffer_state(path) {
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
    agent_doc_orchestration::debounce::editor_buffer_stable(path, debounce_ms as u64).is_some() as i32
}

/// Try to acquire the sync lock. Returns `true` if acquired, `false` if already held.
///
/// Editors call this before triggering `agent-doc sync`. If it returns `false`,
/// skip the sync (another sync is in progress). Call `agent_doc_sync_unlock()`
/// when the sync completes.
///
/// This is a cross-editor shared lock — prevents concurrent syncs from IntelliJ
/// and VS Code plugins simultaneously.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_try_lock() -> i32 {
    SYNC_LOCKED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok() as i32
}

/// Release the sync lock acquired by `agent_doc_sync_try_lock()`.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_sync_unlock() {
    SYNC_LOCKED.store(false, Ordering::SeqCst);
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
/// generates the ack response internally based on the return value.
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
        let result = agent_doc_orchestration::ipc_socket::start_listener(&root_path, move |msg| {
            // Lend the message to the callback (no ownership transfer)
            let c_msg = match CString::new(msg) {
                Ok(c) => c,
                Err(_) => return Some(r#"{"type":"ack","status":"error"}"#.to_string()),
            };
            let success = callback(c_msg.as_ptr()) != 0;
            if success {
                Some(r#"{"type":"ack","status":"ok"}"#.to_string())
            } else {
                Some(r#"{"type":"ack","status":"error"}"#.to_string())
            }
        });
        if let Err(e) = result {
            eprintln!("[ffi] IPC listener error: {}", e);
        }
    });

    1
}

/// V2 of [`agent_doc_start_ipc_listener`] with extended ack-result encoding.
///
/// The callback returns one of three values:
/// - `0` → ack `{"type":"ack","status":"error"}` (apply failed)
/// - `1` → ack `{"type":"ack","status":"ok"}` (apply succeeded)
/// - `2` → ack `{"type":"ack","status":"error","reason":"already_applied"}`
///   (plugin detected the patch is already in the live buffer and chose NOT
///   to re-apply; binary skips the file-IPC fallback so a duplicate response
///   heading cannot land).
///
/// Plugins should prefer v2 when available. Existing plugins built against
/// [`agent_doc_start_ipc_listener`] keep working unchanged.
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
        let result = agent_doc_orchestration::ipc_socket::start_listener(&root_path, move |msg| {
            let c_msg = match CString::new(msg) {
                Ok(c) => c,
                Err(_) => return Some(r#"{"type":"ack","status":"error"}"#.to_string()),
            };
            match callback(c_msg.as_ptr()) {
                1 => Some(r#"{"type":"ack","status":"ok"}"#.to_string()),
                2 => Some(
                    r#"{"type":"ack","status":"error","reason":"already_applied"}"#.to_string(),
                ),
                _ => Some(r#"{"type":"ack","status":"error"}"#.to_string()),
            }
        });
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
    let sock = agent_doc_orchestration::ipc_socket::socket_path(std::path::Path::new(root_str));
    if let Err(e) = std::fs::remove_file(&sock)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("[ffi] failed to remove socket {:?}: {}", sock, e);
    }
}

/// Write the final applied document content to the ack-content sidecar file.
///
/// The binary reads this after receiving IPC ACK to use as snapshot content,
/// eliminating the 200ms sleep + re-read. If the plugin doesn't call this,
/// the binary falls back to the 200ms sleep heuristic.
///
/// Sidecar path: `<project_root>/.agent-doc/ack-content/<patch_id>.md`
///
/// # Safety
///
/// All three pointers must be valid, NUL-terminated UTF-8 strings.
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
    let content_str = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let ack_dir = std::path::Path::new(root_str).join(".agent-doc/ack-content");
    if let Err(e) = std::fs::create_dir_all(&ack_dir) {
        eprintln!("[ffi] agent_doc_write_ack_content: mkdir error: {e}");
        return 0;
    }

    let sidecar = ack_dir.join(format!("{patch_id_str}.md"));
    match std::fs::write(&sidecar, content_str) {
        Ok(_) => {
            eprintln!(
                "[ffi] ack_content written: {} bytes for patch_id {}",
                content_str.len(),
                &patch_id_str[..patch_id_str.len().min(8)]
            );
            1
        }
        Err(e) => {
            eprintln!("[ffi] agent_doc_write_ack_content: write error: {e}");
            0
        }
    }
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

/// Walk up from `path` to find the nearest ancestor containing `.agent-doc/`.
/// Mirrors `snapshot::find_project_root` (binary crate) for the library crate.
fn find_project_root_ffi(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = if path.is_file() { path.parent()? } else { path };
    loop {
        if current.join(".agent-doc").is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
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
// `agent_doc_core::ffi` (Wave 5 / `#k9e1` proof-of-concept). They are
// re-exported below via `pub use agent_doc_core::ffi::*;`. The
// `force_link_core_ffi_symbols` function below references them to
// prevent the static linker from stripping them out of the main cdylib.
pub use agent_doc_core::ffi::*;

/// Prevent the static linker from stripping the `agent_doc_core::ffi`
/// symbols out of `libagent_doc.{so,dylib,dll}`. Called from `lib.rs`
/// via a constructor-style reference path so editor plugins
/// (JetBrains, VS Code) continue to find the symbols at runtime.
///
/// The function itself is never called — we just need the symbol
/// references to exist in the main crate's compilation unit so the
/// linker keeps them in the cdylib export table.
#[allow(dead_code)]
fn force_link_core_ffi_symbols() {
    use agent_doc_core::ffi::{
        FfiComponentList, FfiMergeResult, FfiPatchResult, agent_doc_apply_patch,
        agent_doc_apply_patch_with_boundary, agent_doc_apply_patch_with_caret,
        agent_doc_converge_queue_auto, agent_doc_crdt_merge, agent_doc_free_state,
        agent_doc_free_string, agent_doc_merge_crdt, agent_doc_merge_frontmatter,
        agent_doc_normalize_template_structure,
        agent_doc_parse_components, agent_doc_reposition_boundary_to_end,
        agent_doc_reposition_boundary_to_end_preserve_head,
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

    fn parse_visual_tokens_json(doc: &str) -> Vec<serde_json::Value> {
        let c_doc = CString::new(doc).unwrap();
        let ptr = unsafe { agent_doc_visual_tokens_json(c_doc.as_ptr()) };
        assert!(!ptr.is_null(), "visual token JSON should not be null");
        let json_str = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        unsafe { agent_doc_free_string(ptr) };
        serde_json::from_str(&json_str).unwrap()
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
    fn is_idle_untracked_returns_true() {
        let path = CString::new("/tmp/ffi-test-untracked-file.md").unwrap();
        let result = unsafe { agent_doc_is_idle(path.as_ptr(), 500) };
        assert_eq!(result, 1, "untracked file should report idle");
    }

    #[test]
    fn is_idle_after_change_returns_false() {
        let path = CString::new("/tmp/ffi-test-just-changed.md").unwrap();
        unsafe { agent_doc_document_changed(path.as_ptr()) };
        let result = unsafe { agent_doc_is_idle(path.as_ptr(), 2000) };
        assert_eq!(
            result, 0,
            "file changed <2s ago should not be idle with 2000ms window"
        );
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
    fn test_write_ack_content_creates_file() {
        let tmp = TempDir::new().unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("test-patch-id-123").unwrap();
        let content = CString::new("hello world").unwrap();

        let result = unsafe {
            agent_doc_write_ack_content(project_root.as_ptr(), patch_id.as_ptr(), content.as_ptr())
        };
        assert_eq!(result, 1, "should return 1 on success");

        let sidecar = tmp
            .path()
            .join(".agent-doc/ack-content/test-patch-id-123.md");
        assert!(
            sidecar.exists(),
            "sidecar file should exist at {:?}",
            sidecar
        );
        assert_eq!(std::fs::read_to_string(&sidecar).unwrap(), "hello world");
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

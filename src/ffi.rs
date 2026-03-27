//! # Module: ffi
//!
//! ## Spec
//! - Exports a C ABI (`extern "C"`) surface consumed by the JetBrains plugin via JNA and the VS
//!   Code extension via Node native addons, eliminating duplicated parsing/merge logic in Kotlin
//!   and TypeScript.
//! - `agent_doc_parse_components(doc)`: parses all `<!-- agent:name -->` components and returns a
//!   JSON-encoded array with fields `name`, `attrs`, `open_start`, `open_end`, `close_start`,
//!   `close_end`, `content`.
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
//! - `agent_doc_is_tracked(file_path)`: returns whether at least one change event has been recorded
//!   for the file.
//! - `agent_doc_await_idle(file_path, debounce_ms, timeout_ms)`: blocks until the document has been
//!   idle for `debounce_ms`, or `timeout_ms` expires.  Returns `true` on idle, `false` on timeout.
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

use crate::component;
use crate::crdt;
use crate::frontmatter;
use crate::template;

/// Cross-editor sync lock — prevents concurrent layout syncs.
static SYNC_LOCKED: AtomicBool = AtomicBool::new(false);

/// Sync debounce generation counter — only the latest scheduled sync fires.
static SYNC_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Serialized component info returned by [`agent_doc_parse_components`].
#[repr(C)]
pub struct FfiComponentList {
    /// JSON-encoded array of components. Free with [`agent_doc_free_string`].
    pub json: *mut c_char,
    /// Number of components parsed (convenience — also available in the JSON).
    pub count: usize,
}

/// Result of [`agent_doc_apply_patch`].
#[repr(C)]
pub struct FfiPatchResult {
    /// The patched document text, or null on error. Free with [`agent_doc_free_string`].
    pub text: *mut c_char,
    /// Error message if `text` is null. Free with [`agent_doc_free_string`].
    pub error: *mut c_char,
}

/// Result of [`agent_doc_crdt_merge`].
#[repr(C)]
pub struct FfiMergeResult {
    /// Merged document text, or null on error. Free with [`agent_doc_free_string`].
    pub text: *mut c_char,
    /// Updated CRDT state bytes (caller must copy). Null on error.
    pub state: *mut u8,
    /// Length of `state` in bytes.
    pub state_len: usize,
    /// Error message if `text` is null. Free with [`agent_doc_free_string`].
    pub error: *mut c_char,
}

/// Parse components from a document.
///
/// Returns a [`FfiComponentList`] with a JSON-encoded array of components.
/// Each component object has: `name`, `attrs`, `open_start`, `open_end`,
/// `close_start`, `close_end`, `content`.
///
/// # Safety
///
/// `doc` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_parse_components(doc: *const c_char) -> FfiComponentList {
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            return FfiComponentList {
                json: ptr::null_mut(),
                count: 0,
            };
        }
    };

    let components = match component::parse(doc_str) {
        Ok(c) => c,
        Err(_) => {
            return FfiComponentList {
                json: ptr::null_mut(),
                count: 0,
            };
        }
    };

    let count = components.len();

    // Serialize to JSON with content included
    let json_items: Vec<serde_json::Value> = components
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "attrs": c.attrs,
                "open_start": c.open_start,
                "open_end": c.open_end,
                "close_start": c.close_start,
                "close_end": c.close_end,
                "content": c.content(doc_str),
            })
        })
        .collect();

    let json_str = serde_json::to_string(&json_items).unwrap_or_default();
    let c_json = CString::new(json_str).unwrap_or_default();

    FfiComponentList {
        json: c_json.into_raw(),
        count,
    }
}

/// Apply a patch to a document component.
///
/// `mode` must be one of: `"replace"`, `"append"`, `"prepend"`.
///
/// # Safety
///
/// All string pointers must be valid, NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_apply_patch(
    doc: *const c_char,
    component_name: *const c_char,
    content: *const c_char,
    mode: *const c_char,
) -> FfiPatchResult {
    let make_err = |msg: &str| FfiPatchResult {
        text: ptr::null_mut(),
        error: CString::new(msg).unwrap_or_default().into_raw(),
    };

    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid doc UTF-8: {e}")),
    };
    let name = match unsafe { CStr::from_ptr(component_name) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid component name UTF-8: {e}")),
    };
    let patch_content = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid content UTF-8: {e}")),
    };
    let mode_str = match unsafe { CStr::from_ptr(mode) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid mode UTF-8: {e}")),
    };

    // Build a patch block and apply it
    let patch = template::PatchBlock {
        name: name.to_string(),
        content: patch_content.to_string(),
    };

    // Use mode overrides to force the specified mode
    let mut overrides = std::collections::HashMap::new();
    overrides.insert(name.to_string(), mode_str.to_string());

    // apply_patches_with_overrides needs a file path for config lookup — use a dummy
    // since we're providing explicit overrides
    let dummy_path = std::path::Path::new("/dev/null");
    match template::apply_patches_with_overrides(
        doc_str,
        &[patch],
        "",
        dummy_path,
        &overrides,
    ) {
        Ok(result) => FfiPatchResult {
            text: CString::new(result).unwrap_or_default().into_raw(),
            error: ptr::null_mut(),
        },
        Err(e) => make_err(&format!("{e}")),
    }
}

/// Apply a component patch with cursor-aware ordering for append mode.
///
/// When `mode` is `"append"` and `caret_offset >= 0`, the content is inserted
/// at the line boundary before the caret position (if the caret is inside the
/// component). This ensures agent responses appear above where the user is typing.
///
/// Pass `caret_offset = -1` for normal behavior (identical to `agent_doc_apply_patch`).
///
/// # Safety
///
/// All pointers must be valid, non-null, NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_apply_patch_with_caret(
    doc: *const c_char,
    component_name: *const c_char,
    content: *const c_char,
    mode: *const c_char,
    caret_offset: i32,
) -> FfiPatchResult {
    let make_err = |msg: &str| FfiPatchResult {
        text: ptr::null_mut(),
        error: CString::new(msg).unwrap_or_default().into_raw(),
    };

    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid doc UTF-8: {e}")),
    };
    let name = match unsafe { CStr::from_ptr(component_name) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid component name UTF-8: {e}")),
    };
    let patch_content = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid content UTF-8: {e}")),
    };
    let mode_str = match unsafe { CStr::from_ptr(mode) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid mode UTF-8: {e}")),
    };

    // If append mode with a valid caret, use cursor-aware insertion
    if mode_str == "append" && caret_offset >= 0 {
        let components = match component::parse(doc_str) {
            Ok(c) => c,
            Err(e) => return make_err(&format!("{e}")),
        };
        if let Some(comp) = components.iter().find(|c| c.name == name) {
            let result = comp.append_with_caret(
                doc_str,
                patch_content,
                Some(caret_offset as usize),
            );
            return FfiPatchResult {
                text: CString::new(result).unwrap_or_default().into_raw(),
                error: ptr::null_mut(),
            };
        }
    }

    // Fall back to normal apply_patch behavior
    let patch = template::PatchBlock {
        name: name.to_string(),
        content: patch_content.to_string(),
    };
    let mut overrides = std::collections::HashMap::new();
    overrides.insert(name.to_string(), mode_str.to_string());
    let dummy_path = std::path::Path::new("/dev/null");
    match template::apply_patches_with_overrides(doc_str, &[patch], "", dummy_path, &overrides) {
        Ok(result) => FfiPatchResult {
            text: CString::new(result).unwrap_or_default().into_raw(),
            error: ptr::null_mut(),
        },
        Err(e) => make_err(&format!("{e}")),
    }
}

/// Apply a component patch using a boundary marker for insertion point.
///
/// When `mode` is `"append"` and `boundary_id` is provided, the content is
/// inserted at the boundary marker position (replacing the marker). This ensures
/// agent responses appear after the prompt that triggered them, even if the user
/// has typed new text below.
///
/// Falls back to normal patch application if the boundary is not found.
///
/// # Safety
///
/// All pointers must be valid, non-null, NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_apply_patch_with_boundary(
    doc: *const c_char,
    component_name: *const c_char,
    content: *const c_char,
    mode: *const c_char,
    boundary_id: *const c_char,
) -> FfiPatchResult {
    let make_err = |msg: &str| FfiPatchResult {
        text: ptr::null_mut(),
        error: CString::new(msg).unwrap_or_default().into_raw(),
    };

    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid doc UTF-8: {e}")),
    };
    let name = match unsafe { CStr::from_ptr(component_name) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid component name UTF-8: {e}")),
    };
    let patch_content = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid content UTF-8: {e}")),
    };
    let mode_str = match unsafe { CStr::from_ptr(mode) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid mode UTF-8: {e}")),
    };
    let bid = match unsafe { CStr::from_ptr(boundary_id) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid boundary_id UTF-8: {e}")),
    };

    // Use boundary-aware insertion for append mode
    if mode_str == "append" && !bid.is_empty() {
        let components = match component::parse(doc_str) {
            Ok(c) => c,
            Err(e) => return make_err(&format!("{e}")),
        };
        if let Some(comp) = components.iter().find(|c| c.name == name) {
            let result = comp.append_with_boundary(doc_str, patch_content, bid);
            return FfiPatchResult {
                text: CString::new(result).unwrap_or_default().into_raw(),
                error: ptr::null_mut(),
            };
        }
    }

    // Fall back to normal apply_patch behavior
    let patch = template::PatchBlock {
        name: name.to_string(),
        content: patch_content.to_string(),
    };
    let mut overrides = std::collections::HashMap::new();
    overrides.insert(name.to_string(), mode_str.to_string());
    let dummy_path = std::path::Path::new("/dev/null");
    match template::apply_patches_with_overrides(doc_str, &[patch], "", dummy_path, &overrides) {
        Ok(result) => FfiPatchResult {
            text: CString::new(result).unwrap_or_default().into_raw(),
            error: ptr::null_mut(),
        },
        Err(e) => make_err(&format!("{e}")),
    }
}

/// CRDT merge (3-way conflict-free).
///
/// `base_state` may be null (first merge). `base_state_len` is ignored when null.
///
/// # Safety
///
/// - `ours` and `theirs` must be valid, NUL-terminated UTF-8.
/// - If `base_state` is non-null, `base_state_len` bytes must be readable from it.
/// - The caller must free `text` and `error` with [`agent_doc_free_string`].
/// - The caller must free `state` with [`agent_doc_free_state`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_crdt_merge(
    base_state: *const u8,
    base_state_len: usize,
    ours: *const c_char,
    theirs: *const c_char,
) -> FfiMergeResult {
    let make_err = |msg: &str| FfiMergeResult {
        text: ptr::null_mut(),
        state: ptr::null_mut(),
        state_len: 0,
        error: CString::new(msg).unwrap_or_default().into_raw(),
    };

    let ours_str = match unsafe { CStr::from_ptr(ours) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid ours UTF-8: {e}")),
    };
    let theirs_str = match unsafe { CStr::from_ptr(theirs) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid theirs UTF-8: {e}")),
    };

    let base = if base_state.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(base_state, base_state_len) })
    };

    match crdt::merge(base, ours_str, theirs_str) {
        Ok(merged_text) => {
            // Encode the merged state for persistence
            let doc = crdt::CrdtDoc::from_text(&merged_text);
            let state_bytes = doc.encode_state();
            let state_len = state_bytes.len();
            let state_ptr = {
                let mut boxed = state_bytes.into_boxed_slice();
                let ptr = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                ptr
            };

            FfiMergeResult {
                text: CString::new(merged_text).unwrap_or_default().into_raw(),
                state: state_ptr,
                state_len,
                error: ptr::null_mut(),
            }
        }
        Err(e) => make_err(&format!("{e}")),
    }
}

/// Merge YAML key/value pairs into a document's frontmatter.
///
/// `yaml_fields` is a YAML string of fields to merge (additive — never removes keys).
/// Returns the updated document content via [`FfiPatchResult`].
///
/// # Safety
///
/// All string pointers must be valid, NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_merge_frontmatter(
    doc: *const c_char,
    yaml_fields: *const c_char,
) -> FfiPatchResult {
    let make_err = |msg: &str| FfiPatchResult {
        text: ptr::null_mut(),
        error: CString::new(msg).unwrap_or_default().into_raw(),
    };

    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid doc UTF-8: {e}")),
    };
    let fields_str = match unsafe { CStr::from_ptr(yaml_fields) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid yaml_fields UTF-8: {e}")),
    };

    match frontmatter::merge_fields(doc_str, fields_str) {
        Ok(result) => FfiPatchResult {
            text: CString::new(result).unwrap_or_default().into_raw(),
            error: ptr::null_mut(),
        },
        Err(e) => make_err(&format!("{e}")),
    }
}

/// Reposition boundary marker to end of exchange component.
///
/// Removes all existing boundary markers from the document and inserts a single
/// fresh one at the end of the exchange component. Returns the document unchanged
/// if no exchange component exists.
///
/// # Safety
///
/// `doc` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reposition_boundary_to_end(
    doc: *const c_char,
) -> FfiPatchResult {
    let make_err = |msg: &str| FfiPatchResult {
        text: ptr::null_mut(),
        error: CString::new(msg).unwrap_or_default().into_raw(),
    };

    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return make_err(&format!("invalid doc UTF-8: {e}")),
    };

    let result = template::reposition_boundary_to_end(doc_str);
    FfiPatchResult {
        text: CString::new(result).unwrap_or_default().into_raw(),
        error: ptr::null_mut(),
    }
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
        crate::debounce::document_changed(path);
    }
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
pub unsafe extern "C" fn agent_doc_is_tracked(file_path: *const c_char) -> bool {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    crate::debounce::is_tracked(path)
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
) -> bool {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return true, // Invalid path — don't block
    };
    crate::debounce::await_idle(path, debounce_ms as u64, timeout_ms as u64)
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
pub unsafe extern "C" fn agent_doc_set_status(
    file_path: *const c_char,
    status: *const c_char,
) {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    let st = match unsafe { CStr::from_ptr(status) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    crate::debounce::set_status(path, st);
}

/// Get the response status for a file (Option B: in-process).
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
    let status = crate::debounce::get_status(path);
    CString::new(status).unwrap_or_else(|_| CString::new("idle").unwrap()).into_raw()
}

/// Check if any operation is in progress for a file (Option B: in-process).
///
/// Returns `true` if status is NOT "idle". Plugins should skip route
/// operations when this returns `true` to prevent cascading.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_busy(file_path: *const c_char) -> bool {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    crate::debounce::is_busy(path)
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
pub extern "C" fn agent_doc_sync_try_lock() -> bool {
    SYNC_LOCKED.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok()
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
pub extern "C" fn agent_doc_sync_check_generation(generation: u64) -> bool {
    SYNC_GENERATION.load(Ordering::SeqCst) == generation
}

/// Get the agent-doc library version.
///
/// Returns a NUL-terminated string like "0.26.1".
/// Caller must free with `agent_doc_free_string`.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_version() -> *mut c_char {
    CString::new(env!("CARGO_PKG_VERSION")).unwrap().into_raw()
}

/// Free a string returned by any `agent_doc_*` function.
///
/// # Safety
///
/// `ptr` must have been returned by an `agent_doc_*` function, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

/// Free a state buffer returned by [`agent_doc_crdt_merge`].
///
/// # Safety
///
/// `ptr` and `len` must match a state buffer returned by `agent_doc_crdt_merge`, or `ptr` must be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_free_state(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(unsafe { Vec::from_raw_parts(ptr, len, len) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn apply_patch_replace() {
        let doc = "<!-- agent:output -->\nold\n<!-- /agent:output -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_name = CString::new("output").unwrap();
        let c_content = CString::new("new content\n").unwrap();
        let c_mode = CString::new("replace").unwrap();
        let result = unsafe {
            agent_doc_apply_patch(c_doc.as_ptr(), c_name.as_ptr(), c_content.as_ptr(), c_mode.as_ptr())
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("new content"));
        assert!(!text.contains("old"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn merge_frontmatter_adds_field() {
        let doc = "---\nagent_doc_session: abc\n---\nBody\n";
        let fields = "model: opus";
        let c_doc = CString::new(doc).unwrap();
        let c_fields = CString::new(fields).unwrap();
        let result = unsafe {
            agent_doc_merge_frontmatter(c_doc.as_ptr(), c_fields.as_ptr())
        };
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
        assert_eq!(boundary_count, 1, "should have exactly 1 boundary, got {}", boundary_count);
        // The boundary should be just before the close tag
        assert!(text.contains("more\n<!-- agent:boundary:"));
        assert!(text.contains(" -->\n<!-- /agent:exchange -->"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn crdt_merge_no_base() {
        let c_ours = CString::new("hello world").unwrap();
        let c_theirs = CString::new("hello world").unwrap();
        let result = unsafe {
            agent_doc_crdt_merge(ptr::null(), 0, c_ours.as_ptr(), c_theirs.as_ptr())
        };
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

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
    let patch = template::PatchBlock::new(name, patch_content);

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
    let patch = template::PatchBlock::new(name, patch_content);
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
    let patch = template::PatchBlock::new(name, patch_content);
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

/// Text-based CRDT 3-way merge. Simpler interface than [`agent_doc_crdt_merge`].
///
/// All three parameters are plain UTF-8 text (not CRDT state bytes).
/// Returns the conflict-free merged text. On any error, falls back to `ours`.
///
/// Intended for editor plugin use (replaces `git merge-file` in `PromptPoller`).
///
/// # Safety
///
/// `base`, `ours`, and `theirs` must be valid, NUL-terminated UTF-8.
/// The caller must free the returned pointer with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_merge_crdt(
    base: *const c_char,
    ours: *const c_char,
    theirs: *const c_char,
) -> *mut c_char {
    let base_str = match unsafe { CStr::from_ptr(base) }.to_str() {
        Ok(s) => s,
        Err(_) => return CString::new("").unwrap_or_default().into_raw(),
    };
    let ours_str = match unsafe { CStr::from_ptr(ours) }.to_str() {
        Ok(s) => s,
        Err(_) => return CString::new("").unwrap_or_default().into_raw(),
    };
    let theirs_str = match unsafe { CStr::from_ptr(theirs) }.to_str() {
        Ok(s) => s,
        Err(_) => return CString::new("").unwrap_or_default().into_raw(),
    };

    // Encode base text as CRDT state for proper 3-way merge
    let base_doc = crdt::CrdtDoc::from_text(base_str);
    let base_state = base_doc.encode_state();

    let merged = crdt::merge(Some(&base_state), ours_str, theirs_str)
        .unwrap_or_else(|_| ours_str.to_string());
    CString::new(merged).unwrap_or_default().into_raw()
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

/// Return the number of files tracked in the debounce state.
/// Used by IDE plugins for state diagnostics.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_tracked_count() -> u32 {
    crate::debounce::tracked_count() as u32
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
pub unsafe extern "C" fn agent_doc_is_idle(
    file_path: *const c_char,
    debounce_ms: i64,
) -> bool {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return true, // Invalid path — don't block callers
    };
    let in_process_idle = crate::debounce::is_idle(path, debounce_ms as u64);
    if !in_process_idle {
        return false;
    }
    // In-process says idle. If the file was never tracked in this process (e.g., after
    // plugin restart), also check the file-based indicator so cross-process typing state
    // from another plugin instance isn't silently lost.
    if !crate::debounce::is_tracked(path) {
        return !crate::debounce::is_typing_via_file(path, debounce_ms as u64);
    }
    true
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
    // When the file is untracked in-process (e.g., after plugin restart), bridge to
    // file-based indicator so cross-process typing state isn't silently ignored.
    if !crate::debounce::is_tracked(path) {
        return crate::debounce::await_idle_via_file(
            path,
            debounce_ms as u64,
            timeout_ms as u64,
        );
    }
    crate::debounce::await_idle(path, debounce_ms as u64, timeout_ms as u64)
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
) -> bool {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    crate::debounce::is_typing_via_file(path, debounce_ms as u64)
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
) -> bool {
    let path = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return true, // Invalid path — don't block
    };
    crate::debounce::await_idle_via_file(path, debounce_ms as u64, timeout_ms as u64)
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
    let status = crate::debounce::get_status(path);
    CString::new(status).unwrap_or_else(|_| CString::new("idle").unwrap()).into_raw()
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
    callback: extern "C" fn(message: *const c_char) -> bool,
) -> bool {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return false,
    };
    let root_path = std::path::PathBuf::from(&root_str);

    std::thread::spawn(move || {
        let result = crate::ipc_socket::start_listener(&root_path, move |msg| {
            // Lend the message to the callback (no ownership transfer)
            let c_msg = match CString::new(msg) {
                Ok(c) => c,
                Err(_) => return Some(r#"{"type":"ack","status":"error"}"#.to_string()),
            };
            let success = callback(c_msg.as_ptr());
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

    true
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
pub unsafe extern "C" fn agent_doc_stop_ipc_listener(
    project_root: *const c_char,
) {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };
    let sock = crate::ipc_socket::socket_path(std::path::Path::new(root_str));
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
) -> bool {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let patch_id_str = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let content_str = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let ack_dir = std::path::Path::new(root_str).join(".agent-doc/ack-content");
    if let Err(e) = std::fs::create_dir_all(&ack_dir) {
        eprintln!("[ffi] agent_doc_write_ack_content: mkdir error: {e}");
        return false;
    }

    let sidecar = ack_dir.join(format!("{patch_id_str}.md"));
    match std::fs::write(&sidecar, content_str) {
        Ok(_) => {
            eprintln!("[ffi] ack_content written: {} bytes for patch_id {}",
                content_str.len(), &patch_id_str[..patch_id_str.len().min(8)]);
            true
        }
        Err(e) => {
            eprintln!("[ffi] agent_doc_write_ack_content: write error: {e}");
            false
        }
    }
}

/// Check if --force-disk claimed this patch by writing a sentinel file.
/// Returns true if the sentinel `.agent-doc/claimed-patches/<patch_id>` exists.
/// Deletes the sentinel on success (one-time use).
///
/// # Safety
///
/// Both pointers must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_is_claimed_by_force_disk(
    project_root: *const c_char,
    patch_id: *const c_char,
) -> bool {
    let root_str = match unsafe { CStr::from_ptr(project_root) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let patch_id_str = match unsafe { CStr::from_ptr(patch_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let sentinel = std::path::Path::new(root_str)
        .join(".agent-doc/claimed-patches")
        .join(patch_id_str);

    if sentinel.exists() {
        eprintln!("[ffi] patch_id {} claimed by force-disk — skipping apply",
            &patch_id_str[..patch_id_str.len().min(8)]);
        let _ = std::fs::remove_file(&sentinel);
        true
    } else {
        false
    }
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
pub unsafe extern "C" fn agent_doc_commit(file_path: *const c_char) -> bool {
    if file_path.is_null() {
        return false;
    }
    let path_str = match unsafe { CStr::from_ptr(file_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let path = std::path::Path::new(path_str);
    ffi_git_commit(path)
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
        Ok(o) if o.status.success() => std::path::PathBuf::from(
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        ),
        _ => return false,
    };

    let doc_name = file.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
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
        eprintln!("[ffi] agent_doc_commit: git add failed for {}", file.display());
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
    fn is_idle_untracked_returns_true() {
        let path = CString::new("/tmp/ffi-test-untracked-file.md").unwrap();
        let result = unsafe { agent_doc_is_idle(path.as_ptr(), 500) };
        assert!(result, "untracked file should report idle");
    }

    #[test]
    fn is_idle_after_change_returns_false() {
        let path = CString::new("/tmp/ffi-test-just-changed.md").unwrap();
        unsafe { agent_doc_document_changed(path.as_ptr()) };
        let result = unsafe { agent_doc_is_idle(path.as_ptr(), 2000) };
        assert!(!result, "file changed <2s ago should not be idle with 2000ms window");
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
        assert!(!result.project_root.is_null(), "project_root should be non-null");
        assert!(!result.relative_path.is_null(), "relative_path should be non-null");

        let root = unsafe { CStr::from_ptr(result.project_root) }.to_str().unwrap();
        let rel = unsafe { CStr::from_ptr(result.relative_path) }.to_str().unwrap();
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
        let root = unsafe { CStr::from_ptr(result.project_root) }.to_str().unwrap();
        let rel = unsafe { CStr::from_ptr(result.relative_path) }.to_str().unwrap();
        assert_eq!(std::path::Path::new(root), mid.canonicalize().unwrap(),
            "should prefer nearest (mid) over outer");
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
        let rel = unsafe { CStr::from_ptr(result.relative_path) }.to_str().unwrap();
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
            agent_doc_write_ack_content(
                project_root.as_ptr(),
                patch_id.as_ptr(),
                content.as_ptr(),
            )
        };
        assert!(result, "should return true on success");

        let sidecar = tmp.path().join(".agent-doc/ack-content/test-patch-id-123.md");
        assert!(sidecar.exists(), "sidecar file should exist at {:?}", sidecar);
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

        let claimed = unsafe { agent_doc_is_claimed_by_force_disk(project_root.as_ptr(), patch_id.as_ptr()) };
        assert!(claimed, "should return true when sentinel exists");
        assert!(!claimed_dir.join("test-patch-456").exists(), "sentinel should be deleted after check");
    }

    #[test]
    fn test_is_claimed_by_force_disk_absent() {
        let tmp = TempDir::new().unwrap();
        let project_root = CString::new(tmp.path().to_str().unwrap()).unwrap();
        let patch_id = CString::new("nonexistent-patch").unwrap();

        let claimed = unsafe { agent_doc_is_claimed_by_force_disk(project_root.as_ptr(), patch_id.as_ptr()) };
        assert!(!claimed, "should return false when sentinel absent");
    }

    // --- Fix 4: agent_doc_commit FFI export ---

    #[test]
    fn agent_doc_commit_returns_false_for_null() {
        let result = unsafe { agent_doc_commit(std::ptr::null()) };
        assert!(!result, "null path should return false");
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

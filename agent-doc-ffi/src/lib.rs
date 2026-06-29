//! C-ABI exports for FFI consumers (editor plugins, Python bindings).
//!
//! Pure subset of the FFI surface: functions that depend only on focused
//! data/merge crates and need no orchestration-layer state. The full
//! editor-plugin FFI lives in `agent_doc::ffi` (main crate), which force-links
//! these symbols into the single shipped cdylib.
//!
//! Wave 5 / `#k9e1` of `#adcr` — proof-of-concept relocation. Adding more
//! pure functions to this module is tracked under follow-up sub-tasks of
//! `#k9e1`. See `tasks/agent-doc/prd-crate-decomposition.md`.
//!
//! `#k9e1-ffi-simple` (`#epv5`) relocated the four simplest pure FFI
//! functions (`agent_doc_parse_components`, `agent_doc_visual_tokens_json`,
//! `agent_doc_merge_frontmatter`, `agent_doc_normalize_template_structure`)
//! plus the shared `FfiPatchResult` C-ABI type and its constructor helpers,
//! which the boundary/apply-patch surfaces (`#vb8h` / `#e130`) also depend on.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use agent_doc_element::element;

use agent_doc_frontmatter::frontmatter;
use agent_doc_merge::crdt;
use agent_doc_merge::crdt_sync::ReplicaState;
use agent_doc_syntax as syntax;
use agent_doc_template as template;

/// Free a string returned by an `agent_doc_*` function.
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

/// Free a state buffer returned by an `agent_doc_*` function (e.g.
/// `agent_doc_crdt_merge`).
///
/// # Safety
///
/// `ptr` and `len` must match a state buffer returned by an
/// `agent_doc_*` function, or `ptr` must be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_free_state(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(unsafe { Vec::from_raw_parts(ptr, len, len) });
    }
}

/// Serialized component info returned by [`agent_doc_parse_components`].
#[repr(C)]
pub struct FfiComponentList {
    /// JSON-encoded array of components. Free with [`agent_doc_free_string`].
    pub json: *mut c_char,
    /// Number of components parsed (convenience — also available in the JSON).
    pub count: usize,
}

/// Result of a patch-style FFI function (`agent_doc_apply_patch`,
/// `agent_doc_merge_frontmatter`, `agent_doc_normalize_template_structure`,
/// the `agent_doc_reposition_boundary_*` family, …).
#[repr(C)]
pub struct FfiPatchResult {
    /// The patched document text, or null on error. Free with [`agent_doc_free_string`].
    pub text: *mut c_char,
    /// Error message if `text` is null. Free with [`agent_doc_free_string`].
    pub error: *mut c_char,
}

/// Build a successful [`FfiPatchResult`] from patched document text.
pub fn ffi_patch_ok(text: String) -> FfiPatchResult {
    FfiPatchResult {
        text: CString::new(text).unwrap_or_default().into_raw(),
        error: ptr::null_mut(),
    }
}

/// Build an error [`FfiPatchResult`] from a message.
pub fn ffi_patch_err(msg: &str) -> FfiPatchResult {
    FfiPatchResult {
        text: ptr::null_mut(),
        error: CString::new(msg).unwrap_or_default().into_raw(),
    }
}

/// Convert an `anyhow::Result<String>` into an [`FfiPatchResult`].
pub fn ffi_patch_from_result(result: anyhow::Result<String>) -> FfiPatchResult {
    match result {
        Ok(text) => ffi_patch_ok(text),
        Err(e) => ffi_patch_err(&format!("{e:#}")),
    }
}

/// Run the editor-visible template-structure normalization pass over patched text.
pub fn normalize_editor_visible_result(text: String) -> anyhow::Result<String> {
    template::normalize_editor_visible_template_structure(&text)
}

/// Map UTF-8 byte offsets to UTF-16 code-unit offsets in a single pass.
///
/// Editor range APIs (JetBrains, VS Code) consume UTF-16 offsets, while the
/// document parser works in UTF-8 byte offsets. The `targets` are byte offsets
/// to translate; the returned map keys them to their UTF-16 positions.
fn utf8_offsets_to_utf16_offsets(doc: &str, offsets: &[usize]) -> HashMap<usize, usize> {
    let mut targets = offsets.to_vec();
    targets.sort_unstable();
    targets.dedup();

    let mut mapped = HashMap::with_capacity(targets.len());
    let mut target_idx = 0usize;
    let mut utf8_offset = 0usize;
    let mut utf16_offset = 0usize;

    while target_idx < targets.len() && targets[target_idx] == 0 {
        mapped.insert(0, 0);
        target_idx += 1;
    }

    for ch in doc.chars() {
        let next_utf8 = utf8_offset + ch.len_utf8();
        while target_idx < targets.len() && targets[target_idx] < next_utf8 {
            mapped.insert(targets[target_idx], utf16_offset);
            target_idx += 1;
        }

        utf8_offset = next_utf8;
        utf16_offset += ch.len_utf16();

        while target_idx < targets.len() && targets[target_idx] == utf8_offset {
            mapped.insert(targets[target_idx], utf16_offset);
            target_idx += 1;
        }
    }

    while target_idx < targets.len() {
        mapped.insert(targets[target_idx], utf16_offset);
        target_idx += 1;
    }

    mapped
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

    let components = match element::parse(doc_str) {
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

/// Collect editor-facing visual token ranges from a markdown document.
///
/// The returned JSON array contains `{ kind, start, end }` objects, where
/// offsets are UTF-16 document positions suitable for JetBrains and VS Code
/// range APIs.
///
/// # Safety
///
/// `doc` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_visual_tokens_json(doc: *const c_char) -> *mut c_char {
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    let tokens = syntax::collect_visual_tokens(doc_str);
    let offsets: Vec<usize> = tokens
        .iter()
        .flat_map(|token| [token.start, token.end])
        .collect();
    let utf16_offsets = utf8_offsets_to_utf16_offsets(doc_str, &offsets);
    let editor_tokens: Vec<_> = tokens
        .iter()
        .map(|token| {
            serde_json::json!({
                "kind": token.kind,
                "start": utf16_offsets[&token.start],
                "end": utf16_offsets[&token.end],
            })
        })
        .collect();
    let json = match serde_json::to_string(&editor_tokens) {
        Ok(json) => json,
        Err(_) => return ptr::null_mut(),
    };
    match CString::new(json) {
        Ok(value) => value.into_raw(),
        Err(_) => ptr::null_mut(),
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

/// Converge the `agent:queue` opening-tag `auto` attribute to `want_auto`.
///
/// A content patch replaces only a component's body, so it cannot add or remove
/// the `auto` attribute on the `<!-- agent:queue auto -->` opening tag. Editor
/// plugins call this to converge a live route-owned buffer's queue tag to the
/// committed inactive shape after a queue halt (`#adoc-queue-ipc-buffer-divergence`),
/// pairing with the `queue_active` frontmatter merge that the same convergence
/// patch carries. Returns the (possibly unchanged) document via [`FfiPatchResult`];
/// an absent `queue` component or an already-converged tag returns the input
/// unchanged so the editor's normal no-op path applies.
///
/// `want_auto` is a C int (nonzero = ensure `auto`, zero = strip `auto`) rather
/// than a C `bool` so the JNA/FFI boundary stays on the reliable 32-bit-int ABI.
///
/// # Safety
///
/// `doc` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_converge_queue_auto(
    doc: *const c_char,
    want_auto: std::os::raw::c_int,
) -> FfiPatchResult {
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid doc UTF-8: {e}")),
    };
    let converged = element::converge_queue_auto(doc_str, want_auto != 0)
        .unwrap_or_else(|| doc_str.to_string());
    FfiPatchResult {
        text: CString::new(converged).unwrap_or_default().into_raw(),
        error: ptr::null_mut(),
    }
}

/// Normalize/fail-close template structure before editor-visible IPC writes.
///
/// Safe duplicate scaffold shells are repaired. Ambiguous duplicate scaffold
/// content, conversation text outside exchange, or malformed component shape
/// returns an error so editor plugins can refuse to mutate the visible buffer.
///
/// # Safety
///
/// `doc` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_normalize_template_structure(
    doc: *const c_char,
) -> FfiPatchResult {
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid doc UTF-8: {e}")),
    };

    ffi_patch_from_result(template::normalize_editor_visible_template_structure(
        doc_str,
    ))
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

/// 3-way CRDT merge over opaque state bytes.
///
/// Returns the merged text plus updated CRDT state for persistence.
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

    match crdt::merge_by_component(base, ours_str, theirs_str) {
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

// ---------------------------------------------------------------------------
// Editor-as-replica FFI (`#crdtauth2`) — the cdylib hosts a per-editor yrs replica.
//
// FFI-first per the Shared Foundation pattern: the CRDT node (a durable
// [`crate::crdt_sync::ReplicaState`]) lives in the shared library; thin editor
// plugins are bindings that forward local Document deltas (`apply_local`) and
// apply remote updates (`apply_update`), exchanging state vectors / updates with
// the supervisor's canonical replica. Each replica is keyed by a caller-chosen
// `replica_id` (also its yrs client id, so distinct replicas order concurrent
// inserts deterministically). Authority gating (sync only under a multi-replica
// authority, `#crdtauth1sv`) lives in the orchestration layer over this transport.
// ---------------------------------------------------------------------------

/// Process-wide registry of cdylib-hosted CRDT replicas, keyed by `replica_id`.
static REPLICA_REGISTRY: OnceLock<Mutex<HashMap<u64, ReplicaState>>> = OnceLock::new();

fn replica_registry() -> &'static Mutex<HashMap<u64, ReplicaState>> {
    REPLICA_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Leak `bytes` to the C caller (freed with [`agent_doc_free_state`]); writes the
/// length to `out_len` when non-null.
fn leak_state(bytes: Vec<u8>, out_len: *mut usize) -> *mut u8 {
    let len = bytes.len();
    let mut boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    if !out_len.is_null() {
        unsafe { *out_len = len };
    }
    ptr
}

/// The null byte-buffer result: null pointer with `*out_len = 0`.
fn null_state(out_len: *mut usize) -> *mut u8 {
    if !out_len.is_null() {
        unsafe { *out_len = 0 };
    }
    ptr::null_mut()
}

/// Open (or reset) the cdylib-hosted CRDT replica `replica_id`, optionally
/// bootstrapping it from a previously encoded state (`init_state` / `init_len`;
/// pass null / 0 for a fresh empty replica). `replica_id` is also the yrs client
/// id, so distinct live replicas MUST use distinct ids.
///
/// Returns 0 on success, -1 on a poisoned registry, -2 on an invalid init state.
///
/// # Safety
/// If `init_state` is non-null, `init_len` bytes must be readable from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_open(
    replica_id: u64,
    init_state: *const u8,
    init_len: usize,
) -> i32 {
    let replica = if init_state.is_null() || init_len == 0 {
        ReplicaState::new(replica_id)
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(init_state, init_len) };
        match ReplicaState::from_encoded(replica_id, bytes) {
            Ok(r) => r,
            Err(_) => return -2,
        }
    };
    match replica_registry().lock() {
        Ok(mut reg) => {
            reg.insert(replica_id, replica);
            0
        }
        Err(_) => -1,
    }
}

/// Apply a local edit to replica `replica_id` (the editor forwarding a local
/// `Document` delta): delete `delete_len` chars at `offset`, then insert
/// `insert`. Returns 0 ok, -1 poison, -2 bad UTF-8, -3 replica not open.
///
/// # Safety
/// `insert` must be a valid NUL-terminated UTF-8 string (may be empty), or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_apply_local(
    replica_id: u64,
    offset: u32,
    delete_len: u32,
    insert: *const c_char,
) -> i32 {
    let insert_str = if insert.is_null() {
        ""
    } else {
        match unsafe { CStr::from_ptr(insert) }.to_str() {
            Ok(s) => s,
            Err(_) => return -2,
        }
    };
    match replica_registry().lock() {
        Ok(reg) => match reg.get(&replica_id) {
            Some(replica) => {
                replica.apply_local_edit(offset, delete_len, insert_str);
                0
            }
            None => -3,
        },
        Err(_) => -1,
    }
}

/// The current text of replica `replica_id`, or null if not open / poisoned.
/// Free with [`agent_doc_free_string`].
///
/// # Safety
/// Always safe to call (no pointer arguments). The returned pointer, when
/// non-null, must be freed exactly once with [`agent_doc_free_string`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_text(replica_id: u64) -> *mut c_char {
    match replica_registry().lock() {
        Ok(reg) => match reg.get(&replica_id) {
            Some(replica) => CString::new(replica.text()).unwrap_or_default().into_raw(),
            None => ptr::null_mut(),
        },
        Err(_) => ptr::null_mut(),
    }
}

/// The encoded state vector of replica `replica_id` (the compact causal summary a
/// replica announces to a peer). Writes the length to `out_len`. Returns null
/// (with `*out_len = 0`) if not open / poisoned. Free with [`agent_doc_free_state`].
///
/// # Safety
/// `out_len` must be a valid writable `usize` pointer, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_state_vector(
    replica_id: u64,
    out_len: *mut usize,
) -> *mut u8 {
    match replica_registry().lock() {
        Ok(reg) => match reg.get(&replica_id) {
            Some(replica) => leak_state(replica.state_vector(), out_len),
            None => null_state(out_len),
        },
        Err(_) => null_state(out_len),
    }
}

/// The incremental update replica `replica_id` should send a peer whose state
/// vector is `their_sv` — only the ops that peer is missing (a delta, not a
/// snapshot). Writes length to `out_len`. Returns null on not-open / bad-sv /
/// poison. Free with [`agent_doc_free_state`].
///
/// # Safety
/// `their_sv` must point to `their_sv_len` readable bytes; `out_len` writable or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_diff(
    replica_id: u64,
    their_sv: *const u8,
    their_sv_len: usize,
    out_len: *mut usize,
) -> *mut u8 {
    if their_sv.is_null() {
        return null_state(out_len);
    }
    let sv = unsafe { std::slice::from_raw_parts(their_sv, their_sv_len) };
    match replica_registry().lock() {
        Ok(reg) => match reg.get(&replica_id) {
            Some(replica) => match replica.diff(sv) {
                Ok(update) => leak_state(update, out_len),
                Err(_) => null_state(out_len),
            },
            None => null_state(out_len),
        },
        Err(_) => null_state(out_len),
    }
}

/// Apply a remote update to replica `replica_id` (the editor applying a peer's
/// ops — idempotent and yrs causal-buffered). Returns 0 ok, -1 poison, -2 bad
/// update bytes, -3 replica not open.
///
/// # Safety
/// `update` must point to `update_len` readable bytes, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_apply_update(
    replica_id: u64,
    update: *const u8,
    update_len: usize,
) -> i32 {
    if update.is_null() {
        return -2;
    }
    let bytes = unsafe { std::slice::from_raw_parts(update, update_len) };
    match replica_registry().lock() {
        Ok(reg) => match reg.get(&replica_id) {
            Some(replica) => match replica.apply_update(bytes) {
                Ok(()) => 0,
                Err(_) => -2,
            },
            None => -3,
        },
        Err(_) => -1,
    }
}

/// The full encoded state of replica `replica_id` (a durable checkpoint, or the
/// snapshot a peer needs on first contact). Writes length to `out_len`. Returns
/// null if not open / poisoned. Free with [`agent_doc_free_state`].
///
/// # Safety
/// `out_len` must be writable or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_encode_state(
    replica_id: u64,
    out_len: *mut usize,
) -> *mut u8 {
    match replica_registry().lock() {
        Ok(reg) => match reg.get(&replica_id) {
            Some(replica) => leak_state(replica.encode_state(), out_len),
            None => null_state(out_len),
        },
        Err(_) => null_state(out_len),
    }
}

/// Close (drop) replica `replica_id`. Returns 0 if it was open, -1 on poison,
/// -3 if it was not open.
///
/// # Safety
/// Always safe to call (no pointer arguments).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_close(replica_id: u64) -> i32 {
    match replica_registry().lock() {
        Ok(mut reg) => {
            if reg.remove(&replica_id).is_some() {
                0
            } else {
                -3
            }
        }
        Err(_) => -1,
    }
}

/// Persist replica `replica_id` to a local file for crash safety (`#crdtauth4`,
/// disk demotion / plan phase 6). Each FFI node writes its OWN replica's encoded
/// state through this so a plugin/IDE crash mid-lag does not lose un-synced ops.
///
/// The file is a **write-through durable recovery projection only** — it is read
/// back by [`agent_doc_replica_recover`] on restart, never the coordination
/// medium. The write is atomic (temp file + rename) so a crash mid-write cannot
/// truncate the projection.
///
/// Returns 0 on success, -1 on poison, -2 on a bad path / IO error, -3 if the
/// replica is not open.
///
/// # Safety
/// `path` must be a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_persist(replica_id: u64, path: *const c_char) -> i32 {
    if path.is_null() {
        return -2;
    }
    let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let state = match replica_registry().lock() {
        Ok(reg) => match reg.get(&replica_id) {
            Some(replica) => replica.encode_state(),
            None => return -3,
        },
        Err(_) => return -1,
    };
    match atomic_write_bytes(std::path::Path::new(path_str), &state) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Recover (open) replica `replica_id` from a local durable recovery projection
/// written by [`agent_doc_replica_persist`] (`#crdtauth4`, disk demotion / plan
/// phase 6). On restart the node rebuilds its in-memory replica from disk; live
/// peers re-sync any newer ops via the normal state-vector exchange afterward, so
/// the recovered projection is a starting point, not authority.
///
/// Returns 0 on success, -1 on poison, -2 on a bad path / IO / decode error.
///
/// # Safety
/// `path` must be a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_replica_recover(replica_id: u64, path: *const c_char) -> i32 {
    if path.is_null() {
        return -2;
    }
    let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let bytes = match std::fs::read(path_str) {
        Ok(b) => b,
        Err(_) => return -2,
    };
    let replica = match ReplicaState::from_encoded(replica_id, &bytes) {
        Ok(r) => r,
        Err(_) => return -2,
    };
    match replica_registry().lock() {
        Ok(mut reg) => {
            reg.insert(replica_id, replica);
            0
        }
        Err(_) => -1,
    }
}

/// Atomically write `bytes` to `path` (sibling temp file + rename) so a crash
/// mid-write cannot truncate a recovery projection.
fn atomic_write_bytes(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let pid = std::process::id();
    let tmp_path = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => path.with_extension(format!("{ext}.tmp.{pid}")),
        None => path.with_extension(format!("tmp.{pid}")),
    };
    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.flush()?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
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

    let merged = crdt::merge_by_component(Some(&base_state), ours_str, theirs_str)
        .unwrap_or_else(|_| ours_str.to_string());
    CString::new(merged).unwrap_or_default().into_raw()
}

/// Reposition boundary marker to end of exchange component.
///
/// Removes all existing boundary markers from the document, strips transient
/// heading-level ` (HEAD)` suffixes, and inserts a single fresh boundary at the
/// end of the exchange component. Returns the document unchanged if no exchange
/// component exists.
///
/// # Safety
///
/// `doc` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reposition_boundary_to_end(
    doc: *const c_char,
) -> FfiPatchResult {
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid doc UTF-8: {e}")),
    };

    let result = template::reposition_boundary_to_end_clean(doc_str);
    ffi_patch_from_result(normalize_editor_visible_result(result))
}

/// Reposition boundary marker to end of exchange component using an explicit ID.
///
/// This is used by post-commit editor refresh so the live buffer can be
/// normalized back to the exact boundary marker already committed in `HEAD`
/// rather than generating a fresh boundary-only local diff.
///
/// # Safety
///
/// `doc` and `boundary_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reposition_boundary_to_end_with_id(
    doc: *const c_char,
    boundary_id: *const c_char,
) -> FfiPatchResult {
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid doc UTF-8: {e}")),
    };
    let boundary_id_str = match unsafe { CStr::from_ptr(boundary_id) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid boundary_id UTF-8: {e}")),
    };

    let result = template::reposition_boundary_to_end_clean_with_id(doc_str, Some(boundary_id_str));
    ffi_patch_from_result(normalize_editor_visible_result(result))
}

/// Reposition boundary marker to end of exchange, preserving `(HEAD)` markers.
///
/// Used for post-commit working-tree cleanup where `(HEAD)` annotations should
/// remain visible to the user. The committed blob and snapshot use the `_clean`
/// variant; the working tree and editor buffer use this variant.
///
/// # Safety
///
/// `doc` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reposition_boundary_to_end_preserve_head(
    doc: *const c_char,
) -> FfiPatchResult {
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid doc UTF-8: {e}")),
    };

    let result = template::reposition_boundary_to_end_preserve_head(doc_str);
    ffi_patch_from_result(normalize_editor_visible_result(result))
}

/// Reposition boundary using an explicit ID, preserving `(HEAD)` markers.
///
/// # Safety
///
/// `doc` and `boundary_id` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_reposition_boundary_to_end_preserve_head_with_id(
    doc: *const c_char,
    boundary_id: *const c_char,
) -> FfiPatchResult {
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid doc UTF-8: {e}")),
    };
    let boundary_id_str = match unsafe { CStr::from_ptr(boundary_id) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid boundary_id UTF-8: {e}")),
    };

    let result =
        template::reposition_boundary_to_end_preserve_head_with_id(doc_str, Some(boundary_id_str));
    ffi_patch_from_result(normalize_editor_visible_result(result))
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
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid doc UTF-8: {e}")),
    };
    let name = match unsafe { CStr::from_ptr(component_name) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid component name UTF-8: {e}")),
    };
    let patch_content = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid content UTF-8: {e}")),
    };
    let mode_str = match unsafe { CStr::from_ptr(mode) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid mode UTF-8: {e}")),
    };

    // Build a patch block and apply it
    let patch = template::PatchBlock::new(name, patch_content);

    // Use mode overrides to force the specified mode
    let mut overrides = HashMap::new();
    overrides.insert(name.to_string(), mode_str.to_string());

    // Editor FFI applies to in-memory content with explicit overrides — no
    // backing file, so summary is None and component/max_lines configs are empty.
    ffi_patch_from_result(
        template::apply_patches_with_overrides_pure(
            doc_str,
            &[patch],
            "",
            None,
            &HashMap::new(),
            &HashMap::new(),
            &overrides,
        )
        .and_then(normalize_editor_visible_result),
    )
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
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid doc UTF-8: {e}")),
    };
    let name = match unsafe { CStr::from_ptr(component_name) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid component name UTF-8: {e}")),
    };
    let patch_content = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid content UTF-8: {e}")),
    };
    let mode_str = match unsafe { CStr::from_ptr(mode) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid mode UTF-8: {e}")),
    };

    // If append mode with a valid caret, use cursor-aware insertion
    if mode_str == "append" && caret_offset >= 0 {
        let components = match element::parse(doc_str) {
            Ok(c) => c,
            Err(e) => return ffi_patch_err(&format!("{e}")),
        };
        if let Some(comp) = components.iter().find(|c| c.name == name) {
            let result =
                comp.append_with_caret(doc_str, patch_content, Some(caret_offset as usize));
            return ffi_patch_from_result(normalize_editor_visible_result(result));
        }
    }

    // Fall back to normal apply_patch behavior
    let patch = template::PatchBlock::new(name, patch_content);
    let mut overrides = HashMap::new();
    overrides.insert(name.to_string(), mode_str.to_string());
    ffi_patch_from_result(
        template::apply_patches_with_overrides_pure(
            doc_str,
            &[patch],
            "",
            None,
            &HashMap::new(),
            &HashMap::new(),
            &overrides,
        )
        .and_then(normalize_editor_visible_result),
    )
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
    let doc_str = match unsafe { CStr::from_ptr(doc) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid doc UTF-8: {e}")),
    };
    let name = match unsafe { CStr::from_ptr(component_name) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid component name UTF-8: {e}")),
    };
    let patch_content = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid content UTF-8: {e}")),
    };
    let mode_str = match unsafe { CStr::from_ptr(mode) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid mode UTF-8: {e}")),
    };
    let bid = match unsafe { CStr::from_ptr(boundary_id) }.to_str() {
        Ok(s) => s,
        Err(e) => return ffi_patch_err(&format!("invalid boundary_id UTF-8: {e}")),
    };

    // Use boundary-aware insertion for append mode
    if mode_str == "append" && !bid.is_empty() {
        let components = match element::parse(doc_str) {
            Ok(c) => c,
            Err(e) => return ffi_patch_err(&format!("{e}")),
        };
        if let Some(comp) = components.iter().find(|c| c.name == name) {
            let result = comp.append_with_boundary(doc_str, patch_content, bid);
            let result = if name == "exchange" {
                template::annotate_exchange_headings_against_baseline(&result, doc_str)
            } else {
                result
            };
            return ffi_patch_from_result(normalize_editor_visible_result(result));
        }
    }

    // Fall back to normal apply_patch behavior
    let patch = template::PatchBlock::new(name, patch_content);
    let mut overrides = HashMap::new();
    overrides.insert(name.to_string(), mode_str.to_string());
    ffi_patch_from_result(
        template::apply_patches_with_overrides_pure(
            doc_str,
            &[patch],
            "",
            None,
            &HashMap::new(),
            &HashMap::new(),
            &overrides,
        )
        .and_then(normalize_editor_visible_result),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    #[test]
    fn apply_patch_append_preserves_leading_code_fence_after_prompt_fence() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ show fenced prompt\n",
            "```\n",
            "prompt body\n",
            "```\n",
            "<!-- /agent:exchange -->\n",
        );
        let c_doc = CString::new(doc).unwrap();
        let c_name = CString::new("exchange").unwrap();
        let c_content = CString::new("```\nresponse body\n```\n").unwrap();
        let c_mode = CString::new("append").unwrap();

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
        assert_eq!(
            text.matches("```").count(),
            4,
            "native IPC append must keep prompt and response fences:\n{text}"
        );
        assert!(
            text.contains("```\n```\nresponse body\n```"),
            "native IPC append stripped the response opening fence:\n{text}"
        );
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn replica_ffi_round_trip_converges_via_state_vector_exchange() {
        use std::ffi::CString;

        let a: u64 = 0xA11CE;
        let b: u64 = 0xB0B;
        assert_eq!(unsafe { agent_doc_replica_open(a, std::ptr::null(), 0) }, 0);
        assert_eq!(unsafe { agent_doc_replica_open(b, std::ptr::null(), 0) }, 0);

        // Each replica forwards a local Document delta (concurrent, non-overlapping).
        let ins_a = CString::new("alpha").unwrap();
        let ins_b = CString::new("beta").unwrap();
        assert_eq!(
            unsafe { agent_doc_replica_apply_local(a, 0, 0, ins_a.as_ptr()) },
            0
        );
        assert_eq!(
            unsafe { agent_doc_replica_apply_local(b, 0, 0, ins_b.as_ptr()) },
            0
        );

        // Exchange state vectors → deltas (only the missing ops), then apply.
        let mut b_sv_len: usize = 0;
        let b_sv = unsafe { agent_doc_replica_state_vector(b, &mut b_sv_len) };
        assert!(!b_sv.is_null());
        let mut a_to_b_len: usize = 0;
        let a_to_b = unsafe { agent_doc_replica_diff(a, b_sv, b_sv_len, &mut a_to_b_len) };
        assert!(!a_to_b.is_null());

        let mut a_sv_len: usize = 0;
        let a_sv = unsafe { agent_doc_replica_state_vector(a, &mut a_sv_len) };
        let mut b_to_a_len: usize = 0;
        let b_to_a = unsafe { agent_doc_replica_diff(b, a_sv, a_sv_len, &mut b_to_a_len) };

        assert_eq!(
            unsafe { agent_doc_replica_apply_update(b, a_to_b, a_to_b_len) },
            0
        );
        assert_eq!(
            unsafe { agent_doc_replica_apply_update(a, b_to_a, b_to_a_len) },
            0
        );

        let ta = unsafe { agent_doc_replica_text(a) };
        let tb = unsafe { agent_doc_replica_text(b) };
        let sa = unsafe { CStr::from_ptr(ta) }.to_str().unwrap().to_string();
        let sb = unsafe { CStr::from_ptr(tb) }.to_str().unwrap().to_string();
        assert_eq!(sa, sb, "replicas converge after FFI state-vector exchange");
        assert!(sa.contains("alpha") && sa.contains("beta"));

        // Re-applying a known update over the FFI is idempotent.
        assert_eq!(
            unsafe { agent_doc_replica_apply_update(b, a_to_b, a_to_b_len) },
            0
        );
        let tb2 = unsafe { agent_doc_replica_text(b) };
        let sb2 = unsafe { CStr::from_ptr(tb2) }.to_str().unwrap().to_string();
        assert_eq!(sb2, sb, "re-applying a known update via FFI is idempotent");

        unsafe {
            agent_doc_free_state(b_sv, b_sv_len);
            agent_doc_free_state(a_sv, a_sv_len);
            agent_doc_free_state(a_to_b, a_to_b_len);
            agent_doc_free_state(b_to_a, b_to_a_len);
            agent_doc_free_string(ta);
            agent_doc_free_string(tb);
            agent_doc_free_string(tb2);
        }

        assert_eq!(unsafe { agent_doc_replica_close(a) }, 0);
        assert_eq!(unsafe { agent_doc_replica_close(b) }, 0);
        // Closing an already-closed replica reports not-open.
        assert_eq!(unsafe { agent_doc_replica_close(a) }, -3);
    }

    #[test]
    fn replica_ffi_reports_unopened_replica() {
        let id: u64 = 0xDEAD_BEEF;
        let ins = std::ffi::CString::new("x").unwrap();
        assert_eq!(
            unsafe { agent_doc_replica_apply_local(id, 0, 0, ins.as_ptr()) },
            -3,
            "apply_local on an unopened replica reports not-open"
        );
        // A null update is rejected before the lookup.
        assert_eq!(
            unsafe { agent_doc_replica_apply_update(id, std::ptr::null(), 0) },
            -2
        );
        assert!(unsafe { agent_doc_replica_text(id) }.is_null());
        let mut len: usize = 123;
        assert!(unsafe { agent_doc_replica_state_vector(id, &mut len) }.is_null());
        assert_eq!(len, 0, "null byte result sets out_len to 0");
        assert_eq!(unsafe { agent_doc_replica_close(id) }, -3);
    }

    #[test]
    fn replica_ffi_persist_then_recover_round_trips_disk_projection() {
        // Disk demotion (#crdtauth4): each FFI node persists its OWN replica to a
        // local recovery projection and recovers it on restart.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.yrs");
        let path_c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();

        let src: u64 = 0x5151;
        assert_eq!(
            unsafe { agent_doc_replica_open(src, std::ptr::null(), 0) },
            0
        );
        let ins = std::ffi::CString::new("crash-safe").unwrap();
        assert_eq!(
            unsafe { agent_doc_replica_apply_local(src, 0, 0, ins.as_ptr()) },
            0
        );
        // Persist the node's own replica.
        assert_eq!(
            unsafe { agent_doc_replica_persist(src, path_c.as_ptr()) },
            0,
            "persist writes the recovery projection"
        );
        assert!(path.exists());
        assert_eq!(unsafe { agent_doc_replica_close(src) }, 0);

        // Recover into a fresh replica id (a restarted node) from disk.
        let recovered: u64 = 0x5252;
        assert_eq!(
            unsafe { agent_doc_replica_recover(recovered, path_c.as_ptr()) },
            0,
            "recover rebuilds the in-memory replica from disk"
        );
        let t = unsafe { agent_doc_replica_text(recovered) };
        assert!(!t.is_null());
        let text = unsafe { CStr::from_ptr(t) }.to_str().unwrap().to_string();
        unsafe { agent_doc_free_string(t) };
        assert_eq!(text, "crash-safe");
        assert_eq!(unsafe { agent_doc_replica_close(recovered) }, 0);
    }

    #[test]
    fn replica_ffi_persist_recover_reject_bad_args() {
        let id: u64 = 0x6363;
        // persist on an unopened replica → not-open.
        let p = std::ffi::CString::new("/tmp/agent-doc-nope.yrs").unwrap();
        assert_eq!(unsafe { agent_doc_replica_persist(id, p.as_ptr()) }, -3);
        // null path → bad arg.
        assert_eq!(
            unsafe { agent_doc_replica_persist(id, std::ptr::null()) },
            -2
        );
        // recover from a missing file → bad arg / IO.
        let missing = std::ffi::CString::new("/nonexistent/dir/agent-doc-missing.yrs").unwrap();
        assert_eq!(
            unsafe { agent_doc_replica_recover(id, missing.as_ptr()) },
            -2
        );
    }
}

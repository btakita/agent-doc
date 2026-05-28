//! C-ABI exports for FFI consumers (editor plugins, Python bindings).
//!
//! Pure subset of the FFI surface: functions that depend only on
//! `agent-doc-core` types and need no orchestration-layer state. The full
//! editor-plugin FFI lives in `agent_doc::ffi` (main crate), which
//! re-exports the symbols defined here via `pub use agent_doc_core::ffi::*`.
//!
//! Wave 5 / `#k9e1` of `#adcr` — proof-of-concept relocation. Adding more
//! pure functions to this module is tracked under follow-up sub-tasks of
//! `#k9e1`. See `tasks/agent-doc/plan-agent-doc-core-extraction.md`.
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

use crate::{component, frontmatter, syntax, template};

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

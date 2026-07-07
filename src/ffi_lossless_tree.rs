//! `#lzlosstree` Phase 4 FFI surface: lossless-tree frame exchange for editor plugins.
//!
//! Editor plugins that advertise `lossless_tree_crdt_v1` (see
//! [`agent_doc_debounce::LOSSLESS_TREE_CRDT_CAPABILITY`]) call these to move between a
//! session document's text and its durable lossless-tree projection (a full op-stream
//! plus a rendered-text hash). A plugin renders an incoming projection back to buffer
//! text, and projects its own buffer to a frame it can ship back.
//!
//! The projection functions delegate to `agent-doc-merge`'s `lossless_tree`
//! re-exports (which wrap `agent-doc-markdown-lossless`), so this FFI layer needs no
//! new direct dependency in the root crate.
//!
//! # Memory
//!
//! Heap `*mut c_char` returned here must be freed exactly once with the existing
//! `agent_doc_free_string`. Null is returned on invalid UTF-8, unparseable JSON, or a
//! document containing an interior NUL (which cannot cross a C string boundary).

use std::ffi::{CStr, CString, c_char};

/// The capability token a plugin advertises to speak lossless-tree frames. Returns a
/// borrowed static C string — do **not** free. Mirrors
/// `agent_doc_debounce::LOSSLESS_TREE_CRDT_CAPABILITY`.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_lossless_tree_capability() -> *const c_char {
    debug_assert_eq!(
        agent_doc_debounce::LOSSLESS_TREE_CRDT_CAPABILITY,
        "lossless_tree_crdt_v1"
    );
    c"lossless_tree_crdt_v1".as_ptr()
}

/// The on-disk frame path a capable plugin should poll for `file` (see
/// `agent_doc_write_ipc_io::lossless_frame_path`): the binary owns the document hash,
/// so the plugin asks for the path rather than re-deriving it. Returns a heap C string
/// (free with `agent_doc_free_string`) or null on invalid UTF-8 / resolution error.
///
/// # Safety
///
/// `file_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_lossless_tree_frame_path(
    file_path: *const c_char,
) -> *mut c_char {
    if file_path.is_null() {
        return std::ptr::null_mut();
    }
    let Ok(path) = (unsafe { CStr::from_ptr(file_path) }).to_str() else {
        return std::ptr::null_mut();
    };
    let Ok(frame) = agent_doc_write_ipc_io::lossless_frame_path(std::path::Path::new(path)) else {
        return std::ptr::null_mut();
    };
    match CString::new(frame.to_string_lossy().into_owned()) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Read the lossless-tree frame file at `frame_path` and render it to document text —
/// the plugin's one-call apply source. Returns a heap C string (free with
/// `agent_doc_free_string`), or null when the frame is absent / unparseable (the
/// plugin then keeps its buffer). Equivalent to reading the file and calling
/// [`agent_doc_lossless_tree_render`], but handles the file read too.
///
/// # Safety
///
/// `frame_path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_lossless_tree_render_frame(
    frame_path: *const c_char,
) -> *mut c_char {
    if frame_path.is_null() {
        return std::ptr::null_mut();
    }
    let Ok(path) = (unsafe { CStr::from_ptr(frame_path) }).to_str() else {
        return std::ptr::null_mut();
    };
    match agent_doc_merge::lossless_tree::read_frame_render(std::path::Path::new(path)) {
        Ok(Some(text)) => match CString::new(text) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        _ => std::ptr::null_mut(),
    }
}

/// Project `doc_text` into a durable lossless-tree JSON projection (op-stream +
/// rendered hash). Returns a heap C string (free with `agent_doc_free_string`) or
/// null on invalid UTF-8 / serialization failure / interior NUL.
///
/// # Safety
///
/// `doc_text` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_lossless_tree_project(doc_text: *const c_char) -> *mut c_char {
    if doc_text.is_null() {
        return std::ptr::null_mut();
    }
    let Ok(text) = (unsafe { CStr::from_ptr(doc_text) }).to_str() else {
        return std::ptr::null_mut();
    };
    let projection = agent_doc_merge::lossless_tree::project(text);
    let Ok(bytes) = agent_doc_merge::lossless_tree::projection_to_bytes(&projection) else {
        return std::ptr::null_mut();
    };
    match CString::new(bytes) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Render an incoming lossless-tree JSON projection back to document text. Returns a
/// heap C string (free with `agent_doc_free_string`) or null on invalid UTF-8 /
/// unparseable projection / a rendered document containing an interior NUL.
///
/// # Safety
///
/// `projection_json` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_lossless_tree_render(
    projection_json: *const c_char,
) -> *mut c_char {
    if projection_json.is_null() {
        return std::ptr::null_mut();
    }
    let Ok(json) = (unsafe { CStr::from_ptr(projection_json) }).to_str() else {
        return std::ptr::null_mut();
    };
    let Ok(projection) = agent_doc_merge::lossless_tree::projection_from_bytes(json.as_bytes())
    else {
        return std::ptr::null_mut();
    };
    let text = agent_doc_merge::lossless_tree::restore(&projection);
    match CString::new(text) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Whether `projection_json` still describes `visible_text` — the frontier/hash proof
/// that must hold before a projection may reconstruct current document text. Returns
/// `1` when current, `0` when stale **or** on any parse / UTF-8 error: an
/// unvalidatable projection is treated as not-current so a plugin never overrides the
/// editor-visible buffer from a projection it cannot prove fresh.
///
/// # Safety
///
/// `projection_json` and `visible_text` must be valid, NUL-terminated UTF-8 strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agent_doc_lossless_tree_projection_current(
    projection_json: *const c_char,
    visible_text: *const c_char,
) -> i32 {
    if projection_json.is_null() || visible_text.is_null() {
        return 0;
    }
    let (Ok(json), Ok(visible)) = (
        unsafe { CStr::from_ptr(projection_json) }.to_str(),
        unsafe { CStr::from_ptr(visible_text) }.to_str(),
    ) else {
        return 0;
    };
    match agent_doc_merge::lossless_tree::projection_from_bytes(json.as_bytes()) {
        Ok(projection) if projection.is_current_for(visible) => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip a document through the FFI project → render surface and confirm the
    /// exact bytes come back, then that the staleness gate accepts the current
    /// document and rejects a moved-on one.
    #[test]
    fn ffi_project_render_round_trip_and_staleness() {
        let doc = "<!-- agent:status -->\nffi round trip\n<!-- /agent:status -->\n";
        let doc_c = CString::new(doc).unwrap();

        let proj_ptr = unsafe { agent_doc_lossless_tree_project(doc_c.as_ptr()) };
        assert!(!proj_ptr.is_null(), "project returned null");
        let proj_json = unsafe { CStr::from_ptr(proj_ptr) }
            .to_str()
            .unwrap()
            .to_owned();

        let rendered_ptr = unsafe {
            agent_doc_lossless_tree_render(CString::new(proj_json.clone()).unwrap().as_ptr())
        };
        assert!(!rendered_ptr.is_null(), "render returned null");
        let rendered = unsafe { CStr::from_ptr(rendered_ptr) }
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            rendered, doc,
            "FFI project→render must reconstruct the document"
        );

        let proj_c = CString::new(proj_json).unwrap();
        let cur =
            unsafe { agent_doc_lossless_tree_projection_current(proj_c.as_ptr(), doc_c.as_ptr()) };
        assert_eq!(cur, 1, "projection should be current for its own document");

        let moved =
            CString::new("<!-- agent:status -->\nedited\n<!-- /agent:status -->\n").unwrap();
        let stale =
            unsafe { agent_doc_lossless_tree_projection_current(proj_c.as_ptr(), moved.as_ptr()) };
        assert_eq!(stale, 0, "stale projection must not claim to be current");

        // Free the heap strings via the same allocator path plugins use.
        unsafe {
            drop(CString::from_raw(proj_ptr));
            drop(CString::from_raw(rendered_ptr));
        }
    }

    #[test]
    fn ffi_capability_token_matches_the_negotiation_constant() {
        let token = unsafe { CStr::from_ptr(agent_doc_lossless_tree_capability()) }
            .to_str()
            .unwrap();
        assert_eq!(token, agent_doc_debounce::LOSSLESS_TREE_CRDT_CAPABILITY);
    }

    #[test]
    fn ffi_render_rejects_garbage_json() {
        let bad = CString::new("not json").unwrap();
        assert!(unsafe { agent_doc_lossless_tree_render(bad.as_ptr()) }.is_null());
        assert_eq!(
            unsafe { agent_doc_lossless_tree_projection_current(bad.as_ptr(), bad.as_ptr()) },
            0
        );
    }
}

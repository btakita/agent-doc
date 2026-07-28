//! Lossless-tree frame exchange for editor plugins.

use std::ffi::{CStr, CString, c_char};

/// The capability token a plugin advertises to speak lossless-tree frames.
/// Returns a borrowed static C string; callers must not free it.
#[unsafe(no_mangle)]
pub extern "C" fn agent_doc_lossless_tree_capability() -> *const c_char {
    debug_assert_eq!(
        agent_doc_document_realtime::editor_contract::LOSSLESS_TREE_CRDT_CAPABILITY,
        "lossless_tree_crdt_v1"
    );
    c"lossless_tree_crdt_v1".as_ptr()
}

/// Project document text into a lossless-tree JSON projection.
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
    CString::new(bytes)
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

/// Render a lossless-tree JSON projection back to document text.
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
    CString::new(agent_doc_merge::lossless_tree::restore(&projection))
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

/// Return `1` when the projection still describes `visible_text`, otherwise `0`.
///
/// # Safety
///
/// Both pointers must reference valid, NUL-terminated UTF-8 strings.
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

    #[test]
    fn project_render_round_trip_and_staleness() {
        let doc = "<!-- agent:status -->\nffi round trip\n<!-- /agent:status -->\n";
        let doc_c = CString::new(doc).unwrap();
        let projection_ptr = unsafe { agent_doc_lossless_tree_project(doc_c.as_ptr()) };
        assert!(!projection_ptr.is_null());
        let projection_json = unsafe { CStr::from_ptr(projection_ptr) }
            .to_str()
            .unwrap()
            .to_owned();

        let projection_c = CString::new(projection_json).unwrap();
        let rendered_ptr = unsafe { agent_doc_lossless_tree_render(projection_c.as_ptr()) };
        assert!(!rendered_ptr.is_null());
        assert_eq!(
            unsafe { CStr::from_ptr(rendered_ptr) }.to_str().unwrap(),
            doc
        );
        assert_eq!(
            unsafe {
                agent_doc_lossless_tree_projection_current(projection_c.as_ptr(), doc_c.as_ptr())
            },
            1
        );

        let moved =
            CString::new("<!-- agent:status -->\nedited\n<!-- /agent:status -->\n").unwrap();
        assert_eq!(
            unsafe {
                agent_doc_lossless_tree_projection_current(projection_c.as_ptr(), moved.as_ptr())
            },
            0
        );
        unsafe {
            drop(CString::from_raw(projection_ptr));
            drop(CString::from_raw(rendered_ptr));
        }
    }

    #[test]
    fn capability_matches_editor_contract() {
        let token = unsafe { CStr::from_ptr(agent_doc_lossless_tree_capability()) }
            .to_str()
            .unwrap();
        assert_eq!(
            token,
            agent_doc_document_realtime::editor_contract::LOSSLESS_TREE_CRDT_CAPABILITY
        );
    }

    #[test]
    fn render_rejects_garbage_json() {
        let bad = CString::new("not json").unwrap();
        assert!(unsafe { agent_doc_lossless_tree_render(bad.as_ptr()) }.is_null());
    }
}

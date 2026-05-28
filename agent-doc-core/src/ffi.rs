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

use std::ffi::CString;
use std::os::raw::c_char;

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

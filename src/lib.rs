//! agent-doc library — shared components for CLI, FFI, and editor plugins.
//!
//! Re-exports the core document manipulation modules that are shared between
//! the CLI binary and native plugin bindings (JNI, napi-rs).

pub mod component;
pub mod crdt;
pub mod ffi;
pub mod frontmatter;
pub mod merge;
pub mod template;

/// Default number of hex characters for boundary IDs.
pub const BOUNDARY_ID_LEN: usize = 8;

/// Generate a new boundary ID (short hex string from UUID v4).
pub fn new_boundary_id() -> String {
    let full = uuid::Uuid::new_v4().to_string().replace('-', "");
    full[..BOUNDARY_ID_LEN.min(full.len())].to_string()
}

/// Format a boundary marker comment.
pub fn format_boundary_marker(id: &str) -> String {
    format!("<!-- agent:boundary:{} -->", id)
}

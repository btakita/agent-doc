//! agent-doc library — shared components for CLI, FFI, and editor plugins.
//!
//! Re-exports the core document manipulation modules that are shared between
//! the CLI binary and native plugin bindings (JNI, napi-rs).

pub mod component;
pub mod crdt;
pub mod debounce;
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

/// Generate a boundary ID with an optional summary suffix.
///
/// Format: `a0cfeb34` or `a0cfeb34:boundary-fix` (with summary).
/// The summary is slugified (lowercase, non-alphanumeric → `-`, max 20 chars).
pub fn new_boundary_id_with_summary(summary: Option<&str>) -> String {
    let id = new_boundary_id();
    match summary {
        Some(s) if !s.is_empty() => {
            let slug: String = s.to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .take(3) // max 3 words
                .collect::<Vec<&str>>()
                .join("-");
            let slug = &slug[..slug.len().min(20)];
            format!("{}:{}", id, slug)
        }
        _ => id,
    }
}

/// Format a boundary marker comment.
pub fn format_boundary_marker(id: &str) -> String {
    format!("<!-- agent:boundary:{} -->", id)
}

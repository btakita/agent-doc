//! # Module: lib (agent_doc)
//!
//! ## Spec
//! - Exposes the public API surface consumed by the CLI binary, FFI layer, and editor plugins.
//! - Re-exports: `component`, `crdt`, `debounce`, `ffi`, `frontmatter`, `merge`, `project_config`, `template`.
//! - `component::strip_comments(content)` is the shared entry point for comment stripping,
//!   usable by both the binary (`diff::compute`) and external crates (`eval-runner`).
//! - Provides two boundary-ID utilities used across all write paths:
//!   - `new_boundary_id()` — 8-hex-char UUID v4 prefix (length controlled by `BOUNDARY_ID_LEN`).
//!   - `new_boundary_id_with_summary(summary)` — same ID optionally suffixed with a 3-word,
//!     20-char-max slug derived from `summary` (format: `a0cfeb34:boundary-fix`).
//! - `format_boundary_marker(id)` — renders the canonical HTML comment form
//!   `<!-- agent:boundary:<id> -->` used in document component boundaries.
//! - `BOUNDARY_ID_LEN = 8` is a public constant; callers may read but should not override.
//!
//! ## Agentic Contracts
//! - All public symbols are safe to call from FFI consumers (JNA, napi-rs) via `ffi` module.
//! - `new_boundary_id` is non-deterministic (UUID v4); callers must not rely on ordering.
//! - `new_boundary_id_with_summary(None)` and `new_boundary_id_with_summary(Some(""))` both
//!   return a plain 8-char ID with no suffix.
//! - Slug derivation: lowercase, non-alphanumeric chars → `-`, collapse runs, take first 3 words,
//!   truncate to 20 chars.
//!
//! ## Evals
//! - new_boundary_id_len: result is exactly `BOUNDARY_ID_LEN` chars of hex
//! - new_boundary_id_with_summary_none: `None` summary → plain 8-char hex
//! - new_boundary_id_with_summary_empty: `Some("")` → plain 8-char hex
//! - new_boundary_id_with_summary_slug: `Some("Boundary Fix")` → `"<id>:boundary-fix"`
//! - new_boundary_id_with_summary_truncate: long summary → slug capped at 20 chars
//! - format_boundary_marker: `"abc123"` → `"<!-- agent:boundary:abc123 -->"`

pub mod component;
pub mod crdt;
pub mod debounce;
pub mod ffi;
pub mod frontmatter;
pub mod ipc_socket;
pub mod merge;
pub mod model_tier;
pub mod project_config;
pub mod secret_redact;
pub mod security;
pub mod syntax;
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
            let slug: String = s
                .to_lowercase()
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

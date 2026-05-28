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
pub mod template_io;

// Boundary ID helpers moved to `agent_doc_core::id` (Wave 1 of #adcr extraction).
// Re-exported here so existing `crate::new_boundary_id` etc. call sites in
// component.rs, template.rs, boundary.rs, etc. keep compiling unchanged.
pub use agent_doc_core::id::{
    BOUNDARY_ID_LEN, format_boundary_marker, new_boundary_id, new_boundary_id_with_summary,
};

//! Frontmatter parsing — pure half in `agent_doc_core::frontmatter` (Wave 2
//! of #adcr); `&Path` / `std::fs` wrappers (parse_for_file,
//! ensure_session_for_file, read_session_id) live in
//! [`agent_doc_orchestration::frontmatter_io`] (Wave 5 / `#0c4e`).
//!
//! Thin re-export shim. All `crate::frontmatter::*` call sites continue to resolve.

pub use agent_doc_core::frontmatter::*;
pub use agent_doc_orchestration::frontmatter_io::{ensure_session_for_file, parse_for_file, read_session_id};

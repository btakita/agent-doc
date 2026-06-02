//! Frontmatter shim — pure half from `agent_doc_core::frontmatter`, `&Path` /
//! `std::fs` wrappers from `crate::frontmatter_io` (now in orchestration).
//! Mirrors the main crate's shim so moved modules resolve unchanged.

pub use crate::frontmatter_io::{
    ensure_session_for_file, ensure_session_for_file_with_context, is_agent_doc_document_for_file,
    parse_for_file, parse_for_file_with_context, read_session_id, require_agent_doc_document,
};
pub use agent_doc_core::frontmatter::*;

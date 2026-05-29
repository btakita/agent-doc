//! Frontmatter shim — pure half from `agent_doc_core::frontmatter`, `&Path` /
//! `std::fs` wrappers from `crate::frontmatter_io` (now in orchestration).
//! Mirrors the main crate's shim so moved modules resolve unchanged.

pub use agent_doc_core::frontmatter::*;
pub use crate::frontmatter_io::{ensure_session_for_file, parse_for_file, read_session_id};

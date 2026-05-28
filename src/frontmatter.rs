//! Frontmatter parsing — moved to `agent_doc_core::frontmatter` (Wave 2 of #adcr).
//!
//! Thin re-export shim. All `crate::frontmatter::*` call sites continue to resolve.

pub use agent_doc_core::frontmatter::*;

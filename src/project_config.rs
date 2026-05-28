//! Project config — moved to `agent_doc_core::project_config` (Wave 2 of #adcr).
//!
//! Thin re-export shim. All `crate::project_config::*` call sites continue to resolve.

pub use agent_doc_core::project_config::*;

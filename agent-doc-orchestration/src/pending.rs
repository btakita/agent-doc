//! Pending/backlog operations — moved to `agent_doc_core::pending` (Wave 1 of #adcr).
//!
//! Thin re-export shim. All `crate::pending::*` call sites in write.rs,
//! preflight.rs, session_check.rs, etc. continue to resolve.

pub use agent_doc_core::pending::*;

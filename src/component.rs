//! Component parser — moved to `agent_doc_core::component` (Wave 1 of #adcr).
//!
//! Thin re-export so `crate::component::*` call sites in preflight.rs,
//! template.rs, write.rs, session_check.rs, etc. continue to resolve.

pub use agent_doc_core::component::*;

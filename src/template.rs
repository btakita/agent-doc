//! Template-mode support — moved to `agent_doc_core::template` (Wave 3 of #adcr).
//!
//! Thin re-export so `crate::template::*` call sites in run.rs, stream.rs,
//! write.rs, preflight.rs, repair.rs, etc. continue to resolve.

pub use agent_doc_core::template::*;

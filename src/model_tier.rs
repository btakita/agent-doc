//! Model tier resolution — moved to `agent_doc_core::model_tier` (Wave 1 of #adcr).
//!
//! Thin re-export shim. Note: `detect_harness()` reads `AGENT_DOC_HARNESS` env
//! and is env-coupled rather than pure data — it lives in the core crate for
//! now since it does not touch the filesystem, git, or tmux. A future wave may
//! lift it into a small main-crate wrapper around a pure
//! `Tier::from_harness_str` function.

pub use agent_doc_core::model_tier::*;

//! Compatibility re-export for agent backend process adapters.
//!
//! The concrete backend implementations live in `agent-doc-agent-io`; this
//! module preserves the existing `agent_doc_orchestration::agent::*` path while
//! callers migrate to the focused crate.

pub use agent_doc_agent_io::agent::*;

//! Compatibility re-export for session-liveness tmux predicates.
//!
//! The concrete implementation lives in `agent-doc-supervisor-process`; this keeps the
//! existing `agent_doc_orchestration::session_liveness::*` path available while
//! call sites migrate.

pub use agent_doc_supervisor_process::session_liveness::*;

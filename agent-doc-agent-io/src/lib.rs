//! Agent backend process adapters for agent-doc.
//!
//! This crate owns spawning and parsing concrete agent CLI backends. Shared
//! launch vocabulary remains in `agent-doc-turn-executor`; orchestration only
//! chooses when to invoke these adapters.

pub mod agent;

pub use agent::*;

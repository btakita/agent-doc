//! Pure queue scheduling and drain policy for agent-doc.
//!
//! This crate owns queue-specific decisions shared by supervisors, turn
//! executors, and orchestration. It does not inspect panes, read documents,
//! submit commands, or mutate files.

pub mod queue;

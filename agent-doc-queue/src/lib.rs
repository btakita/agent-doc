//! Pure queue scheduling, syntax, and drain policy for agent-doc.
//!
//! This crate owns queue-specific decisions and queue-body transforms shared by
//! supervisors, turn executors, and orchestration. It does not inspect panes,
//! read documents, submit commands, or mutate files.

pub mod document_queue;
pub mod queue;
pub mod queue_command;
pub mod queue_edit_owner;
pub mod queue_preemption;

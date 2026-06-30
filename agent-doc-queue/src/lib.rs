//! Pure queue scheduling, syntax, and drain policy for agent-doc.
//!
//! This crate owns queue-specific decisions and queue-body transforms shared by
//! supervisors, turn executors, and orchestration. It does not inspect panes,
//! read documents, submit commands, or mutate files.

pub mod backlog_sync;
pub mod component_attrs;
pub mod document_queue;
pub mod drain_owner;
pub mod queue;
pub mod queue_closeout_guard;
pub mod queue_command;
pub mod queue_consume;
pub mod queue_continuation;
pub mod queue_convergence;
pub mod queue_directive;
pub mod queue_edit_owner;
pub mod queue_heads;
pub mod queue_journal;
pub mod queue_preemption;
pub mod queue_response;
pub mod route_dispatch;

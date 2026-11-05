//! Pure queue scheduling, syntax, and drain policy for agent-doc.
//!
//! This crate owns queue-specific decisions and queue-body transforms shared by
//! supervisors, turn executors, and orchestration. It does not inspect panes,
//! read documents, submit commands, or mutate files.

pub mod ack_stays_open;
pub mod answer_ack;
pub mod backlog_sync;
pub mod capture_scope;
pub mod claim_release_receipt;
pub mod component_attrs;
pub mod control_binding;
pub mod coordinator_drain;
pub mod dispatch_item;
pub mod document_queue;
pub mod free_text_admission;
pub mod goal_preset_terminal;
pub mod idle_drain;
pub mod nested_claim_steer;
pub mod no_changes_explanation;
pub mod operator_request_packet;
pub mod prompt_component_attrs;
pub mod prompt_identity;
pub mod queue;
pub mod queue_auto_clear;
pub mod queue_blocked;
pub mod queue_claim;
pub use agent_doc_element_queue::QueueReportState;
pub mod queue_closeout_guard;
pub mod queue_command;
pub mod queue_consume;
pub mod queue_continuation;
pub mod queue_convergence;
pub mod queue_directive;
pub mod queue_graph;
pub mod queue_head_alignment;
pub mod queue_head_annotation;
pub mod queue_head_entry;
pub mod queue_heads;
pub mod queue_inbound;
pub mod queue_mirror_projection;
pub mod queue_preemption;
pub mod queue_projection;
pub mod queue_prompt_drift;
pub mod queue_recency;
pub mod queue_repair_generation;
pub mod queue_replay;
pub mod queue_response;
pub mod queue_schedule;
pub mod queue_schedule_runtime;
pub mod queue_set;
pub mod queue_tree;
pub mod reported_closeout;
pub mod tree_status;
pub mod queue_sync_payload;
pub mod route_dispatch;
pub mod stale_completion_evidence;
pub mod subagent_brief;
pub mod subagent_intent;
pub mod waiting_archive;
pub mod waiting_packet;
pub mod waiting_status;
pub mod waiting_subtree;
#[cfg(test)]
mod stray_close_text_tests;
#[cfg(test)]
mod waiting_fixture_tests;
#[cfg(test)]
mod waiting_done_tests;

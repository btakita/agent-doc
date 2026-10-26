//! Queue-related marker storage I/O for agent-doc.

pub mod claim_release_receipt;
pub mod continuation_detect;
pub mod continuation_marker;
pub mod continuation_request;
pub mod controller_pause;
pub mod drain_owner;
pub mod one_shot_sync;
pub mod queue_add;
pub mod queue_attr;
pub mod queue_auto_clear;
pub mod queue_claim;
pub mod queue_cmd;
pub mod queue_consume;
pub mod queue_consumption_proof;
pub mod queue_continuation;
pub mod queue_edit_owner;
pub mod queue_lineage;
pub mod queue_schedule;
pub mod queue_tombstone;
pub mod stale_completion_repair;
pub mod subagent_dispatch;
pub mod write_queue;

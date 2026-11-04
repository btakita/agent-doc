//! Queue-related marker storage I/O for agent-doc.

pub mod active_lifecycle;
pub mod claim_release_receipt;
pub mod queue_marker_audit;
pub mod continuation_detect;
pub mod continuation_marker;
pub mod continuation_request;
pub mod reported_writeback_reminder;
pub mod controller_pause;
pub mod drain_owner;
pub mod goal_loop_handoff;
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
pub mod sync_payload;
pub mod write_queue;

/// `#goalpresetterminal`: install `file`'s effective prompt presets (document
/// frontmatter plus `.agent-doc/project.md` inheritance) for the pure
/// strike/consumption/residue predicates on this thread, so a goal-like preset
/// declared only in the project config still gates closeout on its declared
/// final states. Hold the returned guard across the decision.
pub fn goal_terminal_presets_guard(
    file: &std::path::Path,
    content: &str,
) -> agent_doc_queue::goal_preset_terminal::InheritedPresetsGuard {
    agent_doc_queue::goal_preset_terminal::InheritedPresetsGuard::install(
        &agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(content, file),
    )
}

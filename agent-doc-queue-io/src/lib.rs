//! Queue-related marker storage I/O for agent-doc.

pub mod active_lifecycle;
pub mod address_effect;
pub mod attempt_lineage_effect;
pub mod blocker_disregard;
pub mod build_slot;
pub mod claim_release_receipt;
pub mod codex_worker_completion;
pub mod completion_cell_restore;
pub mod completion_feedback_lock;
pub mod continuation_detect;
pub mod continuation_marker;
pub mod continuation_reconcile;
pub mod continuation_request;
pub mod controller_pause;
pub mod cross_harness_handoff;
pub mod drain_owner;
pub mod followup_owner_ledger;
pub mod followup_provenance_ledger;
pub mod goal_loop_handoff;
pub mod lifecycle_projection;
pub mod lost_running;
pub mod nested_reply_followup;
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
pub mod queue_maintenance_claim;
pub mod queue_marker_audit;
pub mod queue_row_edit;
pub mod queue_schedule;
pub mod queue_tombstone;
pub mod reported_closeout_exec;
pub mod reported_writeback_reminder;
pub mod restart_journal;
pub mod self_repair;
pub mod stale_completion_repair;
pub mod state_read_scope;
pub mod subagent_dispatch;
pub mod subagent_pool;
pub mod sync_payload;
pub mod transient_closeout;
pub mod transient_closeout_refusal;
pub mod worker_quota_autodetect;
pub mod worktree_inventory;
pub mod write_queue;

/// `#goalpresetterminal`: install `file`'s effective prompt presets (document
/// frontmatter plus `.agent-doc/project.md` inheritance) for the pure
/// strike/consumption/residue predicates on this thread, so a goal-like preset
/// declared only in the project config still gates closeout on its declared
/// final states. Hold the returned guard across the decision.
/// Preset map plus, when GitHub observation is enabled, the address-effect
/// reads for queue rows in `content`. Dropping the guard restores both.
pub struct CloseoutDecisionGuard {
    _presets: agent_doc_queue::goal_preset_terminal::InheritedPresetsGuard,
    _effect: Option<agent_doc_queue::address_completion_proof::AddressEffectGuard>,
}

pub fn goal_terminal_presets_guard(file: &std::path::Path, content: &str) -> CloseoutDecisionGuard {
    let presets = agent_doc_queue::goal_preset_terminal::InheritedPresetsGuard::install(
        &agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(content, file),
    );
    let effect = address_effect::install_for_content(file, content);
    CloseoutDecisionGuard {
        _presets: presets,
        _effect: effect,
    }
}

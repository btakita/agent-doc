//! # agent-doc-orchestration
//!
//! The orchestration layer of agent-doc: routing, git, sessions, IPC,
//! process supervision, and tmux sync. Sits between the CLI shell
//! (`agent-doc`) and the focused domain crates.
//!
//! Extraction tracked under `#adoc-orchestration-crate` / `#bz6s`. See
//! `tasks/agent-doc/plan-agent-doc-orchestration-extraction.md` for the wave
//! plan.
//!
//! Wave 0 (scaffold) + Wave 1a: `ipc_socket` (the one dependency-free leaf).
//! Direction A, increment 2: `secret_redact`; environment expansion now lives
//! in `agent-doc-config`.
//! Direction A, increment 3: global config now lives in `agent-doc-config`.
//! Project config file I/O lives in `agent-doc-project-config-io`.
//! Direction A, increment 4: `ops_log` — best-effort operational logging.
//! Pulled project-root discovery and optional file reads into `agent-doc-fs`
//! so effectful adapters no longer reach through `snapshot`.
//! Direction A, increment 5: `input_diag` — structured tmux/supervisor input
//! diagnostic emission. Pure formatting/hash/gating policy now lives in
//! `agent-doc-tmux-commands`; orchestration keeps only stderr/ops-log adapters.
//! Direction A, increment 6 (big-bang): the entire entangled cluster +
//! sessions/supervisor + neighbors moved in one migration. Orchestration now
//! depends on focused crates directly for extracted document, merge, turn, and
//! realtime policy.
//!
//! The next boundary is to retire this crate as an authority holder. Pure
//! document projection lives in `agent-doc-document`, document authority
//! scheduling should move into `agent-doc-document-realtime`, turn lifecycle
//! state lives in `agent-doc-turn`, shared turn-executor vocabulary in
//! `agent-doc-turn-executor`, shared tmux facts/effects in the
//! `agent-doc-tmux` crate family, and tmux-to-turn readiness in
//! `agent-doc-turn-executor-tmux`. This crate remains a transitional adapter
//! for harness, git, editor, and remaining command ports while those ports are
//! split into narrower crates.

// The orchestration cluster + sessions/supervisor + neighbors (increment 6).
pub mod codex_hook;
pub mod preflight;
pub mod repair;
pub mod route;
pub mod start;
pub mod write;

pub(crate) struct BacklogCommandEffects;

pub(crate) static BACKLOG_COMMAND_EFFECTS: BacklogCommandEffects = BacklogCommandEffects;

impl agent_doc_element_backlog_io::BacklogCommandEffects for BacklogCommandEffects {
    fn converge_or_disk_write(
        &self,
        file: &std::path::Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            current_content,
            target_content,
            reason,
        )
    }

    fn record_document_write_provenance(&self, file: &std::path::Path, content: &str) {
        agent_doc_document_realtime_io::record_document_write_provenance(file, content);
    }
}

pub use agent_doc_document_realtime_io::{
    RUNTIME_PIPELINE_FRONTMATTER_EFFECTS as PIPELINE_FRONTMATTER_EFFECTS,
    RuntimePipelineFrontmatterEffects as PipelineFrontmatterEffects,
};

pub struct OrchestrationRepairIoEffects;

pub static REPAIR_IO_EFFECTS: OrchestrationRepairIoEffects = OrchestrationRepairIoEffects;

impl agent_doc_repair_io::RepairIoEffects for OrchestrationRepairIoEffects {
    fn atomic_write(&self, file: &std::path::Path, content: &str) -> anyhow::Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn mark_committed_frontmatter(
        &self,
        file: &std::path::Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> anyhow::Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }

    fn mark_abandoned_frontmatter(
        &self,
        file: &std::path::Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> anyhow::Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }

    fn apply_closeout_recovery_mutation(
        &self,
        file: &std::path::Path,
        mutation: agent_doc_flow_io::closeout::CloseoutRecoveryMutation<'_>,
    ) -> anyhow::Result<()> {
        agent_doc_flow_io::closeout::apply_closeout_recovery_mutation(
            file,
            mutation,
            &crate::closeout_effects(),
        )
    }
}

impl agent_doc_repair_io::RepairTemplateWriteEffects for OrchestrationRepairIoEffects {
    fn atomic_write(&self, file: &std::path::Path, content: &str) -> anyhow::Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn repair_response_prompt_order_for_file(
        &self,
        content: &str,
        known_response: Option<&str>,
        file: &std::path::Path,
        fallback_snapshot: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        agent_doc_template_io::repair_response_prompt_order_for_file(
            content,
            known_response,
            file,
            fallback_snapshot,
        )
    }

    fn normalize_template_structure_or_fail_preserving(
        &self,
        content: &str,
        file: &std::path::Path,
        prompt_input: Option<&str>,
    ) -> anyhow::Result<String> {
        agent_doc_template_io::normalize_template_structure_or_fail_preserving(
            content,
            file,
            prompt_input,
        )
    }
}

pub struct OrchestrationRepairReplayWriteEffects;

pub static REPAIR_REPLAY_WRITE_EFFECTS: OrchestrationRepairReplayWriteEffects =
    OrchestrationRepairReplayWriteEffects;

pub(crate) fn repair_coordinator_effects() -> agent_doc_repair_io::RepairCoordinatorEffects<
    'static,
    OrchestrationRepairIoEffects,
    OrchestrationRepairReplayWriteEffects,
> {
    agent_doc_repair_io::RepairCoordinatorEffects {
        repair_io_effects: &REPAIR_IO_EFFECTS,
        replay_write_effects: &REPAIR_REPLAY_WRITE_EFFECTS,
        complete_required_closeout: repair_complete_required_closeout,
        inspect_session: repair_inspect_session,
        recover_missing_committed_head_response:
            agent_doc_repair_runtime_io::recover_missing_committed_head_response,
        recover_dedupe_only_drift: agent_doc_repair_runtime_io::recover_dedupe_only_drift,
    }
}

fn repair_complete_required_closeout(file: &std::path::Path) -> anyhow::Result<bool> {
    agent_doc_flow_io::closeout::complete_required_closeout(file, &crate::closeout_effects())
}

fn repair_inspect_session(
    file: &std::path::Path,
) -> anyhow::Result<agent_doc_session_check_io::SessionCheckStatus> {
    agent_doc_session_check_io::inspect(file, &crate::session_check_effects())
}

impl agent_doc_repair_io::RepairReplayWriteEffects for OrchestrationRepairReplayWriteEffects {
    fn run_strict_write_replay(
        &self,
        file: &std::path::Path,
        response: &str,
        is_template: bool,
        is_stream: bool,
        force_disk: bool,
        queue_completion_ids: &[String],
    ) -> anyhow::Result<()> {
        let commit_mode = if agent_doc_git_io::status::is_in_git_repo(file) {
            agent_doc_write_command_io::CommitMode::Required
        } else {
            agent_doc_write_command_io::CommitMode::None
        };
        crate::write::run_command_with_response(
            agent_doc_write_command_io::CommandOptions {
                file: file.to_path_buf(),
                baseline_file: None,
                is_template,
                is_stream,
                is_ipc: false,
                force_disk,
                origin: Some("repair_replay".to_string()),
                pending_add: Vec::new(),
                pending_add_to: Vec::new(),
                pending_add_gated: Vec::new(),
                pending_add_after: Vec::new(),
                pending_add_before: Vec::new(),
                pending_add_back: Vec::new(),
                icebox_add: Vec::new(),
                icebox_add_after: Vec::new(),
                icebox_add_before: Vec::new(),
                icebox_add_back: Vec::new(),
                icebox_edit: Vec::new(),
                icebox_clear: false,
                icebox_reorder: None,
                pending_done: Vec::new(),
                pending_edit: Vec::new(),
                pending_clear: false,
                pending_reorder: None,
                pending_gate: Vec::new(),
                pending_ungate: Vec::new(),
                pending_resolve_gate: Vec::new(),
                pending_set_gate_type: Vec::new(),
                pending_set_verify: Vec::new(),
                review_add: Vec::new(),
                review_edit: Vec::new(),
                review_remove: Vec::new(),
                review_resolve: Vec::new(),
                queue_completion_ids: queue_completion_ids.to_vec(),
                allow_replace_pending: false,
                pending_only: false,
                status: None,
                lint_override: None,
                commit_sibling: Vec::new(),
                commit_sibling_message: Vec::new(),
            },
            commit_mode,
            response.to_string(),
        )
    }

    fn apply_template_from_string(
        &self,
        file: &std::path::Path,
        response: &str,
        force_disk: bool,
    ) -> anyhow::Result<()> {
        crate::write::apply_template_from_string_with_options(
            file,
            response,
            crate::write::TemplateApplyOptions { force_disk },
        )
    }

    fn apply_append_from_string(
        &self,
        file: &std::path::Path,
        response: &str,
    ) -> anyhow::Result<()> {
        crate::write::apply_append_from_string(file, response)
    }

    fn strike_recovered_free_text_queue_head(&self, file: &std::path::Path) -> anyhow::Result<()> {
        match agent_doc_queue_io::queue_consume::consume_queue_prompt_force_disk(
            file,
            &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
        ) {
            Ok(Some(outcome)) => {
                eprintln!(
                    "[repair] struck consumed free-text queue head (remaining: {})",
                    outcome.remaining
                );
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(err) => Err(err),
        }
    }
}

pub struct OrchestrationSessionCheckEffects;

pub fn session_check_effects() -> OrchestrationSessionCheckEffects {
    OrchestrationSessionCheckEffects
}

impl agent_doc_session_check_io::SessionCheckEffects for OrchestrationSessionCheckEffects {
    fn closeout_recovery_hint(&self, file: &std::path::Path) -> String {
        closeout_recovery_hint(file)
    }

    fn atomic_write(&self, file: &std::path::Path, content: &str) -> anyhow::Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn repair_committed_historical_snapshot_drift(
        &self,
        file: &std::path::Path,
    ) -> anyhow::Result<Option<&'static str>> {
        agent_doc_repair_io::repair_committed_historical_snapshot_drift(file)
    }

    fn recover_missing_commit_boundary(
        &self,
        file: &std::path::Path,
        event: &str,
    ) -> anyhow::Result<Option<&'static str>> {
        agent_doc_repair_io::recover_missing_commit_boundary(&crate::REPAIR_IO_EFFECTS, file, event)
    }
}

pub struct OrchestrationDirectRunEffects;

pub static DIRECT_RUN_EFFECTS: OrchestrationDirectRunEffects = OrchestrationDirectRunEffects;

impl agent_doc_run_io::DirectRunEffects for OrchestrationDirectRunEffects {
    fn guard_no_exchange_compaction_request_for_diff(
        &self,
        file: &std::path::Path,
        diff_text: &str,
    ) -> anyhow::Result<()> {
        agent_doc_run_io::guard_no_exchange_compaction_request_for_diff(file, diff_text)
    }

    fn commit(&self, file: &std::path::Path) -> anyhow::Result<bool> {
        agent_doc_commit_io::commit(file)
    }

    fn normalize_template_structure_or_fail(
        &self,
        content: &str,
        file: &std::path::Path,
    ) -> anyhow::Result<String> {
        agent_doc_template_io::normalize_template_structure_or_fail(content, file)
    }

    fn atomic_write(&self, file: &std::path::Path, content: &str) -> anyhow::Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn consume_queue_prompts_for_done_ids_with_outcome(
        &self,
        file: &std::path::Path,
        done_ids: &[String],
        force_disk: bool,
    ) -> anyhow::Result<Option<agent_doc_queue_io::queue_consume::QueueConsumptionOutcome>> {
        if force_disk {
            agent_doc_queue_io::queue_consume::consume_queue_prompts_with_outcome(
                file,
                done_ids,
                true,
                &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
            )
        } else {
            agent_doc_queue_io::queue_consume::consume_queue_prompts_for_done_ids_with_outcome(
                file,
                done_ids,
                &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
            )
        }
    }

    fn complete_required_closeout(&self, file: &std::path::Path) -> anyhow::Result<()> {
        agent_doc_flow_io::closeout::complete_required_closeout(file, &crate::closeout_effects())
            .map(|_| ())
    }

    fn abandon_recursive_cycle(
        &self,
        file: &std::path::Path,
        event: &str,
        diagnostic: &str,
    ) -> anyhow::Result<()> {
        agent_doc_run_io::abandon_run_recursive_cycle(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            diagnostic,
        )
    }
}

pub struct OrchestrationCloseoutEffects;

pub fn closeout_effects() -> OrchestrationCloseoutEffects {
    OrchestrationCloseoutEffects
}

impl agent_doc_flow_io::closeout::CloseoutEffects for OrchestrationCloseoutEffects {
    fn commit(&self, file: &std::path::Path) -> anyhow::Result<bool> {
        agent_doc_commit_io::commit(file)
    }

    fn run_pending_maintenance(
        &self,
        file: &std::path::Path,
        force_disk: bool,
    ) -> anyhow::Result<agent_doc_preflight_io::PendingMaintenanceReport> {
        if force_disk {
            agent_doc_preflight_io::run_pending_maintenance_force_disk(
                file,
                &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
            )
        } else {
            agent_doc_preflight_io::run_pending_maintenance(
                file,
                &agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
            )
        }
    }

    fn enforce_clean_closeout(&self, file: &std::path::Path) -> anyhow::Result<()> {
        agent_doc_session_check_io::enforce_clean_closeout(file, &crate::session_check_effects())
    }

    fn cancel_preflight_cycle(&self, file: &std::path::Path) -> anyhow::Result<()> {
        agent_doc_repair_io::cancel_preflight_cycle(&crate::REPAIR_IO_EFFECTS, file).map(|_| ())
    }

    fn detect_jb_cache_conflict_cancel_recoverable(
        &self,
        file: &std::path::Path,
    ) -> anyhow::Result<bool> {
        agent_doc_session_check_io::detect_jb_cache_conflict_cancel_recoverable(file)
    }

    fn detect_bypassed_response_write(
        &self,
        file: &std::path::Path,
    ) -> anyhow::Result<Option<String>> {
        agent_doc_session_check_io::detect_bypassed_response_write(file)
    }

    fn mark_committed_frontmatter(
        &self,
        file: &std::path::Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> anyhow::Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }
}

pub fn closeout_recovery_hint(file: &std::path::Path) -> String {
    let state = agent_doc_flow_io::closeout::classify_closeout_recovery_state_for_file(
        file,
        &crate::closeout_effects(),
    );
    match agent_doc_flow_io::closeout::closeout_recovery_command_for_file(file, state) {
        Some(command) => format!("Recovery [{}]: {}.", state.as_str(), command),
        None => format!(
            "Use `agent-doc write --commit {}` once the visible response body is final, then re-run `agent-doc session-check {}`.",
            file.display(),
            file.display()
        ),
    }
}

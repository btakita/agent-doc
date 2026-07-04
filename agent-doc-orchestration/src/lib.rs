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
pub mod git;
pub mod preflight;
pub mod repair;
pub mod route;
#[cfg(test)]
mod run;
pub mod start;
pub mod write;

#[cfg(test)]
mod session_check;

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
            &crate::write::WRITE_CONVERGENCE_EFFECTS,
            file,
            current_content,
            target_content,
            reason,
        )
    }

    fn record_document_write_provenance(&self, file: &std::path::Path, content: &str) {
        crate::write::record_document_write_provenance(file, content);
    }
}

pub(crate) struct PipelineFrontmatterEffects;

pub(crate) const PIPELINE_FRONTMATTER_EFFECTS: PipelineFrontmatterEffects =
    PipelineFrontmatterEffects;

impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects
    for PipelineFrontmatterEffects
{
    fn converge_or_disk_write(
        &self,
        file: &std::path::Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &crate::write::WRITE_CONVERGENCE_EFFECTS,
            file,
            current_content,
            target_content,
            reason,
        )
    }

    fn log_op(&self, file: &std::path::Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

pub struct OrchestrationRepairIoEffects;

pub static REPAIR_IO_EFFECTS: OrchestrationRepairIoEffects = OrchestrationRepairIoEffects;

impl agent_doc_repair_io::RepairIoEffects for OrchestrationRepairIoEffects {
    fn atomic_write(&self, file: &std::path::Path, content: &str) -> anyhow::Result<()> {
        crate::write::atomic_write_pub(file, content)
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

pub(crate) struct SessionActorWriteQueueSubmitter;

pub(crate) static SESSION_ACTOR_WRITE_QUEUE: SessionActorWriteQueueSubmitter =
    SessionActorWriteQueueSubmitter;

impl agent_doc_queue_io::write_queue::DocumentWriteQueueSubmitter
    for SessionActorWriteQueueSubmitter
{
    fn submit<R, F>(
        &self,
        base_dir: &std::path::Path,
        file: &str,
        kind: agent_doc_document_realtime::session_ops::SessionOpKind,
        job: F,
    ) -> anyhow::Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        let actor = agent_doc_session_actor_io::document_actor_in(base_dir, file);
        actor.submit(kind, move |_ctx| job())
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
        crate::write::atomic_write_pub(file, content)
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
        crate::write::guard_no_exchange_compaction_request_for_diff(file, diff_text)
    }

    fn commit(&self, file: &std::path::Path) -> anyhow::Result<bool> {
        crate::git::commit(file)
    }

    fn normalize_template_structure_or_fail(
        &self,
        content: &str,
        file: &std::path::Path,
    ) -> anyhow::Result<String> {
        crate::write::normalize_template_structure_or_fail(content, file)
    }

    fn atomic_write(&self, file: &std::path::Path, content: &str) -> anyhow::Result<()> {
        crate::write::atomic_write_pub(file, content)
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
                &crate::write::QUEUE_CONSUME_WRITEBACK_EFFECTS,
            )
        } else {
            agent_doc_queue_io::queue_consume::consume_queue_prompts_for_done_ids_with_outcome(
                file,
                done_ids,
                &crate::write::QUEUE_CONSUME_WRITEBACK_EFFECTS,
            )
        }
    }

    fn complete_required_closeout(&self, file: &std::path::Path) -> anyhow::Result<()> {
        crate::write::complete_required_closeout(file).map(|_| ())
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
        crate::git::commit(file)
    }

    fn run_pending_maintenance(
        &self,
        file: &std::path::Path,
        force_disk: bool,
    ) -> anyhow::Result<agent_doc_preflight_io::PendingMaintenanceReport> {
        if force_disk {
            agent_doc_preflight_io::run_pending_maintenance_force_disk(
                file,
                &crate::preflight::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
            )
        } else {
            agent_doc_preflight_io::run_pending_maintenance(
                file,
                &crate::preflight::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
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

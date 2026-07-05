use std::path::Path;

use anyhow::Result;

pub use agent_doc_document_realtime_io::{
    RUNTIME_PIPELINE_FRONTMATTER_EFFECTS as PIPELINE_FRONTMATTER_EFFECTS,
    RuntimePipelineFrontmatterEffects as PipelineFrontmatterEffects,
};

pub struct RuntimeRepairIoEffects;

pub static REPAIR_IO_EFFECTS: RuntimeRepairIoEffects = RuntimeRepairIoEffects;

impl agent_doc_repair_io::RepairIoEffects for RuntimeRepairIoEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn mark_committed_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }

    fn mark_abandoned_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
            &PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }

    fn apply_closeout_recovery_mutation(
        &self,
        file: &Path,
        mutation: agent_doc_flow_io::closeout::CloseoutRecoveryMutation<'_>,
    ) -> Result<()> {
        agent_doc_flow_io::closeout::apply_closeout_recovery_mutation(
            file,
            mutation,
            &closeout_effects(),
        )
    }
}

impl agent_doc_repair_io::RepairTemplateWriteEffects for RuntimeRepairIoEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn repair_response_prompt_order_for_file(
        &self,
        content: &str,
        known_response: Option<&str>,
        file: &Path,
        fallback_snapshot: Option<&str>,
    ) -> Result<Option<String>> {
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
        file: &Path,
        prompt_input: Option<&str>,
    ) -> Result<String> {
        agent_doc_template_io::normalize_template_structure_or_fail_preserving(
            content,
            file,
            prompt_input,
        )
    }
}

pub struct RuntimeSessionCheckEffects;

pub fn session_check_effects() -> RuntimeSessionCheckEffects {
    RuntimeSessionCheckEffects
}

impl agent_doc_session_check_io::SessionCheckEffects for RuntimeSessionCheckEffects {
    fn closeout_recovery_hint(&self, file: &Path) -> String {
        closeout_recovery_hint(file)
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn repair_committed_historical_snapshot_drift(
        &self,
        file: &Path,
    ) -> Result<Option<&'static str>> {
        agent_doc_repair_io::repair_committed_historical_snapshot_drift(file)
    }

    fn recover_missing_commit_boundary(
        &self,
        file: &Path,
        event: &str,
    ) -> Result<Option<&'static str>> {
        agent_doc_repair_io::recover_missing_commit_boundary(&REPAIR_IO_EFFECTS, file, event)
    }
}

pub struct RuntimeCloseoutEffects;

pub fn closeout_effects() -> RuntimeCloseoutEffects {
    RuntimeCloseoutEffects
}

impl agent_doc_flow_io::closeout::CloseoutEffects for RuntimeCloseoutEffects {
    fn commit(&self, file: &Path) -> Result<bool> {
        agent_doc_commit_io::commit(file)
    }

    fn run_pending_maintenance(
        &self,
        file: &Path,
        force_disk: bool,
    ) -> Result<agent_doc_preflight_io::PendingMaintenanceReport> {
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

    fn enforce_clean_closeout(&self, file: &Path) -> Result<()> {
        agent_doc_session_check_io::enforce_clean_closeout(file, &session_check_effects())
    }

    fn cancel_preflight_cycle(&self, file: &Path) -> Result<()> {
        agent_doc_repair_io::cancel_preflight_cycle(&REPAIR_IO_EFFECTS, file).map(|_| ())
    }

    fn detect_jb_cache_conflict_cancel_recoverable(&self, file: &Path) -> Result<bool> {
        agent_doc_session_check_io::detect_jb_cache_conflict_cancel_recoverable(file)
    }

    fn detect_bypassed_response_write(&self, file: &Path) -> Result<Option<String>> {
        agent_doc_session_check_io::detect_bypassed_response_write(file)
    }

    fn resolve_current_document(
        &self,
        file: &Path,
        _source: &str,
    ) -> Result<agent_doc_document_realtime_io::CurrentDocument> {
        agent_doc_document_realtime_io::try_resolve_current_document(file)
    }

    fn resolve_current_document_for_authority(
        &self,
        file: &Path,
        source: &str,
        force_disk: bool,
    ) -> Result<agent_doc_document_realtime_io::CurrentDocument> {
        if force_disk {
            agent_doc_document_realtime_io::resolve_disk_current_document(file, source)
        } else {
            self.resolve_current_document(file, source)
        }
    }

    fn write_current_document(
        &self,
        doc: &agent_doc_document_realtime_io::CurrentDocument,
        content: &str,
        source: &str,
    ) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
            doc.key().as_path(),
            content,
            doc.content(),
            source,
        )
    }

    fn mark_committed_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )
    }
}

pub fn closeout_recovery_hint(file: &Path) -> String {
    let state = agent_doc_flow_io::closeout::classify_closeout_recovery_state_for_file(
        file,
        &closeout_effects(),
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

//! Runtime adapters for preflight maintenance writes.

use agent_doc_frontmatter::frontmatter;
use anyhow::Result;
use std::path::Path;

use agent_doc_template_io::normalize_user_prompts_in_exchange_safe;

pub struct RuntimePreflightMaintenanceWriteEffects;

pub static PREFLIGHT_MAINTENANCE_WRITE_EFFECTS: RuntimePreflightMaintenanceWriteEffects =
    RuntimePreflightMaintenanceWriteEffects;

impl agent_doc_preflight_io::PreflightMaintenanceWriteEffects
    for RuntimePreflightMaintenanceWriteEffects
{
    fn record_document_write_provenance(&self, file: &Path, content: &str) {
        agent_doc_document_realtime_io::record_document_write_provenance(file, content);
    }

    fn guard_visible_write_idle_and_current(
        &self,
        file: &Path,
        source: &str,
        expected_current: &str,
    ) -> Result<()> {
        agent_doc_document_realtime_io::guard_visible_write_idle_and_current(
            file,
            source,
            expected_current,
        )
    }

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        source: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            current_content,
            target_content,
            source,
        )
    }
}

pub fn relocate_out_of_exchange_prompt_before_diff(
    file: &Path,
    doc_content: &str,
) -> Result<Option<String>> {
    let (frontmatter, _) = frontmatter::parse(doc_content).map_err(|err| {
        anyhow::anyhow!(
            "failed to parse document frontmatter {}: {err}",
            file.display()
        )
    })?;
    if !frontmatter.resolve_mode().is_template() {
        return Ok(None);
    }

    let Some(mut repaired) = agent_doc_template::repair_prompt_tail_outside_exchange(doc_content)?
    else {
        return Ok(None);
    };

    if let Some(snapshot_content) = agent_doc_snapshot_io::load(file)? {
        repaired =
            normalize_user_prompts_in_exchange_safe(&repaired, &repaired, &snapshot_content, file);
        repaired = agent_doc_template_io::normalize_template_structure_or_fail(&repaired, file)?;
    }

    Ok((repaired != doc_content).then_some(repaired))
}

pub fn remove_duplicate_answered_exchange_prompt_tail_for_preflight(file: &Path) -> Result<bool> {
    let Some(cleaned_doc) = agent_doc_template::remove_duplicate_answered_exchange_prompt_tail(
        &std::fs::read_to_string(file)?,
    ) else {
        return Ok(false);
    };

    agent_doc_document_realtime_io::atomic_write_through_authority(file, &cleaned_doc)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "duplicate_answered_exchange_prompt_tail_removed file={} source=preflight",
            file.display()
        ),
    );
    eprintln!(
        "[preflight] removed duplicate answered prompt tail after exchange boundary in {}",
        file.display()
    );
    Ok(true)
}

pub fn remove_post_exchange_duplicate_prompt_comments_for_preflight(
    file: &Path,
    rc: &agent_doc_run_context_io::RunContext,
) -> Result<bool> {
    let current = std::fs::read_to_string(file)?;
    let snapshot_doc = agent_doc_snapshot_io::load(file).ok().flatten();
    let head_doc = rc.head_content();
    let mut preserve_docs = Vec::new();
    preserve_docs.push(current.as_str());
    if let Some(head_doc) = head_doc.as_deref() {
        preserve_docs.push(head_doc.as_str());
    }
    if let Some(snapshot_doc) = snapshot_doc.as_deref() {
        preserve_docs.push(snapshot_doc);
    }
    let Some(cleaned_doc) =
        agent_doc_template::remove_post_exchange_duplicate_prompt_comments_preserving_docs(
            &current,
            &preserve_docs,
        )
    else {
        return Ok(false);
    };

    agent_doc_document_realtime_io::atomic_write_through_authority(file, &cleaned_doc)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "post_exchange_duplicate_prompt_comment_removed file={} source=preflight",
            file.display()
        ),
    );
    eprintln!(
        "[preflight] scrubbed duplicate prompt text from comment after exchange in {}",
        file.display()
    );
    Ok(true)
}

pub fn recover_route_queue_snapshot_commit_boundary(
    file: &Path,
    rc: &agent_doc_run_context_io::RunContext,
) -> Result<bool> {
    if !detect_route_queue_snapshot_commit_boundary_recoverable(file, rc)? {
        return Ok(false);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_queue_snapshot_auto_recovery_attempt file={}",
            file.display()
        ),
    );
    eprintln!(
        "[preflight] route_queue_snapshot: queued dispatch snapshot is not committed for {}; running auto-commit",
        file.display()
    );
    match agent_doc_commit_io::commit(file) {
        Ok(_) => {
            rc.invalidate_head_content();
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_queue_snapshot_auto_recovery_succeeded file={}",
                    file.display()
                ),
            );
            Ok(true)
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_queue_snapshot_auto_recovery_failed file={} error={}",
                    file.display(),
                    e.to_string().replace('\n', " ")
                ),
            );
            eprintln!(
                "[preflight] route_queue_snapshot auto-commit failed for {}: {}",
                file.display(),
                e
            );
            Ok(false)
        }
    }
}

pub fn detect_route_queue_snapshot_commit_boundary_recoverable(
    file: &Path,
    rc: &agent_doc_run_context_io::RunContext,
) -> Result<bool> {
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(false);
    };
    if state.is_open() {
        return Ok(false);
    }
    if !matches!(
        rc.snapshot_commit_status(),
        agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
    ) {
        return Ok(false);
    }

    let Some(snapshot) = rc.snapshot_content() else {
        return Ok(false);
    };
    let Some(head) = rc.head_content() else {
        return Ok(false);
    };
    if agent_doc_turn::document_drift::detect_bypassed_response_write_between(&head, &snapshot)
        .is_some()
    {
        return Ok(false);
    }

    let snapshot_prompts =
        agent_doc_queue::route_dispatch::active_auto_route_queue_prompt_texts(&snapshot)?;
    let head_prompts =
        agent_doc_queue::route_dispatch::active_auto_route_queue_prompt_texts(&head)?;
    // Recover only genuine active-auto-queue commit-boundary churn: either the
    // snapshot still carries the queued dispatch (enqueue case) or HEAD carried
    // an active auto-queue that the snapshot has since drained to inactive
    // residue via queue maintenance (#drained-done-queue-clear). The drained
    // case reduces to an empty stripped diff below (queue body + `queue_active`
    // are both stripped before comparison), so it auto-commits only when no
    // non-queue user change exists. Bail on any other snapshot/HEAD drift.
    if snapshot_prompts.is_empty() && head_prompts.is_empty() {
        return Ok(false);
    }

    let head_norm =
        agent_doc_queue::route_dispatch::strip_route_queue_state_for_boundary_compare(&head);
    let snapshot_norm =
        agent_doc_queue::route_dispatch::strip_route_queue_state_for_boundary_compare(&snapshot);
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(&head_norm, &snapshot_norm)
    else {
        return Ok(true);
    };
    let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff_text)
        .into_iter()
        .filter(|change| {
            !matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
                    | agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact
            )
        })
        .collect::<Vec<_>>();
    if changes.is_empty() {
        return Ok(true);
    }

    Ok(changes.iter().all(|change| {
        change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
            && agent_doc_queue::route_dispatch::route_prompt_text_for_change(&change.text)
                .is_some_and(|text| snapshot_prompts.iter().any(|prompt| prompt == &text))
    }))
}

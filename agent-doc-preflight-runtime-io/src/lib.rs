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

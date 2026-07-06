use anyhow::Result;
use std::path::Path;

pub trait PipelineFrontmatterEffects {
    fn read_current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> Result<()>;

    fn log_op(&self, file: &Path, message: &str);
}

/// #22a8 (Phase 5b write-side): mirror the live cycle phase into the session
/// document's `agent_doc_pipeline:` frontmatter block so any later invocation or
/// editor can read where the pipeline is without parsing the sidecar JSON.
///
/// Best-effort and non-fatal. The write is byte-precise and goes through the
/// editor-aware convergence path so a live IDE buffer does not raise a file
/// cache conflict.
pub fn mirror_pipeline_frontmatter(
    effects: &impl PipelineFrontmatterEffects,
    file: &Path,
    state: &crate::CycleState,
) {
    if let Err(e) = (|| -> Result<()> {
        let content = effects.read_current_document_content(file, "pipeline_mirror")?;
        let updated = agent_doc_frontmatter::frontmatter::splice_pipeline_block(
            &content,
            &state.to_pipeline(),
        )?;
        if updated != content {
            effects.converge_or_disk_write(file, &content, &updated, "pipeline_mirror")?;
        }
        Ok(())
    })() {
        effects.log_op(
            file,
            &format!("pipeline_mirror_failed file={} err={}", file.display(), e),
        );
    }
}

/// Clear the live `agent_doc_pipeline:` frontmatter mirror after a terminal
/// cycle transition through the injected editor-aware document convergence port.
pub fn clear_pipeline_frontmatter(effects: &impl PipelineFrontmatterEffects, file: &Path) {
    if let Err(e) = (|| -> Result<()> {
        let content = effects.read_current_document_content(file, "pipeline_clear")?;
        if !content.contains("agent_doc_pipeline:") {
            return Ok(());
        }
        let updated = agent_doc_frontmatter::frontmatter::splice_pipeline_block(
            &content,
            &Default::default(),
        )?;
        if updated != content {
            effects.converge_or_disk_write(file, &content, &updated, "pipeline_clear")?;
        }
        Ok(())
    })() {
        effects.log_op(
            file,
            &format!("pipeline_clear_failed file={} err={}", file.display(), e),
        );
    }
}

pub fn mark_committed(
    effects: &impl PipelineFrontmatterEffects,
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<crate::CycleState> {
    let state = crate::mark_committed(file, event, snapshot_content, file_content)?;
    clear_pipeline_frontmatter(effects, file);
    Ok(state)
}

pub fn mark_abandoned(
    effects: &impl PipelineFrontmatterEffects,
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<crate::CycleState> {
    let state = crate::mark_abandoned(file, event, snapshot_content, file_content)?;
    clear_pipeline_frontmatter(effects, file);
    Ok(state)
}

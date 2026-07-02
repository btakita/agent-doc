use anyhow::Result;
use std::path::Path;

/// #22a8 (Phase 5b write-side): mirror the live cycle phase into the session
/// document's `agent_doc_pipeline:` frontmatter block so any later invocation or
/// editor can read where the pipeline is without parsing the sidecar JSON.
///
/// Best-effort and non-fatal. The write is byte-precise and goes through the
/// editor-aware convergence path so a live IDE buffer does not raise a file
/// cache conflict.
pub(crate) fn mirror_pipeline_frontmatter(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
) {
    if let Err(e) = (|| -> Result<()> {
        let content = std::fs::read_to_string(file)?;
        let updated = agent_doc_frontmatter::frontmatter::splice_pipeline_block(
            &content,
            &state.to_pipeline(),
        )?;
        if updated != content {
            crate::write::converge_or_disk_write(file, &content, &updated, "pipeline_mirror")?;
        }
        Ok(())
    })() {
        crate::ops_log::log_op(
            file,
            &format!("pipeline_mirror_failed file={} err={}", file.display(), e),
        );
    }
}

/// Clear the live `agent_doc_pipeline:` frontmatter mirror after a terminal
/// cycle transition. This remains in orchestration because it uses the
/// editor-aware document convergence port.
pub(crate) fn clear_pipeline_frontmatter(file: &Path) {
    if let Err(e) = (|| -> Result<()> {
        let content = std::fs::read_to_string(file)?;
        if !content.contains("agent_doc_pipeline:") {
            return Ok(());
        }
        let updated = agent_doc_frontmatter::frontmatter::splice_pipeline_block(
            &content,
            &Default::default(),
        )?;
        if updated != content {
            crate::write::converge_or_disk_write(file, &content, &updated, "pipeline_clear")?;
        }
        Ok(())
    })() {
        crate::ops_log::log_op(
            file,
            &format!("pipeline_clear_failed file={} err={}", file.display(), e),
        );
    }
}

pub(crate) fn mark_committed(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<agent_doc_cycle_state_io::CycleState> {
    let state =
        agent_doc_cycle_state_io::mark_committed(file, event, snapshot_content, file_content)?;
    clear_pipeline_frontmatter(file);
    Ok(state)
}

pub(crate) fn mark_abandoned(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<agent_doc_cycle_state_io::CycleState> {
    let state =
        agent_doc_cycle_state_io::mark_abandoned(file, event, snapshot_content, file_content)?;
    clear_pipeline_frontmatter(file);
    Ok(state)
}

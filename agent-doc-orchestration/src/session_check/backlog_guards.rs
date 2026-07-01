use super::*;
use agent_doc_element_backlog::guard_policy::{
    dropped_from_history_guard, malformed_tracked_item_guard, shadow_backlog_guard,
};

/// Where a reaped `do #id` directive's `### Re: ... #id` response heading
pub(crate) fn check_shadow_backlog_guard(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    Ok(shadow_backlog_guard(&rc.doc_content())?.into())
}

pub(crate) fn check_malformed_tracked_item_guard(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    Ok(malformed_tracked_item_guard(&rc.doc_content(), &rc.components()).into())
}

pub(crate) fn check_backlog_replay_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let current_content = rc.doc_content();

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let hash = agent_doc_fs::document_state_hash(&canonical).unwrap_or_default();
    let baseline_content = agent_doc_fs::find_project_root(&canonical)
        .map(|root| root.join(format!(".agent-doc/baselines/{}.md", hash)))
        .and_then(|p| std::fs::read_to_string(p).ok());

    let baseline = match baseline_content {
        Some(content) => content,
        None => match rc.head_content() {
            Some(content) => content.to_string(),
            None => return Ok(GuardResult::None),
        },
    };

    let resolved_ids = crate::cycle_state::resolved_pending_ids(file)?;

    let external_done_ids = crate::preflight::external_done_archive_ids(file, &current_content)?;
    Ok(dropped_from_history_guard(
        &current_content,
        &baseline,
        &resolved_ids,
        &external_done_ids,
    )?
    .into())
}

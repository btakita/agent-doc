use std::path::Path;

use agent_doc_element_backlog::guard_policy::{
    dropped_from_history_guard, malformed_tracked_item_guard, shadow_backlog_guard,
};
use agent_doc_run_context_io::{AgentDocContextExt, RunContext};
use agent_doc_workflow::session_check::GuardResult;
use anyhow::Result;

/// Where a reaped `do #id` directive's `### Re: ... #id` response heading
pub fn check_shadow_backlog_guard(_file: &Path, rc: &RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    Ok(shadow_backlog_guard(&rc.doc_content())?.into())
}

pub fn check_malformed_tracked_item_guard(_file: &Path, rc: &RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    Ok(malformed_tracked_item_guard(&rc.doc_content(), &rc.components()).into())
}

pub fn check_backlog_replay_guard(file: &Path, rc: &RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let current_content = rc.doc_content();

    let baseline_content = agent_doc_fs::baseline_path_for(file)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok());

    let baseline = match baseline_content {
        Some(content) => content,
        None => match rc.head_content() {
            Some(content) => content.to_string(),
            None => return Ok(GuardResult::None),
        },
    };

    let resolved_ids = agent_doc_cycle_state_io::resolved_pending_ids(file)?;

    let external_done_ids = agent_doc_element_backlog_io::done_archive::external_done_archive_ids(
        file,
        &current_content,
    )?;
    Ok(dropped_from_history_guard(
        &current_content,
        &baseline,
        &resolved_ids,
        &external_done_ids,
    )?
    .into())
}

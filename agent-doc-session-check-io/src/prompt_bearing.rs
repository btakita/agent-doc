use anyhow::Result;
use std::path::Path;

pub fn detect_unstarted_prompt_bearing_diff(file: &Path) -> Result<Option<String>> {
    let Some(change) = first_unstarted_prompt_bearing_change(file)? else {
        return Ok(None);
    };
    let label = match change.kind {
        agent_doc_diff::PromptBearingChangeKind::PromptTarget => "prompt_target",
        agent_doc_diff::PromptBearingChangeKind::ContentEdit => "content_edit",
        agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
        | agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact => return Ok(None),
    };
    let preview = change
        .text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(change.text.as_str())
        .trim();
    Ok(Some(format!("{label}: {preview}")))
}

pub fn first_unstarted_prompt_bearing_change(
    file: &Path,
) -> Result<Option<agent_doc_diff::PromptBearingChange>> {
    // A fresh session can carry an unanswered exchange tail prompt before any
    // cycle snapshot exists. The queue path activates independently of the
    // snapshot, so without a snapshot we fall back to the committed `HEAD` blob
    // and then to an empty baseline for untracked docs.
    let baseline = match agent_doc_snapshot_io::load(file)? {
        Some(snapshot) => snapshot,
        None => agent_doc_git_io::revision::show_head(file)?.unwrap_or_default(),
    };
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let norm = |s: &str| {
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(s)
    };
    let snap_norm =
        norm(&agent_doc_diff::prompt_bearing_body_for_unstarted_prompt_guard(&baseline));
    let cur_norm = norm(&agent_doc_diff::prompt_bearing_body_for_unstarted_prompt_guard(&current));
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(&snap_norm, &cur_norm) else {
        return Ok(None);
    };
    Ok(agent_doc_diff::first_unstarted_prompt_bearing_change_from_diff(&diff_text, &current))
}

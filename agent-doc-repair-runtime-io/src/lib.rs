use std::path::Path;

use agent_doc_document::write_normalization::{
    latest_response_block_missing_from_current, splice_response_block_into_current_exchange,
};
use agent_doc_turn::response_replay::dedupe_responses;
use anyhow::{Context, Result};

pub fn recover_missing_committed_head_response(file: &Path) -> Result<bool> {
    let Some(head_content) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(false);
    };
    let current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let Some(response_block) = latest_response_block_missing_from_current(&head_content, &current)
    else {
        return Ok(false);
    };
    let Some(recovered) = splice_response_block_into_current_exchange(&current, &response_block)
    else {
        return Ok(false);
    };
    if recovered == current {
        return Ok(false);
    }
    eprintln!(
        "[write] empty response stdin; merged latest committed HEAD response back into visible document for {}",
        file.display()
    );
    agent_doc_document_realtime_io::guard_visible_write_idle_and_current(
        file,
        "recover_committed_head_response",
        &current,
    )?;
    agent_doc_document_realtime_io::atomic_write_through_authority(file, &recovered)?;
    agent_doc_snapshot_io::save(file, &recovered, agent_doc_ops_log_io::log_op)?;
    agent_doc_commit_io::commit(file)?;
    Ok(true)
}

pub fn recover_dedupe_only_drift(file: &Path) -> Result<bool> {
    let Some(head_content) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(false);
    };
    let current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    if current == head_content {
        return Ok(false);
    }
    let dedupe_of_head = dedupe_responses(&head_content);
    if dedupe_of_head == head_content {
        return Ok(false);
    }
    if dedupe_of_head != current {
        return Ok(false);
    }
    eprintln!(
        "[write] empty response stdin; current file matches dedupe(HEAD) for {} — committing dedupe-only working-tree drift through the binary closeout path",
        file.display()
    );
    agent_doc_snapshot_io::save(file, &current, agent_doc_ops_log_io::log_op)?;
    agent_doc_commit_io::commit(file)?;
    Ok(true)
}

use std::path::Path;

use agent_doc_document::write_normalization::{
    latest_response_block_missing_from_current, splice_response_block_into_current_exchange,
};
use agent_doc_turn::response_replay::dedupe_responses;
use anyhow::{Context, Result};

pub fn repair_coordinator_effects<ReplayWriteEffects>(
    replay_write_effects: &'static ReplayWriteEffects,
) -> agent_doc_repair_io::RepairCoordinatorEffects<
    'static,
    agent_doc_closeout_runtime_io::RuntimeRepairIoEffects,
    ReplayWriteEffects,
>
where
    ReplayWriteEffects: agent_doc_repair_io::RepairReplayWriteEffects,
{
    agent_doc_repair_io::RepairCoordinatorEffects {
        repair_io_effects: &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
        replay_write_effects,
        complete_required_closeout: repair_complete_required_closeout,
        inspect_session: repair_inspect_session,
        recover_missing_committed_head_response,
        recover_dedupe_only_drift,
    }
}

fn repair_complete_required_closeout(file: &Path) -> Result<bool> {
    agent_doc_flow_io::closeout::complete_required_closeout(
        file,
        &agent_doc_closeout_runtime_io::closeout_effects(),
    )
}

fn repair_inspect_session(file: &Path) -> Result<agent_doc_session_check_io::SessionCheckStatus> {
    agent_doc_session_check_io::inspect(
        file,
        &agent_doc_closeout_runtime_io::session_check_effects(),
    )
}

pub fn recover_missing_committed_head_response(file: &Path) -> Result<bool> {
    let Some(head_content) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(false);
    };
    let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "recover_missing_committed_head_response",
    )
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
    agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
        file,
        &recovered,
        &current,
        "recover_committed_head_response",
    )?;
    agent_doc_snapshot_io::save(file, &recovered, agent_doc_ops_log_io::log_op)?;
    agent_doc_commit_io::commit(file)?;
    Ok(true)
}

pub fn recover_dedupe_only_drift(file: &Path) -> Result<bool> {
    let Some(head_content) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(false);
    };
    let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "recover_dedupe_only_drift",
    )
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

use std::path::Path;

use agent_doc_document::write_normalization::{
    latest_response_block_missing_from_current, splice_response_block_into_current_exchange,
};
use agent_doc_turn::response_replay::dedupe_responses;
use anyhow::{Context, Result};

pub fn repair_coordinator_effects<'a, ReplayWriteEffects>(
    repair_io_effects: &'a agent_doc_closeout_runtime_io::RuntimeRepairIoEffects<'a>,
    replay_write_effects: &'a ReplayWriteEffects,
) -> agent_doc_repair_io::RepairCoordinatorEffects<
    'a,
    agent_doc_closeout_runtime_io::RuntimeRepairIoEffects<'a>,
    ReplayWriteEffects,
>
where
    ReplayWriteEffects: agent_doc_repair_io::RepairReplayWriteEffects,
{
    agent_doc_repair_io::RepairCoordinatorEffects {
        repair_io_effects,
        replay_write_effects,
        apply_guard: repair_io_effects.apply_guard(),
        complete_required_closeout: repair_complete_required_closeout,
        inspect_session: repair_inspect_session,
        recover_missing_committed_head_response,
        recover_dedupe_only_drift,
    }
}

fn repair_complete_required_closeout(
    effects: &agent_doc_closeout_runtime_io::RuntimeRepairIoEffects<'_>,
    file: &Path,
) -> Result<bool> {
    agent_doc_closeout_runtime_io::complete_required_closeout_under_apply_guard(
        file,
        false,
        effects.apply_guard(),
    )
}

fn repair_inspect_session(
    effects: &agent_doc_closeout_runtime_io::RuntimeRepairIoEffects<'_>,
    file: &Path,
) -> Result<agent_doc_session_check_io::SessionCheckStatus> {
    let session_effects = agent_doc_closeout_runtime_io::session_check_effects_under_apply_guard(
        file,
        effects.apply_guard(),
    )?;
    agent_doc_session_check_io::inspect_under_apply_guard(
        file,
        &session_effects,
        effects.apply_guard(),
    )
}

pub fn recover_missing_committed_head_response(
    effects: &agent_doc_closeout_runtime_io::RuntimeRepairIoEffects<'_>,
    file: &Path,
) -> Result<bool> {
    effects.apply_guard().ensure_protects(file)?;
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
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &recovered,
        agent_doc_ops_log_io::log_op,
    )?;
    agent_doc_commit_io::commit(file)?;
    Ok(true)
}

pub fn recover_dedupe_only_drift(
    effects: &agent_doc_closeout_runtime_io::RuntimeRepairIoEffects<'_>,
    file: &Path,
) -> Result<bool> {
    effects.apply_guard().ensure_protects(file)?;
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
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &current,
        agent_doc_ops_log_io::log_op,
    )?;
    agent_doc_commit_io::commit(file)?;
    Ok(true)
}

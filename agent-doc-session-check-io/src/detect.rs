use std::path::Path;

use agent_doc_run_context_io::RunContext;
use agent_doc_turn::response_replay::{
    JbCacheConflictAcceptDuplicateReplay, LateIpcResponseOverapplication,
    classify_jb_cache_conflict_accept_duplicate_replay, classify_late_ipc_response_overapplication,
};
use agent_doc_workflow::session_check::GuardResult;
use anyhow::{Context, Result};

/// Detect the late JetBrains File Cache Conflict "accept" replay shape.
///
/// The stale editor/cache payload lands after the cycle already committed, so
/// the working tree contains an extra adjacent response block while `HEAD`
/// still contains the correct single-response document. This is not a fresh
/// direct patchback; it is safe to repair by replacing the working tree and
/// snapshot with `dedupe(current)` when that result matches `HEAD` modulo
/// transient editor markers.
pub fn detect_jb_cache_conflict_accept_duplicate_replay(
    file: &Path,
) -> Result<Option<JbCacheConflictAcceptDuplicateReplay>> {
    let rc = RunContext::new(file.to_path_buf());
    detect_jb_cache_conflict_accept_duplicate_replay_with_context(file, &rc)
}

pub fn detect_jb_cache_conflict_accept_duplicate_replay_with_context(
    file: &Path,
    rc: &RunContext,
) -> Result<Option<JbCacheConflictAcceptDuplicateReplay>> {
    let current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let Some(head) = rc.head_content() else {
        return Ok(None);
    };
    Ok(classify_jb_cache_conflict_accept_duplicate_replay(
        &current, &head,
    ))
}

/// Detect the late-IPC committed-response over-application shape.
///
/// Unlike [`detect_jb_cache_conflict_accept_duplicate_replay`], this does not
/// require the duplicate to be a *consecutive* `### Re:` block. The reposition
/// signal can leave the re-applied copy separated by boundary markers, which
/// the consecutive-only dedupe collapse misses.
pub fn detect_late_ipc_response_overapplication(
    file: &Path,
) -> Result<Option<LateIpcResponseOverapplication>> {
    let rc = RunContext::new(file.to_path_buf());
    detect_late_ipc_response_overapplication_with_context(file, &rc)
}

pub fn detect_late_ipc_response_overapplication_with_context(
    file: &Path,
    rc: &RunContext,
) -> Result<Option<LateIpcResponseOverapplication>> {
    let current = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    let Some(head) = rc.head_content() else {
        return Ok(None);
    };
    Ok(classify_late_ipc_response_overapplication(&current, &head))
}

pub fn check_parent_submodule_pointer_guard(file: &Path) -> Result<GuardResult> {
    let Some(drift) = agent_doc_git_io::submodule::submodule_pointer_drift(file)? else {
        return Ok(GuardResult::None);
    };
    let file_display = file.display().to_string();
    let msg = agent_doc_git::parent_submodule_pointer_guard_message(&drift, &file_display);
    eprintln!("{}", msg);
    agent_doc_ops_log_io::log_op(
        file,
        &agent_doc_git::parent_submodule_pointer_guard_log_line(&drift, &file_display),
    );
    Ok(GuardResult::Error(msg))
}

pub fn check_prompt_only_exchange_tail_guard(file: &Path, rc: &RunContext) -> Result<GuardResult> {
    let content = rc.doc_content();
    let file_display = file.display().to_string();
    Ok(
        agent_doc_workflow::session_check::prompt_only_exchange_tail_guard(&content, &file_display)
            .map(GuardResult::Error)
            .unwrap_or(GuardResult::None),
    )
}

/// Phase 3 (#jbccc3): JB File Cache Conflict cancel auto-recovery detection.
///
/// Returns true when the document is in the recoverable post-write pre-commit
/// shape: the cycle is at `WriteApplied` (or already-marked `Committed` whose
/// commit boundary never landed in `HEAD`), the snapshot has the visible
/// response, `HEAD` does not, and the working tree matches the snapshot modulo
/// transient `(HEAD)` / boundary markers.
pub fn detect_jb_cache_conflict_cancel_recoverable(file: &Path) -> Result<bool> {
    let rc = RunContext::new(file.to_path_buf());
    detect_jb_cache_conflict_cancel_recoverable_with_context(file, &rc)
}

pub fn detect_jb_cache_conflict_cancel_recoverable_with_context(
    file: &Path,
    rc: &RunContext,
) -> Result<bool> {
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(false);
    };
    if !matches!(
        state.phase,
        agent_doc_turn::CyclePhase::WriteApplied | agent_doc_turn::CyclePhase::Committed
    ) {
        return Ok(false);
    }
    if !matches!(
        rc.snapshot_commit_status(),
        agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
    ) {
        return Ok(false);
    }
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let Some(snapshot) = rc.snapshot_content() else {
        return Ok(false);
    };
    let normalized_doc =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&doc);
    let normalized_snapshot =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&snapshot);
    Ok(normalized_doc == normalized_snapshot)
}

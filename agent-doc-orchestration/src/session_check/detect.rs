//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_turn::response_replay::{
    JbCacheConflictAcceptDuplicateReplay, LateIpcResponseOverapplication,
    classify_jb_cache_conflict_accept_duplicate_replay, classify_late_ipc_response_overapplication,
};

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
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    detect_jb_cache_conflict_accept_duplicate_replay_with_context(file, &rc)
}

pub fn detect_jb_cache_conflict_accept_duplicate_replay_with_context(
    file: &Path,
    rc: &crate::graph::RunContext,
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

/// A late-IPC reposition / stale-patch replay re-inserted the committed
/// response into the working tree after the cycle already reached `HEAD`.
///
/// The duplicate body matches `HEAD`'s committed response (possibly wrapped in
/// redundant `<!-- agent:boundary:* -->` markers and non-adjacent), so the
/// safe repair is to restore the committed `HEAD` content over the working tree
/// and snapshot. See `tasks/agent-doc/plan-duplicate-response-after-commit.md`.
/// Detect the late-IPC committed-response over-application shape.
///
/// Unlike [`detect_jb_cache_conflict_accept_duplicate_replay`], this does not
/// require the duplicate to be a *consecutive* `### Re:` block — the reposition
/// signal can leave the re-applied copy separated by boundary markers, which
/// the consecutive-only `dedupe_responses` collapse misses, letting the generic
/// `detect_bypassed_response_write` guard misclassify it as a manual patchback.
/// We instead prove that the working tree is `HEAD` plus extra duplicate copies
/// of already-committed responses (identical scaffold, same response set), in
/// which case restoring `HEAD` is provably safe.
pub fn detect_late_ipc_response_overapplication(
    file: &Path,
) -> Result<Option<LateIpcResponseOverapplication>> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    detect_late_ipc_response_overapplication_with_context(file, &rc)
}

pub fn detect_late_ipc_response_overapplication_with_context(
    file: &Path,
    rc: &crate::graph::RunContext,
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

pub(crate) fn check_parent_submodule_pointer_guard(file: &Path) -> Result<GuardResult> {
    let Some(drift) = agent_doc_git_io::submodule::submodule_pointer_drift(file)? else {
        return Ok(GuardResult::None);
    };
    let file_display = file.display().to_string();
    let msg = agent_doc_git::parent_submodule_pointer_guard_message(&drift, &file_display);
    eprintln!("{}", msg);
    crate::ops_log::log_op(
        file,
        &agent_doc_git::parent_submodule_pointer_guard_log_line(&drift, &file_display),
    );
    Ok(GuardResult::Error(msg))
}

pub(crate) fn check_prompt_only_exchange_tail_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
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
/// response, `HEAD` does not, and the working tree matches the snapshot
/// modulo transient `(HEAD)` / boundary markers (no live exchange edits beyond
/// the response). When this returns true, `git::commit(file)` reliably closes
/// the cycle and `session_check` must avoid misclassifying the same state as
/// a `likely_direct_response_patchback`.
///
/// See `tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md` Phase 3.
pub fn detect_jb_cache_conflict_cancel_recoverable(file: &Path) -> Result<bool> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    detect_jb_cache_conflict_cancel_recoverable_with_context(file, &rc)
}

pub fn detect_jb_cache_conflict_cancel_recoverable_with_context(
    file: &Path,
    rc: &crate::graph::RunContext,
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

pub fn detect_uncommitted_closeout_drift(file: &Path) -> Result<Option<String>> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    detect_uncommitted_closeout_drift_with_context(file, &rc)
}

pub fn detect_uncommitted_closeout_drift_with_context(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<Option<String>> {
    if crate::git::repair_committed_historical_snapshot_drift(file)?.is_some() {
        return Ok(None);
    }
    if let Some(drift) = agent_doc_git_io::submodule::submodule_pointer_drift(file)? {
        return Ok(Some(agent_doc_git::parent_submodule_pointer_message(
            &drift.relative_path,
            drift.parent_head.as_deref(),
            &drift.submodule_head,
            &file.display().to_string(),
        )));
    }
    // Phase 3 (#jbccc3): jb_cache_conflict_cancel is auto-recoverable through
    // `git::commit`. Skip the lower-precision `detect_bypassed_response_write`
    // and `SnapshotDiffersFromHead` paths below so neither this caller nor
    // standalone `session-check` accuses the user of a direct patchback when
    // the binary-owned write path actually applied the response but the commit
    // boundary never landed. Preflight's `enforce_no_uncommitted_closeout_drift`
    // separately runs `git::commit` to close the cycle.
    if detect_jb_cache_conflict_cancel_recoverable_with_context(file, rc)? {
        return Ok(None);
    }
    if let Some(marker) = detect_bypassed_response_write(file)? {
        return Ok(Some(format!(
            "found likely direct response patchback without agent-doc cycle: {}{} {}",
            marker,
            agent_doc_git_io::status::tracked_side_effect_note(file)?,
            closeout_recovery_hint(file)
        )));
    }
    if let Some(marker) = detect_uncommitted_exchange_drift(file)? {
        if detect_unstarted_prompt_bearing_diff(file)?.is_some() {
            return Ok(None);
        }
        return Ok(Some(format!(
            "document has uncommitted exchange changes beyond the committed snapshot: {}{} {}",
            marker,
            agent_doc_git_io::status::tracked_side_effect_note(file)?,
            closeout_recovery_hint(file)
        )));
    }
    match rc.snapshot_commit_status() {
        agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len,
            head_len,
        } => {
            if detect_unstarted_prompt_bearing_diff(file)?.is_some() {
                return Ok(None);
            }
            Ok(Some(format!(
                "snapshot differs from HEAD without an open or recoverable agent-doc cycle (snapshot_len={}, head_len={}){} {}",
                snapshot_len,
                head_len,
                agent_doc_git_io::status::tracked_side_effect_note(file)?,
                closeout_recovery_hint(file)
            )))
        }
        agent_doc_snapshot_io::SnapshotCommitStatus::Committed
        | agent_doc_snapshot_io::SnapshotCommitStatus::NoSnapshot
        | agent_doc_snapshot_io::SnapshotCommitStatus::NoHead
        | agent_doc_snapshot_io::SnapshotCommitStatus::NotInGitRepo => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::process::Command;
    #[test]
    fn enforce_clean_closeout_self_heals_late_ipc_overapplication() {
        // #late-ipc-patch-response-uncommitted: a late-IPC stale-patch replay
        // re-adds a duplicate `### Re:` block to the working tree after the cycle
        // committed. enforce_clean_closeout (the finalize boundary) must self-heal
        // by restoring committed HEAD instead of bailing — otherwise the
        // interruption stalls the agent:queue auto-loop.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\n\n",
            "Answer A.\n",
            "### Re: second — opus-4-8\n\n",
            "Answer B.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, crate::ops_log::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed", "--no-verify"])
            .output()
            .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::pipeline_frontmatter::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        // Late stale-patch replay re-inserts an earlier committed response (A) at
        // the tail (non-adjacent over-application), leaving the real responses in
        // HEAD untouched.
        let overapplied = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: first — opus-4-8\n\nAnswer A.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &overapplied).unwrap();
        assert!(
            detect_late_ipc_response_overapplication(&doc)
                .unwrap()
                .is_some(),
            "precondition: late-IPC over-application present"
        );

        // The finalize boundary must NOT bail — it self-heals.
        enforce_clean_closeout(&doc).expect("enforce_clean_closeout should self-heal, not bail");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            committed,
            "working tree restored to committed HEAD (duplicate dropped)"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().unwrap(),
            committed,
            "snapshot restored to committed HEAD"
        );
    }
    #[test]
    fn detects_prompt_prefixed_corrupted_duplicate_as_overapplication() {
        // #finalize-retry-ipc-response-duplication: a multi-retry / late-IPC
        // reposition left a duplicate response whose stale copy had its body
        // wrongly prefixed with `❯ `. HEAD still holds a single clean copy, so
        // the over-application detector must recognize the corrupted duplicate
        // and remediate by restoring committed HEAD — no manual `git checkout`.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "do [#fix-thing]\n",
            "### Re: fix thing — gpt-5\n\n",
            "**Scope:** narrow.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, crate::ops_log::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed response", "--no-verify"])
            .output()
            .unwrap();

        // Working tree gains a stale duplicate whose body line is `❯ `-prefixed.
        let corrupted = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: fix thing — gpt-5\n\n❯ **Scope:** narrow.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, corrupted).unwrap();

        let overapplication = detect_late_ipc_response_overapplication(&doc)
            .unwrap()
            .expect("prompt-prefixed corrupted duplicate must be detected");
        assert_eq!(
            overapplication.remediated_content, committed,
            "remediation must restore the clean committed HEAD"
        );
    }
}

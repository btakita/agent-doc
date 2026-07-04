use anyhow::Result;
use std::path::Path;

use agent_doc_document::transient_markers::normalize_for_replay_hash;
use agent_doc_element_exchange::post_commit_ipc_reposition_only_exchange_safe;
use agent_doc_git::PostCommitLocalDriftKind;

pub struct QueueContinuationProof {
    pub head_prompt: String,
    pub head_id: Option<String>,
}

pub trait PostCommitCleanupEffects {
    fn read_to_string(&self, file: &Path) -> Result<String>;
    fn load_snapshot(&self, file: &Path) -> Option<String>;
    fn log_cycle(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    );
    fn log_op(&self, file: &Path, message: &str);
    fn log_closeout_commit_completed(&self, file: &Path, reason: &str);
    fn mark_pipeline_committed(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<()>;
    fn mark_capture_committed(&self, file: &Path) -> Result<()>;
    fn clear_queue_journal(&self, file: &Path);
    fn reconcile_queue_continuation(
        &self,
        file: &Path,
        phase: &str,
    ) -> Option<QueueContinuationProof>;
    fn read_session_id(&self, file: &Path) -> String;
    fn fire_post_commit(&self, file: &Path, session_id: &str);
    fn fire_doc_event(&self, file: &Path, event: &str);
}

/// Emit a post-commit working-tree-vs-HEAD proof line without mutating disk.
///
/// The realtime CRDT replica owns disk reconciliation, so this observes residual
/// drift only. The comparison ignores legitimate transient marker churn through
/// the replay normalizer.
pub fn emit_postcommit_worktree_check(effects: &impl PostCommitCleanupEffects, file: &Path) {
    let head_doc = match crate::revision::show_head(file) {
        Ok(Some(head)) => head,
        Ok(None) => return,
        Err(e) => {
            eprintln!("[commit] postcommit worktree check: HEAD read failed (non-fatal): {e}");
            return;
        }
    };
    let working = match effects.read_to_string(file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "[commit] postcommit worktree check: working-tree read failed (non-fatal): {e}"
            );
            return;
        }
    };
    let head_norm = normalize_for_replay_hash(&head_doc);
    let tree_norm = normalize_for_replay_hash(&working);
    let head_sha = agent_doc_hash::content_hash(&head_norm);
    let tree_sha = agent_doc_hash::content_hash(&tree_norm);
    let matches = head_norm == tree_norm;
    effects.log_op(
        file,
        &format!(
            "postcommit_worktree_check file={} head={} tree={} match={} action=observe_only_realtime_replica_owns_disk",
            file.display(),
            &head_sha[..head_sha.len().min(12)],
            &tree_sha[..tree_sha.len().min(12)],
            matches
        ),
    );
    if !matches {
        eprintln!(
            "[commit] postcommit_worktree_check match=false for {} - working tree differs from HEAD; realtime replica owns reconciliation, no revert (#realtimecutover)",
            file.display()
        );
    }
}

pub fn should_send_post_commit_ipc_reposition(file: &Path) -> bool {
    let Ok(Some(parent_doc)) = crate::revision::show_rev(file, "HEAD^") else {
        return false;
    };
    let Ok(Some(head_doc)) = crate::revision::show_rev(file, "HEAD") else {
        return false;
    };
    post_commit_ipc_reposition_only_exchange_safe(&parent_doc, &head_doc)
}

pub fn finalize_successful_commit(
    effects: &impl PostCommitCleanupEffects,
    file: &Path,
    prior_head_doc: Option<&str>,
) {
    effects.log_cycle(file, "commit", None, None);
    effects.log_op(file, &format!("commit_success file={}", file.display()));
    effects.log_closeout_commit_completed(file, "commit_success");
    let snap = effects.load_snapshot(file);
    let file_content = effects.read_to_string(file).ok();
    if let Err(e) = effects.mark_pipeline_committed(
        file,
        "commit_success",
        snap.as_deref(),
        file_content.as_deref(),
    ) {
        eprintln!("[commit] cycle-state update failed: {} (non-fatal)", e);
    }
    if let Err(e) = effects.mark_capture_committed(file) {
        eprintln!("[commit] capture-state update failed: {} (non-fatal)", e);
    }
    effects.clear_queue_journal(file);
    if let Some(continuation) = effects.reconcile_queue_continuation(file, "commit") {
        effects.log_op(
            file,
            &format!(
                "queue_continuation_required file={} head={}",
                file.display(),
                continuation.head_prompt.replace('\n', " ")
            ),
        );
        if let (Some(prior), Some(current)) = (prior_head_doc, snap.as_deref())
            && agent_doc_queue::queue_continuation::review_phase_routed(prior, current)
        {
            let next_head = continuation
                .head_id
                .as_deref()
                .unwrap_or(continuation.head_prompt.as_str());
            effects.log_op(
                file,
                &format!(
                    "drain_continue_after_review file={} next_head={} (#mphaseloop)",
                    file.display(),
                    next_head.replace('\n', " ")
                ),
            );
        }
    }
    let session_id = effects.read_session_id(file);
    effects.fire_post_commit(file, &session_id);
    effects.fire_doc_event(file, "post_commit");
}

pub fn finalize_already_committed_noop(
    effects: &impl PostCommitCleanupEffects,
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
    drift_kind: Option<PostCommitLocalDriftKind>,
) {
    effects.log_cycle(file, "commit_noop", snapshot_content, file_content);
    let drift_kind = drift_kind
        .map(PostCommitLocalDriftKind::as_str)
        .unwrap_or("none");
    effects.log_op(
        file,
        &format!(
            "commit_noop file={} reason=already_current drift_kind={} basis=head",
            file.display(),
            drift_kind
        ),
    );
    let closeout_reason = format!("already_current_{drift_kind}");
    effects.log_closeout_commit_completed(file, &closeout_reason);
    effects.log_op(
        file,
        &format!("commit_already_current file={} basis=head", file.display()),
    );
    if let Err(e) = effects.mark_pipeline_committed(file, event, snapshot_content, file_content) {
        eprintln!("[commit] cycle-state update failed: {} (non-fatal)", e);
    }
    if let Err(e) = effects.mark_capture_committed(file) {
        eprintln!("[commit] capture-state update failed: {} (non-fatal)", e);
    }
    let _ = effects.reconcile_queue_continuation(file, "commit_already_current");
}

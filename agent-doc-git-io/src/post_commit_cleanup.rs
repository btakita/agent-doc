use anyhow::Result;
use std::path::Path;

use agent_doc_document::transient_markers::normalize_for_replay_hash;
use agent_doc_element_exchange::post_commit_ipc_reposition_only_exchange_safe;

pub trait PostCommitCleanupEffects {
    fn read_to_string(&self, file: &Path) -> Result<String>;
    fn log_op(&self, file: &Path, message: &str);
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

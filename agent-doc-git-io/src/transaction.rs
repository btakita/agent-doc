use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use agent_doc_document::commit_normalization::canonicalize_answered_prompt_prefixes;
use agent_doc_document::transient_markers::{strip_guard_markers, strip_head_markers};
use agent_doc_git::{
    output_has_index_lock_contention, parent_submodule_pointer_commit_message, relative_to_root,
    render_git_process_output,
};

/// After a successful commit inside a submodule, stage and partial-commit the
/// updated submodule pointer in the superproject. Uses an explicit pathspec on
/// the commit so any other staged files in the parent index are preserved.
pub fn update_parent_submodule_pointer(
    super_root: &Path,
    submodule_root: &Path,
    msg: &str,
) -> anyhow::Result<()> {
    let rel = match submodule_root.strip_prefix(super_root) {
        Ok(r) => r,
        Err(_) => anyhow::bail!(
            "parent submodule pointer is not committed: cannot compute submodule relative path. Run `agent-doc commit` to retry the idempotent parent-pointer closeout."
        ),
    };
    let rel_str = rel.to_string_lossy().to_string();

    let add = crate::index::add_path(super_root, rel);
    match add {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            anyhow::bail!(
                "parent submodule pointer is not committed: git add {} failed: {}. Run `agent-doc commit` to retry the idempotent parent-pointer closeout.",
                rel_str,
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Err(e) => {
            anyhow::bail!(
                "parent submodule pointer is not committed: git add {} error: {}. Run `agent-doc commit` to retry the idempotent parent-pointer closeout.",
                rel_str,
                e
            );
        }
    }

    let parent_msg = parent_submodule_pointer_commit_message(msg);
    let commit = crate::commit::commit_no_verify_pathspec(super_root, &parent_msg, rel);
    match commit {
        Ok(o) if o.status.success() => {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let t = line.trim();
                if t.starts_with('[') && t.contains(']') {
                    eprintln!("{}", line);
                }
            }
            eprintln!("[commit] parent submodule pointer updated for {}", rel_str);
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let stdout = String::from_utf8_lossy(&o.stdout);
            // "nothing to commit" / "no changes added" is fine; pointer was already current.
            if stderr.contains("nothing to commit")
                || stdout.contains("nothing to commit")
                || stderr.contains("no changes added")
            {
                return Ok(());
            }
            anyhow::bail!(
                "parent submodule pointer is not committed: git commit {} failed: {}. Run `agent-doc commit` to retry the idempotent parent-pointer closeout.",
                rel_str,
                stderr.trim()
            );
        }
        Err(e) => anyhow::bail!(
            "parent submodule pointer is not committed: git commit {} error: {}. Run `agent-doc commit` to retry the idempotent parent-pointer closeout.",
            rel_str,
            e
        ),
    }
    Ok(())
}

#[derive(Debug)]
pub enum CommitTransactionError {
    RetryableIndexLock { phase: &'static str, detail: String },
    RetryableHeadMoved { detail: String },
    IgnoredPath { path: String },
    Fatal(anyhow::Error),
}

fn git_path_is_tracked(
    git_root: &Path,
    rel_path: &Path,
) -> std::result::Result<bool, CommitTransactionError> {
    crate::status::is_repo_path_tracked(git_root, rel_path).map_err(CommitTransactionError::Fatal)
}

fn git_path_is_ignored(
    git_root: &Path,
    rel_path: &Path,
) -> std::result::Result<bool, CommitTransactionError> {
    crate::status::is_repo_path_ignored(git_root, rel_path).map_err(CommitTransactionError::Fatal)
}

fn git_path_is_ignored_untracked(
    git_root: &Path,
    rel_path: &Path,
) -> std::result::Result<bool, CommitTransactionError> {
    if git_path_is_tracked(git_root, rel_path)? {
        return Ok(false);
    }
    git_path_is_ignored(git_root, rel_path)
}

/// Produce the exact document surface selected by private-index commit
/// transactions.
pub fn normalize_session_document_content(content: &str) -> String {
    strip_guard_markers(&strip_head_markers(&canonicalize_answered_prompt_prefixes(
        content,
    )))
}

fn staged_blob_for_commit(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
) -> std::result::Result<(String, String), CommitTransactionError> {
    let rel_path = relative_to_root(resolved, git_root);
    if git_path_is_ignored_untracked(git_root, &rel_path)? {
        return Err(CommitTransactionError::IgnoredPath {
            path: rel_path.to_string_lossy().into_owned(),
        });
    }

    let staged_content = if let Some(snap) = snapshot_content {
        normalize_session_document_content(snap)
    } else {
        std::fs::read_to_string(resolved).map_err(|err| {
            CommitTransactionError::Fatal(anyhow::anyhow!(
                "failed to read commit path {}: {err}",
                resolved.display()
            ))
        })?
    };
    let hash = crate::index::hash_object(git_root, &staged_content)
        .map_err(CommitTransactionError::Fatal)?;
    Ok((hash, rel_path.to_string_lossy().into_owned()))
}

fn git_output_or_transaction_error(
    output: Output,
    phase: &'static str,
) -> std::result::Result<Output, CommitTransactionError> {
    if output.status.success() {
        return Ok(output);
    }
    let detail = render_git_process_output(&output);
    if output_has_index_lock_contention(&output) {
        return Err(CommitTransactionError::RetryableIndexLock { phase, detail });
    }
    Err(CommitTransactionError::Fatal(anyhow::anyhow!(
        "git {phase} failed: {detail}"
    )))
}

pub fn stage_and_commit_once(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
    msg: &str,
) -> std::result::Result<Output, CommitTransactionError> {
    stage_and_commit_exact_paths_once(git_root, resolved, snapshot_content, &[], msg)
}

/// Commit the session document and its explicitly typed binary-owned side
/// effects from one private index.
///
/// `additional_paths` must never be populated from generic repository status:
/// callers identify the exact files their mutation owns (for example the
/// configured external done archive). This preserves unrelated staged and
/// working-tree changes while keeping one logical closeout atomic.
pub fn stage_and_commit_exact_paths_once(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
    additional_paths: &[PathBuf],
    msg: &str,
) -> std::result::Result<Output, CommitTransactionError> {
    let (blob_hash, rel_path) = staged_blob_for_commit(git_root, resolved, snapshot_content)?;
    let mut cacheinfos = vec![format!("100644,{blob_hash},{rel_path}")];
    for path in additional_paths {
        let (side_effect_hash, side_effect_rel_path) =
            staged_blob_for_commit(git_root, path, None)?;
        if side_effect_rel_path == rel_path {
            continue;
        }
        cacheinfos.push(format!("100644,{side_effect_hash},{side_effect_rel_path}"));
    }
    let base_output = Command::new("git")
        .current_dir(git_root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .map_err(|err| CommitTransactionError::Fatal(err.into()))?;
    let base = base_output.status.success().then(|| {
        String::from_utf8_lossy(&base_output.stdout)
            .trim()
            .to_string()
    });

    // Build the commit from a private index rooted at the exact observed HEAD.
    // The user's real index may contain unrelated staged work (or conflicts) and
    // must remain outside the agent-doc transaction.
    let temp_dir = tempfile::tempdir().map_err(|err| CommitTransactionError::Fatal(err.into()))?;
    let temp_index = temp_dir.path().join("index");
    let mut read_tree = Command::new("git");
    read_tree
        .current_dir(git_root)
        .env("GIT_INDEX_FILE", &temp_index)
        .arg("read-tree");
    if let Some(base) = base.as_deref() {
        read_tree.arg(base);
    } else {
        read_tree.arg("--empty");
    }
    git_output_or_transaction_error(
        read_tree
            .output()
            .map_err(|err| CommitTransactionError::Fatal(err.into()))?,
        "read-tree",
    )?;

    for cacheinfo in &cacheinfos {
        git_output_or_transaction_error(
            Command::new("git")
                .current_dir(git_root)
                .env("GIT_INDEX_FILE", &temp_index)
                .args(["update-index", "--add", "--cacheinfo", cacheinfo])
                .output()
                .map_err(|err| CommitTransactionError::Fatal(err.into()))?,
            "update-index",
        )?;
    }
    let tree_output = git_output_or_transaction_error(
        Command::new("git")
            .current_dir(git_root)
            .env("GIT_INDEX_FILE", &temp_index)
            .arg("write-tree")
            .output()
            .map_err(|err| CommitTransactionError::Fatal(err.into()))?,
        "write-tree",
    )?;
    let tree = String::from_utf8_lossy(&tree_output.stdout)
        .trim()
        .to_string();

    let mut commit_tree = Command::new("git");
    commit_tree
        .current_dir(git_root)
        .args(["commit-tree", &tree]);
    if let Some(base) = base.as_deref() {
        commit_tree.args(["-p", base]);
    }
    let commit_output = git_output_or_transaction_error(
        commit_tree
            .args(["-m", msg])
            .output()
            .map_err(|err| CommitTransactionError::Fatal(err.into()))?,
        "commit-tree",
    )?;
    let commit_oid = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let zero_oid = "0000000000000000000000000000000000000000";
    let expected = base.as_deref().unwrap_or(zero_oid);
    let update_ref = Command::new("git")
        .current_dir(git_root)
        .args(["update-ref", "HEAD", &commit_oid, expected])
        .output()
        .map_err(|err| CommitTransactionError::Fatal(err.into()))?;
    if !update_ref.status.success() {
        let detail = render_git_process_output(&update_ref);
        if detail.contains("expected") || detail.contains("cannot lock ref") {
            return Err(CommitTransactionError::RetryableHeadMoved { detail });
        }
        return git_output_or_transaction_error(update_ref, "update-ref");
    }

    // Advance only the typed transaction paths in the real index to their new
    // committed blobs. Every unrelated staged entry remains byte-for-byte intact.
    for cacheinfo in &cacheinfos {
        match crate::index::update_index_cacheinfo(git_root, cacheinfo) {
            Ok(output) if output.status.success() => {}
            Ok(output) => eprintln!(
                "[commit] warning: committed exact path but could not align its real-index entry: {}",
                render_git_process_output(&output)
            ),
            Err(err) => eprintln!(
                "[commit] warning: committed exact path but could not align its real-index entry: {err}"
            ),
        }
    }
    Ok(update_ref)
}

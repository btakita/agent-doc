use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::process::Output;

use agent_doc_document::commit_normalization::canonicalize_answered_prompt_prefixes;
use agent_doc_document::transient_markers::{strip_guard_markers, strip_head_markers};
use agent_doc_git::{
    output_has_index_lock_contention, parent_submodule_pointer_commit_message, relative_to_root,
    render_git_process_output,
};

/// Best-effort RAII guard for serializing commit transactions per git repo /
/// submodule. Contention is deliberately nonblocking; git index-lock retries
/// remain the hard safety net.
pub struct CommitLock {
    _file: File,
}

impl CommitLock {
    pub fn acquire(git_root: &Path) -> Option<Self> {
        let lock_path = crate::dirs::commit_lock_path_for_git_root(git_root)?;
        let scope = crate::dirs::commit_lock_scope_path(git_root)?;
        let lock_dir = lock_path.parent()?.to_path_buf();
        if let Err(e) = std::fs::create_dir_all(&lock_dir) {
            eprintln!(
                "[commit] commit-lock dir create failed: {} (proceeding unlocked)",
                e
            );
            return None;
        }
        let file = match OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
        {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "[commit] commit-lock open failed: {} (proceeding unlocked)",
                    e
                );
                return None;
            }
        };
        if let Err(e) = file.try_lock_exclusive() {
            eprintln!(
                "[commit] repo commit-lock contended for {}: {} (proceeding unlocked)",
                scope.display(),
                e
            );
            return None;
        }
        Some(Self { _file: file })
    }
}

impl Drop for CommitLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

/// After a successful commit inside a submodule, stage and partial-commit the
/// updated submodule pointer in the superproject. Uses an explicit pathspec on
/// the commit so any other staged files in the parent index are preserved.
pub fn update_parent_submodule_pointer(super_root: &Path, submodule_root: &Path, msg: &str) {
    let _commit_lock = CommitLock::acquire(super_root);
    let rel = match submodule_root.strip_prefix(super_root) {
        Ok(r) => r,
        Err(_) => {
            eprintln!("[commit] cannot compute submodule relative path; skipping pointer update");
            return;
        }
    };
    let rel_str = rel.to_string_lossy().to_string();

    let add = crate::index::add_path(super_root, rel);
    match add {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprintln!(
                "[commit] parent git add for submodule pointer failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            return;
        }
        Err(e) => {
            eprintln!("[commit] parent git add error: {}", e);
            return;
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
                return;
            }
            eprintln!(
                "[commit] parent submodule pointer commit failed: {}",
                stderr.trim()
            );
        }
        Err(e) => eprintln!("[commit] parent submodule pointer commit error: {}", e),
    }
}

#[derive(Debug)]
pub enum CommitTransactionError {
    RetryableIndexLock { phase: &'static str, detail: String },
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

fn git_add_force(
    git_root: &Path,
    resolved: &Path,
) -> std::result::Result<(), CommitTransactionError> {
    let rel_path = relative_to_root(resolved, git_root);
    let output =
        crate::index::add_force(git_root, &rel_path).map_err(CommitTransactionError::Fatal)?;
    if !output.status.success() {
        let detail = render_git_process_output(&output);
        if output_has_index_lock_contention(&output) {
            return Err(CommitTransactionError::RetryableIndexLock {
                phase: "git add",
                detail,
            });
        }
        return Err(CommitTransactionError::Fatal(anyhow::anyhow!(
            "git add failed: {}",
            detail
        )));
    }
    Ok(())
}

fn stage_snapshot_for_commit(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
) -> std::result::Result<(), CommitTransactionError> {
    let rel_path = relative_to_root(resolved, git_root);
    if git_path_is_ignored_untracked(git_root, &rel_path)? {
        return Err(CommitTransactionError::IgnoredPath {
            path: rel_path.to_string_lossy().into_owned(),
        });
    }

    if let Some(snap) = snapshot_content {
        let staged_content = strip_guard_markers(&strip_head_markers(
            &canonicalize_answered_prompt_prefixes(snap),
        ));
        if let Ok(hash) = crate::index::hash_object(git_root, &staged_content) {
            let cacheinfo = format!("100644,{},{}", hash, rel_path.to_string_lossy());
            let output = crate::index::update_index_cacheinfo(git_root, &cacheinfo)
                .map_err(CommitTransactionError::Fatal)?;
            if !output.status.success() {
                if output_has_index_lock_contention(&output) {
                    return Err(CommitTransactionError::RetryableIndexLock {
                        phase: "update-index",
                        detail: render_git_process_output(&output),
                    });
                }
                eprintln!("[commit] update-index failed, falling back to git add");
                return git_add_force(git_root, resolved);
            }
            return Ok(());
        }
    }

    git_add_force(git_root, resolved)
}

pub fn stage_and_commit_once(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
    msg: &str,
) -> std::result::Result<Output, CommitTransactionError> {
    stage_snapshot_for_commit(git_root, resolved, snapshot_content)?;

    // Capture stdout so callers that reserve stdout for JSON are not polluted.
    let output =
        crate::commit::commit_no_verify(git_root, msg).map_err(CommitTransactionError::Fatal)?;
    if !output.status.success() && output_has_index_lock_contention(&output) {
        return Err(CommitTransactionError::RetryableIndexLock {
            phase: "git commit",
            detail: render_git_process_output(&output),
        });
    }
    Ok(output)
}

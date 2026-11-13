use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agent_doc_document::commit_normalization::canonicalize_answered_prompt_prefixes;
use agent_doc_document::transient_markers::{strip_guard_markers, strip_head_markers};
use agent_doc_git::{
    output_has_index_lock_contention, parent_submodule_pointer_commit_message, relative_to_root,
    render_git_process_output,
};
use anyhow::Context;

/// After a successful commit inside a submodule, stage and partial-commit the
/// updated submodule pointer in the superproject. Uses an explicit pathspec on
/// the commit so any other staged files in the parent index are preserved.
pub fn update_parent_submodule_pointer(
    super_root: &Path,
    submodule_root: &Path,
    msg: &str,
) -> anyhow::Result<()> {
    update_parent_submodule_pointer_inner(super_root, submodule_root, msg, None, None).with_context(
        || {
            "parent submodule pointer is not committed; run `agent-doc commit` to retry the idempotent parent-pointer closeout"
        },
    )
}

pub fn update_parent_submodule_pointer_with_fence(
    super_root: &Path,
    submodule_root: &Path,
    msg: &str,
    fence_target: &Path,
    fence: &dyn CommitTransactionFence,
) -> anyhow::Result<()> {
    update_parent_submodule_pointer_inner(
        super_root,
        submodule_root,
        msg,
        Some(fence_target),
        Some(fence),
    )
    .with_context(|| {
        "parent submodule pointer is not committed; run `agent-doc commit` to retry the idempotent parent-pointer closeout"
    })
}

fn update_parent_submodule_pointer_inner(
    super_root: &Path,
    submodule_root: &Path,
    msg: &str,
    fence_target: Option<&Path>,
    fence: Option<&dyn CommitTransactionFence>,
) -> anyhow::Result<()> {
    let rel = match submodule_root.strip_prefix(super_root) {
        Ok(r) => r,
        Err(_) => anyhow::bail!(
            "parent submodule pointer is not committed: cannot compute submodule relative path. Run `agent-doc commit` to retry the idempotent parent-pointer closeout."
        ),
    };
    let rel_str = rel.to_string_lossy().to_string();

    let parent_msg = parent_submodule_pointer_commit_message(msg);
    let submodule_oid = command_stdout(
        Command::new("git")
            .current_dir(submodule_root)
            .args(["rev-parse", "HEAD"]),
        "resolve committed submodule HEAD",
    )?;
    if let (Some(fence), Some(target)) = (fence, fence_target) {
        fence.ensure(target, CommitFencePhase::BeforeStage)?;
    }
    let add = crate::index::add_path(super_root, rel)?;
    anyhow::ensure!(
        add.status.success(),
        "git add {rel_str} failed: {}",
        String::from_utf8_lossy(&add.stderr).trim(),
    );
    let temp = tempfile::tempdir()?;
    let temp_index = temp.path().join("parent-index");
    let mut committed = false;
    for attempt in 0..3 {
        let expected = command_stdout(
            Command::new("git")
                .current_dir(super_root)
                .args(["rev-parse", "HEAD"]),
            "resolve parent HEAD",
        )?;
        command_success(
            Command::new("git")
                .current_dir(super_root)
                .env("GIT_INDEX_FILE", &temp_index)
                .args(["read-tree", "HEAD"]),
            "seed private parent index",
        )?;
        command_success(
            Command::new("git")
                .current_dir(super_root)
                .env("GIT_INDEX_FILE", &temp_index)
                .args([
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("160000,{submodule_oid},{rel_str}"),
                ]),
            "stage submodule pointer in private parent index",
        )?;
        let tree = command_stdout(
            Command::new("git")
                .current_dir(super_root)
                .env("GIT_INDEX_FILE", &temp_index)
                .args(["write-tree"]),
            "write private parent tree",
        )?;
        let head_tree = command_stdout(
            Command::new("git")
                .current_dir(super_root)
                .args(["rev-parse", "HEAD^{tree}"]),
            "resolve parent HEAD tree",
        )?;
        if tree == head_tree {
            committed = true;
            break;
        }
        let commit_oid = commit_tree(super_root, &tree, &expected, &parent_msg)?;
        if let (Some(fence), Some(target)) = (fence, fence_target) {
            fence.ensure(target, CommitFencePhase::BeforeParentRefUpdate)?;
        }
        let update = Command::new("git")
            .current_dir(super_root)
            .args(["update-ref", "HEAD", &commit_oid, &expected])
            .output()?;
        if update.status.success() {
            if let (Some(fence), Some(target)) = (fence, fence_target) {
                fence.ref_updated(target);
            }
            committed = true;
            break;
        }
        if attempt == 2 {
            anyhow::bail!(
                "parent submodule pointer is not committed: parent HEAD moved during three fenced ref-CAS attempts: {}",
                String::from_utf8_lossy(&update.stderr).trim(),
            );
        }
    }
    anyhow::ensure!(
        committed,
        "parent submodule pointer transaction did not settle"
    );
    eprintln!("[commit] parent submodule pointer updated for {}", rel_str);
    Ok(())
}

fn command_stdout(command: &mut Command, action: &str) -> anyhow::Result<String> {
    let output = command.output()?;
    anyhow::ensure!(
        output.status.success(),
        "{action} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim(),
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn command_success(command: &mut Command, action: &str) -> anyhow::Result<()> {
    let output = command.output()?;
    anyhow::ensure!(
        output.status.success(),
        "{action} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim(),
    );
    Ok(())
}

fn commit_tree(repo: &Path, tree: &str, parent: &str, message: &str) -> anyhow::Result<String> {
    let mut child = Command::new("git")
        .current_dir(repo)
        .args(["commit-tree", tree, "-p", parent])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .context("open git commit-tree stdin")?
        .write_all(message.as_bytes())?;
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "git commit-tree failed: {}",
        String::from_utf8_lossy(&output.stderr).trim(),
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Debug)]
pub enum CommitTransactionError {
    RetryableIndexLock { phase: &'static str, detail: String },
    RetryableHeadMoved { detail: String },
    IgnoredPath { path: String },
    Fatal(anyhow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitFencePhase {
    BeforeStage,
    BeforeRefUpdate,
    BeforeParentRefUpdate,
    BeforeSiblingRefUpdate,
}

/// Borrowed authority checked at the actual private-index transaction
/// boundaries. Implementations are minted by the owning coordination layer;
/// git-io only invokes the proof and never reconstructs it from scalar ids.
pub trait CommitTransactionFence {
    fn ensure(&self, target: &Path, phase: CommitFencePhase) -> anyhow::Result<()>;
    fn ref_updated(&self, target: &Path);
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
    stage_and_commit_exact_paths_once_with_fence(
        git_root,
        resolved,
        snapshot_content,
        additional_paths,
        msg,
        None,
    )
}

pub fn stage_and_commit_exact_paths_once_with_fence(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
    additional_paths: &[PathBuf],
    msg: &str,
    fence: Option<&dyn CommitTransactionFence>,
) -> std::result::Result<Output, CommitTransactionError> {
    if let Some(fence) = fence {
        fence
            .ensure(resolved, CommitFencePhase::BeforeStage)
            .map_err(CommitTransactionError::Fatal)?;
    }
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
    if let Some(fence) = fence {
        fence
            .ensure(resolved, CommitFencePhase::BeforeRefUpdate)
            .map_err(CommitTransactionError::Fatal)?;
    }
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
    if let Some(fence) = fence {
        fence.ref_updated(resolved);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RejectParentRefFence {
        parent_checks: AtomicUsize,
        ref_updates: AtomicUsize,
    }

    impl CommitTransactionFence for RejectParentRefFence {
        fn ensure(&self, _target: &Path, phase: CommitFencePhase) -> anyhow::Result<()> {
            if phase == CommitFencePhase::BeforeParentRefUpdate {
                self.parent_checks.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("simulated closeout owner loss at parent ref CAS");
            }
            Ok(())
        }

        fn ref_updated(&self, _target: &Path) {
            self.ref_updates.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn init_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        command_success(
            Command::new("git")
                .current_dir(path)
                .args(["init", "-q", "--initial-branch=main"]),
            "init test repo",
        )
        .unwrap();
        command_success(
            Command::new("git").current_dir(path).args([
                "config",
                "user.email",
                "test@example.com",
            ]),
            "configure test email",
        )
        .unwrap();
        command_success(
            Command::new("git")
                .current_dir(path)
                .args(["config", "user.name", "Test"]),
            "configure test name",
        )
        .unwrap();
    }

    fn commit_all(repo: &Path, message: &str) {
        command_success(
            Command::new("git").current_dir(repo).args(["add", "-A"]),
            "stage test repo",
        )
        .unwrap();
        command_success(
            Command::new("git")
                .current_dir(repo)
                .args(["commit", "-q", "-m", message]),
            "commit test repo",
        )
        .unwrap();
    }

    #[test]
    fn fenced_parent_submodule_rejects_loss_after_tree_before_ref_cas() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("parent");
        let child_source = tmp.path().join("child-source");
        init_repo(&parent);
        init_repo(&child_source);
        std::fs::write(parent.join("README.md"), "parent\n").unwrap();
        commit_all(&parent, "parent initial");
        std::fs::write(child_source.join("child.txt"), "one\n").unwrap();
        commit_all(&child_source, "child initial");
        command_success(
            Command::new("git").current_dir(&parent).args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                child_source.to_str().unwrap(),
                "child",
            ]),
            "add test submodule",
        )
        .unwrap();
        commit_all(&parent, "add child");
        let child = parent.join("child");
        command_success(
            Command::new("git").current_dir(&child).args([
                "config",
                "user.email",
                "test@example.com",
            ]),
            "configure submodule email",
        )
        .unwrap();
        command_success(
            Command::new("git")
                .current_dir(&child)
                .args(["config", "user.name", "Test"]),
            "configure submodule name",
        )
        .unwrap();
        std::fs::write(child.join("child.txt"), "two\n").unwrap();
        commit_all(&child, "child update");
        let parent_before = command_stdout(
            Command::new("git")
                .current_dir(&parent)
                .args(["rev-parse", "HEAD"]),
            "read parent before",
        )
        .unwrap();
        let fence = RejectParentRefFence {
            parent_checks: AtomicUsize::new(0),
            ref_updates: AtomicUsize::new(0),
        };
        let session_doc = child.join("child.txt");
        let result = update_parent_submodule_pointer_with_fence(
            &parent,
            &child,
            "child closeout",
            &session_doc,
            &fence,
        );
        assert!(result.is_err());
        assert_eq!(fence.parent_checks.load(Ordering::SeqCst), 1);
        assert_eq!(fence.ref_updates.load(Ordering::SeqCst), 0);
        assert_eq!(
            command_stdout(
                Command::new("git")
                    .current_dir(&parent)
                    .args(["rev-parse", "HEAD"]),
                "read parent after rejection",
            )
            .unwrap(),
            parent_before
        );
        update_parent_submodule_pointer(&parent, &child, "child closeout").unwrap();
        assert_ne!(
            command_stdout(
                Command::new("git")
                    .current_dir(&parent)
                    .args(["rev-parse", "HEAD"]),
                "read parent successor",
            )
            .unwrap(),
            parent_before
        );
    }
}

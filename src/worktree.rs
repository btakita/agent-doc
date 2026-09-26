//! # Module: worktree
//!
//! ## Spec
//! - `create(project_root, session_id, index)` creates a git worktree for a parallel
//!   deep task. Path: `.agent-doc/worktrees/<session_short>-<index>`. Branch:
//!   `deep/<session_short>/<index>`. `session_short` = first 8 characters of `session_id`.
//! - `diff(worktree_path)` returns the unified diff of the worktree against HEAD
//!   (`git diff HEAD`) as a UTF-8 string. Empty string when no staged or unstaged changes.
//! - `remove(project_root, worktree_path, branch)` removes the worktree directory
//!   (`git worktree remove --force`) and deletes the associated branch (`git branch -D`).
//! - `create()` records the worktree's `base_commit`, so disposal can tell a branch
//!   that moved from one that never did. `list_session()` cannot know it and sets `None`.
//! - `residue(worktree_path, base_commit)` measures what a task left: tracked changes,
//!   untracked paths, and commits since the base — one `git status --porcelain` call
//!   plus `git rev-list --count` when the base is known.
//! - `classify_worktree_residue(residue)` is the pure disposal rule (`#orchworktreeleak`):
//!   `Remove` when nothing but `SCAFFOLDING_FILENAMES` is present, `Retain { reason }`
//!   naming every kind of authored work found otherwise.
//! - `cleanup_session(project_root, session_id)` removes all worktrees belonging to
//!   a session by listing via `list_session()` then calling `remove()` on each.
//! - `list_session(project_root, session_id)` scans `.agent-doc/worktrees/` for
//!   directories matching the session prefix and parses their numeric index suffix.
//!   Returns an empty vec when the worktree directory does not exist.
//! - All git operations use `std::process::Command` (no `git2` dependency).
//! - Worktree parent directories are created with `create_dir_all` before `git worktree add`.
//! - `session_short()` truncates to a maximum of 8 characters; sessions shorter than 8
//!   characters use their full length.
//!
//! ## Agentic Contracts
//! - `create()`, `diff()`, `remove()`, `cleanup_session()`, and `list_session()` are
//!   the public API; all naming helpers are private.
//! - `create()` always checks out from `HEAD` at the time of creation; the caller is
//!   responsible for branching strategy.
//! - `diff()` returns an empty string (not an error) when the worktree is clean.
//! - `remove()` IS called by the parallel task lifecycle — `parallel::run` disposes
//!   of each worktree after result collection, and unwinds the ones it created when
//!   a fan-out aborts mid-spawn. `cleanup_session()` and `list_session()` remain
//!   unwired and keep `#[allow(dead_code)]`.
//! - Errors from git subprocesses propagate as `anyhow::Error` with context messages.
//!
//! ## Evals
//! - create_worktree: valid git repo → worktree dir exists, branch = "deep/abcdefgh/0", README.md present
//! - create_multiple_worktrees: two creates for same session → distinct paths and branches
//! - diff_empty_when_no_changes: fresh worktree with no edits → empty diff string
//! - diff_shows_changes: staged new file → diff contains file content
//! - remove_worktree: created worktree → dir gone, branch deleted after remove
//! - a_worktree_holding_only_scaffolding_is_removed / an_empty_worktree_is_removed
//! - tracked_changes_retain_the_worktree / commits_on_the_branch_retain_the_worktree
//! - an_authored_untracked_file_retains_the_worktree_beside_scaffolding: scaffolding is
//!   never the stated reason
//! - residue_reads_a_fresh_worktree_as_scaffolding_only
//! - residue_sees_a_tracked_edit_and_a_commit_the_task_made: a committed-only worktree
//!   still retains, since its diff is empty
//! - cleanup_session_removes_all: two worktrees for same session → both gone after cleanup
//! - list_session_finds_matching: session A has 2, session B has 1 → counts correct, no cross-contamination
//! - list_session_empty_when_no_dir: no `.agent-doc/worktrees/` dir → empty vec, no error
//! - session_short_truncates: 16-char input → 8 chars; 5-char input → 5 chars unchanged

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKTREE_DIR: &str = ".agent-doc/worktrees";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    /// Commit the worktree branched from, when known.
    ///
    /// `create` records it so disposal can tell "the task committed work here"
    /// from "the branch is still exactly where it started". `list_session`
    /// rebuilds names from the directory layout and cannot know it, so it is
    /// `None` there and commits are simply not counted.
    pub base_commit: Option<String>,
}

/// Files orchestrate itself writes into every worktree.
///
/// They exist in every run, finished or not, so their presence alone never
/// means the task produced work worth keeping (`#orchworktreeleak`).
pub const SCAFFOLDING_FILENAMES: &[&str] = &[
    ".agent-doc-prompt.txt",
    ".agent-doc-result.json",
    ".agent-doc-result.log",
];

/// What a finished task actually left behind in its worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeResidue {
    /// Tracked files differ from the index/HEAD.
    pub tracked_changes: bool,
    /// Untracked paths, scaffolding included — the classifier filters them.
    pub untracked_paths: Vec<String>,
    /// Commits made on the branch since `base_commit`.
    pub commits_ahead: usize,
}

/// Whether a finished task's worktree may be removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeDisposition {
    Remove,
    /// Keep it, and say exactly what would have been destroyed.
    Retain { reason: String },
}

/// Decide a finished task's worktree disposition (`#orchworktreeleak`).
///
/// Pure, so the rule is testable without a git repo. Removal is the default:
/// a worktree holding only orchestrate's own scaffolding is scratch space, and
/// leaving it behind accumulates one directory and one branch per task per run.
/// Anything the task authored — a tracked edit, a non-scaffolding untracked
/// file, or a commit on the branch — makes removal destructive, so the worktree
/// stays and the caller reports why.
pub fn classify_worktree_residue(residue: &WorktreeResidue) -> WorktreeDisposition {
    let mut reasons = Vec::new();
    if residue.tracked_changes {
        reasons.push("uncommitted tracked changes".to_string());
    }
    let authored = authored_untracked_paths(&residue.untracked_paths);
    if !authored.is_empty() {
        reasons.push(format!("untracked file(s) {}", authored.join(", ")));
    }
    if residue.commits_ahead > 0 {
        reasons.push(format!("{} commit(s) on its branch", residue.commits_ahead));
    }

    if reasons.is_empty() {
        WorktreeDisposition::Remove
    } else {
        WorktreeDisposition::Retain {
            reason: reasons.join(" and "),
        }
    }
}

/// Untracked paths the task authored — everything but orchestrate scaffolding.
fn authored_untracked_paths(untracked: &[String]) -> Vec<&str> {
    untracked
        .iter()
        .map(String::as_str)
        .filter(|path| !SCAFFOLDING_FILENAMES.contains(path))
        .collect()
}

/// Truncate session_id to first 8 characters for directory/branch naming.
fn session_short(session_id: &str) -> &str {
    &session_id[..session_id.len().min(8)]
}

/// Build the worktree directory name for a given session and index.
fn worktree_name(session_id: &str, index: usize) -> String {
    format!("{}-{}", session_short(session_id), index)
}

/// Build the branch name for a given session and index.
fn branch_name(session_id: &str, index: usize) -> String {
    format!("deep/{}/{}", session_short(session_id), index)
}

/// Create a worktree for a deep task.
/// Path: .agent-doc/worktrees/<session_short>-<index>
/// Branch: deep/<session_short>/<index>
/// session_short = first 8 chars of session_id
pub fn create(project_root: &Path, session_id: &str, index: usize) -> Result<WorktreeInfo> {
    let name = worktree_name(session_id, index);
    let branch = branch_name(session_id, index);
    let wt_path = project_root.join(WORKTREE_DIR).join(&name);

    // Ensure parent directory exists
    if let Some(parent) = wt_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create worktree parent dir {}", parent.display())
        })?;
    }

    let output = Command::new("git")
        .current_dir(project_root)
        .args([
            "worktree",
            "add",
            &wt_path.to_string_lossy(),
            "-b",
            &branch,
            "HEAD",
        ])
        .output()
        .context("failed to spawn git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree add failed: {}", stderr.trim());
    }

    let base_commit = head_commit(&wt_path).ok();

    Ok(WorktreeInfo {
        path: wt_path,
        branch,
        base_commit,
    })
}

/// Resolved `HEAD` commit of a worktree.
fn head_commit(worktree_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(worktree_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("failed to spawn git rev-parse HEAD")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git rev-parse HEAD failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Measure what a task left in its worktree (`#orchworktreeleak`).
///
/// One `git status --porcelain` call answers both tracked and untracked state;
/// commits are counted only when the base commit is known.
pub fn residue(worktree_path: &Path, base_commit: Option<&str>) -> Result<WorktreeResidue> {
    let output = Command::new("git")
        .current_dir(worktree_path)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .context("failed to spawn git status --porcelain")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git status --porcelain failed: {}", stderr.trim());
    }

    let mut tracked_changes = false;
    let mut untracked_paths = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.trim().is_empty() {
            continue;
        }
        match line.strip_prefix("?? ") {
            Some(path) => untracked_paths.push(path.trim().to_string()),
            None => tracked_changes = true,
        }
    }

    let commits_ahead = match base_commit {
        Some(base) => commits_since(worktree_path, base).unwrap_or(0),
        None => 0,
    };

    Ok(WorktreeResidue {
        tracked_changes,
        untracked_paths,
        commits_ahead,
    })
}

/// Commits on the worktree's current branch since `base`.
fn commits_since(worktree_path: &Path, base: &str) -> Result<usize> {
    let output = Command::new("git")
        .current_dir(worktree_path)
        .args(["rev-list", "--count", &format!("{base}..HEAD")])
        .output()
        .context("failed to spawn git rev-list --count")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git rev-list --count failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0))
}

/// Get the unified diff of a worktree against HEAD (its branch point).
pub fn diff(worktree_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(worktree_path)
        .args(["diff", "HEAD"])
        .output()
        .context("failed to spawn git diff")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git diff failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Remove a single worktree and delete its branch.
pub fn remove(project_root: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .current_dir(project_root)
        .args([
            "worktree",
            "remove",
            "--force",
            &worktree_path.to_string_lossy(),
        ])
        .output()
        .context("failed to spawn git worktree remove")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree remove failed: {}", stderr.trim());
    }

    let output = Command::new("git")
        .current_dir(project_root)
        .args(["branch", "-D", branch])
        .output()
        .context("failed to spawn git branch -D")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git branch -D {} failed: {}", branch, stderr.trim());
    }

    Ok(())
}

/// Remove all worktrees for a session.
#[allow(dead_code)]
pub fn cleanup_session(project_root: &Path, session_id: &str) -> Result<()> {
    let worktrees = list_session(project_root, session_id)?;
    for wt in worktrees {
        remove(project_root, &wt.path, &wt.branch)?;
    }
    Ok(())
}

/// List active worktrees for a session.
#[allow(dead_code)]
pub fn list_session(project_root: &Path, session_id: &str) -> Result<Vec<WorktreeInfo>> {
    let prefix = session_short(session_id);
    let wt_dir = project_root.join(WORKTREE_DIR);

    if !wt_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let entries = std::fs::read_dir(&wt_dir)
        .with_context(|| format!("failed to read worktree dir {}", wt_dir.display()))?;

    for entry in entries {
        let entry = entry.context("failed to read worktree dir entry")?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !name_str.starts_with(prefix) {
            continue;
        }

        // Parse index from name: "<prefix>-<index>"
        let suffix = &name_str[prefix.len()..];
        if !suffix.starts_with('-') {
            continue;
        }
        let index_str = &suffix[1..];
        let index: usize = match index_str.parse() {
            Ok(i) => i,
            Err(_) => continue,
        };

        let path = wt_dir.join(&*name_str);
        if !path.is_dir() {
            continue;
        }

        results.push(WorktreeInfo {
            path,
            branch: branch_name(session_id, index),
            base_commit: None,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Set up an isolated git repo with an initial commit.
    fn setup_git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        // Create an initial commit so HEAD exists
        let readme = root.join("README.md");
        fs::write(&readme, "# Test repo\n").unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial commit", "--no-verify"])
            .output()
            .unwrap();

        dir
    }

    #[test]
    fn create_worktree() {
        let dir = setup_git_repo();
        let root = dir.path();

        let info = create(root, "abcdefghij", 0).unwrap();

        assert!(info.path.is_dir(), "worktree directory should exist");
        assert_eq!(info.branch, "deep/abcdefgh/0");
        assert!(info.path.ends_with("abcdefgh-0"));

        // The worktree should contain the repo files
        assert!(info.path.join("README.md").exists());
    }

    #[test]
    fn create_multiple_worktrees() {
        let dir = setup_git_repo();
        let root = dir.path();

        let wt0 = create(root, "sess1234xxxx", 0).unwrap();
        let wt1 = create(root, "sess1234xxxx", 1).unwrap();

        assert_ne!(wt0.path, wt1.path);
        assert_ne!(wt0.branch, wt1.branch);
        assert!(wt0.path.is_dir());
        assert!(wt1.path.is_dir());
    }

    #[test]
    fn diff_empty_when_no_changes() {
        let dir = setup_git_repo();
        let root = dir.path();

        let info = create(root, "difftest1", 0).unwrap();
        let d = diff(&info.path).unwrap();

        assert!(d.is_empty(), "diff should be empty with no changes");
    }

    #[test]
    fn diff_shows_changes() {
        let dir = setup_git_repo();
        let root = dir.path();

        let info = create(root, "difftest2", 0).unwrap();

        // Make a change in the worktree
        let file = info.path.join("new_file.txt");
        fs::write(&file, "hello world\n").unwrap();

        // Stage the file so git diff HEAD shows it
        Command::new("git")
            .current_dir(&info.path)
            .args(["add", "new_file.txt"])
            .output()
            .unwrap();

        let d = diff(&info.path).unwrap();
        assert!(
            d.contains("hello world"),
            "diff should contain the new content"
        );
    }

    // `#orchworktreeleak` — the disposal rule, pure.

    #[test]
    fn a_worktree_holding_only_scaffolding_is_removed() {
        let residue = WorktreeResidue {
            tracked_changes: false,
            untracked_paths: SCAFFOLDING_FILENAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            commits_ahead: 0,
        };

        assert_eq!(
            classify_worktree_residue(&residue),
            WorktreeDisposition::Remove
        );
    }

    #[test]
    fn an_empty_worktree_is_removed() {
        assert_eq!(
            classify_worktree_residue(&WorktreeResidue::default()),
            WorktreeDisposition::Remove
        );
    }

    #[test]
    fn tracked_changes_retain_the_worktree() {
        let residue = WorktreeResidue {
            tracked_changes: true,
            ..Default::default()
        };

        let WorktreeDisposition::Retain { reason } = classify_worktree_residue(&residue) else {
            panic!("tracked changes must retain");
        };
        assert!(reason.contains("uncommitted tracked changes"), "{reason}");
    }

    #[test]
    fn an_authored_untracked_file_retains_the_worktree_beside_scaffolding() {
        let residue = WorktreeResidue {
            untracked_paths: vec![
                ".agent-doc-prompt.txt".to_string(),
                ".agent-doc-result.json".to_string(),
                "src/new_module.rs".to_string(),
            ],
            ..Default::default()
        };

        let WorktreeDisposition::Retain { reason } = classify_worktree_residue(&residue) else {
            panic!("an authored untracked file must retain");
        };
        assert!(reason.contains("src/new_module.rs"), "{reason}");
        assert!(
            !reason.contains(".agent-doc-prompt.txt"),
            "scaffolding must not be reported as the reason: {reason}"
        );
    }

    #[test]
    fn commits_on_the_branch_retain_the_worktree() {
        let residue = WorktreeResidue {
            commits_ahead: 2,
            ..Default::default()
        };

        let WorktreeDisposition::Retain { reason } = classify_worktree_residue(&residue) else {
            panic!("commits must retain");
        };
        assert!(reason.contains("2 commit(s)"), "{reason}");
    }

    #[test]
    fn residue_reads_a_fresh_worktree_as_scaffolding_only() {
        let dir = setup_git_repo();
        let root = dir.path();
        let info = create(root, "abcdefgh12345678", 0).unwrap();
        for name in SCAFFOLDING_FILENAMES {
            fs::write(info.path.join(name), "scratch\n").unwrap();
        }

        let residue = residue(&info.path, info.base_commit.as_deref()).unwrap();

        assert!(!residue.tracked_changes);
        assert_eq!(residue.commits_ahead, 0);
        assert_eq!(
            classify_worktree_residue(&residue),
            WorktreeDisposition::Remove
        );
    }

    #[test]
    fn residue_sees_a_tracked_edit_and_a_commit_the_task_made() {
        let dir = setup_git_repo();
        let root = dir.path();
        let info = create(root, "abcdefgh12345678", 1).unwrap();
        assert!(info.base_commit.is_some(), "create must record the base");

        fs::write(info.path.join("README.md"), "# edited by the task\n").unwrap();
        let residue_edit = residue(&info.path, info.base_commit.as_deref()).unwrap();
        assert!(residue_edit.tracked_changes);
        assert!(matches!(
            classify_worktree_residue(&residue_edit),
            WorktreeDisposition::Retain { .. }
        ));

        Command::new("git")
            .current_dir(&info.path)
            .args(["commit", "-am", "task work", "--no-verify"])
            .output()
            .unwrap();
        let residue_commit = residue(&info.path, info.base_commit.as_deref()).unwrap();
        assert!(!residue_commit.tracked_changes, "commit cleared the diff");
        assert_eq!(residue_commit.commits_ahead, 1);
        assert!(
            matches!(
                classify_worktree_residue(&residue_commit),
                WorktreeDisposition::Retain { .. }
            ),
            "a committed-only worktree must still be kept"
        );
    }

    #[test]
    fn remove_worktree() {
        let dir = setup_git_repo();
        let root = dir.path();

        let info = create(root, "rmtest12", 0).unwrap();
        let wt_path = info.path.clone();
        let branch = info.branch.clone();

        assert!(wt_path.is_dir());
        remove(root, &wt_path, &branch).unwrap();
        assert!(!wt_path.exists(), "worktree directory should be removed");

        // Branch should be deleted too
        let output = Command::new("git")
            .current_dir(root)
            .args(["branch", "--list", &branch])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.trim().is_empty(), "branch should be deleted");
    }

    #[test]
    fn cleanup_session_removes_all() {
        let dir = setup_git_repo();
        let root = dir.path();
        let session = "cleanup1xxxx";

        let wt0 = create(root, session, 0).unwrap();
        let wt1 = create(root, session, 1).unwrap();

        assert!(wt0.path.is_dir());
        assert!(wt1.path.is_dir());

        cleanup_session(root, session).unwrap();

        assert!(!wt0.path.exists(), "worktree 0 should be removed");
        assert!(!wt1.path.exists(), "worktree 1 should be removed");
    }

    #[test]
    fn list_session_finds_matching() {
        let dir = setup_git_repo();
        let root = dir.path();
        let session_a = "listAAAAxxxx";
        let session_b = "listBBBBxxxx";

        create(root, session_a, 0).unwrap();
        create(root, session_a, 1).unwrap();
        create(root, session_b, 0).unwrap();

        let list_a = list_session(root, session_a).unwrap();
        assert_eq!(list_a.len(), 2, "should find 2 worktrees for session A");

        let list_b = list_session(root, session_b).unwrap();
        assert_eq!(list_b.len(), 1, "should find 1 worktree for session B");
    }

    #[test]
    fn list_session_empty_when_no_dir() {
        let dir = TempDir::new().unwrap();
        let result = list_session(dir.path(), "nonexist").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn session_short_truncates() {
        assert_eq!(session_short("abcdefghijklmnop"), "abcdefgh");
        assert_eq!(session_short("short"), "short");
        assert_eq!(session_short("12345678"), "12345678");
    }
}

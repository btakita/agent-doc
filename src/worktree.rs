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
//! - `remove()` and `cleanup_session()` are marked `#[allow(dead_code)]`; they are
//!   used by the parallel task lifecycle but not yet wired to a subcommand.
//! - Errors from git subprocesses propagate as `anyhow::Error` with context messages.
//!
//! ## Evals
//! - create_worktree: valid git repo → worktree dir exists, branch = "deep/abcdefgh/0", README.md present
//! - create_multiple_worktrees: two creates for same session → distinct paths and branches
//! - diff_empty_when_no_changes: fresh worktree with no edits → empty diff string
//! - diff_shows_changes: staged new file → diff contains file content
//! - remove_worktree: created worktree → dir gone, branch deleted after remove
//! - cleanup_session_removes_all: two worktrees for same session → both gone after cleanup
//! - list_session_finds_matching: session A has 2, session B has 1 → counts correct, no cross-contamination
//! - list_session_empty_when_no_dir: no `.agent-doc/worktrees/` dir → empty vec, no error
//! - session_short_truncates: 16-char input → 8 chars; 5-char input → 5 chars unchanged

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKTREE_DIR: &str = ".agent-doc/worktrees";

pub struct WorktreeInfo {
    pub path: PathBuf,
    #[allow(dead_code)]
    pub branch: String,
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

    Ok(WorktreeInfo {
        path: wt_path,
        branch,
    })
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
#[allow(dead_code)]
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

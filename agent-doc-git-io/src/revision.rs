use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::dirs::{narrow_to_submodule, resolve_to_git_root};

/// Get the content of `file` from `rev`.
///
/// Returns `None` if the path is not tracked at that revision or the revision
/// does not exist. The owning git root is narrowed to the submodule root when
/// `file` lives inside a submodule.
pub fn show_rev(file: &Path, rev: &str) -> Result<Option<String>> {
    let (super_root, resolved) = resolve_to_git_root(file)?;
    let (git_root, _) = narrow_to_submodule(&super_root, &resolved);
    let rel_path = relative_path_for_git_show(&resolved, &git_root);

    let output = Command::new("git")
        .current_dir(&git_root)
        .args(["show", &format!("{rev}:{}", rel_path.to_string_lossy())])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}

pub fn show_head(file: &Path) -> Result<Option<String>> {
    show_rev(file, "HEAD")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadWorktreeFallback {
    NoHead,
    MatchesCurrent,
    DiffersFromCurrent(String),
}

/// Return HEAD content only when it is a useful recovery baseline.
///
/// A missing per-document snapshot can happen on a first submit after a commit
/// or when state was deleted. HEAD is only useful for recovery when the
/// committed content differs from the current worktree content.
pub fn head_fallback_when_differs_from_worktree(file: &Path) -> Result<HeadWorktreeFallback> {
    if last_commit_mtime(file).unwrap_or(None).is_none() {
        return Ok(HeadWorktreeFallback::NoHead);
    }

    let Some(head_content) = show_head(file)? else {
        return Ok(HeadWorktreeFallback::NoHead);
    };
    let current = std::fs::read_to_string(file).unwrap_or_default();
    if head_content == current {
        Ok(HeadWorktreeFallback::MatchesCurrent)
    } else {
        Ok(HeadWorktreeFallback::DiffersFromCurrent(head_content))
    }
}

pub fn rev_parse(repo: &Path, rev: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", rev])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecentCommitLog {
    Lines(Vec<String>),
    Empty,
    GitUnavailable,
    LogFailed,
}

/// Return recent `git log --oneline` entries for `file`.
pub fn recent_commit_lines(
    file: &Path,
    since: Option<std::time::SystemTime>,
    limit: usize,
) -> RecentCommitLog {
    let since_arg = since.and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| format!("--since={}", d.as_secs()))
    });

    let (git_root, resolved) = match resolve_to_git_root(file) {
        Ok(pair) => pair,
        Err(_) => return RecentCommitLog::GitUnavailable,
    };
    let rel_path = relative_path_for_git_show(&resolved, &git_root);

    let mut args = vec![
        "log".to_string(),
        "--oneline".to_string(),
        format!("-{}", limit),
    ];
    if let Some(since_arg) = since_arg {
        args.push(since_arg);
    }
    args.push("--".to_string());
    args.push(rel_path.to_string_lossy().to_string());

    let output = Command::new("git")
        .current_dir(&git_root)
        .args(&args)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let lines = String::from_utf8_lossy(&out.stdout)
                .lines()
                .take(limit)
                .map(str::to_string)
                .collect::<Vec<_>>();
            if lines.is_empty() {
                RecentCommitLog::Empty
            } else {
                RecentCommitLog::Lines(lines)
            }
        }
        _ => RecentCommitLog::LogFailed,
    }
}

/// Get the author timestamp of the last commit touching `file`.
///
/// Returns `None` if the path has no commits in the resolved git root.
pub fn last_commit_mtime(file: &Path) -> Result<Option<std::time::SystemTime>> {
    let (git_root, resolved) = resolve_to_git_root(file)?;
    let rel_path = relative_path_for_git_show(&resolved, &git_root);

    let output = Command::new("git")
        .current_dir(&git_root)
        .args([
            "log",
            "-1",
            "--format=%ct",
            "--",
            &rel_path.to_string_lossy(),
        ])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let ts_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ts_str.is_empty() {
        return Ok(None);
    }

    let epoch: u64 = ts_str.parse().unwrap_or(0);
    if epoch == 0 {
        return Ok(None);
    }

    Ok(Some(
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(epoch),
    ))
}

/// Get the full hash of the last commit touching `file`.
///
/// Returns `None` if the path has no commits in the resolved git root.
pub fn last_commit_hash(file: &Path) -> Result<Option<String>> {
    let (git_root, resolved) = resolve_to_git_root(file)?;
    let rel_path = relative_path_for_git_show(&resolved, &git_root);

    let output = Command::new("git")
        .current_dir(&git_root)
        .args([
            "log",
            "-1",
            "--format=%H",
            "--",
            &rel_path.to_string_lossy(),
        ])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.is_empty() {
        Ok(None)
    } else {
        Ok(Some(hash))
    }
}

fn relative_path_for_git_show(resolved: &Path, git_root: &Path) -> PathBuf {
    if resolved.is_absolute() {
        resolved
            .strip_prefix(git_root)
            .unwrap_or(resolved)
            .to_path_buf()
    } else {
        resolved.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::UNIX_EPOCH;

    fn init_repo(root: &Path) {
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
    }

    #[test]
    fn show_head_reads_committed_file_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "committed\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init"])
            .output()
            .unwrap();
        fs::write(&doc, "working tree\n").unwrap();

        assert_eq!(show_head(&doc).unwrap(), Some("committed\n".to_string()));
    }

    #[test]
    fn show_head_returns_none_for_untracked_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "untracked\n").unwrap();

        assert_eq!(show_head(&doc).unwrap(), None);
    }

    #[test]
    fn head_fallback_returns_head_only_when_worktree_differs() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "committed\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init"])
            .output()
            .unwrap();

        assert_eq!(
            head_fallback_when_differs_from_worktree(&doc).unwrap(),
            HeadWorktreeFallback::MatchesCurrent
        );

        fs::write(&doc, "working tree\n").unwrap();

        assert_eq!(
            head_fallback_when_differs_from_worktree(&doc).unwrap(),
            HeadWorktreeFallback::DiffersFromCurrent("committed\n".to_string())
        );
    }

    #[test]
    fn head_fallback_returns_no_head_for_untracked_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "untracked\n").unwrap();

        assert_eq!(
            head_fallback_when_differs_from_worktree(&doc).unwrap(),
            HeadWorktreeFallback::NoHead
        );
    }

    #[test]
    fn last_commit_mtime_returns_timestamp_for_committed_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "committed\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init"])
            .output()
            .unwrap();

        let mtime = last_commit_mtime(&doc).unwrap().unwrap();
        assert!(mtime > UNIX_EPOCH);
    }

    #[test]
    fn last_commit_mtime_returns_none_for_untracked_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "untracked\n").unwrap();

        assert_eq!(last_commit_mtime(&doc).unwrap(), None);
    }

    #[test]
    fn last_commit_hash_returns_hash_for_committed_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "committed\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init"])
            .output()
            .unwrap();

        let hash = last_commit_hash(&doc).unwrap().unwrap();
        assert_eq!(hash, rev_parse(root, "HEAD").unwrap().unwrap());
    }

    #[test]
    fn last_commit_hash_returns_none_for_untracked_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "untracked\n").unwrap();

        assert_eq!(last_commit_hash(&doc).unwrap(), None);
    }

    #[test]
    fn recent_commit_lines_returns_oneline_entries_for_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "first\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "first"])
            .output()
            .unwrap();
        fs::write(&doc, "second\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "second"])
            .output()
            .unwrap();

        let RecentCommitLog::Lines(lines) = recent_commit_lines(&doc, None, 5) else {
            panic!("expected recent commit lines");
        };
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("second"), "{lines:?}");
        assert!(lines[1].contains("first"), "{lines:?}");
    }

    #[test]
    fn recent_commit_lines_returns_empty_for_path_without_commits() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::write(root.join("other.md"), "committed\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "other.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "other"])
            .output()
            .unwrap();
        let doc = root.join("doc.md");
        fs::write(&doc, "untracked\n").unwrap();

        assert_eq!(recent_commit_lines(&doc, None, 5), RecentCommitLog::Empty);
    }

    #[test]
    fn rev_parse_returns_commit_for_valid_ref() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "committed\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "init"])
            .output()
            .unwrap();

        let head = rev_parse(root, "HEAD").unwrap().unwrap();
        assert_eq!(head.len(), 40);
        assert!(head.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn rev_parse_returns_none_for_missing_ref() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);

        assert_eq!(rev_parse(root, "HEAD").unwrap(), None);
    }
}

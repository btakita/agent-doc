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

    // GH #274: committed document text is authority for recovery and for the
    // committed-text integrity guard. A lossy decode would substitute U+FFFD
    // and let a corrupted copy read as committed, so a non-UTF-8 blob fails
    // closed instead.
    let content = String::from_utf8(output.stdout).map_err(|err| {
        anyhow::anyhow!(
            "refusing to read {rev}:{} as document text: the committed blob is not valid UTF-8 (first invalid byte at offset {})",
            rel_path.display(),
            err.utf8_error().valid_up_to(),
        )
    })?;
    Ok(Some(content))
}

/// Visit the committed versions of `file`, newest first, for at most `limit`
/// commits that touched it.
///
/// `visit` receives each commit's content (`None` when the path is absent at
/// that commit) and returns `false` to stop the walk. Returns the number of
/// commits visited. A file outside git, or with no commits, visits nothing.
/// The owning git root is narrowed to the submodule root, as in [`show_rev`].
pub fn walk_file_revisions(
    file: &Path,
    limit: usize,
    mut visit: impl FnMut(Option<&str>) -> bool,
) -> Result<usize> {
    walk_file_revisions_with_time(file, limit, |_, content| visit(content))
}

/// [`walk_file_revisions`] that also hands `visit` each commit's committer
/// time (Unix seconds).
pub fn walk_file_revisions_with_time(
    file: &Path,
    limit: usize,
    mut visit: impl FnMut(u64, Option<&str>) -> bool,
) -> Result<usize> {
    let (super_root, resolved) = resolve_to_git_root(file)?;
    let (git_root, _) = narrow_to_submodule(&super_root, &resolved);
    let rel_path = relative_path_for_git_show(&resolved, &git_root);
    let rel = rel_path.to_string_lossy().to_string();

    let output = Command::new("git")
        .current_dir(&git_root)
        .args([
            "log",
            &format!("-{limit}"),
            "--format=%H %ct",
            "--",
            rel.as_str(),
        ])
        .output()?;
    if !output.status.success() {
        return Ok(0);
    }
    let mut visited = 0usize;
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let mut fields = line.split_whitespace();
        let Some(sha) = fields.next() else {
            continue;
        };
        let committed_secs = fields
            .next()
            .and_then(|secs| secs.parse::<u64>().ok())
            .unwrap_or(0);
        let shown = Command::new("git")
            .current_dir(&git_root)
            .args(["show", &format!("{sha}:{rel}")])
            .output()?;
        visited += 1;
        let content = shown
            .status
            .success()
            .then(|| String::from_utf8_lossy(&shown.stdout).to_string());
        if !visit(committed_secs, content.as_deref()) {
            break;
        }
    }
    Ok(visited)
}

/// `#qheadresiduescope`: the exchange as it stood just before the live
/// free-text head `head_text` last (re-)entered queue `queue_id` of `file`, or
/// `None` when every echo counts.
///
/// Walks the document's committed versions newest first (at most
/// [`FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT`]) through
/// [`agent_doc_queue::queue_head_entry::FreeTextHeadEntryWalk`], which owns
/// the decisions. Shared by the preflight catch-up residue strike and the
/// queue-continuation answered-head skip so both scope the exchange to the
/// same entry point. A document with no history counts every echo (the
/// behavior before the scoping).
///
/// [`FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT`]: agent_doc_queue::queue_head_entry::FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT
pub fn free_text_head_entry_anchor_exchange(
    file: &Path,
    queue_id: &str,
    head_text: &str,
) -> Option<String> {
    use agent_doc_queue::queue_head_entry::{
        FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT, FreeTextHeadEntryWalk,
    };
    let mut walk = FreeTextHeadEntryWalk::new(queue_id, head_text);
    // A git failure leaves the walk as far as it got; with nothing visited
    // every echo counts.
    let _ = walk_file_revisions(file, FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT, |content| {
        walk.visit(content)
    });
    walk.finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT)
}

/// GH #327 pass 4: the exchange an echo must be newer
/// than to answer the live head `head_text` of the `loop` queue `queue_id`,
/// whose current pass opened at `opened_secs`, or `None` when every echo
/// counts.
///
/// Walks the document's committed versions newest first (at most
/// [`FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT`]) through
/// [`agent_doc_queue::queue_head_entry::LoopPassEchoWalk`], which owns the
/// decisions, starting from `current_content`. A git failure leaves the walk
/// as far as it got.
///
/// [`FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT`]: agent_doc_queue::queue_head_entry::FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT
pub fn loop_pass_head_echo_anchor_exchange(
    file: &Path,
    queue_id: &str,
    head_text: &str,
    opened_secs: u64,
    current_content: &str,
) -> Option<agent_doc_queue::queue_head_entry::LoopPassEchoAnchor> {
    use agent_doc_queue::queue_head_entry::{FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT, LoopPassEchoWalk};
    let mut walk = LoopPassEchoWalk::new(queue_id, head_text, opened_secs, current_content);
    let _ =
        walk_file_revisions_with_time(file, FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT, |secs, content| {
            walk.visit(secs, content)
        });
    walk.finish(FREE_TEXT_HEAD_ENTRY_HISTORY_LIMIT)
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

    /// GH #274: committed text round-trips byte-exact, and a non-UTF-8 blob
    /// fails closed instead of decoding to U+FFFD.
    #[test]
    fn show_head_is_byte_exact_utf8_and_fails_closed_on_invalid_utf8() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        let text = "### Re: x \u{2014} opus \u{b7} 0.35.505 \u{2192} 0.35.508 \u{1F600}\n";
        fs::write(&doc, text).unwrap();
        for args in [vec!["add", "doc.md"], vec!["commit", "-m", "utf8"]] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }
        assert_eq!(show_head(&doc).unwrap().as_deref(), Some(text));

        fs::write(&doc, b"latin-1 \xb7 middle dot\n").unwrap();
        for args in [vec!["add", "doc.md"], vec!["commit", "-m", "latin1"]] {
            Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }
        let err = show_head(&doc).expect_err("non-UTF-8 HEAD must fail closed");
        assert!(format!("{err:#}").contains("not valid UTF-8"), "{err:#}");
    }

    #[test]
    fn walk_file_revisions_visits_newest_first_and_stops_on_request() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        for version in ["one\n", "two\n", "three\n"] {
            fs::write(&doc, version).unwrap();
            Command::new("git")
                .current_dir(root)
                .args(["add", "doc.md"])
                .output()
                .unwrap();
            Command::new("git")
                .current_dir(root)
                .args(["commit", "-m", version.trim()])
                .output()
                .unwrap();
        }
        let mut seen = Vec::new();
        let visited = walk_file_revisions(&doc, 10, |content| {
            seen.push(content.unwrap_or_default().to_string());
            content != Some("two\n")
        })
        .unwrap();
        assert_eq!(visited, 2);
        assert_eq!(seen, vec!["three\n".to_string(), "two\n".to_string()]);

        let mut limited = 0;
        walk_file_revisions(&doc, 1, |_| {
            limited += 1;
            true
        })
        .unwrap();
        assert_eq!(limited, 1);
    }

    #[test]
    fn walk_file_revisions_visits_nothing_without_commits() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let doc = root.join("doc.md");
        fs::write(&doc, "untracked\n").unwrap();
        assert_eq!(walk_file_revisions(&doc, 10, |_| true).unwrap(), 0);
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

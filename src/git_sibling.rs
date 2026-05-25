//! # Module: git_sibling
//!
//! Cross-repo sibling commits driven by an `agent-doc finalize` /
//! `agent-doc write --commit` invocation.
//!
//! When a session document in repo A drives a change in a sibling repo B
//! (e.g. a session-share session document committing a `build-party` script
//! tweak), this module stages and commits the sibling repo with a
//! `Session-Doc:` git trailer pointing back at the session-document commit
//! that originated the work.
//!
//! Trailer format:
//!
//! ```text
//! Session-Doc: https://github.com/<org>/<repo>/blob/<sha>/<path>
//! ```
//!
//! - `<org>` / `<repo>` parsed from the session document's `origin` remote.
//! - `<sha>` is the session-document commit produced by the same
//!   `finalize` / `write --commit` invocation.
//! - `<path>` is the session-document path relative to its repo root.
//!
//! The trailer round-trips through
//! `git log --format='%(trailers:key=Session-Doc,valueonly)'` so the
//! companion `backfill-updates.sh` parsers in sibling repos can read it
//! without re-parsing the body.

use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Parsed `origin` remote URL pointing at GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRemote {
    pub org: String,
    pub repo: String,
}

/// Parse a GitHub `origin` URL into `(org, repo)`.
///
/// Accepts the standard SSH and HTTPS forms (with optional `.git` suffix):
/// - `git@github.com:org/repo.git`
/// - `git@github.com:org/repo`
/// - `https://github.com/org/repo.git`
/// - `https://github.com/org/repo`
/// - `ssh://git@github.com/org/repo.git`
///
/// Returns `None` for unparseable or non-GitHub URLs.
pub fn parse_github_remote(url: &str) -> Option<ParsedRemote> {
    let trimmed = url.trim();
    let without_git = trimmed.strip_suffix(".git").unwrap_or(trimmed);

    let path_part = if let Some(rest) = without_git.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = without_git.strip_prefix("https://github.com/") {
        rest
    } else if let Some(rest) = without_git.strip_prefix("http://github.com/") {
        rest
    } else if let Some(rest) = without_git.strip_prefix("ssh://git@github.com/") {
        rest
    } else {
        without_git.strip_prefix("ssh://git@github.com:")?
    };

    let mut segments = path_part.split('/').filter(|s| !s.is_empty());
    let org = segments.next()?.to_string();
    let repo = segments.next()?.to_string();
    if org.is_empty() || repo.is_empty() {
        return None;
    }
    Some(ParsedRemote { org, repo })
}

/// Build the canonical `Session-Doc:` trailer URL.
pub fn build_session_doc_url(remote: &ParsedRemote, sha: &str, doc_path_in_repo: &str) -> String {
    let cleaned_path = doc_path_in_repo.trim_start_matches('/');
    format!(
        "https://github.com/{}/{}/blob/{}/{}",
        remote.org, remote.repo, sha, cleaned_path
    )
}

/// Resolve the `origin` remote URL for a git working tree containing `inside`.
pub fn origin_url(inside: &Path) -> Result<String> {
    let cwd = if inside.is_dir() {
        inside.to_path_buf()
    } else {
        inside
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let output = Command::new("git")
        .current_dir(&cwd)
        .args(["remote", "get-url", "origin"])
        .output()
        .with_context(|| format!("invoke git remote get-url origin in {}", cwd.display()))?;
    if !output.status.success() {
        bail!(
            "git remote get-url origin failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Resolve the absolute git toplevel for the working tree containing `inside`.
pub fn toplevel(inside: &Path) -> Result<PathBuf> {
    let cwd = if inside.is_dir() {
        inside.to_path_buf()
    } else {
        inside
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let output = Command::new("git")
        .current_dir(&cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .with_context(|| format!("invoke git rev-parse --show-toplevel in {}", cwd.display()))?;
    if !output.status.success() {
        bail!(
            "git rev-parse --show-toplevel failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

/// Read the absolute git dir for the working tree containing `inside`.
pub fn absolute_git_dir(inside: &Path) -> Result<PathBuf> {
    let cwd = if inside.is_dir() {
        inside.to_path_buf()
    } else {
        inside
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let output = Command::new("git")
        .current_dir(&cwd)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .with_context(|| format!("invoke git rev-parse --absolute-git-dir in {}", cwd.display()))?;
    if !output.status.success() {
        bail!(
            "git rev-parse --absolute-git-dir failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

/// Read HEAD sha for the working tree containing `inside`.
pub fn head_sha(inside: &Path) -> Result<String> {
    let cwd = if inside.is_dir() {
        inside.to_path_buf()
    } else {
        inside
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let output = Command::new("git")
        .current_dir(&cwd)
        .args(["rev-parse", "HEAD"])
        .output()
        .with_context(|| format!("invoke git rev-parse HEAD in {}", cwd.display()))?;
    if !output.status.success() {
        bail!(
            "git rev-parse HEAD failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check whether the given working tree has staged changes (cached diff).
pub fn has_staged_changes(repo: &Path) -> Result<bool> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["diff", "--cached", "--name-only"])
        .output()
        .with_context(|| format!("invoke git diff --cached --name-only in {}", repo.display()))?;
    if !output.status.success() {
        bail!(
            "git diff --cached --name-only failed in {}: {}",
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(!output.stdout.is_empty())
}

/// Run `git -C <sibling> commit -m <message> --trailer 'Session-Doc: <url>'`.
///
/// Returns the resulting sibling-commit short sha for logging.
pub fn commit_sibling_with_trailer(sibling: &Path, message: &str, trailer_url: &str) -> Result<String> {
    let trailer = format!("Session-Doc: {}", trailer_url);
    let output = Command::new("git")
        .current_dir(sibling)
        .args(["commit", "-m", message, "--trailer", &trailer])
        .output()
        .with_context(|| format!("invoke git commit in {}", sibling.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "git commit failed in sibling {}: {} {}",
            sibling.display(),
            stderr.trim(),
            stdout.trim()
        );
    }
    head_sha(sibling)
}

/// Plan + execute sibling commits for a successful session-doc cycle.
///
/// `sibling_pairs` is an aligned `(repo_path, message)` list — both vectors
/// from the CLI must have the same length; the caller validates that before
/// invoking this function.
///
/// Behavior:
/// 1. Build the `Session-Doc:` URL from the session document's `origin`
///    remote and the freshly-committed session-doc sha.
/// 2. For each sibling, run the staged commit with the trailer appended.
/// 3. Fail closed if any sibling is not a git working tree, has no staged
///    changes, or shares the same git dir as the session document.
///
/// A single sibling failure is fatal — earlier siblings already committed
/// stay committed; the caller is responsible for surfacing partial outcomes.
pub fn commit_siblings_for_session_doc(
    session_doc: &Path,
    sibling_pairs: &[(PathBuf, String)],
) -> Result<Vec<(PathBuf, String)>> {
    if sibling_pairs.is_empty() {
        return Ok(Vec::new());
    }

    let session_remote_url = origin_url(session_doc)?;
    let remote = parse_github_remote(&session_remote_url).ok_or_else(|| {
        anyhow!(
            "session document {} `origin` remote is not a recognized GitHub URL: {}",
            session_doc.display(),
            session_remote_url
        )
    })?;
    let session_sha = head_sha(session_doc)?;
    let session_top = toplevel(session_doc)?;
    let doc_abs = std::fs::canonicalize(session_doc)
        .with_context(|| format!("canonicalize {}", session_doc.display()))?;
    let session_top_canon = std::fs::canonicalize(&session_top).unwrap_or(session_top.clone());
    let doc_rel = doc_abs
        .strip_prefix(&session_top_canon)
        .map_err(|_| {
            anyhow!(
                "session document {} is not inside its git toplevel {}",
                doc_abs.display(),
                session_top_canon.display()
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");
    let trailer_url = build_session_doc_url(&remote, &session_sha, &doc_rel);

    let session_git_dir = absolute_git_dir(session_doc)?;
    let session_git_dir_canon =
        std::fs::canonicalize(&session_git_dir).unwrap_or(session_git_dir.clone());

    let mut results = Vec::with_capacity(sibling_pairs.len());
    for (sibling, message) in sibling_pairs {
        if message.trim().is_empty() {
            bail!(
                "--commit-sibling-message for {} is empty; provide a non-empty subject",
                sibling.display()
            );
        }
        if !sibling.exists() {
            bail!(
                "--commit-sibling path does not exist: {}",
                sibling.display()
            );
        }
        let sibling_top = toplevel(sibling).with_context(|| {
            format!("{} is not a git working tree", sibling.display())
        })?;
        let sibling_git_dir = absolute_git_dir(sibling)?;
        let sibling_git_dir_canon =
            std::fs::canonicalize(&sibling_git_dir).unwrap_or(sibling_git_dir.clone());
        if sibling_git_dir_canon == session_git_dir_canon {
            bail!(
                "--commit-sibling {} resolves to the session document's own repo ({}); siblings must be a different repo",
                sibling.display(),
                sibling_git_dir_canon.display()
            );
        }
        if !has_staged_changes(&sibling_top)? {
            bail!(
                "--commit-sibling {} has no staged changes (git diff --cached is empty); stage files before invoking finalize",
                sibling.display()
            );
        }
        let sha = commit_sibling_with_trailer(&sibling_top, message, &trailer_url)?;
        eprintln!(
            "[commit-sibling] {} → {} (trailer Session-Doc: {})",
            sibling_top.display(),
            sha,
            trailer_url
        );
        results.push((sibling_top, sha));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_remote_ssh_with_git_suffix() {
        let parsed = parse_github_remote("git@github.com:btakita/agent-loop.git").unwrap();
        assert_eq!(parsed.org, "btakita");
        assert_eq!(parsed.repo, "agent-loop");
    }

    #[test]
    fn parse_remote_ssh_without_git_suffix() {
        let parsed = parse_github_remote("git@github.com:btakita/agent-loop").unwrap();
        assert_eq!(parsed.org, "btakita");
        assert_eq!(parsed.repo, "agent-loop");
    }

    #[test]
    fn parse_remote_https() {
        let parsed = parse_github_remote("https://github.com/ClaudeScore/SessionShare.git").unwrap();
        assert_eq!(parsed.org, "ClaudeScore");
        assert_eq!(parsed.repo, "SessionShare");
    }

    #[test]
    fn parse_remote_https_no_suffix() {
        let parsed = parse_github_remote("https://github.com/ClaudeScore/SessionShare").unwrap();
        assert_eq!(parsed.org, "ClaudeScore");
        assert_eq!(parsed.repo, "SessionShare");
    }

    #[test]
    fn parse_remote_ssh_proto() {
        let parsed = parse_github_remote("ssh://git@github.com/btakita/repo.git").unwrap();
        assert_eq!(parsed.org, "btakita");
        assert_eq!(parsed.repo, "repo");
    }

    #[test]
    fn parse_remote_rejects_gitlab() {
        assert!(parse_github_remote("git@gitlab.com:foo/bar.git").is_none());
    }

    #[test]
    fn parse_remote_rejects_empty() {
        assert!(parse_github_remote("").is_none());
        assert!(parse_github_remote("https://github.com/").is_none());
        assert!(parse_github_remote("git@github.com:").is_none());
    }

    #[test]
    fn build_session_doc_url_basic() {
        let remote = ParsedRemote {
            org: "ClaudeScore".to_string(),
            repo: "SessionShare".to_string(),
        };
        let url = build_session_doc_url(&remote, "abc123", "tasks/docs.md");
        assert_eq!(
            url,
            "https://github.com/ClaudeScore/SessionShare/blob/abc123/tasks/docs.md"
        );
    }

    #[test]
    fn build_session_doc_url_strips_leading_slash() {
        let remote = ParsedRemote {
            org: "btakita".to_string(),
            repo: "agent-loop".to_string(),
        };
        let url = build_session_doc_url(&remote, "deadbeef", "/tasks/agent-doc/bug.md");
        assert_eq!(
            url,
            "https://github.com/btakita/agent-loop/blob/deadbeef/tasks/agent-doc/bug.md"
        );
    }

    #[test]
    fn commit_siblings_empty_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let session_doc = dir.path().join("session.md");
        std::fs::write(&session_doc, "# session\n").unwrap();
        let result = commit_siblings_for_session_doc(&session_doc, &[]).unwrap();
        assert!(result.is_empty());
    }

    fn init_repo_with_origin(path: &Path, origin: &str) {
        Command::new("git")
            .current_dir(path)
            .args(["init", "-q", "--initial-branch=main"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(path)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(path)
            .args(["config", "user.name", "Test"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(path)
            .args(["remote", "add", "origin", origin])
            .status()
            .unwrap();
    }

    fn commit_file(repo: &Path, name: &str, body: &str, msg: &str) {
        let path = repo.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["add", name])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["commit", "-q", "-m", msg])
            .status()
            .unwrap();
    }

    fn stage_file(repo: &Path, name: &str, body: &str) {
        let path = repo.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["add", name])
            .status()
            .unwrap();
    }

    fn last_trailer(repo: &Path) -> String {
        let output = Command::new("git")
            .current_dir(repo)
            .args([
                "log",
                "-1",
                "--format=%(trailers:key=Session-Doc,valueonly)",
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn commit_siblings_injects_session_doc_trailer() {
        let tmp = tempfile::tempdir().unwrap();
        let session_root = tmp.path().join("session");
        let sibling_root = tmp.path().join("sibling");
        std::fs::create_dir_all(&session_root).unwrap();
        std::fs::create_dir_all(&sibling_root).unwrap();
        init_repo_with_origin(
            &session_root,
            "git@github.com:btakita/agent-loop.git",
        );
        init_repo_with_origin(
            &sibling_root,
            "git@github.com:btakita/build-party.git",
        );
        commit_file(&session_root, "tasks/doc.md", "# doc\n", "agent-doc(doc): seed");
        commit_file(&sibling_root, "README.md", "# sibling\n", "initial");
        stage_file(&sibling_root, "scripts/run.sh", "#!/bin/sh\necho hi\n");

        let session_doc = session_root.join("tasks/doc.md");
        let pairs = vec![(sibling_root.clone(), "scripts: add run.sh".to_string())];
        let results =
            commit_siblings_for_session_doc(&session_doc, &pairs).expect("commit_siblings");

        assert_eq!(results.len(), 1);
        let trailer = last_trailer(&sibling_root);
        let session_sha = head_sha(&session_doc).unwrap();
        let expected = format!(
            "https://github.com/btakita/agent-loop/blob/{}/tasks/doc.md",
            session_sha
        );
        assert_eq!(trailer, expected);
    }

    #[test]
    fn commit_siblings_uses_session_doc_commit_sha() {
        let tmp = tempfile::tempdir().unwrap();
        let session_root = tmp.path().join("s");
        let sibling_root = tmp.path().join("b");
        std::fs::create_dir_all(&session_root).unwrap();
        std::fs::create_dir_all(&sibling_root).unwrap();
        init_repo_with_origin(&session_root, "https://github.com/foo/bar");
        init_repo_with_origin(&sibling_root, "https://github.com/foo/baz");
        commit_file(&session_root, "doc.md", "first\n", "agent-doc(doc): first");
        commit_file(&sibling_root, "x.md", "x\n", "initial");
        commit_file(&session_root, "doc.md", "second\n", "agent-doc(doc): second");
        let session_sha = head_sha(&session_root.join("doc.md")).unwrap();
        stage_file(&sibling_root, "scripts/y.sh", "y\n");

        let results = commit_siblings_for_session_doc(
            &session_root.join("doc.md"),
            &[(sibling_root.clone(), "scripts: y".to_string())],
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        let trailer = last_trailer(&sibling_root);
        assert!(
            trailer.contains(&session_sha),
            "expected trailer {} to contain session_sha {}",
            trailer,
            session_sha
        );
    }

    #[test]
    fn commit_siblings_fails_closed_on_same_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let session_root = tmp.path().join("only");
        std::fs::create_dir_all(&session_root).unwrap();
        init_repo_with_origin(&session_root, "git@github.com:foo/only.git");
        commit_file(&session_root, "doc.md", "x\n", "agent-doc(doc): seed");
        stage_file(&session_root, "next.md", "next\n");
        let err = commit_siblings_for_session_doc(
            &session_root.join("doc.md"),
            &[(session_root.clone(), "msg".to_string())],
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("session document's own repo"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn commit_siblings_fails_closed_on_no_staged_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let session_root = tmp.path().join("s");
        let sibling_root = tmp.path().join("b");
        std::fs::create_dir_all(&session_root).unwrap();
        std::fs::create_dir_all(&sibling_root).unwrap();
        init_repo_with_origin(&session_root, "git@github.com:foo/s.git");
        init_repo_with_origin(&sibling_root, "git@github.com:foo/b.git");
        commit_file(&session_root, "doc.md", "x\n", "seed");
        commit_file(&sibling_root, "x.md", "x\n", "seed");
        let err = commit_siblings_for_session_doc(
            &session_root.join("doc.md"),
            &[(sibling_root, "msg".to_string())],
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("no staged changes"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn commit_siblings_fails_closed_on_non_github_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let session_root = tmp.path().join("s");
        let sibling_root = tmp.path().join("b");
        std::fs::create_dir_all(&session_root).unwrap();
        std::fs::create_dir_all(&sibling_root).unwrap();
        init_repo_with_origin(&session_root, "git@gitlab.com:foo/s.git");
        init_repo_with_origin(&sibling_root, "git@github.com:foo/b.git");
        commit_file(&session_root, "doc.md", "x\n", "seed");
        commit_file(&sibling_root, "x.md", "x\n", "seed");
        stage_file(&sibling_root, "y.md", "y\n");
        let err = commit_siblings_for_session_doc(
            &session_root.join("doc.md"),
            &[(sibling_root, "msg".to_string())],
        )
        .unwrap_err();
        assert!(err.to_string().contains("not a recognized GitHub URL"));
    }
}

//! # Module: history
//!
//! ## Spec
//! - `list(file)`: walks `git log` for the file, extracts the `exchange` component from each
//!   commit's snapshot, and prints a table of `COMMIT`, `DATE`, and the first non-empty line of
//!   the exchange (truncated to 72 chars).  Reports "(no exchange component)" or "(file not
//!   available)" when the component is absent or the git show fails.
//! - `restore(file, commit)`: verifies the commit exists, extracts its `exchange` content, prepends
//!   it to the current document's exchange (separated by `---\n\n`), writes atomically via
//!   `tempfile::NamedTempFile`, and updates the snapshot.  If the current exchange is empty, no
//!   separator is inserted.
//! - Both functions require the file to be inside a git repository; `list` on a non-git file
//!   returns `Err`.
//!
//! ## Agentic Contracts
//! - `list` prints to stdout (the table) and stderr (warnings/empty messages); it never modifies
//!   any file.
//! - `restore` returns `Err` for a non-existent commit, a commit that does not contain the file,
//!   or a commit whose exchange component is empty.
//! - After `restore` returns `Ok`, old exchange content is prepended before the current content;
//!   the rest of the document is unchanged.
//! - Snapshot update on `restore` is best-effort: failure logs a warning but does not propagate.
//!
//! ## Evals
//! - extract_exchange_found: doc with exchange component → content extracted correctly
//! - extract_exchange_not_found: plain doc without component → returns None
//! - extract_exchange_empty: empty exchange tags → returns empty string
//! - extract_exchange_nested_components: exchange nested inside outer component → inner content extracted
//! - list_in_git_repo: file committed in temp git repo → list returns Ok without error
//! - restore_prepends_exchange: v1 committed, v2 current → restore inserts v1 before v2 with separator
//! - restore_into_empty_exchange: restore into empty exchange → content inserted without separator
//! - restore_nonexistent_commit_fails: bogus commit hash → returns Err

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::component;
use crate::snapshot;

const EXCHANGE_COMPONENT: &str = "exchange";

/// Entry in the history list: one commit that touched the file.
struct HistoryEntry {
    commit: String,
    date: String,
    summary: String,
}

/// Extract the exchange component content from a document string.
/// Returns None if no exchange component is found.
fn extract_exchange(doc: &str) -> Option<String> {
    let components = component::parse(doc).ok()?;
    let exchange = components.iter().find(|c| c.name == EXCHANGE_COMPONENT)?;
    Some(exchange.content(doc).to_string())
}

/// Resolve the git root and relative path for a file.
/// Returns (git_root, relative_path).
fn resolve_git_paths(file: &Path) -> Result<(std::path::PathBuf, String)> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("file not found: {}", file.display()))?;
    let parent = canonical.parent().unwrap_or(Path::new("/"));

    let output = Command::new("git")
        .current_dir(parent)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;

    if !output.status.success() {
        bail!("file is not in a git repository: {}", file.display());
    }

    let git_root = std::path::PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    );
    let rel_path = canonical
        .strip_prefix(&git_root)
        .with_context(|| format!(
            "file {} is not under git root {}",
            canonical.display(),
            git_root.display()
        ))?
        .to_string_lossy()
        .to_string();

    Ok((git_root, rel_path))
}

/// List exchange component versions from git history.
///
/// Walks `git log` for the file, extracts the exchange content from each commit,
/// and prints a table with commit hash, date, and first line of exchange content.
pub fn list(file: &Path) -> Result<()> {
    let (git_root, rel_path) = resolve_git_paths(file)?;

    // Get commits that touched this file
    let output = Command::new("git")
        .current_dir(&git_root)
        .args(["log", "--format=%H %ai", "--", &rel_path])
        .output()
        .context("failed to run git log")?;

    if !output.status.success() {
        bail!("git log failed for {}", file.display());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if lines.is_empty() {
        eprintln!("No git history found for {}", file.display());
        return Ok(());
    }

    // Collect history entries
    let mut entries: Vec<HistoryEntry> = Vec::new();

    for line in &lines {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() < 2 {
            continue;
        }
        let commit = parts[0].to_string();
        let date = parts[1].to_string();

        // Get file content at this commit
        let show_output = Command::new("git")
            .current_dir(&git_root)
            .args(["show", &format!("{}:{}", commit, rel_path)])
            .output();

        let summary = match show_output {
            Ok(ref o) if o.status.success() => {
                let content = String::from_utf8_lossy(&o.stdout);
                match extract_exchange(&content) {
                    Some(exchange) => {
                        let first_line = exchange
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("")
                            .to_string();
                        // Truncate for display
                        if first_line.len() > 72 {
                            format!("{}...", &first_line[..72])
                        } else {
                            first_line
                        }
                    }
                    None => "(no exchange component)".to_string(),
                }
            }
            _ => "(file not available)".to_string(),
        };

        entries.push(HistoryEntry {
            commit,
            date,
            summary,
        });
    }

    if entries.is_empty() {
        eprintln!("No commits found for {}", file.display());
        return Ok(());
    }

    // Print table header
    println!("{:<12} {:<26} EXCHANGE", "COMMIT", "DATE");
    println!("{}", "-".repeat(80));

    for entry in &entries {
        let short_commit = &entry.commit[..12.min(entry.commit.len())];
        println!("{:<12} {:<26} {}", short_commit, entry.date, entry.summary);
    }

    Ok(())
}

/// Restore exchange content from a specific commit.
///
/// Extracts the exchange content from the given commit, reads the current file,
/// and prepends the old content inside the exchange component (before existing
/// content), separated by `---\n\n`. Writes atomically and updates the snapshot.
pub fn restore(file: &Path, commit: &str) -> Result<()> {
    let (git_root, rel_path) = resolve_git_paths(file)?;

    // Verify the commit exists
    let output = Command::new("git")
        .current_dir(&git_root)
        .args(["cat-file", "-t", commit])
        .output()
        .context("failed to verify commit")?;

    if !output.status.success() {
        bail!("commit does not exist: {}", commit);
    }

    // Get file content at the specified commit
    let output = Command::new("git")
        .current_dir(&git_root)
        .args(["show", &format!("{}:{}", commit, rel_path)])
        .output()
        .context("failed to run git show")?;

    if !output.status.success() {
        bail!(
            "file {} not found in commit {}",
            file.display(),
            commit
        );
    }

    let old_content = String::from_utf8_lossy(&output.stdout).to_string();

    // Extract exchange content from the old commit
    let old_exchange = extract_exchange(&old_content)
        .with_context(|| format!("no exchange component found in commit {}", commit))?;

    if old_exchange.trim().is_empty() {
        bail!("exchange component is empty in commit {}", commit);
    }

    // Read current file
    let current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    // Parse current document to find exchange component
    let components = component::parse(&current_content)
        .with_context(|| "failed to parse current document")?;

    let exchange = components
        .iter()
        .find(|c| c.name == EXCHANGE_COMPONENT)
        .with_context(|| "no exchange component in current document")?;

    let current_exchange = exchange.content(&current_content);

    // Build new exchange content: old content + separator + existing content
    let new_exchange = if current_exchange.trim().is_empty() {
        old_exchange
    } else {
        format!("{}\n---\n\n{}", old_exchange.trim_end(), current_exchange)
    };

    // Replace exchange content in document
    let new_doc = exchange.replace_content(&current_content, &new_exchange);

    // Atomic write: tempfile + rename
    let parent = file.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, new_doc.as_bytes())
        .context("failed to write temp file")?;
    tmp.persist(file)
        .with_context(|| format!("failed to persist to {}", file.display()))?;

    // Update snapshot (best-effort — may fail in environments without .agent-doc/)
    if let Err(e) = snapshot::save(file, &new_doc) {
        eprintln!("[history] Warning: failed to update snapshot: {}", e);
    }

    let short_commit = &commit[..12.min(commit.len())];
    eprintln!(
        "[history] Restored exchange from {} into {}",
        short_commit,
        file.display()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn extract_exchange_found() {
        let doc = "\
# Title
<!-- agent:exchange -->
Hello world
Second line
<!-- /agent:exchange -->
Footer
";
        let content = extract_exchange(doc).unwrap();
        assert_eq!(content, "Hello world\nSecond line\n");
    }

    #[test]
    fn extract_exchange_not_found() {
        let doc = "# Just a plain doc\n";
        assert!(extract_exchange(doc).is_none());
    }

    #[test]
    fn extract_exchange_empty() {
        let doc = "<!-- agent:exchange --><!-- /agent:exchange -->\n";
        let content = extract_exchange(doc).unwrap();
        assert_eq!(content, "");
    }

    #[test]
    fn extract_exchange_nested_components() {
        let doc = "\
<!-- agent:outer -->
<!-- agent:exchange -->
inner content
<!-- /agent:exchange -->
<!-- /agent:outer -->
";
        let content = extract_exchange(doc).unwrap();
        assert_eq!(content, "inner content\n");
    }

    /// Integration test: list requires a git repo, so we set one up in a tempdir.
    #[test]
    fn list_in_git_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let content = "\
---
title: Test
---
<!-- agent:exchange -->
First exchange
<!-- /agent:exchange -->
";
        fs::write(&doc, content).unwrap();

        // Init git repo and commit
        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "test.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "initial", "--no-verify"])
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();

        // list should succeed without error
        let result = list(&doc);
        assert!(result.is_ok(), "list failed: {:?}", result.err());
    }

    /// Integration test: restore prepends old exchange content.
    #[test]
    fn restore_prepends_exchange() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        // First version
        let v1 = "\
<!-- agent:exchange -->
Old exchange content
<!-- /agent:exchange -->
";
        fs::write(&doc, v1).unwrap();

        // Init git repo and commit v1
        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "test.md"])
            .output()
            .unwrap();
        let commit_out = Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "v1", "--no-verify"])
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();
        assert!(commit_out.status.success(), "commit v1 failed");

        // Get the commit hash
        let log_out = Command::new("git")
            .current_dir(dir.path())
            .args(["log", "--format=%H", "-1"])
            .output()
            .unwrap();
        let v1_commit = String::from_utf8_lossy(&log_out.stdout).trim().to_string();

        // Write v2 (new content)
        let v2 = "\
<!-- agent:exchange -->
New exchange content
<!-- /agent:exchange -->
";
        fs::write(&doc, v2).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "test.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "v2", "--no-verify"])
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();

        // Restore v1 exchange into current
        let result = restore(&doc, &v1_commit);
        assert!(result.is_ok(), "restore failed: {:?}", result.err());

        // Verify the result
        let restored = fs::read_to_string(&doc).unwrap();
        assert!(
            restored.contains("Old exchange content"),
            "should contain old content"
        );
        assert!(
            restored.contains("New exchange content"),
            "should still contain new content"
        );
        assert!(
            restored.contains("---"),
            "should contain separator"
        );

        // Verify order: old content should come before new content
        let old_pos = restored.find("Old exchange content").unwrap();
        let new_pos = restored.find("New exchange content").unwrap();
        assert!(
            old_pos < new_pos,
            "old content should be prepended before new content"
        );
    }

    /// Edge case: restore into an empty exchange component.
    #[test]
    fn restore_into_empty_exchange() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        // v1 with content
        let v1 = "\
<!-- agent:exchange -->
Historical content
<!-- /agent:exchange -->
";
        fs::write(&doc, v1).unwrap();

        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "test.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "v1", "--no-verify"])
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();

        let log_out = Command::new("git")
            .current_dir(dir.path())
            .args(["log", "--format=%H", "-1"])
            .output()
            .unwrap();
        let v1_commit = String::from_utf8_lossy(&log_out.stdout).trim().to_string();

        // v2 with empty exchange
        let v2 = "\
<!-- agent:exchange -->
<!-- /agent:exchange -->
";
        fs::write(&doc, v2).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "test.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "v2", "--no-verify"])
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();

        let result = restore(&doc, &v1_commit);
        assert!(result.is_ok(), "restore failed: {:?}", result.err());

        let restored = fs::read_to_string(&doc).unwrap();
        assert!(
            restored.contains("Historical content"),
            "should contain restored content"
        );
        // Should NOT have separator since exchange was empty
        assert!(
            !restored.contains("---"),
            "should not have separator when restoring into empty exchange"
        );
    }

    #[test]
    fn restore_nonexistent_commit_fails() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n").unwrap();

        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "test.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "init", "--no-verify"])
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();

        let result = restore(&doc, "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        assert!(result.is_err(), "should fail for nonexistent commit");
    }

    #[test]
    fn list_file_not_in_git_fails() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();
        // No git init — should fail
        let result = list(&doc);
        assert!(result.is_err(), "should fail when file is not in git repo");
    }
}

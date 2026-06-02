//! # Module: checkpoint
//!
//! Guided recovery for pre-mutation checkpoint tags (`#kc5e`). Exposed as the
//! `agent-doc checkpoint` command (the name `recover` is already a `repair`
//! alias for orphaned-response recovery, a distinct concern).
//!
//! ## Spec
//! - `list_recovery_tags(file)` returns the document's `agent-doc/<doc>/pre-auto-run-N`
//!   and `pre-compact-N` checkpoint tags, newest-first (by commit date, then ordinal).
//! - `run(file, restore, diff)`:
//!   - default (both `None`): prints the recovery checkpoints newest-first plus the
//!     inspect/restore command hints.
//!   - `diff = Some(tag)`: prints `git diff <tag> -- <file>` so the operator can see
//!     what changed since the checkpoint.
//!   - `restore = Some(tag)`: runs `git checkout <tag> -- <file>`, restoring **only**
//!     this document from the checkpoint (other files untouched), then asks the
//!     operator to review and commit.
//!
//! ## Agentic Contracts
//! - Recovery is surgical and non-destructive to unrelated files: `--restore` only
//!   rewrites the named document from the tag; it never resets the whole tree.
//! - A document with no checkpoint tags prints guidance rather than erroring.
//! - This is the guided exit for the checkpoints created by `create_pre_mutation_tag`
//!   (`compact`'s `pre-compact-N`, the queue auto-run's `pre-auto-run-N`).

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// A pre-mutation recovery checkpoint tag for a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryTag {
    pub name: String,
    pub slug: String,
    pub ordinal: u64,
    pub short_sha: String,
    pub date: String,
    pub subject: String,
}

fn git_root(file: &Path) -> Result<std::path::PathBuf> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("file not found: {}", file.display()))?;
    let parent = canonical.parent().unwrap_or(Path::new("/"));
    let out = Command::new("git")
        .current_dir(parent)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;
    if !out.status.success() {
        anyhow::bail!("file is not in a git repository");
    }
    Ok(std::path::PathBuf::from(
        String::from_utf8_lossy(&out.stdout).trim(),
    ))
}

fn doc_stem(file: &Path) -> String {
    file.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("doc")
        .to_string()
}

/// Parse the checkpoint tag lines for `<doc-stem>` into `RecoveryTag`s, newest-first.
/// `tag_lines` is the raw `git tag -l agent-doc/<stem>/*` output; `meta` resolves
/// `(short_sha, date, subject)` for a tag name (so the parser stays testable
/// without a live repo).
fn parse_recovery_tags(
    tag_lines: &str,
    meta: &mut dyn FnMut(&str) -> (String, String, String),
) -> Vec<RecoveryTag> {
    let mut tags = Vec::new();
    for name in tag_lines
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
    {
        let Some((prefix, ord)) = name.rsplit_once('-') else {
            continue;
        };
        if !(prefix.ends_with("pre-auto-run") || prefix.ends_with("pre-compact")) {
            continue;
        }
        let Ok(ordinal) = ord.parse::<u64>() else {
            continue;
        };
        let slug = prefix.rsplit('/').next().unwrap_or(prefix).to_string();
        let (short_sha, date, subject) = meta(name);
        tags.push(RecoveryTag {
            name: name.to_string(),
            slug,
            ordinal,
            short_sha,
            date,
            subject,
        });
    }
    // Newest first: commit date desc, then ordinal desc as a tiebreak.
    tags.sort_by(|a, b| b.date.cmp(&a.date).then(b.ordinal.cmp(&a.ordinal)));
    tags
}

/// List recovery checkpoint tags for `file`, newest-first.
pub fn list_recovery_tags(file: &Path) -> Result<Vec<RecoveryTag>> {
    let root = git_root(file)?;
    let pattern = format!("agent-doc/{}/*", doc_stem(file));
    let out = Command::new("git")
        .current_dir(&root)
        .args(["tag", "-l", &pattern])
        .output()
        .context("failed to list recovery tags")?;
    if !out.status.success() {
        return Ok(vec![]);
    }
    let lines = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut meta = |name: &str| -> (String, String, String) {
        match Command::new("git")
            .current_dir(&root)
            .args(["log", "-1", "--format=%h%x00%cs%x00%s", name])
            .output()
        {
            Ok(o) if o.status.success() => {
                let s = String::from_utf8_lossy(&o.stdout);
                let mut parts = s.trim().splitn(3, '\0');
                (
                    parts.next().unwrap_or("").to_string(),
                    parts.next().unwrap_or("").to_string(),
                    parts.next().unwrap_or("").to_string(),
                )
            }
            _ => (String::new(), String::new(), String::new()),
        }
    };
    Ok(parse_recovery_tags(&lines, &mut meta))
}

pub fn run(file: &Path, restore: Option<&str>, diff: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    let root = git_root(file)?;
    let file_arg = file.to_string_lossy().to_string();

    if let Some(tag) = restore {
        let status = Command::new("git")
            .current_dir(&root)
            .args(["checkout", tag, "--", &file_arg])
            .status()
            .context("git checkout failed")?;
        if !status.success() {
            anyhow::bail!("failed to restore {} from {}", file.display(), tag);
        }
        println!(
            "Restored {} from {} (other files untouched). Review the change and commit when satisfied.",
            file.display(),
            tag
        );
        return Ok(());
    }

    if let Some(tag) = diff {
        let status = Command::new("git")
            .current_dir(&root)
            .args(["--no-pager", "diff", tag, "--", &file_arg])
            .status()
            .context("git diff failed")?;
        if !status.success() {
            anyhow::bail!("git diff failed for {}", tag);
        }
        return Ok(());
    }

    let tags = list_recovery_tags(file)?;
    if tags.is_empty() {
        println!("No recovery checkpoint tags for {}.", file.display());
        println!(
            "(Checkpoints are created as agent-doc/<doc>/pre-auto-run-N before a queue auto-run, and pre-compact-N before compaction.)"
        );
        return Ok(());
    }
    println!(
        "Recovery checkpoints for {} (newest first):\n",
        file.display()
    );
    for t in &tags {
        println!(
            "  {}  [{}]  {}  {}  {}",
            t.name, t.slug, t.short_sha, t.date, t.subject
        );
    }
    println!(
        "\nInspect:  agent-doc recover {} --diff <TAG>",
        file.display()
    );
    println!(
        "Restore:  agent-doc recover {} --restore <TAG>   (restores only this file; review + commit after)",
        file.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_recovery_tags_filters_and_sorts_newest_first() {
        let lines = "agent-doc/doc/pre-auto-run-1\n\
                     agent-doc/doc/pre-auto-run-2\n\
                     agent-doc/doc/pre-compact-1\n\
                     agent-doc/doc/unrelated-tag\n\
                     v1.0.0\n";
        // Stub metadata: encode an ascending date by ordinal so newest sorts first.
        let mut meta = |name: &str| -> (String, String, String) {
            let date = if name.ends_with("pre-auto-run-2") {
                "2026-06-02"
            } else if name.ends_with("pre-auto-run-1") {
                "2026-06-01"
            } else {
                "2026-05-30"
            };
            ("abc1234".to_string(), date.to_string(), "checkpoint".to_string())
        };
        let tags = parse_recovery_tags(lines, &mut meta);
        // Only the two pre-auto-run + one pre-compact checkpoints; unrelated dropped.
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].name, "agent-doc/doc/pre-auto-run-2");
        assert_eq!(tags[0].slug, "pre-auto-run");
        assert_eq!(tags[0].ordinal, 2);
        assert_eq!(tags[1].name, "agent-doc/doc/pre-auto-run-1");
        assert_eq!(tags[2].slug, "pre-compact");
        assert!(tags.iter().all(|t| t.name.contains("pre-")));
    }

    #[test]
    fn parse_recovery_tags_empty_when_no_checkpoints() {
        let mut meta =
            |_: &str| -> (String, String, String) { (String::new(), String::new(), String::new()) };
        assert!(parse_recovery_tags("v1.0.0\nrelease-2\n", &mut meta).is_empty());
    }
}

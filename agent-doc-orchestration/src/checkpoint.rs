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

use agent_doc_git::{RecoveryTag, doc_stem, parse_recovery_tags};
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

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

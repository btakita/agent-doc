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

/// Create a lightweight git tag at the current HEAD to mark a pre-mutation
/// recovery checkpoint before a destructive document/backlog rewrite.
///
/// `slug` names the checkpoint class (e.g. `pre-compact`, `pre-auto-run`). If
/// `tag_override` is provided it is used as the tag name verbatim. Otherwise the
/// document name is derived from the file stem and the tag auto-generates
/// `agent-doc/<doc-name>/<slug>-N` where N is the next unused ordinal.
pub fn create_pre_mutation_tag(file: &Path, slug: &str, tag_override: Option<&str>) -> Result<()> {
    let git_root = git_root(file)?;
    let tag_name = match tag_override {
        Some(name) => name.to_string(),
        None => {
            let doc_name = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("doc")
                .to_string();

            let pattern = format!("agent-doc/{doc_name}/{slug}-*");
            let count = Command::new("git")
                .current_dir(&git_root)
                .args(["tag", "-l", &pattern])
                .output()
                .map(|o| {
                    if o.status.success() {
                        String::from_utf8_lossy(&o.stdout)
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .count()
                    } else {
                        0
                    }
                })
                .unwrap_or(0);
            format!("agent-doc/{doc_name}/{slug}-{}", count + 1)
        }
    };

    let tag_output = Command::new("git")
        .current_dir(&git_root)
        .args(["tag", &tag_name])
        .output()
        .with_context(|| format!("failed to create git tag {tag_name}"))?;

    if !tag_output.status.success() {
        let stderr = String::from_utf8_lossy(&tag_output.stderr);
        anyhow::bail!("git tag {} failed: {}", tag_name, stderr.trim());
    }

    eprintln!("[agent-doc] Tagged {} state as {}", slug, tag_name);
    Ok(())
}

/// Number of recovery checkpoint tags to retain per `<doc>/<slug>` series.
pub const KEEP_RECOVERY_TAGS: usize = 20;

/// Prune accumulated recovery checkpoint tags (`agent-doc/<doc>/pre-auto-run-N`
/// and `agent-doc/<doc>/pre-compact-N`), keeping the newest
/// [`KEEP_RECOVERY_TAGS`] per `<doc>/<slug>` series. One tag is created per queue
/// auto-run / compact, so without pruning they grow unbounded over a document's
/// life (`#x8aw` / `#misfire-recovery-snapshot`).
///
/// Best-effort: a non-git root or git failure yields `(0, 0)` rather than
/// erroring.
pub fn prune_old_recovery_tags(project_root: &Path, dry_run: bool) -> Result<(usize, usize)> {
    let output = Command::new("git")
        .current_dir(project_root)
        .args(["tag", "-l", "agent-doc/*"])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Ok((0, 0)),
    };

    let mut groups: std::collections::HashMap<String, Vec<(u64, String)>> =
        std::collections::HashMap::new();
    for tag in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
    {
        let Some((prefix, ord)) = tag.rsplit_once('-') else {
            continue;
        };
        if !(prefix.ends_with("pre-auto-run") || prefix.ends_with("pre-compact")) {
            continue;
        }
        let Ok(n) = ord.parse::<u64>() else {
            continue;
        };
        groups
            .entry(prefix.to_string())
            .or_default()
            .push((n, tag.to_string()));
    }

    let mut deleted = 0usize;
    let mut kept = 0usize;
    for (_prefix, mut series) in groups {
        series.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        for (idx, (_n, tag)) in series.iter().enumerate() {
            if idx < KEEP_RECOVERY_TAGS {
                kept += 1;
                continue;
            }
            if dry_run {
                eprintln!("[gc] would delete old recovery tag: {}", tag);
                deleted += 1;
                continue;
            }
            match Command::new("git")
                .current_dir(project_root)
                .args(["tag", "-d", tag])
                .output()
            {
                Ok(o) if o.status.success() => deleted += 1,
                _ => kept += 1,
            }
        }
    }
    Ok((deleted, kept))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_in(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {:?} failed", args);
    }

    fn tag_count(dir: &Path, pattern: &str) -> usize {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["tag", "-l", pattern])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
    }

    #[test]
    fn create_pre_mutation_tag_auto_increments_ordinal_per_slug() {
        // #misfire-recovery-snapshot: the shared pre-mutation checkpoint tag
        // auto-generates `agent-doc/<doc>/<slug>-N`, incrementing N per slug.
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-q"]);
        git_in(root, &["config", "user.email", "t@t.t"]);
        git_in(root, &["config", "user.name", "t"]);
        let doc_path = root.join("session.md");
        std::fs::write(&doc_path, "---\nsession: test\n---\n").unwrap();
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "init"]);

        create_pre_mutation_tag(&doc_path, "pre-auto-run", None).unwrap();
        create_pre_mutation_tag(&doc_path, "pre-auto-run", None).unwrap();
        create_pre_mutation_tag(&doc_path, "pre-compact", None).unwrap();

        let tags = String::from_utf8(
            Command::new("git")
                .current_dir(root)
                .args(["tag", "-l"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert!(
            tags.contains("agent-doc/session/pre-auto-run-1"),
            "tags: {tags}"
        );
        assert!(
            tags.contains("agent-doc/session/pre-auto-run-2"),
            "tags: {tags}"
        );
        assert!(
            tags.contains("agent-doc/session/pre-compact-1"),
            "tags: {tags}"
        );
    }

    #[test]
    fn create_pre_mutation_tag_honors_override_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-q"]);
        git_in(root, &["config", "user.email", "t@t.t"]);
        git_in(root, &["config", "user.name", "t"]);
        let doc_path = root.join("session.md");
        std::fs::write(&doc_path, "x").unwrap();
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "init"]);

        create_pre_mutation_tag(&doc_path, "pre-auto-run", Some("my-checkpoint")).unwrap();
        let tags = String::from_utf8(
            Command::new("git")
                .current_dir(root)
                .args(["tag", "-l"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert!(tags.contains("my-checkpoint"), "tags: {tags}");
    }

    #[test]
    fn prune_old_recovery_tags_keeps_newest_per_series() {
        // #x8aw: recovery tags accumulate one-per-run; GC keeps the newest
        // KEEP_RECOVERY_TAGS per <doc>/<slug> series, leaving other series and
        // unrelated tags untouched.
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-q"]);
        git_in(root, &["config", "user.email", "t@t.t"]);
        git_in(root, &["config", "user.name", "t"]);
        std::fs::write(root.join("f"), "x").unwrap();
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "init"]);

        // 22 pre-auto-run tags (2 over the cap), 3 pre-compact, 1 unrelated.
        for n in 1..=22 {
            git_in(
                root,
                &["tag", &format!("agent-doc/session/pre-auto-run-{n}")],
            );
        }
        for n in 1..=3 {
            git_in(
                root,
                &["tag", &format!("agent-doc/session/pre-compact-{n}")],
            );
        }
        git_in(root, &["tag", "v1.0.0"]);

        let (deleted, _kept) = prune_old_recovery_tags(root, false).unwrap();
        assert_eq!(deleted, 2, "should delete 22-20 oldest pre-auto-run tags");
        assert_eq!(
            tag_count(root, "agent-doc/session/pre-auto-run-*"),
            KEEP_RECOVERY_TAGS,
            "newest {KEEP_RECOVERY_TAGS} pre-auto-run tags retained"
        );
        assert_eq!(tag_count(root, "agent-doc/session/pre-auto-run-1"), 0);
        assert_eq!(tag_count(root, "agent-doc/session/pre-auto-run-22"), 1);
        assert_eq!(tag_count(root, "agent-doc/session/pre-compact-*"), 3);
        assert_eq!(tag_count(root, "v1.0.0"), 1);
    }

    #[test]
    fn prune_old_recovery_tags_dry_run_deletes_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-q"]);
        git_in(root, &["config", "user.email", "t@t.t"]);
        git_in(root, &["config", "user.name", "t"]);
        std::fs::write(root.join("f"), "x").unwrap();
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "init"]);
        for n in 1..=25 {
            git_in(root, &["tag", &format!("agent-doc/d/pre-auto-run-{n}")]);
        }
        let (deleted, _kept) = prune_old_recovery_tags(root, true).unwrap();
        assert_eq!(deleted, 5, "dry-run reports 25-20 deletions");
        assert_eq!(
            tag_count(root, "agent-doc/d/pre-auto-run-*"),
            25,
            "dry-run deletes nothing"
        );
    }
}

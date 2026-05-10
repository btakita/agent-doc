//! # Module: migrate
//!
//! ## Spec
//! - Migrates session documents from deprecated component names and attributes to canonical forms.
//! - Renames `<!-- agent:pending ... -->` to `<!-- agent:backlog ... -->` (open+close tags).
//! - Renames `<!-- agent:pending-done -->` to `<!-- agent:backlog-done -->` (open+close tags).
//! - Strips deprecated `patch=`/`mode=` attributes from backlog component tags.
//! - Accepts one or more file paths, or `--all` to scan for session documents under the project root.
//! - `--dry-run` previews changes without writing.
//! - Skips files that are already up-to-date (no deprecated markers found).
//! - Updates snapshot after each successful migration.
//!
//! ## Agentic Contracts
//! - `run(files, all, dry_run) -> Result<()>` — public entry point.
//! - Pure text transformation: no agent interaction, no git commits.
//! - Idempotent: running twice on the same file produces identical output.
//!
//! ## Evals
//! - pending_to_backlog: `<!-- agent:pending -->` → `<!-- agent:backlog -->`
//! - pending_close_to_backlog: `<!-- /agent:pending -->` → `<!-- /agent:backlog -->`
//! - pending_done_to_backlog_done: `<!-- agent:pending-done -->` → `<!-- agent:backlog-done -->`
//! - strip_patch_attr: `<!-- agent:pending patch=replace -->` → `<!-- agent:backlog -->`
//! - strip_mode_attr: `<!-- agent:backlog mode=append -->` → `<!-- agent:backlog -->`
//! - preserves_other_attrs: `<!-- agent:pending foo=bar patch=replace -->` → `<!-- agent:backlog foo=bar -->`
//! - already_canonical: file with `<!-- agent:backlog -->` → no changes, skip
//! - code_block_immune: markers inside fenced code blocks are not transformed
//! - dry_run_no_write: `--dry-run` reports changes but does not modify files

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::{component, snapshot, write};

pub fn run(files: &[PathBuf], all: bool, dry_run: bool) -> Result<()> {
    let targets = if all {
        discover_session_docs()?
    } else if files.is_empty() {
        anyhow::bail!("no files specified — pass file paths or use --all");
    } else {
        files.to_vec()
    };

    if targets.is_empty() {
        eprintln!("[migrate] no session documents found");
        return Ok(());
    }

    let mut migrated = 0usize;
    let mut skipped = 0usize;

    for file in &targets {
        if !file.exists() {
            eprintln!("[migrate] skipping {} — file not found", file.display());
            skipped += 1;
            continue;
        }

        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;

        let result = migrate_content(&content);

        if result == content {
            skipped += 1;
            continue;
        }

        if dry_run {
            eprintln!("[migrate] would migrate: {}", file.display());
            migrated += 1;
            continue;
        }

        write::atomic_write_pub(file, &result)?;
        snapshot::save(file, &result)?;
        eprintln!("[migrate] migrated: {}", file.display());
        migrated += 1;
    }

    eprintln!(
        "[migrate] {} migrated, {} skipped (already canonical)",
        migrated, skipped
    );
    Ok(())
}

fn migrate_content(content: &str) -> String {
    let code_ranges = compute_code_ranges(content);
    let mut result = String::with_capacity(content.len());
    let mut pos = 0;

    while pos < content.len() {
        if let Some(comment_start) = content[pos..].find("<!--") {
            let abs_start = pos + comment_start;

            // Copy everything before this comment
            result.push_str(&content[pos..abs_start]);

            if let Some(comment_end_rel) = content[abs_start..].find("-->") {
                let abs_end = abs_start + comment_end_rel + 3;
                let comment = &content[abs_start..abs_end];

                if in_code_range(abs_start, &code_ranges) {
                    result.push_str(comment);
                } else {
                    result.push_str(&migrate_comment(comment));
                }

                pos = abs_end;
            } else {
                result.push_str(&content[abs_start..]);
                pos = content.len();
            }
        } else {
            result.push_str(&content[pos..]);
            pos = content.len();
        }
    }

    // Also strip any remaining patch/mode attrs from already-canonical backlog tags
    component::strip_backlog_patch_attr(&result)
}

fn migrate_comment(comment: &str) -> String {
    // Close tag: <!-- /agent:pending -->
    if comment.trim() == "<!-- /agent:pending -->" {
        return "<!-- /agent:backlog -->".to_string();
    }
    if comment.trim() == "<!-- /agent:pending-done -->" {
        return "<!-- /agent:backlog-done -->".to_string();
    }

    // Open tag: <!-- agent:pending ... -->
    if let Some(rest) = comment.strip_prefix("<!-- agent:pending")
        && let Some(inner) = rest.strip_suffix("-->")
    {
        if rest.starts_with("-done") {
            return "<!-- agent:backlog-done -->".to_string();
        }
        let trimmed = inner.trim();
        if trimmed.is_empty() {
            return "<!-- agent:backlog -->".to_string();
        }
        // Filter out patch= and mode= attributes
        let tokens: Vec<&str> = trimmed
            .split_whitespace()
            .filter(|t| {
                t.split_once('=')
                    .map(|(k, _)| k != "patch" && k != "mode")
                    .unwrap_or(true)
            })
            .collect();
        if tokens.is_empty() {
            return "<!-- agent:backlog -->".to_string();
        }
        return format!("<!-- agent:backlog {} -->", tokens.join(" "));
    }

    comment.to_string()
}

fn discover_session_docs() -> Result<Vec<PathBuf>> {
    let cwd = std::env::current_dir()?;
    let root = snapshot::find_project_root(&cwd).unwrap_or_else(|| cwd.clone());

    let snapshots_dir = root.join(".agent-doc/snapshots");
    if !snapshots_dir.is_dir() {
        return Ok(Vec::new());
    }

    // Walk common session document locations
    let mut docs = Vec::new();
    walk_for_session_docs(&root, &mut docs, 0)?;
    Ok(docs)
}

fn walk_for_session_docs(dir: &Path, docs: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
    if depth > 5 {
        return Ok(());
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden dirs and common non-doc dirs
        if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" {
            continue;
        }

        if path.is_dir() {
            walk_for_session_docs(&path, docs, depth + 1)?;
        } else if name_str.ends_with(".md") {
            // Quick check: does this file contain agent:pending?
            if let Ok(content) = std::fs::read_to_string(&path)
                && (content.contains("<!-- agent:pending")
                    || content.contains("<!-- /agent:pending"))
            {
                docs.push(path);
            }
        }
    }

    Ok(())
}

struct CodeRange {
    start: usize,
    end: usize,
}

fn compute_code_ranges(content: &str) -> Vec<CodeRange> {
    let mut ranges = Vec::new();
    let mut in_fence = false;
    let mut fence_start = 0;

    for (i, line) in content.lines().enumerate() {
        let line_start = content[..]
            .lines()
            .take(i)
            .map(|l| l.len() + 1)
            .sum::<usize>();
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if in_fence {
                ranges.push(CodeRange {
                    start: fence_start,
                    end: line_start + line.len(),
                });
                in_fence = false;
            } else {
                fence_start = line_start;
                in_fence = true;
            }
        }
    }

    // Inline code spans
    let mut pos = 0;
    while pos < content.len() {
        if let Some(tick_start) = content[pos..].find('`') {
            let abs_start = pos + tick_start;
            // Count consecutive backticks
            let tick_count = content[abs_start..]
                .chars()
                .take_while(|&c| c == '`')
                .count();
            let after_open = abs_start + tick_count;
            // Find matching close
            let close_pattern: String = std::iter::repeat_n('`', tick_count).collect();
            if let Some(close_rel) = content[after_open..].find(&close_pattern) {
                let abs_end = after_open + close_rel + tick_count;
                ranges.push(CodeRange {
                    start: abs_start,
                    end: abs_end,
                });
                pos = abs_end;
            } else {
                pos = after_open;
            }
        } else {
            break;
        }
    }

    ranges
}

fn in_code_range(offset: usize, ranges: &[CodeRange]) -> bool {
    ranges.iter().any(|r| offset >= r.start && offset < r.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_to_backlog() {
        let input = "<!-- agent:pending -->\nContent\n<!-- /agent:pending -->\n";
        let result = migrate_content(input);
        assert!(result.contains("<!-- agent:backlog -->"));
        assert!(result.contains("<!-- /agent:backlog -->"));
        assert!(!result.contains("agent:pending"));
    }

    #[test]
    fn strip_patch_attr() {
        let input = "<!-- agent:pending patch=replace -->\nContent\n<!-- /agent:pending -->\n";
        let result = migrate_content(input);
        assert_eq!(
            result,
            "<!-- agent:backlog -->\nContent\n<!-- /agent:backlog -->\n"
        );
    }

    #[test]
    fn pending_done_to_backlog_done() {
        let input = "<!-- agent:pending-done -->\nDone\n<!-- /agent:pending-done -->\n";
        let result = migrate_content(input);
        assert_eq!(
            result,
            "<!-- agent:backlog-done -->\nDone\n<!-- /agent:backlog-done -->\n"
        );
    }

    #[test]
    fn strip_mode_attr() {
        let input = "<!-- agent:backlog mode=append -->\nContent\n<!-- /agent:backlog -->\n";
        let result = migrate_content(input);
        assert_eq!(
            result,
            "<!-- agent:backlog -->\nContent\n<!-- /agent:backlog -->\n"
        );
    }

    #[test]
    fn preserves_other_attrs() {
        let input =
            "<!-- agent:pending foo=bar patch=replace -->\nContent\n<!-- /agent:pending -->\n";
        let result = migrate_content(input);
        assert!(result.contains("<!-- agent:backlog foo=bar -->"));
        assert!(!result.contains("patch=replace"));
    }

    #[test]
    fn already_canonical_no_change() {
        let input = "<!-- agent:backlog -->\nContent\n<!-- /agent:backlog -->\n";
        let result = migrate_content(input);
        assert_eq!(result, input);
    }

    #[test]
    fn code_block_immune() {
        let input = "```\n<!-- agent:pending -->\n```\n<!-- agent:pending -->\nContent\n<!-- /agent:pending -->\n";
        let result = migrate_content(input);
        // The one inside the code block should be preserved
        assert!(result.contains("```\n<!-- agent:pending -->\n```"));
        // The one outside should be migrated
        assert!(result.contains("<!-- agent:backlog -->\nContent\n<!-- /agent:backlog -->"));
    }

    #[test]
    fn idempotent() {
        let input = "<!-- agent:pending patch=replace -->\nContent\n<!-- /agent:pending -->\n";
        let first = migrate_content(input);
        let second = migrate_content(&first);
        assert_eq!(first, second);
    }
}

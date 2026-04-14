//! # Module: pending_cmd
//!
//! CLI subcommands for managing the `agent:pending` component.
//!
//! - `agent-doc pending <FILE> add <item>` — append a pending item
//! - `agent-doc pending <FILE> remove <target>` — remove by content match
//! - `agent-doc pending <FILE> prune` — remove completed items
//! - `agent-doc pending <FILE> list` — print pending items

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;

use crate::component;
use crate::pending;
use crate::snapshot;

fn find_pending_component(file: &Path) -> Result<(String, component::Component)> {
    let content = std::fs::read_to_string(file)
        .context("failed to read document")?;
    let components = component::parse(&content)
        .context("failed to parse components")?;
    let comp = components.into_iter()
        .find(|c| c.name == "pending")
        .context("document has no pending component")?;
    Ok((content, comp))
}

/// Compute a stable document id from a file path. Uses `snapshot::doc_hash` so
/// the id is consistent across pending ops and backfill.
fn doc_id_for(file: &Path) -> String {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    snapshot::doc_hash(&canonical).unwrap_or_else(|_| file.display().to_string())
}

/// Add a new item to the pending component (assigns a stable hash id + `[ ]`).
pub fn add(file: &Path, item: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = doc_id_for(file);
    let new_content = pending::op_add(existing, item, &doc_id)?;
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Run lazy backfill over the pending component and write if changed.
pub fn backfill(file: &Path) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = doc_id_for(file);
    let (new_content, changed) = pending::backfill(existing, &doc_id, &HashSet::new());
    if !changed {
        eprintln!("[pending] already canonical — no changes");
        return Ok(());
    }
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Mark an item `[x]` by id.
pub fn done(file: &Path, id: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_done(existing, id)?;
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Transition an item to `Gated` (`[/]`) by id.
/// Idempotent on already-gated items; errors on `Done` items.
pub fn gate(file: &Path, id: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_gate(existing, id)?;
    if new_content == existing {
        // No-op (already gated). Skip the disk write so we don't churn mtime.
        return Ok(());
    }
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Transition an item back to `Open` (`[ ]`) by id.
/// Errors on `Open` or `Done` items — the source must be `[/]`.
pub fn ungate(file: &Path, id: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_ungate(existing, id)?;
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Edit an item's text, preserving its hash id.
pub fn edit(file: &Path, id: &str, text: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_edit(existing, id, text)?;
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Clear all items from the pending component.
pub fn clear(file: &Path) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_clear(existing)?;
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Reorder items by id (comma-separated). Missing ids keep their relative order.
pub fn reorder(file: &Path, ids: &[String]) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_reorder(existing, ids)?;
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Reap `[x]` items and print removed ids.
pub fn reap(file: &Path) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let (new_content, removed) = pending::reap(existing);
    if removed.is_empty() {
        eprintln!("[pending] no [x] items to reap");
        return Ok(());
    }
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    eprintln!("[pending] reaped {} item(s): {}", removed.len(), removed.join(", "));
    Ok(())
}

/// Remove a pending item by content match.
pub fn remove(file: &Path, target: &str, contains: bool) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let lines: Vec<&str> = existing.lines().collect();
    let new_lines: Vec<String> = if contains {
        lines.iter()
            .filter(|line| !line.contains(target))
            .map(|s| s.to_string())
            .collect()
    } else {
        lines.iter()
            .filter(|line| {
                let trimmed = line.trim().trim_start_matches("- ").trim();
                trimmed != target
            })
            .map(|s| s.to_string())
            .collect()
    };

    if new_lines.len() == lines.len() {
        eprintln!("[pending] no matching item found");
    }

    let new_content = new_lines.join("\n");
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Remove completed items (lines with [x], [done], or starting with ✅).
/// Legacy helper retained for back-compat tests; new callers should use `reap`.
#[allow(dead_code)]
pub fn prune(file: &Path) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let lines: Vec<&str> = existing.lines().collect();
    let new_lines: Vec<String> = lines.iter()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("- [x]")
                && !trimmed.starts_with("- [X]")
                && !trimmed.starts_with("- [done]")
                && !trimmed.starts_with("\u{2705}")
        })
        .map(|s| s.to_string())
        .collect();

    if new_lines.len() == lines.len() {
        eprintln!("[pending] no completed items to prune");
        return Ok(());
    }

    let removed = lines.len() - new_lines.len();
    let new_content = new_lines.join("\n");
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    eprintln!("[pending] pruned {} completed items", removed);
    Ok(())
}

/// List current pending items.
pub fn list(file: &Path) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];

    if existing.trim().is_empty() {
        println!("(no pending items)");
        return Ok(());
    }

    for line in existing.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            println!("{}", trimmed);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, path::PathBuf};
    use tempfile::TempDir;

    fn setup_test_dir() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        env::set_current_dir(tmp.path()).unwrap();
        let doc = tmp.path().join("test.md");
        (tmp, doc)
    }

    fn doc_with_pending(items: &str) -> (TempDir, PathBuf) {
        let content = format!("---\nagent_doc_session: test\n---\n\n<!-- agent:pending -->\n{}\n<!-- /agent:pending -->\n", items);
        let (tmp, doc) = setup_test_dir();
        fs::write(&doc, content).unwrap();
        (tmp, doc)
    }

    #[test]
    fn add_appends_to_pending_component() {
        let (_tmp, doc) = doc_with_pending("- item one");
        add(&doc, "item two").unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        // `add` now runs op_add which canonicalizes items with hash + checkbox.
        assert!(content.contains("item one"));
        assert!(content.contains("item two"));
        // Both items should have hash ids.
        assert_eq!(content.matches("[#").count(), 2);
    }

    #[test]
    fn add_creates_content_if_empty() {
        let (_tmp, doc) = doc_with_pending("");
        add(&doc, "new item").unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("new item"));
        assert!(content.contains("[ ]"));
        assert!(content.contains("[#"));
    }

    #[test]
    fn remove_by_contains_match() {
        let (_tmp, doc) = doc_with_pending("- implement feature X\n- write tests");
        remove(&doc, "feature X", true).unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("implement feature X"));
        assert!(content.contains("write tests"));
    }

    #[test]
    fn remove_noop_for_nonmatching() {
        let (_tmp, doc) = doc_with_pending("- item one");
        remove(&doc, "not found", true).unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- item one"));
    }

    #[test]
    fn prune_removes_checked_items() {
        let (_tmp, doc) = doc_with_pending("- [ ] active\n- [x] done\n✅ finished");
        prune(&doc).unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- [ ] active"));
        assert!(!content.contains("- [x] done"));
        assert!(!content.contains("finished"));
    }

    #[test]
    fn prune_noop_for_no_checked() {
        let (_tmp, doc) = doc_with_pending("- [ ] active\n- [ ] another");
        prune(&doc).unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- [ ] active"));
        assert!(content.contains("- [ ] another"));
    }

    #[test]
    fn list_prints_pending_items() {
        let (_tmp, doc) = doc_with_pending("- item one\n- item two");
        list(&doc).unwrap();
        // Just checking it doesn't panic
    }
}

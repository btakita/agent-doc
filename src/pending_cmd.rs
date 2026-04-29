//! # Module: pending_cmd
//!
//! CLI subcommands for managing the `agent:backlog` component.
//!
//! - `agent-doc backlog <FILE> add <item>` — add a backlog item at the beginning
//!   (supports canonical `id=<custom> ` syntax and compatibility `[#custom] ` input
//!   to preserve a custom id)
//! - `agent-doc backlog <FILE> remove <target>` — remove by content match
//! - `agent-doc backlog <FILE> prune` — remove completed items
//! - `agent-doc backlog <FILE> list` — print backlog items

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;

use crate::component;
use crate::component::is_backlog_component;
use crate::pending;
use crate::snapshot;

fn find_pending_component(file: &Path) -> Result<(String, component::Component)> {
    let content = std::fs::read_to_string(file).context("failed to read document")?;
    let components = component::parse(&content).context("failed to parse components")?;
    let comp = components
        .into_iter()
        .find(|c| is_backlog_component(&c.name))
        .context("document has no backlog/pending component")?;
    Ok((content, comp))
}

/// Compute a stable document id from a file path. Uses `snapshot::doc_hash` so
/// the id is consistent across pending ops and backfill.
pub fn doc_id_for(file: &Path) -> String {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    snapshot::doc_hash(&canonical).unwrap_or_else(|_| file.display().to_string())
}

/// Add a new item to the pending component (assigns a stable hash id + `[ ]`
/// or `[/]`) at the beginning of the list. Supports canonical `id=<custom> `
/// syntax and compatibility `[#custom] ` input to preserve a custom id. Prints
/// the assigned hash id to stdout.
pub fn add(file: &Path, item: &str, gated: bool) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = doc_id_for(file);
    let (new_content, id) = pending::op_add(existing, item, &doc_id, gated)?;
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    println!("{}", id);
    Ok(())
}

/// Add multiple new items while preserving the caller's sequence order at the
/// beginning of the pending list.
pub fn add_many(file: &Path, items: &[String], gated: bool) -> Result<Vec<String>> {
    let mut ids = Vec::with_capacity(items.len());
    for item in items.iter().rev() {
        let (full_content, comp) = find_pending_component(file)?;
        let existing = &full_content[comp.open_end..comp.close_start];
        let doc_id = doc_id_for(file);
        let (new_content, id) = pending::op_add(existing, item, &doc_id, gated)?;
        let new_doc = comp.replace_content(&full_content, &new_content);
        std::fs::write(file, &new_doc)?;
        ids.push(id);
    }
    ids.reverse();
    Ok(ids)
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
    let doc_id = doc_id_for(file);
    let (canonical_content, changed) = pending::backfill(existing, &doc_id, &HashSet::new());
    let (new_content, removed) = pending::reap(&canonical_content)?;
    let final_content = if removed.is_empty() {
        canonical_content
    } else {
        new_content
    };
    if !changed && removed.is_empty() {
        eprintln!("[pending] no [x] items to reap");
        return Ok(());
    }
    let new_doc = comp.replace_content(&full_content, &final_content);
    std::fs::write(file, &new_doc)?;
    if changed {
        eprintln!("[pending] backfilled missing hash ids / checkboxes before reap");
    }
    if removed.is_empty() {
        eprintln!("[pending] no [x] items to reap");
        return Ok(());
    }
    eprintln!(
        "[pending] reaped {} item(s): {}",
        removed.len(),
        removed.join(", ")
    );
    Ok(())
}

/// Remove a pending item by content match.
pub fn remove(file: &Path, target: &str, contains: bool) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let lines: Vec<&str> = existing.lines().collect();
    let new_lines: Vec<String> = if contains {
        lines
            .iter()
            .filter(|line| !line.contains(target))
            .map(|s| s.to_string())
            .collect()
    } else {
        lines
            .iter()
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
    let new_lines: Vec<String> = lines
        .iter()
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

/// Resolve all items with a matching typed gate (e.g., `[/release]` → `[x]`).
/// Prints resolved ids to stdout.
pub fn resolve_gate(file: &Path, gate_type: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let (new_content, resolved) = pending::op_resolve_gate(existing, gate_type);
    if resolved.is_empty() {
        eprintln!(
            "[pending] no [/{}] items to resolve in {}",
            gate_type,
            file.display()
        );
        return Ok(());
    }
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    eprintln!(
        "[pending] resolved {} [/{}] item(s): {}",
        resolved.len(),
        gate_type,
        resolved.join(", ")
    );
    for id in &resolved {
        println!("{}", id);
    }
    Ok(())
}

/// Set a typed gate on a gated item (e.g., `[/]` → `[/release]`).
pub fn set_gate_type(file: &Path, id: &str, gate_type: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_set_gate_type(existing, id, gate_type)?;
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    eprintln!("[pending] set gate type [/{}] on [#{}]", gate_type, id);
    Ok(())
}

/// Scan documents under a directory for items matching a typed gate and resolve them.
/// Returns total number of resolved items.
pub fn resolve_gate_scan(gate_type: &str, scope: &Path) -> Result<usize> {
    let mut total = 0;
    let mut dirs = vec![scope.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if path.is_dir() {
                // Skip hidden dirs and common non-document dirs
                if !name_str.starts_with('.') && name_str != "node_modules" && name_str != "target"
                {
                    dirs.push(path);
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let components = match component::parse(&content) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let comp = match components
                .into_iter()
                .find(|c| is_backlog_component(&c.name))
            {
                Some(c) => c,
                None => continue,
            };
            let existing = &content[comp.open_end..comp.close_start];
            let (new_content, resolved) = pending::op_resolve_gate(existing, gate_type);
            if !resolved.is_empty() {
                let new_doc = comp.replace_content(&content, &new_content);
                std::fs::write(&path, &new_doc)?;
                eprintln!(
                    "[resolve-gate] {}: resolved {} item(s): {}",
                    path.display(),
                    resolved.len(),
                    resolved.join(", ")
                );
                total += resolved.len();
            }
        }
    }
    Ok(total)
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
        let doc = tmp.path().join("test.md");
        (tmp, doc)
    }

    fn doc_with_pending(items: &str) -> (TempDir, PathBuf) {
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:pending -->\n{}\n<!-- /agent:pending -->\n",
            items
        );
        let (tmp, doc) = setup_test_dir();
        fs::write(&doc, content).unwrap();
        (tmp, doc)
    }

    #[test]
    fn add_prepends_to_pending_component() {
        let (_tmp, doc) = doc_with_pending("- item one");
        add(&doc, "item two", false).unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        let pending = content
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .unwrap();
        let lines: Vec<&str> = pending.lines().collect();
        assert!(
            lines[0].contains("item two"),
            "expected new item first, got: {}",
            pending
        );
        assert!(
            lines[1].contains("item one"),
            "expected existing item second, got: {}",
            pending
        );
        assert_eq!(content.matches("[#").count(), 2);
    }

    #[test]
    fn add_many_preserves_sequence_order_at_front() {
        let (_tmp, doc) = doc_with_pending("- [ ] [#abcd] existing item");
        add_many(
            &doc,
            &["first new".to_string(), "second new".to_string()],
            false,
        )
        .unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        let pending = content
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .unwrap();
        let lines: Vec<&str> = pending.lines().collect();
        assert!(
            lines[0].contains("first new"),
            "expected first batch item first, got: {}",
            pending
        );
        assert!(
            lines[1].contains("second new"),
            "expected second batch item second, got: {}",
            pending
        );
        assert!(
            lines[2].contains("existing item"),
            "expected existing item after new items, got: {}",
            pending
        );
    }

    #[test]
    fn add_creates_content_if_empty() {
        let (_tmp, doc) = doc_with_pending("");
        add(&doc, "new item", false).unwrap();

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

    #[test]
    fn resolve_gate_flips_typed_items() {
        let (_tmp, doc) = doc_with_pending(
            "- [/release] [#a1b2] Release v1.0\n- [/deploy] [#c3d4] Deploy\n- [/] [#e5f6] Generic",
        );
        resolve_gate(&doc, "release").unwrap();
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("[x]"), "release item should be done");
        assert!(content.contains("[/deploy]"), "deploy should be untouched");
        assert!(content.contains("[/]"), "generic gate should be untouched");
    }

    #[test]
    fn resolve_gate_noop_no_match() {
        let (_tmp, doc) = doc_with_pending("- [/release] [#a1b2] Release");
        resolve_gate(&doc, "deploy").unwrap();
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("[/release]"), "should be unchanged");
    }

    #[test]
    fn set_gate_type_on_gated_item() {
        let (_tmp, doc) = doc_with_pending("- [/] [#a1b2] Release v1.0");
        set_gate_type(&doc, "a1b2", "release").unwrap();
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("[/release]"));
    }

    #[test]
    fn resolve_gate_scan_finds_across_dirs() {
        let tmp = TempDir::new().unwrap();
        let subdir = tmp.path().join("tasks");
        fs::create_dir_all(&subdir).unwrap();
        let doc1 = subdir.join("doc1.md");
        let doc2 = subdir.join("doc2.md");
        fs::write(&doc1, "---\nagent_doc_session: t1\n---\n\n<!-- agent:pending -->\n- [/release] [#a1b2] First\n<!-- /agent:pending -->\n").unwrap();
        fs::write(&doc2, "---\nagent_doc_session: t2\n---\n\n<!-- agent:pending -->\n- [/release] [#c3d4] Second\n- [/deploy] [#e5f6] Deploy\n<!-- /agent:pending -->\n").unwrap();

        let total = resolve_gate_scan("release", tmp.path()).unwrap();
        assert_eq!(total, 2);

        let c1 = fs::read_to_string(&doc1).unwrap();
        assert!(c1.contains("[x]"));
        let c2 = fs::read_to_string(&doc2).unwrap();
        assert!(c2.contains("[x]")); // release resolved
        assert!(c2.contains("[/deploy]")); // deploy untouched
    }
}

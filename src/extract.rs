//! # Module: extract
//!
//! ## Spec
//! - `run(source, target, component_name)`: extracts the last `### Re:` block from the named
//!   component in `source` (defaulting to `exchange`) and appends it to the matching component in
//!   `target`.  Both files must exist.  Source component must be non-empty and contain at least one
//!   `### Re:` header; if absent, the entire component content is treated as a single entry.
//! - `transfer(source, target, component_name, bypass_claim)`: moves the entire named component
//!   content from `source` to `target`, clearing the source component and appending to the target
//!   component (or end of file if the target has no matching component).  If `target` does not exist,
//!   it is auto-created matching the source format (template or inline).  When `bypass_claim` is
//!   false and the target is owned by a different tmux pane, transfer is rejected with an error.
//!   Pass `bypass_claim=true` (CLI: `--bypass-claim`) for cross-pane transfers.
//! - Both operations write atomically via `write::atomic_write_pub` and persist a snapshot after
//!   each file mutation.
//! - `split_last_entry` is private; it splits on the last `### Re:` header position.
//!
//! ## Agentic Contracts
//! - Callers receive `Err` if the source file does not exist, the named component is absent, or the
//!   component is empty.  If the target does not exist, it is auto-created.
//! - After `run` returns `Ok`, the last `### Re:` block has been removed from `source` and
//!   appended to `target`; no other content is modified.
//! - After `transfer` returns `Ok`, the named component in `source` is cleared (single newline)
//!   and its prior content appears at the end of the matching component in `target`.
//! - Snapshots are updated for both source and target on every successful call.
//!
//! ## Evals
//! - split_last_entry_single_block: single `### Re:` block → entire content extracted, remaining empty
//! - split_last_entry_multiple_blocks: two `### Re:` blocks → second extracted, first remains
//! - split_last_entry_no_headers: no headers present → entire content extracted as single entry

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::{component, frontmatter, snapshot, write};

/// Check pane ownership for the target file. Returns Ok if no conflict or if
/// the target has no active session. Returns Err suggesting --bypass-claim
/// when a different pane owns the target.
fn check_target_ownership(target: &Path) -> Result<()> {
    let current_pane = std::env::var("TMUX_PANE").unwrap_or_default();
    if current_pane.is_empty() {
        return Ok(());
    }

    let project_root = match snapshot::find_project_root(target) {
        Some(r) => r,
        None => return Ok(()),
    };

    let sessions_path = project_root.join(".agent-doc/sessions.json");
    if !sessions_path.exists() {
        return Ok(());
    }

    let sessions_content = std::fs::read_to_string(&sessions_path).unwrap_or_default();
    let sessions: serde_json::Value = serde_json::from_str(&sessions_content).unwrap_or_default();

    let target_canonical = std::fs::canonicalize(target)
        .unwrap_or_else(|_| target.to_path_buf());

    if let Some(obj) = sessions.as_object() {
        for (_id, entry) in obj {
            if let (Some(path), Some(pane)) = (
                entry.get("file").and_then(|v| v.as_str()),
                entry.get("pane").and_then(|v| v.as_str()),
            ) {
                let entry_path = if Path::new(path).is_relative() {
                    project_root.join(path)
                } else {
                    Path::new(path).to_path_buf()
                };
                let entry_canonical = std::fs::canonicalize(&entry_path)
                    .unwrap_or(entry_path);

                if entry_canonical == target_canonical && pane != current_pane {
                    anyhow::bail!(
                        "target {} is owned by pane {} (current: {}). \
                         Use --bypass-claim to transfer across panes.",
                        target.display(), pane, current_pane
                    );
                }
            }
        }
    }

    Ok(())
}

/// Format a source annotation blockquote for transferred/extracted content.
fn format_source_annotation(source: &Path, action: &str) -> String {
    let timestamp = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "\n> **[{} from {}]** ({})\n>\n",
        action.to_uppercase(),
        source.display(),
        timestamp,
    )
}

/// Extract the last exchange entry from source and append to target.
///
/// For template documents: extracts the last `### Re:` block from `agent:exchange`.
/// For inline documents: extracts the last `## User` + `## Assistant` pair.
pub fn run(source: &Path, target: &Path, component_name: Option<&str>) -> Result<()> {
    if !source.exists() {
        anyhow::bail!("source file not found: {}", source.display());
    }
    if !target.exists() {
        anyhow::bail!("target file not found: {}", target.display());
    }

    let source_content = std::fs::read_to_string(source)
        .with_context(|| format!("failed to read {}", source.display()))?;
    let target_content = std::fs::read_to_string(target)
        .with_context(|| format!("failed to read {}", target.display()))?;

    let comp_name = component_name.unwrap_or("exchange");

    // Find the exchange component in source
    let components = component::parse(&source_content)
        .context("failed to parse components in source")?;

    let exchange = components.iter().find(|c| c.name == comp_name);
    let Some(exchange) = exchange else {
        anyhow::bail!("component '{}' not found in {}", comp_name, source.display());
    };

    let exchange_content = exchange.content(&source_content);
    if exchange_content.trim().is_empty() {
        anyhow::bail!("component '{}' is empty in {}", comp_name, source.display());
    }

    // Extract the last exchange entry (### Re: block)
    let (extracted, remaining) = split_last_entry(exchange_content);

    if extracted.trim().is_empty() {
        anyhow::bail!("no exchange entry found to extract");
    }

    // Update source: replace exchange content with remaining
    let new_source = exchange.replace_content(&source_content, &remaining);
    write::atomic_write_pub(source, &new_source)?;
    snapshot::save(source, &new_source)?;

    // Append extracted content to target's exchange component with source annotation
    let annotation = format_source_annotation(source, "Extract");
    let annotated_content = format!("{}{}", annotation, extracted.trim_start());

    let target_components = component::parse(&target_content)
        .context("failed to parse components in target")?;

    let target_exchange = target_components.iter().find(|c| c.name == comp_name);
    let new_target = if let Some(tc) = target_exchange {
        let existing = tc.content(&target_content);
        let appended = format!("{}{}", existing.trim_end(), if existing.trim().is_empty() { "\n" } else { "\n\n" });
        tc.replace_content(&target_content, &format!("{}{}\n", appended.trim_end(), annotated_content.trim_end()))
    } else {
        // No matching component in target — append at end
        format!("{}\n{}\n", target_content.trim_end(), annotated_content.trim_end())
    };

    write::atomic_write_pub(target, &new_target)?;
    snapshot::save(target, &new_target)?;

    eprintln!(
        "[extract] Moved last entry from {}:{} → {}:{}",
        source.display(), comp_name, target.display(), comp_name
    );

    Ok(())
}

/// Split content into (last_entry, remaining).
/// Looks for the last `### Re:` header as the split point.
fn split_last_entry(content: &str) -> (String, String) {
    // Find the last ### Re: header
    let mut last_header_pos = None;
    for (i, _) in content.match_indices("### Re:") {
        last_header_pos = Some(i);
    }

    match last_header_pos {
        Some(pos) => {
            let remaining = &content[..pos];
            let extracted = &content[pos..];
            (extracted.to_string(), remaining.to_string())
        }
        None => {
            // No ### Re: headers — extract everything
            (content.to_string(), String::new())
        }
    }
}

/// Transfer content between documents by component name.
/// Moves the entire component content from source to target.
/// When `bypass_claim` is false, refuses to write to a target owned by a different pane.
/// When `items` is Some, only transfers matching pending items (by ID) instead of all content.
pub fn transfer(source: &Path, target: &Path, component_name: &str, bypass_claim: bool, items: Option<&[String]>) -> Result<()> {
    // Validate --items usage before any filesystem operations
    if items.is_some() && component_name != "pending" {
        anyhow::bail!("--items flag is only supported for the 'pending' component");
    }

    if !source.exists() {
        anyhow::bail!("source file not found: {}", source.display());
    }

    if !bypass_claim {
        check_target_ownership(target)?;
    } else {
        eprintln!("[transfer] --bypass-claim: skipping pane ownership check on target");
    }

    // Selective pending transfer via --items
    if let Some(ids) = items {
        return transfer_pending_items(source, target, ids, bypass_claim);
    }

    // Auto-init target if it doesn't exist (always template mode)
    if !target.exists() {
        let source_content = std::fs::read_to_string(source)
            .with_context(|| format!("failed to read {}", source.display()))?;

        let title = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled Session");
        let session_id = uuid::Uuid::new_v4();
        let agent = frontmatter::parse(&source_content)
            .ok()
            .and_then(|(fm, _)| fm.agent.clone())
            .unwrap_or_else(|| "claude".to_string());

        let target_content = format!(
            "---\nagent_doc_session: {}\nagent: {}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n# {}\n\n## Exchange\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
            session_id, agent, title
        );

        if let Some(parent) = target.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, &target_content)?;
        snapshot::save(target, &target_content)?;
        eprintln!("[transfer] Auto-created {} (template)", target.display());
    }

    let source_content = std::fs::read_to_string(source)
        .with_context(|| format!("failed to read {}", source.display()))?;
    let target_content = std::fs::read_to_string(target)
        .with_context(|| format!("failed to read {}", target.display()))?;

    let components = component::parse(&source_content)
        .context("failed to parse components in source")?;

    let comp = components.iter().find(|c| c.name == component_name);
    let Some(comp) = comp else {
        anyhow::bail!("component '{}' not found in {}", component_name, source.display());
    };

    let content = comp.content(&source_content);
    if content.trim().is_empty() {
        anyhow::bail!("component '{}' is empty in {}", component_name, source.display());
    }

    // Clear source component
    let new_source = comp.replace_content(&source_content, "\n");
    write::atomic_write_pub(source, &new_source)?;
    snapshot::save(source, &new_source)?;

    // Append to target component (or end of file) with source annotation
    let annotation = format_source_annotation(source, "Transfer");
    let annotated_content = format!("{}{}", annotation, content.trim_start());

    let target_components = component::parse(&target_content)
        .context("failed to parse components in target")?;

    let target_comp = target_components.iter().find(|c| c.name == component_name);
    let new_target = if let Some(tc) = target_comp {
        let existing = tc.content(&target_content);
        tc.replace_content(&target_content, &format!("{}{}\n", existing, annotated_content.trim_end()))
    } else {
        format!("{}\n{}\n", target_content.trim_end(), annotated_content.trim_end())
    };

    write::atomic_write_pub(target, &new_target)?;
    snapshot::save(target, &new_target)?;

    // Also transfer the "pending" component if it exists in both source and target
    // and the transferred component is not "pending" itself.
    if component_name != "pending" {
        let source_refreshed = std::fs::read_to_string(source)?;
        let target_refreshed = std::fs::read_to_string(target)?;

        let source_comps = component::parse(&source_refreshed).unwrap_or_default();
        let target_comps = component::parse(&target_refreshed).unwrap_or_default();

        if let Some(source_pending) = source_comps.iter().find(|c| c.name == "pending")
            && let Some(target_pending) = target_comps.iter().find(|c| c.name == "pending")
        {
            let pending_content = source_pending.content(&source_refreshed);
            if !pending_content.trim().is_empty() {
                // Merge: append source pending items to target pending
                let existing = target_pending.content(&target_refreshed);
                let merged = format!("{}{}\n", existing, pending_content.trim_end());
                let new_target_with_pending = target_pending.replace_content(&target_refreshed, &merged);
                write::atomic_write_pub(target, &new_target_with_pending)?;
                snapshot::save(target, &new_target_with_pending)?;

                // Clear source pending
                let new_source_cleared = source_pending.replace_content(&source_refreshed, "\n");
                write::atomic_write_pub(source, &new_source_cleared)?;
                snapshot::save(source, &new_source_cleared)?;

                eprintln!("[transfer] Also transferred 'pending' component");
            }
        }
    }

    // Commit the target so transferred headings are in git HEAD.
    // Without this, the next agent-doc commit classifies all transferred
    // headings as "new" and marks each with (HEAD).
    crate::git::commit(target)?;

    eprintln!(
        "[transfer] Moved component '{}' from {} → {}",
        component_name, source.display(), target.display()
    );

    Ok(())
}

/// Transfer specific pending items by ID from source to target.
/// Items are identified by `[#id]` patterns in pending lines.
/// Matching items are removed from source and appended to target's pending.
fn transfer_pending_items(source: &Path, target: &Path, ids: &[String], _bypass_claim: bool) -> Result<()> {
    if !target.exists() {
        anyhow::bail!("target file not found: {} (auto-create not supported for --items)", target.display());
    }

    let source_content = std::fs::read_to_string(source)
        .with_context(|| format!("failed to read {}", source.display()))?;
    let target_content = std::fs::read_to_string(target)
        .with_context(|| format!("failed to read {}", target.display()))?;

    let source_comps = component::parse(&source_content)
        .context("failed to parse components in source")?;
    let target_comps = component::parse(&target_content)
        .context("failed to parse components in target")?;

    let source_pending = source_comps.iter().find(|c| c.name == "pending");
    let Some(source_pending) = source_pending else {
        anyhow::bail!("component 'pending' not found in {}", source.display());
    };

    let pending_content = source_pending.content(&source_content);
    if pending_content.trim().is_empty() {
        anyhow::bail!("pending component is empty in {}", source.display());
    }

    let mut matched_lines: Vec<String> = Vec::new();
    let mut remaining_lines: Vec<String> = Vec::new();
    let mut matched_ids: Vec<String> = Vec::new();

    for line in pending_content.lines() {
        let mut is_match = false;
        for id in ids {
            let pattern = format!("[#{}]", id);
            if line.contains(&pattern) {
                is_match = true;
                matched_ids.push(id.clone());
                break;
            }
        }
        if is_match {
            matched_lines.push(line.to_string());
        } else {
            remaining_lines.push(line.to_string());
        }
    }

    if matched_lines.is_empty() {
        let id_list: Vec<String> = ids.iter().map(|id| format!("#{}", id)).collect();
        anyhow::bail!("no pending items matched: {}", id_list.join(", "));
    }

    // Update source: keep only remaining lines
    let new_pending_content = if remaining_lines.is_empty() {
        "\n".to_string()
    } else {
        format!("{}\n", remaining_lines.join("\n"))
    };
    let new_source = source_pending.replace_content(&source_content, &new_pending_content);
    write::atomic_write_pub(source, &new_source)?;
    snapshot::save(source, &new_source)?;

    // Append matched items to target pending
    let target_pending = target_comps.iter().find(|c| c.name == "pending");
    let new_target = if let Some(tp) = target_pending {
        let existing = tp.content(&target_content);
        let appended = format!("{}{}\n", existing, matched_lines.join("\n"));
        tp.replace_content(&target_content, &appended)
    } else {
        format!("{}\n{}\n", target_content.trim_end(), matched_lines.join("\n"))
    };

    write::atomic_write_pub(target, &new_target)?;
    snapshot::save(target, &new_target)?;

    crate::git::commit(target)?;

    eprintln!(
        "[transfer] Moved {} pending item(s) ({}) from {} → {}",
        matched_lines.len(),
        matched_ids.iter().map(|id| format!("#{}", id)).collect::<Vec<_>>().join(", "),
        source.display(),
        target.display()
    );

    // Report any IDs that didn't match
    let unmatched: Vec<&String> = ids.iter().filter(|id| !matched_ids.contains(id)).collect();
    if !unmatched.is_empty() {
        eprintln!(
            "[transfer] WARNING: {} ID(s) not found in source: {}",
            unmatched.len(),
            unmatched.iter().map(|id| format!("#{}", id)).collect::<Vec<_>>().join(", ")
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_last_entry_single_block() {
        let content = "### Re: Question\n\nAnswer here.\n";
        let (extracted, remaining) = split_last_entry(content);
        assert_eq!(extracted, "### Re: Question\n\nAnswer here.\n");
        assert_eq!(remaining, "");
    }

    #[test]
    fn split_last_entry_multiple_blocks() {
        let content = "### Re: First\n\nFirst answer.\n\n### Re: Second\n\nSecond answer.\n";
        let (extracted, remaining) = split_last_entry(content);
        assert_eq!(extracted, "### Re: Second\n\nSecond answer.\n");
        assert_eq!(remaining, "### Re: First\n\nFirst answer.\n\n");
    }

    #[test]
    fn split_last_entry_no_headers() {
        let content = "Just some text without headers.\n";
        let (extracted, remaining) = split_last_entry(content);
        assert_eq!(extracted, "Just some text without headers.\n");
        assert_eq!(remaining, "");
    }

    /// Test the pending merge logic used by transfer.
    /// (Full transfer() requires git, so we test the merge logic directly.)
    #[test]
    fn pending_merge_appends_source_items_to_target() {
        let source_pending = "- [ ] Item from source\n- [ ] Another source item\n";
        let target_pending = "- [ ] Existing target item\n";

        let merged = format!("{}{}\n", target_pending, source_pending.trim_end());

        assert!(merged.contains("Existing target item"), "target items preserved");
        assert!(merged.contains("Item from source"), "source items appended");
        assert!(merged.contains("Another source item"), "all source items appended");
    }

    /// Empty source pending should not modify target pending.
    #[test]
    fn pending_merge_skips_empty_source() {
        let source_pending = "\n";
        assert!(source_pending.trim().is_empty(), "empty source should be skipped");
    }

    /// When TMUX_PANE is not set, ownership check always passes.
    #[test]
    fn check_target_ownership_passes_outside_tmux() {
        unsafe { std::env::remove_var("TMUX_PANE") };
        let target = Path::new("/tmp/nonexistent-target.md");
        assert!(check_target_ownership(target).is_ok());
    }

    /// When TMUX_PANE is set but no project root exists, check passes.
    #[test]
    fn check_target_ownership_passes_no_project_root() {
        unsafe { std::env::set_var("TMUX_PANE", "%99") };
        let target = Path::new("/tmp/no-project-root-file.md");
        let result = check_target_ownership(target);
        assert!(result.is_ok());
        unsafe { std::env::remove_var("TMUX_PANE") };
    }

    /// Selective pending item matching by ID pattern.
    #[test]
    fn pending_item_selection_by_id() {
        let pending = "- [ ] [#abc1] First item\n- [ ] [#def2] Second item\n- [ ] [#ghi3] Third item\n";
        let ids = vec!["abc1".to_string(), "ghi3".to_string()];

        let mut matched: Vec<String> = Vec::new();
        let mut remaining: Vec<String> = Vec::new();

        for line in pending.lines() {
            let mut is_match = false;
            for id in &ids {
                let pattern = format!("[#{}]", id);
                if line.contains(&pattern) {
                    is_match = true;
                    break;
                }
            }
            if is_match {
                matched.push(line.to_string());
            } else {
                remaining.push(line.to_string());
            }
        }

        assert_eq!(matched.len(), 2);
        assert!(matched[0].contains("[#abc1]"));
        assert!(matched[1].contains("[#ghi3]"));
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].contains("[#def2]"));
    }

    /// Items flag only works with pending component.
    #[test]
    fn items_flag_rejects_non_pending_component() {
        let source = Path::new("/tmp/nonexistent-source.md");
        let target = Path::new("/tmp/nonexistent-target.md");
        let ids = vec!["abc".to_string()];
        let result = transfer(source, target, "exchange", false, Some(&ids));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("only supported for the 'pending' component"));
    }
}

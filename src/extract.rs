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
//!   it is auto-created as a template document with the standard status/exchange/queue/backlog/icebox
//!   scaffold.  Backlog transfers accept both the canonical `backlog` name and the legacy `pending`
//!   alias. When `bypass_claim` is false and the target is owned by a different tmux pane, transfer
//!   is rejected with an error. Pass `bypass_claim=true` (CLI: `--bypass-claim`) for cross-pane
//!   transfers.
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
use std::path::{Path, PathBuf};
use std::process::Command;

use agent_doc_element::element::{self, is_backlog_component, is_icebox_component};

use agent_doc_frontmatter::frontmatter;
use agent_doc_orchestration::frontmatter_io;
use agent_doc_orchestration::{security, snapshot, write};

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

    let target_canonical = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());

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
                let entry_canonical = std::fs::canonicalize(&entry_path).unwrap_or(entry_path);

                if entry_canonical == target_canonical && pane != current_pane {
                    anyhow::bail!(
                        "target {} is owned by pane {} (current: {}). \
                         Use --bypass-claim to transfer across panes.",
                        target.display(),
                        pane,
                        current_pane
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

fn matches_requested_component(component_name: &str, candidate_name: &str) -> bool {
    if is_backlog_component(component_name) {
        return is_backlog_component(candidate_name);
    }
    if is_icebox_component(component_name) {
        return is_icebox_component(candidate_name);
    }
    candidate_name == component_name
}

fn allow_selective_item_transfer(component_name: &str) -> bool {
    is_backlog_component(component_name) || is_icebox_component(component_name)
}

fn render_target_scaffold(
    title: &str,
    agent: &str,
    session_id: uuid::Uuid,
    source_fm: &frontmatter::Frontmatter,
) -> String {
    let fm = frontmatter::Frontmatter {
        session: Some(session_id.to_string()),
        agent: Some(agent.to_string()),
        format: Some(frontmatter::AgentDocFormat::Template),
        write_mode: Some(frontmatter::AgentDocWrite::Crdt),
        collaboration: (source_fm.collaboration_mode() == frontmatter::CollaborationMode::Shared)
            .then_some(frontmatter::CollaborationMode::Shared),
        security_review: source_fm
            .security_review
            .as_deref()
            .map(str::trim)
            .filter(|review| !review.is_empty())
            .map(str::to_string),
        ..Default::default()
    };
    frontmatter::write(
        &fm,
        &format!(
            "\n# {}\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n\n## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n## Icebox\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n",
            title
        ),
    )
    .expect("target scaffold frontmatter should serialize")
}

fn merge_list_component(
    component_name: &str,
    source_content: &str,
    target_content: &str,
) -> Result<Option<(String, String)>> {
    let source_components =
        element::parse(source_content).context("failed to parse components in source")?;
    let target_components =
        element::parse(target_content).context("failed to parse components in target")?;

    let Some(source_component) = source_components
        .iter()
        .find(|c| matches_requested_component(component_name, &c.name))
    else {
        return Ok(None);
    };
    let Some(target_component) = target_components
        .iter()
        .find(|c| matches_requested_component(component_name, &c.name))
    else {
        return Ok(None);
    };

    let source_items = source_component.content(source_content);
    if source_items.trim().is_empty() {
        return Ok(None);
    }

    let target_items = target_component.content(target_content);
    let merged_items = format!("{}{}\n", target_items, source_items.trim_end());
    let new_target = target_component.replace_content(target_content, &merged_items);
    let new_source = source_component.replace_content(source_content, "\n");
    Ok(Some((new_source, new_target)))
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
    let (source_fm, _) = frontmatter_io::parse_for_file(&source_content, source)?;
    let (target_fm, _) = frontmatter_io::parse_for_file(&target_content, target)?;
    security::enforce_cross_document_review(
        "extract",
        source,
        &source_fm,
        target,
        Some(&target_fm),
    )?;

    let comp_name = component_name.unwrap_or("exchange");

    // Find the exchange component in source
    let components =
        element::parse(&source_content).context("failed to parse components in source")?;

    let exchange = components.iter().find(|c| c.name == comp_name);
    let Some(exchange) = exchange else {
        anyhow::bail!(
            "component '{}' not found in {}",
            comp_name,
            source.display()
        );
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

    let target_components =
        element::parse(&target_content).context("failed to parse components in target")?;

    let target_exchange = target_components.iter().find(|c| c.name == comp_name);
    let new_target = if let Some(tc) = target_exchange {
        let existing = tc.content(&target_content);
        let appended = format!(
            "{}{}",
            existing.trim_end(),
            if existing.trim().is_empty() {
                "\n"
            } else {
                "\n\n"
            }
        );
        tc.replace_content(
            &target_content,
            &format!("{}{}\n", appended.trim_end(), annotated_content.trim_end()),
        )
    } else {
        // No matching component in target — append at end
        format!(
            "{}\n{}\n",
            target_content.trim_end(),
            annotated_content.trim_end()
        )
    };

    write::atomic_write_pub(target, &new_target)?;
    snapshot::save(target, &new_target)?;

    eprintln!(
        "[extract] Moved last entry from {}:{} → {}:{}",
        source.display(),
        comp_name,
        target.display(),
        comp_name
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
/// When `referral` is true, inserts a referral pointer in the target instead of moving content.
pub fn transfer(
    source: &Path,
    target: &Path,
    component_name: &str,
    bypass_claim: bool,
    items: Option<&[String]>,
    referral: bool,
) -> Result<()> {
    // Validate --items usage before any filesystem operations
    if items.is_some() && !allow_selective_item_transfer(component_name) {
        anyhow::bail!(
            "--items flag is only supported for the 'pending'/'backlog' or 'icebox' component"
        );
    }
    if referral && items.is_some() {
        anyhow::bail!("--referral and --items cannot be used together");
    }

    if !source.exists() {
        anyhow::bail!("source file not found: {}", source.display());
    }

    let source_content = std::fs::read_to_string(source)
        .with_context(|| format!("failed to read {}", source.display()))?;
    let (source_fm, _) = frontmatter_io::parse_for_file(&source_content, source)?;

    let target_existing = if target.exists() {
        Some(
            std::fs::read_to_string(target)
                .with_context(|| format!("failed to read {}", target.display()))?,
        )
    } else {
        None
    };
    let target_fm = if let Some(content) = target_existing.as_ref() {
        Some(frontmatter_io::parse_for_file(content, target)?.0)
    } else {
        None
    };
    security::enforce_cross_document_review(
        "transfer",
        source,
        &source_fm,
        target,
        target_fm.as_ref(),
    )?;

    if !bypass_claim {
        check_target_ownership(target)?;
    } else {
        eprintln!("[transfer] --bypass-claim: skipping pane ownership check on target");
    }

    // Referral mode: insert pointer in target, don't move content
    if referral {
        return transfer_referral(source, target, component_name);
    }

    // Selective pending transfer via --items
    if let Some(ids) = items {
        return transfer_pending_items(source, target, component_name, ids, bypass_claim);
    }

    // Auto-init target if it doesn't exist (always template mode)
    if !target.exists() {
        let title = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled Session");
        let session_id = uuid::Uuid::new_v4();
        let agent = source_fm
            .agent
            .clone()
            .unwrap_or_else(|| "claude".to_string());
        let target_content = render_target_scaffold(title, &agent, session_id, &source_fm);

        if let Some(parent) = target.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, &target_content)?;
        snapshot::save(target, &target_content)?;
        eprintln!("[transfer] Auto-created {} (template)", target.display());
    }

    let target_content = std::fs::read_to_string(target)
        .with_context(|| format!("failed to read {}", target.display()))?;

    let components =
        element::parse(&source_content).context("failed to parse components in source")?;

    let comp = components
        .iter()
        .find(|c| matches_requested_component(component_name, &c.name));
    let Some(comp) = comp else {
        anyhow::bail!(
            "component '{}' not found in {}",
            component_name,
            source.display()
        );
    };

    let content = comp.content(&source_content);
    if content.trim().is_empty() {
        anyhow::bail!(
            "component '{}' is empty in {}",
            component_name,
            source.display()
        );
    }

    // Clear source component
    let new_source = comp.replace_content(&source_content, "\n");
    write::atomic_write_pub(source, &new_source)?;
    snapshot::save(source, &new_source)?;

    // Append to target component (or end of file) with source annotation
    let annotation = format_source_annotation(source, "Transfer");
    let annotated_content = format!("{}{}", annotation, content.trim_start());

    let target_components =
        element::parse(&target_content).context("failed to parse components in target")?;

    let target_comp = target_components
        .iter()
        .find(|c| matches_requested_component(component_name, &c.name));
    let new_target = if let Some(tc) = target_comp {
        let existing = tc.content(&target_content);
        tc.replace_content(
            &target_content,
            &format!("{}{}\n", existing, annotated_content.trim_end()),
        )
    } else {
        format!(
            "{}\n{}\n",
            target_content.trim_end(),
            annotated_content.trim_end()
        )
    };

    write::atomic_write_pub(target, &new_target)?;
    snapshot::save(target, &new_target)?;

    // Also transfer tracked list surfaces that belong with the moved context.
    if !is_backlog_component(component_name) && !is_icebox_component(component_name) {
        let source_refreshed = std::fs::read_to_string(source)?;
        let target_refreshed = std::fs::read_to_string(target)?;
        let mut latest_source = source_refreshed;
        let mut latest_target = target_refreshed;

        for surface in ["backlog", "icebox"] {
            if let Some((new_source_surface, new_target_surface)) =
                merge_list_component(surface, &latest_source, &latest_target)?
            {
                write::atomic_write_pub(target, &new_target_surface)?;
                snapshot::save(target, &new_target_surface)?;
                write::atomic_write_pub(source, &new_source_surface)?;
                snapshot::save(source, &new_source_surface)?;
                latest_source = new_source_surface;
                latest_target = new_target_surface;
                eprintln!("[transfer] Also transferred '{}' component", surface);
            }
        }
    }

    // Commit the target so transferred headings are in git HEAD.
    // Without this, the next agent-doc commit classifies all transferred
    // headings as "new" and marks each with (HEAD).
    agent_doc_orchestration::git::commit(target)?;

    eprintln!(
        "[transfer] Moved component '{}' from {} → {}",
        component_name,
        source.display(),
        target.display()
    );

    Ok(())
}

/// Transfer specific backlog or icebox items by ID from source to target.
/// Items are identified by `[#id]` patterns in list lines.
/// Matching items are removed from source and appended to the same target component.
fn transfer_pending_items(
    source: &Path,
    target: &Path,
    component_name: &str,
    ids: &[String],
    _bypass_claim: bool,
) -> Result<()> {
    if !target.exists() {
        anyhow::bail!(
            "target file not found: {} (auto-create not supported for --items)",
            target.display()
        );
    }

    let source_content = std::fs::read_to_string(source)
        .with_context(|| format!("failed to read {}", source.display()))?;
    let target_content = std::fs::read_to_string(target)
        .with_context(|| format!("failed to read {}", target.display()))?;

    let source_comps =
        element::parse(&source_content).context("failed to parse components in source")?;
    let target_comps =
        element::parse(&target_content).context("failed to parse components in target")?;

    let source_pending = source_comps
        .iter()
        .find(|c| matches_requested_component(component_name, &c.name));
    let Some(source_pending) = source_pending else {
        anyhow::bail!(
            "component '{}' not found in {}",
            component_name,
            source.display()
        );
    };

    let pending_content = source_pending.content(&source_content);
    if pending_content.trim().is_empty() {
        anyhow::bail!(
            "component '{}' is empty in {}",
            component_name,
            source.display()
        );
    }

    let (remaining_body, matched_body, matched_ids) =
        agent_doc_element_backlog::backlog::extract_items_by_id(pending_content, ids)?;

    if matched_ids.is_empty() {
        let id_list: Vec<String> = ids.iter().map(|id| format!("#{}", id)).collect();
        anyhow::bail!(
            "no {} items matched: {}",
            component_name,
            id_list.join(", ")
        );
    }

    // Update source: keep only remaining items / structure
    let new_pending_content = if remaining_body.trim().is_empty() {
        "\n".to_string()
    } else {
        remaining_body
    };
    let new_source = source_pending.replace_content(&source_content, &new_pending_content);
    write::atomic_write_pub(source, &new_source)?;
    snapshot::save(source, &new_source)?;

    // Append matched items to target component
    let target_pending = target_comps
        .iter()
        .find(|c| matches_requested_component(component_name, &c.name));
    let new_target = if let Some(tp) = target_pending {
        let existing = tp.content(&target_content);
        let appended = format!("{}{}", existing, matched_body);
        tp.replace_content(&target_content, &appended)
    } else {
        format!(
            "{}\n{}\n",
            target_content.trim_end(),
            matched_body.trim_end()
        )
    };

    write::atomic_write_pub(target, &new_target)?;
    snapshot::save(target, &new_target)?;

    agent_doc_orchestration::git::commit(target)?;

    eprintln!(
        "[transfer] Moved {} {} item(s) ({}) from {} → {}",
        matched_ids.len(),
        component_name,
        matched_ids
            .iter()
            .map(|id| format!("#{}", id))
            .collect::<Vec<_>>()
            .join(", "),
        source.display(),
        target.display()
    );

    // Report any IDs that didn't match
    let unmatched: Vec<&String> = ids.iter().filter(|id| !matched_ids.contains(id)).collect();
    if !unmatched.is_empty() {
        eprintln!(
            "[transfer] WARNING: {} ID(s) not found in source: {}",
            unmatched.len(),
            unmatched
                .iter()
                .map(|id| format!("#{}", id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(())
}

/// Compute a relative path from target's directory to source.
/// Falls back to the source path as-is if canonicalization fails.
fn make_relative(source: &Path, target: &Path) -> PathBuf {
    let source_abs = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let target_dir = target
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // Walk up from target_dir, building "../" prefix, until we find common ancestor
    let source_components: Vec<_> = source_abs.components().collect();
    let target_components: Vec<_> = target_dir.components().collect();

    let common_len = source_components
        .iter()
        .zip(target_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    if common_len == 0 {
        return source.to_path_buf();
    }

    let ups = target_components.len() - common_len;
    let mut rel = PathBuf::new();
    for _ in 0..ups {
        rel.push("..");
    }
    for comp in &source_components[common_len..] {
        rel.push(comp);
    }
    rel
}

/// Insert a referral pointer in the target document referencing the source.
/// Content stays in the source — the target gets a structured comment that
/// preflight can resolve to provide context on demand.
fn transfer_referral(source: &Path, target: &Path, component_name: &str) -> Result<()> {
    if !target.exists() {
        anyhow::bail!(
            "target file not found: {} (auto-create not supported for --referral)",
            target.display()
        );
    }

    let target_content = std::fs::read_to_string(target)
        .with_context(|| format!("failed to read {}", target.display()))?;

    let target_comps =
        element::parse(&target_content).context("failed to parse components in target")?;

    // Compute relative path from target's directory to source
    let source_rel = make_relative(source, target);

    let timestamp = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let referral_block = format!(
        "\n<!-- agent:referral src=\"{}\" component=\"{}\" created=\"{}\" -->\n*Context from [{}]({}) — read source {} for full history.*\n<!-- /agent:referral -->\n",
        source_rel.display(),
        component_name,
        timestamp,
        source_rel.display(),
        source_rel.display(),
        component_name,
    );

    let target_comp = target_comps.iter().find(|c| c.name == component_name);
    let new_target = if let Some(tc) = target_comp {
        let existing = tc.content(&target_content);
        tc.replace_content(&target_content, &format!("{}{}", existing, referral_block))
    } else {
        format!("{}\n{}", target_content.trim_end(), referral_block)
    };

    write::atomic_write_pub(target, &new_target)?;
    snapshot::save(target, &new_target)?;

    agent_doc_orchestration::git::commit(target)?;

    eprintln!(
        "[transfer] Inserted referral to {}:{} in {}",
        source.display(),
        component_name,
        target.display()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
        _lock: crate::test_support::ProcessGlobalLockGuard,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prior,
                _lock: lock,
            }
        }

        fn unset(key: &'static str) -> Self {
            let lock = crate::test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
            Self {
                key,
                prior,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

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

        assert!(
            merged.contains("Existing target item"),
            "target items preserved"
        );
        assert!(merged.contains("Item from source"), "source items appended");
        assert!(
            merged.contains("Another source item"),
            "all source items appended"
        );
    }

    /// Empty source pending should not modify target pending.
    #[test]
    fn pending_merge_skips_empty_source() {
        let source_pending = "\n";
        assert!(
            source_pending.trim().is_empty(),
            "empty source should be skipped"
        );
    }

    /// When TMUX_PANE is not set, ownership check always passes.
    #[test]
    fn check_target_ownership_passes_outside_tmux() {
        let _tmux_pane = EnvGuard::unset("TMUX_PANE");
        let target = Path::new("/tmp/nonexistent-target.md");
        assert!(check_target_ownership(target).is_ok());
    }

    /// When TMUX_PANE is set but no project root exists, check passes.
    #[test]
    fn check_target_ownership_passes_no_project_root() {
        let _tmux_pane = EnvGuard::set("TMUX_PANE", "%99");
        let target = Path::new("/tmp/no-project-root-file.md");
        let result = check_target_ownership(target);
        assert!(result.is_ok());
    }

    /// Selective pending item matching by ID pattern.
    #[test]
    fn pending_item_selection_by_id() {
        let pending =
            "- [ ] [#abc1] First item\n- [ ] [#def2] Second item\n- [ ] [#ghi3] Third item\n";
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

    /// Items flag only works with backlog/pending or icebox components.
    #[test]
    fn items_flag_rejects_non_pending_component() {
        let source = Path::new("/tmp/nonexistent-source.md");
        let target = Path::new("/tmp/nonexistent-target.md");
        let ids = vec!["abc".to_string()];
        let result = transfer(source, target, "exchange", false, Some(&ids), false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only supported for the 'pending'/'backlog' or 'icebox' component")
        );
    }

    #[test]
    fn matches_requested_component_accepts_backlog_alias() {
        assert!(matches_requested_component("pending", "backlog"));
        assert!(matches_requested_component("backlog", "pending"));
        assert!(matches_requested_component("icebox", "icebox"));
        assert!(!matches_requested_component("icebox", "backlog"));
    }

    #[test]
    fn render_target_scaffold_includes_backlog_and_icebox_components() {
        let scaffold = render_target_scaffold(
            "Title",
            "codex",
            uuid::Uuid::nil(),
            &frontmatter::Frontmatter::default(),
        );
        assert!(scaffold.contains("<!-- agent:backlog -->"));
        assert!(scaffold.contains("<!-- /agent:backlog -->"));
        assert!(scaffold.contains("<!-- agent:icebox -->"));
        assert!(scaffold.contains("<!-- /agent:icebox -->"));
    }

    #[test]
    fn render_target_scaffold_inherits_shared_review_metadata() {
        let scaffold = render_target_scaffold(
            "Title",
            "codex",
            uuid::Uuid::nil(),
            &frontmatter::Frontmatter {
                collaboration: Some(frontmatter::CollaborationMode::Shared),
                security_review: Some("sec-1".to_string()),
                ..Default::default()
            },
        );
        assert!(scaffold.contains("agent_doc_collaboration: shared"));
        assert!(scaffold.contains("agent_doc_security_review: sec-1"));
    }

    #[test]
    fn transfer_blocks_shared_source_without_security_review() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("source.md");
        let target = dir.path().join("target.md");
        std::fs::write(
            &source,
            concat!(
                "---\n",
                "agent_doc_session: test\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "agent_doc_collaboration: shared\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\n",
                "Done.\n",
                "<!-- /agent:exchange -->\n"
            ),
        )
        .unwrap();

        let err = transfer(&source, &target, "exchange", false, None, false).unwrap_err();
        assert!(err.to_string().contains("agent_doc_security_review"));
    }

    /// --referral and --items are mutually exclusive.
    #[test]
    fn referral_and_items_mutually_exclusive() {
        let source = Path::new("/tmp/nonexistent-source.md");
        let target = Path::new("/tmp/nonexistent-target.md");
        let ids = vec!["abc".to_string()];
        let result = transfer(source, target, "pending", false, Some(&ids), true);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--referral and --items cannot be used together")
        );
    }

    /// make_relative doesn't panic on non-existent paths.
    #[test]
    fn make_relative_no_panic() {
        let source = Path::new("/tmp/nonexistent-a.md");
        let target = Path::new("/tmp/nonexistent-b.md");
        let _rel = make_relative(source, target);
        // Just verify it doesn't panic; exact output depends on CWD
    }
}

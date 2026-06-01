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
use crate::component::{is_backlog_component, is_review_component, is_tracked_work_component};
use crate::pending;
use crate::snapshot;

fn trim_tracked_parent_prefix(line: &str) -> &str {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return rest.trim_start();
    }

    let digit_len = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digit_len == 0 {
        return trimmed;
    }
    let (_, tail) = trimmed.split_at(digit_len);
    let Some(tail) = tail.strip_prefix('.') else {
        return trimmed;
    };
    if !tail.starts_with(char::is_whitespace) {
        return trimmed;
    }
    tail.trim_start()
}

fn line_is_legacy_done_item(line: &str) -> bool {
    let trimmed = line.trim();
    let after_marker = trim_tracked_parent_prefix(line);
    trimmed.starts_with("\u{2705}")
        || after_marker.starts_with("[x]")
        || after_marker.starts_with("[X]")
        || after_marker.starts_with("[done]")
}

fn find_pending_component(file: &Path) -> Result<(String, component::Component)> {
    let content = std::fs::read_to_string(file).context("failed to read document")?;
    let components = component::parse(&content).context("failed to parse components")?;
    let comp = components
        .into_iter()
        .find(|c| is_backlog_component(&c.name))
        .context("document has no backlog/pending component")?;
    Ok((content, comp))
}

fn find_review_component_in_content(content: &str) -> Result<Option<component::Component>> {
    let components = component::parse(content).context("failed to parse components")?;
    Ok(components
        .into_iter()
        .find(|c| is_review_component(&c.name)))
}

fn insert_empty_review_after_backlog(content: &str) -> Result<String> {
    let components = component::parse(content).context("failed to parse components")?;
    if components.iter().any(|c| is_review_component(&c.name)) {
        return Ok(content.to_string());
    }
    let backlog = components
        .iter()
        .find(|c| is_backlog_component(&c.name))
        .context("document has no backlog/pending component for review insertion")?;
    let insert = "\n## Review\n\n<!-- agent:review -->\n<!-- /agent:review -->\n";
    let mut out = String::with_capacity(content.len() + insert.len());
    out.push_str(&content[..backlog.close_end]);
    out.push_str(insert);
    out.push_str(&content[backlog.close_end..]);
    Ok(out)
}

fn ensure_review_component(content: &str) -> Result<(String, component::Component)> {
    let content = insert_empty_review_after_backlog(content)?;
    let comp = find_review_component_in_content(&content)?
        .context("document has no review component after insertion")?;
    Ok((content, comp))
}

fn find_component_containing_open_id(
    file: &Path,
    id: &str,
) -> Result<(String, component::Component)> {
    let id = pending::normalize_pending_id(id);
    let content = std::fs::read_to_string(file).context("failed to read document")?;
    let components = component::parse(&content).context("failed to parse components")?;
    let comp = components
        .into_iter()
        .find(|c| {
            if !is_tracked_work_component(&c.name) {
                return false;
            }
            let (_, items, _) = pending::parse_items(c.content(&content));
            items
                .into_iter()
                .any(|item| item.id == id && item.state != pending::PendingState::Done)
        })
        .with_context(|| format!("id not found in backlog/icebox: {}", id))?;
    Ok((content, comp))
}

pub fn open_item_component_name(file: &Path, id: &str) -> Result<Option<String>> {
    let id = pending::normalize_pending_id(id);
    let content = std::fs::read_to_string(file).context("failed to read document")?;
    let components = component::parse(&content).context("failed to parse components")?;
    for comp in components {
        if !is_tracked_work_component(&comp.name) {
            continue;
        }
        let (_, items, _) = pending::parse_items(comp.content(&content));
        if items
            .into_iter()
            .any(|item| item.id == id && item.state != pending::PendingState::Done)
        {
            return Ok(Some(comp.name));
        }
    }
    Ok(None)
}

fn tracked_work_id_already_resolved(file: &Path, id: &str) -> Result<bool> {
    let id = pending::normalize_pending_id(id);
    if id.is_empty() {
        return Ok(false);
    }
    if crate::cycle_state::resolved_pending_ids(file)?.contains(&id) {
        return Ok(true);
    }

    let content = std::fs::read_to_string(file).context("failed to read document")?;
    let components = component::parse(&content).context("failed to parse components")?;
    let archive_ref = format!("[#{}]", id);
    for comp in components {
        let body = comp.content(&content);
        if crate::component::is_backlog_done_component(&comp.name)
            && body
                .lines()
                .any(|line| line.to_ascii_lowercase().contains(&archive_ref))
        {
            return Ok(true);
        }
        if is_tracked_work_component(&comp.name) {
            let (_, items, _) = pending::parse_items(body);
            if items
                .into_iter()
                .any(|item| item.id == id && item.state == pending::PendingState::Done)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Compute a stable document id from a file path. Uses `snapshot::doc_hash` so
/// the id is consistent across pending ops and backfill.
pub fn doc_id_for(file: &Path) -> String {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    snapshot::doc_hash(&canonical).unwrap_or_else(|_| file.display().to_string())
}

fn canonicalize_component_content(file: &Path, content: &str) -> String {
    let doc_id = doc_id_for(file);
    let (canonical, _) = pending::backfill(content, &doc_id, &HashSet::new());
    canonical
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
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
    std::fs::write(file, &new_doc)?;
    let _ = id;
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
        let canonical = canonicalize_component_content(file, &new_content);
        let new_doc = comp.replace_content(&full_content, &canonical);
        std::fs::write(file, &new_doc)?;
        ids.push(id);
    }
    ids.reverse();
    Ok(ids)
}

/// `#ah0s`: insert a new item at an explicit position relative to the active
/// list (after/before an anchor id, or at the tail), instead of the front
/// default. Returns the assigned id.
fn add_at(file: &Path, item: &str, position: pending::AddPosition<'_>) -> Result<String> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = doc_id_for(file);
    let (new_content, id) = pending::op_add_at(existing, item, &doc_id, false, position)?;
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
    std::fs::write(file, &new_doc)?;
    Ok(id)
}

/// `#ah0s`: `--pending-add-after <id> "<text>"`. Repeatable; chaining
/// after A "B" then after B "C" builds A→B→C deterministically.
pub fn add_after(file: &Path, anchor_id: &str, item: &str) -> Result<String> {
    add_at(file, item, pending::AddPosition::After(anchor_id))
}

/// `#ah0s`: `--pending-add-before <id> "<text>"`.
pub fn add_before(file: &Path, anchor_id: &str, item: &str) -> Result<String> {
    add_at(file, item, pending::AddPosition::Before(anchor_id))
}

/// `#ah0s`: `--pending-add-back "<text>"` (alias `--pending-append`) — tail insert.
pub fn add_back(file: &Path, item: &str) -> Result<String> {
    add_at(file, item, pending::AddPosition::Last)
}

/// Summary of an `agent-doc review ungate-tasks` run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UngateTasksReport {
    /// Gated review items scanned.
    pub scanned: usize,
    /// Review item ids a new backlog ungate task was added for.
    pub added: Vec<String>,
    /// Review item ids already covered by an existing ungate task.
    pub skipped: Vec<String>,
}

/// Stable body text for a generated ungate backlog task, so re-runs dedup
/// against their own prior output idempotently.
fn ungate_task_text(normalized_id: &str) -> String {
    format!(
        "[recommended] Ungate review item #{} — validate and move to done",
        normalized_id
    )
}

/// `#jb-run-agent-doc-response-queue-contamination` sibling feature: for each
/// gated item in `agent:review`, ensure a backlog follow-up task exists to drive
/// it back out of review (ungate → done), so gated items are not stranded.
/// On-demand and idempotent: a review id whose ungate task already exists in the
/// backlog is skipped.
pub fn add_ungate_tasks_for_review(file: &Path) -> Result<UngateTasksReport> {
    let content = std::fs::read_to_string(file)?;
    let components = crate::component::parse(&content)?;

    let gated_review_ids: Vec<String> = components
        .iter()
        .filter(|c| crate::component::is_review_component(&c.name))
        .flat_map(|c| {
            let (_, items, _) = pending::parse_items(c.content(&content));
            items
        })
        .filter(|item| item.state == pending::PendingState::Gated)
        .map(|item| pending::normalize_pending_id(&item.id))
        .filter(|id| !id.is_empty())
        .collect();

    let backlog_text: String = components
        .iter()
        .filter(|c| crate::component::is_backlog_component(&c.name))
        .map(|c| c.content(&content).to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let mut report = UngateTasksReport {
        scanned: gated_review_ids.len(),
        ..Default::default()
    };
    let mut seen = std::collections::HashSet::new();
    let mut to_add: Vec<String> = Vec::new();
    for id in gated_review_ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        // Dedup against the existing backlog: an ungate task (this command's own
        // generated text, or any backlog item naming `ungate` + `#id`) counts.
        let task_text = ungate_task_text(&id);
        let id_marker = format!("#{}", id);
        let already_tracked = backlog_text.contains(&task_text)
            || backlog_text.lines().any(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("ungate") && line.contains(&id_marker)
            });
        if already_tracked {
            report.skipped.push(id);
        } else {
            to_add.push(task_text);
            report.added.push(id);
        }
    }
    if !to_add.is_empty() {
        add_many(file, &to_add, false)?;
    }
    Ok(report)
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
    let (full_content, comp) = match find_component_containing_open_id(file, id) {
        Ok(found) => found,
        Err(_) if tracked_work_id_already_resolved(file, id)? => {
            eprintln!(
                "[pending] done: id [#{}] is already resolved; leaving backlog unchanged",
                pending::normalize_pending_id(id)
            );
            return Ok(());
        }
        Err(err) => return Err(err),
    };
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_done(existing, id)?;
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Transition an item to `Gated` (`[/]`) by id.
/// Idempotent on already-gated items; errors on `Done` items.
pub fn gate(file: &Path, id: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let (new_backlog, mut item) = match pending::op_take_item(existing, id) {
        Ok(found) => found,
        Err(_) => {
            if let Some(review_comp) = find_review_component_in_content(&full_content)? {
                let (_, review_items, _) = pending::parse_items(review_comp.content(&full_content));
                let normalized = pending::normalize_pending_id(id);
                if review_items
                    .into_iter()
                    .any(|item| item.id == normalized && !item.is_done())
                {
                    return Ok(());
                }
            }
            return gate_in_place(file, id);
        }
    };

    match pending::validate_transition(item.state, pending::PendingOp::Gate)? {
        pending::TransitionResult::Transition(state) => item.state = state,
        pending::TransitionResult::NoOp => {}
    }
    item.gate_type = None;

    let mut new_doc = comp.replace_content(
        &full_content,
        &canonicalize_component_content(file, &new_backlog),
    );
    let (content_with_review, review_comp) = ensure_review_component(&new_doc)?;
    new_doc = content_with_review;
    let review_body = review_comp.content(&new_doc);
    let new_review = pending::op_insert_item_first(review_body, item);
    let review_comp = find_review_component_in_content(&new_doc)?
        .context("document has no review component after insertion")?;
    let new_doc =
        review_comp.replace_content(&new_doc, &canonicalize_component_content(file, &new_review));
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Add a new review item directly to `agent:review`.
pub fn review_add(file: &Path, item: &str) -> Result<()> {
    let full_content = std::fs::read_to_string(file).context("failed to read document")?;
    let (full_content, comp) = ensure_review_component(&full_content)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = doc_id_for(file);
    let (new_content, _id) = pending::op_add(existing, item, &doc_id, true)?;
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Edit a review item's text, preserving its hash id.
pub fn review_edit(file: &Path, id: &str, text: &str) -> Result<()> {
    let full_content = std::fs::read_to_string(file).context("failed to read document")?;
    let comp = find_review_component_in_content(&full_content)?
        .context("document has no review component")?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_edit(existing, id, text)?;
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Transition an item back to `Open` (`[ ]`) by id.
/// Errors on `Open` or `Done` items — the source must be `[/]`.
pub fn ungate(file: &Path, id: &str) -> Result<()> {
    let full_content = std::fs::read_to_string(file).context("failed to read document")?;
    let Some(review_comp) = find_review_component_in_content(&full_content)? else {
        let (full_content, comp) = find_pending_component(file)?;
        let existing = &full_content[comp.open_end..comp.close_start];
        let new_content = pending::op_ungate(existing, id)?;
        let canonical = canonicalize_component_content(file, &new_content);
        let new_doc = comp.replace_content(&full_content, &canonical);
        std::fs::write(file, &new_doc)?;
        return Ok(());
    };
    let review_body = review_comp.content(&full_content);
    let (new_review, mut item) = match pending::op_take_item(review_body, id) {
        Ok(found) => found,
        Err(_) => {
            let (full_content, comp) = find_pending_component(file)?;
            let existing = &full_content[comp.open_end..comp.close_start];
            let new_content = pending::op_ungate(existing, id)?;
            let canonical = canonicalize_component_content(file, &new_content);
            let new_doc = comp.replace_content(&full_content, &canonical);
            std::fs::write(file, &new_doc)?;
            return Ok(());
        }
    };

    match pending::validate_transition(item.state, pending::PendingOp::Ungate)? {
        pending::TransitionResult::Transition(state) => item.state = state,
        pending::TransitionResult::NoOp => {}
    }
    item.gate_type = None;

    let new_doc = review_comp.replace_content(
        &full_content,
        &canonicalize_component_content(file, &new_review),
    );
    let components = component::parse(&new_doc).context("failed to parse components")?;
    let backlog_comp = components
        .into_iter()
        .find(|c| is_backlog_component(&c.name))
        .context("document has no backlog/pending component")?;
    let backlog_body = backlog_comp.content(&new_doc);
    let new_backlog = pending::op_insert_item_first(backlog_body, item);
    let new_doc = backlog_comp.replace_content(
        &new_doc,
        &canonicalize_component_content(file, &new_backlog),
    );
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Legacy in-place gated-state transition retained for tests and old docs that
/// do not yet have a review component.
fn gate_in_place(file: &Path, id: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_gate(existing, id)?;
    if new_content == existing {
        return Ok(());
    }
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Edit an item's text, preserving its hash id.
pub fn edit(file: &Path, id: &str, text: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = pending::op_edit(existing, id, text)?;
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
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
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

/// Reap `[x]` items and print removed ids.
pub fn reap(file: &Path) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = doc_id_for(file);
    let (canonical_content, changed) = pending::backfill(existing, &doc_id, &HashSet::new());
    let (new_content, removed_items) = pending::reap_with_items(&canonical_content)?;
    let removed: Vec<String> = removed_items.iter().map(|item| item.id.clone()).collect();
    let final_content = if removed.is_empty() {
        canonical_content
    } else {
        new_content
    };
    if !changed && removed.is_empty() {
        eprintln!("[pending] no [x] items to reap");
        return Ok(());
    }
    let canonical = canonicalize_component_content(file, &final_content);
    let mut new_doc = comp.replace_content(&full_content, &canonical);
    if !removed_items.is_empty() {
        let archived = crate::preflight::archive_pending_done(file, &new_doc, &removed_items)
            .context("failed to archive reaped item(s) to agent:done")?
            .context("failed to archive reaped item(s) to agent:done")?;
        new_doc = archived;
    }
    if let Some(reconciled) = crate::status_cmd::reconcile_top_backlog_status_content(&new_doc)? {
        new_doc = reconciled;
    }
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
                let trimmed = trim_tracked_parent_prefix(line);
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
        .filter(|line| !line_is_legacy_done_item(line))
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
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
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
    let canonical = canonicalize_component_content(file, &new_content);
    let new_doc = comp.replace_content(&full_content, &canonical);
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
    use std::{fs, path::PathBuf};
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

    fn doc_with_pending_and_icebox(pending_items: &str, icebox_items: &str) -> (TempDir, PathBuf) {
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:pending -->\n{}\n<!-- /agent:pending -->\n\n<!-- agent:icebox -->\n{}\n<!-- /agent:icebox -->\n",
            pending_items, icebox_items
        );
        let (tmp, doc) = setup_test_dir();
        fs::write(&doc, content).unwrap();
        (tmp, doc)
    }

    fn doc_with_pending_and_archive(
        pending_items: &str,
        archive_items: &str,
    ) -> (TempDir, PathBuf) {
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:pending -->\n{}\n<!-- /agent:pending -->\n\n<!-- agent:done -->\n{}\n<!-- /agent:done -->\n",
            pending_items, archive_items
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
    fn remove_exact_match_supports_ordered_parent_items() {
        let (_tmp, doc) = doc_with_pending("1. [ ] [#abcd] first item\n2. [ ] [#efgh] second item");
        remove(&doc, "[ ] [#abcd] first item", false).unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("[#abcd] first item"));
        assert!(content.contains("[#efgh] second item"));
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
    fn prune_removes_checked_ordered_items() {
        let (_tmp, doc) = doc_with_pending("1. [ ] active\n2. [x] done");
        prune(&doc).unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("1. [ ] active"));
        assert!(!content.contains("2. [x] done"));
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
    fn done_marks_icebox_item_when_backlog_does_not_contain_id() {
        let (_tmp, doc) = doc_with_pending_and_icebox(
            "- [ ] [#keep1] Keep backlog item\n",
            "- [ ] [#ice01] Parked follow-up\n",
        );

        done(&doc, "ice01").unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("<!-- agent:pending -->\n- [ ] [#keep1] Keep backlog item\n"));
        assert!(content.contains("<!-- agent:icebox -->\n- [x] [#ice01] Parked follow-up\n"));
    }

    #[test]
    fn done_noops_when_item_was_already_archived() {
        let (_tmp, doc) = doc_with_pending_and_archive(
            "- [ ] [#keep1] Keep backlog item\n",
            "- 2026-05-09 [#done1] Already completed\n",
        );
        done(&doc, "done1").unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- [ ] [#keep1] Keep backlog item"));
        assert!(content.contains("- 2026-05-09 [#done1] Already completed"));
    }

    #[test]
    fn done_rejects_item_archived_only_in_removed_pending_done_alias() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#keep1] Keep backlog item\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:pending-done -->\n",
            "- 2026-05-09 [#done1] Already completed\n",
            "<!-- /agent:pending-done -->\n"
        );
        let (tmp, doc) = setup_test_dir();
        fs::write(&doc, content).unwrap();
        let err = done(&doc, "done1").unwrap_err();
        assert!(err.to_string().contains("id not found in backlog/icebox"));
        drop(tmp);
    }

    #[test]
    fn edit_canonicalizes_nested_subtask_ids_immediately() {
        let (_tmp, doc) = doc_with_pending("- [ ] [#tmuxcrash] parent task\n");
        edit(
            &doc,
            "tmuxcrash",
            "parent task\n  - child dependency\n  - child subtask",
        )
        .unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        let pending = content
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .unwrap();
        let lines: Vec<&str> = pending.lines().collect();
        assert_eq!(lines[0], "- [ ] [#tmuxcrash] parent task");
        assert!(
            lines[1].starts_with("  - [ ] [#tmuxcrash-"),
            "got: {}",
            lines[1]
        );
        assert!(
            lines[2].starts_with("  - [ ] [#tmuxcrash-"),
            "got: {}",
            lines[2]
        );
    }

    #[test]
    fn edit_replaces_existing_nested_subtasks_instead_of_appending() {
        let (_tmp, doc) = doc_with_pending(concat!(
            "- [ ] [#tmuxcrash] parent task\n",
            "  - [ ] [#tmuxcrash-old1] stale child\n",
            "  - [ ] [#tmuxcrash-old2] stale child two\n",
            "- [ ] [#keep1] sibling task\n"
        ));
        edit(
            &doc,
            "tmuxcrash",
            "parent task\n  - fresh child\n  - fresh child two",
        )
        .unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        let pending = content
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .unwrap();
        assert!(!pending.contains("stale child"));
        assert!(!pending.contains("stale child two"));
        let child_lines: Vec<&str> = pending
            .lines()
            .filter(|line| line.trim_start().starts_with("- [ ] [#tmuxcrash-"))
            .collect();
        assert_eq!(child_lines.len(), 2, "got: {pending}");
        assert!(pending.contains("\n- [ ] [#keep1] sibling task"));
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

    #[test]
    fn ungate_tasks_adds_one_per_gated_review_item_and_is_idempotent() {
        let (_tmp, doc) = setup_test_dir();
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] unrelated open item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#rev1] gated review item one\n",
            "- [/] [#rev2] gated review item two\n",
            "<!-- /agent:review -->\n",
        );
        fs::write(&doc, content).unwrap();

        let report = add_ungate_tasks_for_review(&doc).unwrap();
        assert_eq!(report.scanned, 2);
        assert_eq!(report.added.len(), 2, "{report:?}");
        assert!(report.skipped.is_empty());

        let after = fs::read_to_string(&doc).unwrap();
        assert!(after.contains("Ungate review item #rev1"), "{after}");
        assert!(after.contains("Ungate review item #rev2"), "{after}");

        // Re-run is idempotent: both already tracked, nothing added.
        let report2 = add_ungate_tasks_for_review(&doc).unwrap();
        assert_eq!(report2.scanned, 2);
        assert!(report2.added.is_empty(), "{report2:?}");
        assert_eq!(report2.skipped.len(), 2);
    }

    #[test]
    fn ungate_tasks_noop_when_no_gated_review_items() {
        let (_tmp, doc) = doc_with_pending("- [ ] [#a] open item\n");
        let report = add_ungate_tasks_for_review(&doc).unwrap();
        assert_eq!(report.scanned, 0);
        assert!(report.added.is_empty());
    }
}

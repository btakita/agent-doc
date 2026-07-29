//! # Module: backlog_cmd
//!
//! CLI subcommands for managing tracked work components.
//!
//! - `agent-doc backlog <FILE> add <item>` — add a backlog item at the beginning
//!   (supports canonical `id=<custom> ` syntax and compatibility `[#custom] ` input
//!   to preserve a custom id)
//! - `agent-doc backlog <FILE> remove <target>` — remove by content match
//! - `agent-doc backlog <FILE> prune` — remove completed items
//! - `agent-doc backlog <FILE> list` — print backlog items
//! - `agent-doc icebox <FILE> ...` uses the same granular tracked-work
//!   operations against the `agent:icebox` component.

use anyhow::{Context, Result};
use std::cell::Cell;
use std::collections::HashSet;
use std::path::Path;

use crate::done_archive::{archive_pending_done, external_done_archive_ids};
use agent_doc_element::element;
use agent_doc_element::element::is_backlog_component;
use agent_doc_element_backlog::backlog;
use agent_doc_element_review::{
    ReviewItemView, ReviewListFilter, UngateTasksReport, ensure_review_component_in_document,
    find_review_component_in_content, remove_review_items_from_document,
    resolve_review_items_in_document,
};

thread_local! {
    static FORCE_DISK_PENDING_WRITE: Cell<bool> = const { Cell::new(false) };
}

pub fn with_force_disk_pending_writes<T>(
    force_disk: bool,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if !force_disk {
        return f();
    }

    FORCE_DISK_PENDING_WRITE.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

fn persist_pending_write(file: &Path, current: &str, target: &str) -> Result<()> {
    let force_disk = FORCE_DISK_PENDING_WRITE.with(Cell::get);
    if force_disk {
        std::fs::write(file, target)
            .with_context(|| format!("pending_write: failed to write {}", file.display()))?;
        crate::record_document_write_provenance(file, target);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "pending_write_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                target.len(),
                agent_doc_hash::content_hash(target)
            ),
        );
        return Ok(());
    }

    crate::converge_or_disk_write(file, current, target, "pending_write")
}

/// Apply a caller-supplied pure document rewrite through the tracked-work write
/// path, so it lands with exactly the same reachability as the backlog mutation
/// it accompanies.
///
/// `#queueatcreate` / `#5d9f`: the same-cycle queue enqueue used to persist via
/// `persist_queue_maintenance_doc`, which hard-requires a ready editor model and
/// discards its work otherwise. A backlog add in the SAME write persists via
/// `persist_pending_write` -> `converge_or_disk_write`, which succeeds. So one
/// write could grow `agent:backlog` while silently dropping the matching
/// `agent:queue` head — two persistence paths with different reachability, which
/// is the bug surface itself. Routing the enqueue through here makes backlog and
/// queue move together or not at all.
///
/// `rewrite` receives the current document and returns the new document, or
/// `None` to leave it untouched. Returns whether a write happened.
pub fn apply_document_rewrite(
    file: &Path,
    source: &str,
    rewrite: impl FnOnce(&str) -> Result<Option<String>>,
) -> Result<bool> {
    let current = read_command_document(file, source)?;
    let Some(target) = rewrite(&current)? else {
        return Ok(false);
    };
    if target == current {
        return Ok(false);
    }
    persist_pending_write(file, &current, &target)?;
    Ok(true)
}

fn read_command_document(file: &Path, source: &str) -> Result<String> {
    let force_disk = FORCE_DISK_PENDING_WRITE.with(Cell::get);
    if force_disk {
        crate::force_disk_document_content(file, source)
            .with_context(|| format!("{source}: failed to read disk document"))
    } else {
        crate::current_document_content(file, source)
            .with_context(|| format!("{source}: failed to resolve current document"))
    }
}

fn find_tracked_list_component(
    file: &Path,
    list: backlog::TrackedWorkList,
) -> Result<(String, element::Component)> {
    let content = read_command_document(file, "backlog_find_tracked_list_component")?;
    let comp = backlog::find_tracked_work_component_in_content(&content, list)?;
    Ok((content, comp))
}

fn find_pending_component(file: &Path) -> Result<(String, element::Component)> {
    find_tracked_list_component(file, backlog::TrackedWorkList::Backlog)
}

fn find_component_containing_open_id(
    file: &Path,
    id: &str,
) -> Result<(String, element::Component)> {
    let content = read_command_document(file, "backlog_find_open_id_component")?;
    let comp = backlog::find_open_tracked_work_component_in_content(&content, id)?;
    Ok((content, comp))
}

pub fn open_item_component_name(file: &Path, id: &str) -> Result<Option<String>> {
    let content = read_command_document(file, "backlog_open_item_component")?;
    backlog::open_tracked_work_component_name_in_content(&content, id)
}

fn tracked_work_id_already_resolved_in_content(
    file: &Path,
    content: &str,
    id: &str,
) -> Result<bool> {
    let id = backlog::normalize_pending_id(id);
    if id.is_empty() {
        return Ok(false);
    }
    if agent_doc_cycle_state_io::resolved_pending_ids(file)?.contains(&id) {
        return Ok(true);
    }

    if backlog::content_has_resolved_tracked_work_id(content, &id)? {
        return Ok(true);
    }

    // External `agent:done archive=...` is the canonical completed-work
    // surface for large sessions. A pending-only closeout repair may arrive
    // after mark -> reap moved the item there; treating that as "not found"
    // makes the otherwise-idempotent `--done` repair impossible and leaves the
    // committed cycle permanently failing session-check.
    Ok(external_done_archive_ids(file, content)?.contains(&id))
}

fn tracked_work_id_already_resolved(file: &Path, id: &str) -> Result<bool> {
    let content = read_command_document(file, "backlog_resolved_id")?;
    tracked_work_id_already_resolved_in_content(file, &content, id)
}

fn log_symptom_dedupe(file: &Path, surface: &str, id: &str, key: &backlog::SymptomDedupeKey) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "symptom_dedupe_attached file={} surface={} id={} {}",
            file.display(),
            surface,
            backlog::normalize_pending_id(id),
            key.log_fields()
        ),
    );
}

/// Add a new item to the pending component (assigns a stable hash id + `[ ]`
/// or `[/]`) at the beginning of the list. Supports canonical `id=<custom> `
/// syntax and compatibility `[#custom] ` input to preserve a custom id. Prints
/// the assigned hash id to stdout.
pub fn add(file: &Path, item: &str, gated: bool) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    backlog::ensure_new_item_explicit_id_available(&full_content, item)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = agent_doc_hash::document_id_for_path(file);
    let outcome = backlog::op_add_with_outcome(existing, item, &doc_id, gated)?;
    let canonical = backlog::canonicalize_tracked_work_body(&outcome.body, &doc_id);
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    if let Some(key) = outcome.deduped_key.as_ref() {
        log_symptom_dedupe(file, "backlog", &outcome.id, key);
    }
    Ok(())
}

pub fn icebox_add(file: &Path, item: &str) -> Result<()> {
    add_many_to_list(
        file,
        &[item.to_string()],
        false,
        backlog::TrackedWorkList::Icebox,
    )
    .map(|_| ())
}

/// Add multiple new items while preserving the caller's flag order, top-down, at
/// the beginning of the pending list: the first item lands topmost ("what you
/// read is what you get").
fn add_many_to_list(
    file: &Path,
    items: &[String],
    gated: bool,
    list: backlog::TrackedWorkList,
) -> Result<Vec<String>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    for item in items {
        backlog::ensure_new_item_explicit_id_available(&full_content, item)?;
    }
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = agent_doc_hash::document_id_for_path(file);
    let outcome = backlog::op_prepend_many_with_outcomes(existing, items, &doc_id, gated)?;
    let canonical = backlog::canonicalize_tracked_work_body(&outcome.body, &doc_id);
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    let mut ids = Vec::new();
    for item in &outcome.outcomes {
        if item.inserted {
            ids.push(item.id.clone());
        } else if let Some(key) = item.deduped_key.as_ref() {
            log_symptom_dedupe(file, list.label(), &item.id, key);
        }
    }
    Ok(ids)
}

pub fn add_many(file: &Path, items: &[String], gated: bool) -> Result<Vec<String>> {
    add_many_to_list(file, items, gated, backlog::TrackedWorkList::Backlog)
}

pub fn icebox_add_many(file: &Path, items: &[String]) -> Result<Vec<String>> {
    add_many_to_list(file, items, false, backlog::TrackedWorkList::Icebox)
}

/// `#ah0s`: insert a new item at an explicit position relative to the active
/// list (after/before an anchor id, or at the tail), instead of the front
/// default. Returns the assigned id.
fn add_at_to_list(
    file: &Path,
    item: &str,
    position: backlog::AddPosition<'_>,
    list: backlog::TrackedWorkList,
) -> Result<String> {
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    backlog::ensure_new_item_explicit_id_available(&full_content, item)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = agent_doc_hash::document_id_for_path(file);
    let outcome = backlog::op_add_at_with_outcome(existing, item, &doc_id, false, position)?;
    let canonical = backlog::canonicalize_tracked_work_body(&outcome.body, &doc_id);
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    if let Some(key) = outcome.deduped_key.as_ref() {
        log_symptom_dedupe(file, list.label(), &outcome.id, key);
    }
    Ok(outcome.id)
}

fn add_at(file: &Path, item: &str, position: backlog::AddPosition<'_>) -> Result<String> {
    add_at_to_list(file, item, position, backlog::TrackedWorkList::Backlog)
}

/// `#ah0s`: `--pending-add-after <id> "<text>"`. Repeatable; chaining
/// after A "B" then after B "C" builds A→B→C deterministically.
pub fn add_after(file: &Path, anchor_id: &str, item: &str) -> Result<String> {
    add_at(file, item, backlog::AddPosition::After(anchor_id))
}

/// `#ah0s`: `--pending-add-before <id> "<text>"`.
pub fn add_before(file: &Path, anchor_id: &str, item: &str) -> Result<String> {
    add_at(file, item, backlog::AddPosition::Before(anchor_id))
}

/// `#ah0s`: `--pending-add-back "<text>"` (alias `--pending-append`) — tail insert.
pub fn add_back(file: &Path, item: &str) -> Result<String> {
    add_at(file, item, backlog::AddPosition::Last)
}

pub fn icebox_add_after(file: &Path, anchor_id: &str, item: &str) -> Result<String> {
    add_at_to_list(
        file,
        item,
        backlog::AddPosition::After(anchor_id),
        backlog::TrackedWorkList::Icebox,
    )
}

pub fn icebox_add_before(file: &Path, anchor_id: &str, item: &str) -> Result<String> {
    add_at_to_list(
        file,
        item,
        backlog::AddPosition::Before(anchor_id),
        backlog::TrackedWorkList::Icebox,
    )
}

pub fn icebox_add_back(file: &Path, item: &str) -> Result<String> {
    add_at_to_list(
        file,
        item,
        backlog::AddPosition::Last,
        backlog::TrackedWorkList::Icebox,
    )
}

/// Token-efficient query of gated `agent:review` items (`#review-list-query`).
///
/// Returns one [`ReviewItemView`] per gated item, with extracted hashtags and the
/// `NEXT:` annotation tail surfaced so a quick pass can triage a long review list
/// without reading the whole component. Read-only.
pub fn list_review_items(file: &Path, filter: &ReviewListFilter) -> Result<Vec<ReviewItemView>> {
    let content = read_command_document(file, "review_list_items")?;
    agent_doc_element_review::review_item_views_from_content(&content, filter)
}

/// gated item in `agent:review`, ensure a backlog follow-up task exists to drive
/// it back out of review (ungate → done), so gated items are not stranded.
/// On-demand and idempotent: a review id whose ungate task already exists in the
/// backlog is skipped.
pub fn add_ungate_tasks_for_review(file: &Path) -> Result<UngateTasksReport> {
    let content = read_command_document(file, "review_ungate_tasks")?;
    let plan = agent_doc_element_review::plan_ungate_tasks_for_review(&content)?;
    if !plan.task_texts.is_empty() {
        add_many(file, &plan.task_texts, false)?;
    }
    Ok(plan.report)
}

/// Run lazy backfill over the pending component and write if changed.
fn backfill_list(file: &Path, list: backlog::TrackedWorkList) -> Result<()> {
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = agent_doc_hash::document_id_for_path(file);
    let (new_content, changed, dropped_text) =
        backlog::backfill_reporting_dropped_text(existing, &doc_id, &HashSet::new());
    // `#adbacklogorphanseg`: never delete operator-visible text silently. Each
    // removed segment is logged verbatim (newlines escaped so one drop stays one
    // log line) so the deletion is auditable and recoverable from git, rather
    // than something an operator discovers later with no record of what went.
    for segment in &dropped_text {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "backlog_non_item_text_dropped component={} bytes={} text={}",
                list.label(),
                segment.len(),
                segment.trim().replace('\n', "\\n"),
            ),
        );
        eprintln!(
            "[{}] removed non-item text ({} bytes) — see ops.log `backlog_non_item_text_dropped`",
            list.label(),
            segment.len(),
        );
    }
    if !changed {
        eprintln!("[{}] already canonical — no changes", list.label());
        return Ok(());
    }
    let new_doc = comp.replace_content(&full_content, &new_content);
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Run lazy backfill over the backlog component and write if changed.
pub fn backfill(file: &Path) -> Result<()> {
    backfill_list(file, backlog::TrackedWorkList::Backlog)
}

pub fn icebox_backfill(file: &Path) -> Result<()> {
    backfill_list(file, backlog::TrackedWorkList::Icebox)
}

/// Mark an item `[x]` by id.
pub fn done(file: &Path, id: &str) -> Result<()> {
    let (full_content, comp) = match find_component_containing_open_id(file, id) {
        Ok(found) => found,
        Err(_) if tracked_work_id_already_resolved(file, id)? => {
            eprintln!(
                "[pending] done: id [#{}] is already resolved; leaving backlog unchanged",
                backlog::normalize_pending_id(id)
            );
            return Ok(());
        }
        Err(err) => return Err(err),
    };
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = backlog::op_done(existing, id)?;
    let canonical = backlog::canonicalize_tracked_work_body(
        &new_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Complete and reap tracked-work ids through one authoritative document target.
///
/// Commit-required closeouts use this operation so `--done` never exposes an
/// intermediate `[x]` row that a later maintenance write must rediscover. The
/// removed items are archived and top-backlog status is reconciled before the
/// single editor/disk projection. Non-committing `write --done` continues to use
/// [`done`] so its intentionally visible `[x]` state remains available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneAndReapOutcome {
    pub removed_ids: Vec<String>,
    pub target_content: Option<String>,
}

pub fn done_and_reap_many(file: &Path, ids: &[String]) -> Result<DoneAndReapOutcome> {
    if ids.is_empty() {
        return Ok(DoneAndReapOutcome {
            removed_ids: Vec::new(),
            target_content: None,
        });
    }

    let full_content = read_command_document(file, "backlog_done_and_reap")?;
    let mut target = full_content.clone();
    let mut removed_items = Vec::new();
    let mut removed_ids = Vec::new();
    let mut seen = HashSet::new();
    let mut backlog_changed = false;
    let doc_id = agent_doc_hash::document_id_for_path(file);

    for raw_id in ids {
        let id = backlog::normalize_pending_id(raw_id);
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }

        let comp = match backlog::find_open_tracked_work_component_in_content(&target, &id) {
            Ok(comp) => comp,
            Err(_) if tracked_work_id_already_resolved_in_content(file, &target, &id)? => {
                eprintln!(
                    "[pending] done: id [#{id}] is already resolved; leaving backlog unchanged"
                );
                continue;
            }
            Err(err) => return Err(err),
        };
        let existing = &target[comp.open_end..comp.close_start];
        let (new_content, item) = backlog::op_take_item(existing, &id)?;
        let canonical = backlog::canonicalize_tracked_work_body(&new_content, &doc_id);
        backlog_changed |= is_backlog_component(&comp.name);
        target = comp.replace_content(&target, &canonical);
        removed_ids.push(item.id.clone());
        removed_items.push(item);
    }

    if removed_items.is_empty() {
        return Ok(DoneAndReapOutcome {
            removed_ids: Vec::new(),
            target_content: None,
        });
    }

    target = archive_pending_done(file, &target, &removed_items)
        .context("failed to archive completed item(s) to agent:done")?
        .context("failed to archive completed item(s) to agent:done")?;
    if backlog_changed
        && let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_top_backlog_status_content(&target)?
    {
        target = reconciled;
    }

    persist_pending_write(file, &full_content, &target)?;
    eprintln!(
        "[pending] completed and reaped {} item(s) atomically: {}",
        removed_ids.len(),
        removed_ids.join(", ")
    );
    Ok(DoneAndReapOutcome {
        removed_ids,
        target_content: Some(target),
    })
}

/// Transition an item to `Gated` (`[/]`) by id.
/// Idempotent on already-gated items; errors on `Done` items.
pub fn gate(file: &Path, id: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let (new_backlog, mut item) = match backlog::op_take_item(existing, id) {
        Ok(found) => found,
        Err(_) => {
            if let Some(review_comp) = find_review_component_in_content(&full_content)? {
                let (_, review_items, _) = backlog::parse_items(review_comp.content(&full_content));
                let normalized = backlog::normalize_pending_id(id);
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

    match backlog::validate_transition(item.state, backlog::PendingOp::Gate)? {
        backlog::TransitionResult::Transition(state) => item.state = state,
        backlog::TransitionResult::NoOp => {}
    }
    item.gate_type = None;

    let mut new_doc = comp.replace_content(
        &full_content,
        &backlog::canonicalize_tracked_work_body(
            &new_backlog,
            &agent_doc_hash::document_id_for_path(file),
        ),
    );
    let (content_with_review, review_comp) = ensure_review_component_in_document(&new_doc)?;
    new_doc = content_with_review;
    let review_body = review_comp.content(&new_doc);
    let new_review = backlog::op_insert_item_first(review_body, item);
    let review_comp = find_review_component_in_content(&new_doc)?
        .context("document has no review component after insertion")?;
    let new_doc = review_comp.replace_content(
        &new_doc,
        &backlog::canonicalize_tracked_work_body(
            &new_review,
            &agent_doc_hash::document_id_for_path(file),
        ),
    );
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Add a new review item directly to `agent:review`. Returns the assigned id
/// only when a new item was inserted so the caller can record actual same-cycle
/// adds (`#opsproof-samecycle-add`).
pub fn review_add(file: &Path, item: &str) -> Result<Option<String>> {
    let full_content = read_command_document(file, "review_add")?;
    let (full_content, comp) = ensure_review_component_in_document(&full_content)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = agent_doc_hash::document_id_for_path(file);
    let outcome = backlog::op_add_with_outcome(existing, item, &doc_id, true)?;
    let canonical = backlog::canonicalize_tracked_work_body(&outcome.body, &doc_id);
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    if let Some(key) = outcome.deduped_key.as_ref() {
        log_symptom_dedupe(file, "review", &outcome.id, key);
    }
    Ok(outcome.inserted.then_some(outcome.id))
}

/// Edit a review item's text, preserving its hash id.
pub fn review_edit(file: &Path, id: &str, text: &str) -> Result<()> {
    let full_content = read_command_document(file, "review_edit")?;
    let comp = find_review_component_in_content(&full_content)?
        .context("document has no review component")?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = backlog::op_edit(existing, id, text)?;
    let canonical = backlog::canonicalize_tracked_work_body(
        &new_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Remove a review item by id, deleting every entry that shares the id.
///
/// This is the clean-removal flag the review list previously lacked: a stale or
/// duplicate `agent:review` entry (e.g. the identical `[/]` pair an interleaved
/// finalize leaves behind, which preflight flags as `preset_item_id_collision`)
/// can be deleted outright without an ambiguous edit-by-id. Errors when the
/// document has no review component or no item matches the id.
pub fn review_remove(file: &Path, id: &str) -> Result<()> {
    let full_content = read_command_document(file, "review_remove")?;
    let plan = remove_review_items_from_document(
        &full_content,
        id,
        &agent_doc_hash::document_id_for_path(file),
    )?;
    persist_pending_write(file, &full_content, &plan.content)?;
    eprintln!(
        "[pending] review-remove: removed {} entr{} for #{}",
        plan.removed.len(),
        if plan.removed.len() == 1 { "y" } else { "ies" },
        backlog::normalize_pending_id(id)
    );
    Ok(())
}

/// Resolve a review item by id: remove it from `agent:review` and archive it to
/// `agent:done`. Use this when a gated review item's work is actually complete
/// (the proper completion path, as opposed to [`review_remove`] which discards a
/// stale/duplicate entry). Errors when the document has no review component or
/// no item matches the id.
pub fn review_resolve(file: &Path, id: &str) -> Result<()> {
    let full_content = read_command_document(file, "review_resolve")?;
    let plan = resolve_review_items_in_document(
        &full_content,
        id,
        &agent_doc_hash::document_id_for_path(file),
    )?;
    let archived = archive_pending_done(file, &plan.content, &plan.removed)
        .context("failed to archive resolved review item(s) to agent:done")?
        .unwrap_or(plan.content);
    persist_pending_write(file, &full_content, &archived)?;
    eprintln!(
        "[pending] review-resolve: archived {} entr{} for #{} to agent:done",
        plan.removed.len(),
        if plan.removed.len() == 1 { "y" } else { "ies" },
        backlog::normalize_pending_id(id)
    );
    Ok(())
}

/// Transition an item back to `Open` (`[ ]`) by id.
/// Errors on `Open` or `Done` items — the source must be `[/]`.
pub fn ungate(file: &Path, id: &str) -> Result<()> {
    let full_content = read_command_document(file, "review_ungate")?;
    let Some(review_comp) = find_review_component_in_content(&full_content)? else {
        let (full_content, comp) = find_pending_component(file)?;
        let existing = &full_content[comp.open_end..comp.close_start];
        let new_content = backlog::op_ungate(existing, id)?;
        let canonical = backlog::canonicalize_tracked_work_body(
            &new_content,
            &agent_doc_hash::document_id_for_path(file),
        );
        let new_doc = comp.replace_content(&full_content, &canonical);
        persist_pending_write(file, &full_content, &new_doc)?;
        return Ok(());
    };
    let review_body = review_comp.content(&full_content);
    let (new_review, mut item) = match backlog::op_take_item(review_body, id) {
        Ok(found) => found,
        Err(_) => {
            let (full_content, comp) = find_pending_component(file)?;
            let existing = &full_content[comp.open_end..comp.close_start];
            let new_content = backlog::op_ungate(existing, id)?;
            let canonical = backlog::canonicalize_tracked_work_body(
                &new_content,
                &agent_doc_hash::document_id_for_path(file),
            );
            let new_doc = comp.replace_content(&full_content, &canonical);
            persist_pending_write(file, &full_content, &new_doc)?;
            return Ok(());
        }
    };

    match backlog::validate_transition(item.state, backlog::PendingOp::Ungate)? {
        backlog::TransitionResult::Transition(state) => item.state = state,
        backlog::TransitionResult::NoOp => {}
    }
    item.gate_type = None;

    let new_doc = review_comp.replace_content(
        &full_content,
        &backlog::canonicalize_tracked_work_body(
            &new_review,
            &agent_doc_hash::document_id_for_path(file),
        ),
    );
    let components = element::parse(&new_doc).context("failed to parse components")?;
    let backlog_comp = components
        .into_iter()
        .find(|c| is_backlog_component(&c.name))
        .context("document has no backlog/pending component")?;
    let backlog_body = backlog_comp.content(&new_doc);
    let new_backlog = backlog::op_insert_item_first(backlog_body, item);
    let new_doc = backlog_comp.replace_content(
        &new_doc,
        &backlog::canonicalize_tracked_work_body(
            &new_backlog,
            &agent_doc_hash::document_id_for_path(file),
        ),
    );
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Legacy in-place gated-state transition retained for tests and old docs that
/// do not yet have a review component.
fn gate_in_place(file: &Path, id: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = backlog::op_gate(existing, id)?;
    if new_content == existing {
        return Ok(());
    }
    let canonical = backlog::canonicalize_tracked_work_body(
        &new_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Edit an item's text, preserving its hash id.
fn edit_list(file: &Path, list: backlog::TrackedWorkList, id: &str, text: &str) -> Result<()> {
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = backlog::op_edit(existing, id, text)?;
    let canonical = backlog::canonicalize_tracked_work_body(
        &new_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

fn edit_many_list(
    file: &Path,
    list: backlog::TrackedWorkList,
    edits: &[(String, String)],
) -> Result<()> {
    if edits.is_empty() {
        return Ok(());
    }
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = backlog::op_edit_many(existing, edits)?;
    if new_content == existing {
        return Ok(());
    }
    let canonical = backlog::canonicalize_tracked_work_body(
        &new_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Edit a backlog item's text, preserving its hash id.
pub fn edit(file: &Path, id: &str, text: &str) -> Result<()> {
    edit_list(file, backlog::TrackedWorkList::Backlog, id, text)
}

/// Edit multiple backlog items in one read/modify/write transaction.
pub fn edit_many(file: &Path, edits: &[(String, String)]) -> Result<()> {
    edit_many_list(file, backlog::TrackedWorkList::Backlog, edits)
}

pub fn icebox_edit(file: &Path, id: &str, text: &str) -> Result<()> {
    edit_list(file, backlog::TrackedWorkList::Icebox, id, text)
}

pub fn icebox_edit_many(file: &Path, edits: &[(String, String)]) -> Result<()> {
    edit_many_list(file, backlog::TrackedWorkList::Icebox, edits)
}

fn clear_list(file: &Path, list: backlog::TrackedWorkList) -> Result<()> {
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = backlog::op_clear(existing)?;
    let new_doc = comp.replace_content(&full_content, &new_content);
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Clear all items from the backlog component.
pub fn clear(file: &Path) -> Result<()> {
    clear_list(file, backlog::TrackedWorkList::Backlog)
}

pub fn icebox_clear(file: &Path) -> Result<()> {
    clear_list(file, backlog::TrackedWorkList::Icebox)
}

fn reorder_list(file: &Path, list: backlog::TrackedWorkList, ids: &[String]) -> Result<()> {
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let new_content = backlog::op_reorder(existing, ids)?;
    let canonical = backlog::canonicalize_tracked_work_body(
        &new_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Reorder backlog items by id (comma-separated). Missing ids keep their relative order.
pub fn reorder(file: &Path, ids: &[String]) -> Result<()> {
    reorder_list(file, backlog::TrackedWorkList::Backlog, ids)
}

pub fn icebox_reorder(file: &Path, ids: &[String]) -> Result<()> {
    reorder_list(file, backlog::TrackedWorkList::Icebox, ids)
}

fn reap_list(file: &Path, list: backlog::TrackedWorkList) -> Result<()> {
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let doc_id = agent_doc_hash::document_id_for_path(file);
    let (canonical_content, changed) = backlog::backfill(existing, &doc_id, &HashSet::new());
    let (new_content, removed_items) = backlog::reap_with_items(&canonical_content)?;
    let removed: Vec<String> = removed_items.iter().map(|item| item.id.clone()).collect();
    let final_content = if removed.is_empty() {
        canonical_content
    } else {
        new_content
    };
    if !changed && removed.is_empty() {
        eprintln!("[{}] no [x] items to reap", list.label());
        return Ok(());
    }
    let canonical = backlog::canonicalize_tracked_work_body(
        &final_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let mut new_doc = comp.replace_content(&full_content, &canonical);
    if !removed_items.is_empty() {
        let archived = archive_pending_done(file, &new_doc, &removed_items)
            .context("failed to archive reaped item(s) to agent:done")?
            .context("failed to archive reaped item(s) to agent:done")?;
        new_doc = archived;
    }
    if matches!(list, backlog::TrackedWorkList::Backlog)
        && let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_top_backlog_status_content(&new_doc)?
    {
        new_doc = reconciled;
    }
    persist_pending_write(file, &full_content, &new_doc)?;
    if changed {
        eprintln!(
            "[{}] backfilled missing hash ids / checkboxes before reap",
            list.label()
        );
    }
    if removed.is_empty() {
        eprintln!("[{}] no [x] items to reap", list.label());
        return Ok(());
    }
    eprintln!(
        "[{}] reaped {} item(s): {}",
        list.label(),
        removed.len(),
        removed.join(", ")
    );
    Ok(())
}

/// Reap `[x]` backlog items and print removed ids.
pub fn reap(file: &Path) -> Result<()> {
    reap_list(file, backlog::TrackedWorkList::Backlog)
}

pub fn icebox_reap(file: &Path) -> Result<()> {
    reap_list(file, backlog::TrackedWorkList::Icebox)
}

fn remove_from_list(
    file: &Path,
    list: backlog::TrackedWorkList,
    target: &str,
    contains: bool,
) -> Result<()> {
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let (new_content, removed) =
        backlog::op_remove_matching_tracked_line(existing, target, contains);

    if !removed {
        eprintln!("[{}] no matching item found", list.label());
    }

    let new_doc = comp.replace_content(&full_content, &new_content);
    persist_pending_write(file, &full_content, &new_doc)?;
    Ok(())
}

/// Remove a backlog item by content match.
pub fn remove(file: &Path, target: &str, contains: bool) -> Result<()> {
    remove_from_list(file, backlog::TrackedWorkList::Backlog, target, contains)
}

pub fn icebox_remove(file: &Path, target: &str, contains: bool) -> Result<()> {
    remove_from_list(file, backlog::TrackedWorkList::Icebox, target, contains)
}

/// Resolve all items with a matching typed gate (e.g., `[/release]` → `[x]`).
/// Prints resolved ids to stdout.
pub fn resolve_gate(file: &Path, gate_type: &str) -> Result<()> {
    let (full_content, comp) = find_pending_component(file)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let (new_content, resolved) = backlog::op_resolve_gate(existing, gate_type);
    if resolved.is_empty() {
        eprintln!(
            "[pending] no [/{}] items to resolve in {}",
            gate_type,
            file.display()
        );
        return Ok(());
    }
    let canonical = backlog::canonicalize_tracked_work_body(
        &new_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
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
    let new_content = backlog::op_set_gate_type(existing, id, gate_type)?;
    let canonical = backlog::canonicalize_tracked_work_body(
        &new_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    eprintln!("[pending] set gate type [/{}] on [#{}]", gate_type, id);
    Ok(())
}

/// Set a typed proof/disproof verify predicate on a gated item (`#optverify`).
/// The gate-set timestamp is stamped with the current time so the later ops.log
/// scan only counts markers emitted at or after now.
pub fn set_gate_verify(file: &Path, id: &str, spec: &str) -> Result<()> {
    let (full_content, comp) = find_component_containing_open_id(file, id)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let set_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let new_content = backlog::op_set_gate_verify(existing, id, spec, set_at)?;
    let canonical = backlog::canonicalize_tracked_work_body(
        &new_content,
        &agent_doc_hash::document_id_for_path(file),
    );
    let new_doc = comp.replace_content(&full_content, &canonical);
    persist_pending_write(file, &full_content, &new_doc)?;
    eprintln!(
        "[pending] set verify predicate on [#{}] (set_at={})",
        id, set_at
    );
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
            let components = match element::parse(&content) {
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
            let (new_content, resolved) = backlog::op_resolve_gate(existing, gate_type);
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

fn list_items(file: &Path, list: backlog::TrackedWorkList) -> Result<()> {
    let (full_content, comp) = find_tracked_list_component(file, list)?;
    let existing = &full_content[comp.open_end..comp.close_start];
    let lines = backlog::printable_tracked_work_lines(existing);

    if lines.is_empty() {
        println!("(no {} items)", list.label());
        return Ok(());
    }

    for line in lines {
        println!("{}", line);
    }
    Ok(())
}

/// List current backlog items.
pub fn list(file: &Path) -> Result<()> {
    list_items(file, backlog::TrackedWorkList::Backlog)
}

pub fn icebox_list(file: &Path) -> Result<()> {
    list_items(file, backlog::TrackedWorkList::Icebox)
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

    struct TestBacklogCommandEffects;

    static TEST_BACKLOG_COMMAND_EFFECTS: TestBacklogCommandEffects = TestBacklogCommandEffects;

    thread_local! {
        static TEST_WRITE_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    impl crate::BacklogCommandEffects for TestBacklogCommandEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(fs::read_to_string(file)?)
        }

        fn force_disk_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(fs::read_to_string(file)?)
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current_content: &str,
            target_content: &str,
            _reason: &str,
        ) -> Result<()> {
            TEST_WRITE_COUNT.with(|count| count.set(count.get() + 1));
            fs::write(file, target_content)?;
            Ok(())
        }

        fn record_document_write_provenance(&self, _file: &Path, _content: &str) {}
    }

    fn with_test_effects<T>(f: impl FnOnce() -> T) -> T {
        crate::with_backlog_command_effects(&TEST_BACKLOG_COMMAND_EFFECTS, f)
    }

    fn force_pending<T>(f: impl FnOnce() -> Result<T>) -> T {
        with_test_effects(|| with_force_disk_pending_writes(true, f).unwrap())
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

    fn pending_body(content: &str) -> &str {
        content
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .unwrap()
    }

    fn doc_with_preset_and_pending(preset_key: &str, items: &str) -> (TempDir, PathBuf) {
        let content = format!(
            "---\nagent_doc_session: test\nprompt_presets:\n  '#{preset_key}': do the thing\n---\n\n<!-- agent:pending -->\n{items}\n<!-- /agent:pending -->\n"
        );
        let (tmp, doc) = setup_test_dir();
        fs::write(&doc, content).unwrap();
        (tmp, doc)
    }

    #[test]
    fn add_rejects_explicit_id_colliding_with_prompt_preset() {
        // #preset-item-id-collision-enforce: an explicit id matching a
        // prompt_presets key must fail closed at mutation time.
        let (_tmp, doc) = doc_with_preset_and_pending("next-steps", "- [ ] [#abcd] existing");
        let err =
            with_test_effects(|| add(&doc, "id=next-steps add follow-up", false).unwrap_err());
        let msg = format!("{err:#}");
        assert!(msg.contains("#next-steps"), "{msg}");
        assert!(msg.contains("prompt_presets"), "{msg}");
        // Document is unchanged — the colliding id was never written.
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(content.matches("next-steps").count(), 1, "{content}");
    }

    #[test]
    fn add_rejects_explicit_id_colliding_with_active_item() {
        // Bracketed `[#id]` form colliding with an existing active backlog id.
        let (_tmp, doc) = doc_with_pending("- [ ] [#gscaccess] grant Google Ads access");
        let err = with_test_effects(|| add(&doc, "[#gscaccess] duplicate", false).unwrap_err());
        let msg = format!("{err:#}");
        assert!(msg.contains("#gscaccess"), "{msg}");
        assert!(msg.contains("agent:") || msg.contains("active"), "{msg}");
    }

    #[test]
    fn add_allows_explicit_noncolliding_id() {
        let (_tmp, doc) = doc_with_preset_and_pending("next-steps", "- [ ] [#abcd] existing");
        force_pending(|| add(&doc, "id=fresh01 a new task", false));
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("[#fresh01]"), "{content}");
    }

    #[test]
    fn add_allows_auto_id_even_when_text_mentions_preset() {
        // An ordinary auto-id add (no explicit id prefix) is never blocked, even
        // when the free text references the preset token.
        let (_tmp, doc) = doc_with_preset_and_pending("next-steps", "- [ ] [#abcd] existing");
        force_pending(|| add(&doc, "plan the #next-steps rollout", false));
        let content = fs::read_to_string(&doc).unwrap();
        // The new item got a generated hash id, not the preset id.
        assert!(content.contains("rollout"), "{content}");
    }

    #[test]
    fn add_prepends_to_pending_component() {
        let (_tmp, doc) = doc_with_pending("- item one");
        force_pending(|| add(&doc, "item two", false));

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
        force_pending(|| {
            add_many(
                &doc,
                &["first new".to_string(), "second new".to_string()],
                false,
            )
        });

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
        force_pending(|| add(&doc, "new item", false));

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("new item"));
        assert!(content.contains("[ ]"));
        assert!(content.contains("[#"));
    }

    #[test]
    fn remove_by_contains_match() {
        let (_tmp, doc) = doc_with_pending("- implement feature X\n- write tests");
        force_pending(|| remove(&doc, "feature X", true));

        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("implement feature X"));
        assert!(content.contains("write tests"));
    }

    #[test]
    fn remove_noop_for_nonmatching() {
        let (_tmp, doc) = doc_with_pending("- item one");
        force_pending(|| remove(&doc, "not found", true));

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- item one"));
    }

    #[test]
    fn remove_exact_match_supports_ordered_parent_items() {
        let (_tmp, doc) = doc_with_pending("1. [ ] [#abcd] first item\n2. [ ] [#efgh] second item");
        force_pending(|| remove(&doc, "[ ] [#abcd] first item", false));

        let content = fs::read_to_string(&doc).unwrap();
        assert!(!content.contains("[#abcd] first item"));
        assert!(content.contains("[#efgh] second item"));
    }

    #[test]
    fn reap_removes_checked_items() {
        let (_tmp, doc) = doc_with_pending("- [ ] active\n- [x] done\n✅ finished");
        force_pending(|| reap(&doc));

        let content = fs::read_to_string(&doc).unwrap();
        let pending = pending_body(&content);
        assert!(pending.contains("active"), "{content}");
        assert!(!pending.contains("done"), "{content}");
        assert!(!pending.contains("finished"), "{content}");
        assert!(content.contains("## Completed / Reaped"), "{content}");
    }

    #[test]
    fn reap_removes_checked_ordered_items() {
        let (_tmp, doc) = doc_with_pending("1. [ ] active\n2. [x] done");
        force_pending(|| reap(&doc));

        let content = fs::read_to_string(&doc).unwrap();
        let pending = pending_body(&content);
        assert!(pending.contains("active"), "{content}");
        assert!(!pending.contains("done"), "{content}");
    }

    #[test]
    fn reap_noop_for_no_checked() {
        let (_tmp, doc) = doc_with_pending("- [ ] active\n- [ ] another");
        force_pending(|| reap(&doc));

        let content = fs::read_to_string(&doc).unwrap();
        let pending = pending_body(&content);
        assert!(pending.contains("active"), "{content}");
        assert!(pending.contains("another"), "{content}");
    }

    #[test]
    fn done_marks_icebox_item_when_backlog_does_not_contain_id() {
        let (_tmp, doc) = doc_with_pending_and_icebox(
            "- [ ] [#keep1] Keep backlog item\n",
            "- [ ] [#ice01] Parked follow-up\n",
        );

        force_pending(|| done(&doc, "ice01"));

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("<!-- agent:pending -->\n- [ ] [#keep1] Keep backlog item\n"));
        assert!(content.contains("<!-- agent:icebox -->\n- [x] [#ice01] Parked follow-up\n"));
    }

    #[test]
    fn done_and_reap_many_projects_one_target_without_intermediate_checked_row() {
        let (_tmp, doc) = doc_with_pending(
            "- [ ] [#close1] Close the first loop\n- [ ] [#keep1] Keep the next item\n",
        );
        let before = TEST_WRITE_COUNT.with(Cell::get);

        let outcome =
            with_test_effects(|| done_and_reap_many(&doc, &["close1".to_string()]).unwrap());

        assert_eq!(outcome.removed_ids, vec!["close1"]);
        assert!(outcome.target_content.is_some());
        assert_eq!(TEST_WRITE_COUNT.with(Cell::get) - before, 1);
        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            !content.contains("[#close1]") || content.contains("<!-- agent:done -->"),
            "{content}"
        );
        assert!(
            !pending_body(&content).contains("[#close1]"),
            "the authoritative target must never retain an intermediate [x] row:\n{content}"
        );
        assert!(pending_body(&content).contains("[#keep1]"), "{content}");
        assert!(
            content.contains("<!-- agent:done -->") && content.contains("[#close1]"),
            "{content}"
        );
    }

    #[test]
    fn done_noops_when_item_was_already_archived() {
        let (_tmp, doc) = doc_with_pending_and_archive(
            "- [ ] [#keep1] Keep backlog item\n",
            "- 2026-05-09 [#done1] Already completed\n",
        );
        force_pending(|| done(&doc, "done1"));

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- [ ] [#keep1] Keep backlog item"));
        assert!(content.contains("- 2026-05-09 [#done1] Already completed"));
    }

    #[test]
    fn done_noops_when_item_was_already_reaped_to_external_archive() {
        let (tmp, doc) = setup_test_dir();
        fs::create_dir(tmp.path().join(".agent-doc")).unwrap();
        let archive = tmp.path().join("doc.done.md");
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#keep1] Keep backlog item\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done archive=doc.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        fs::write(&doc, content).unwrap();
        fs::write(&archive, "- 2026-07-16 [#done1] Already reaped\n").unwrap();

        force_pending(|| done(&doc, "done1"));

        assert_eq!(fs::read_to_string(&doc).unwrap(), content);
        assert_eq!(
            fs::read_to_string(&archive).unwrap(),
            "- 2026-07-16 [#done1] Already reaped\n"
        );
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
        let err = with_test_effects(|| done(&doc, "done1").unwrap_err());
        assert!(err.to_string().contains("id not found in backlog/icebox"));
        drop(tmp);
    }

    #[test]
    fn edit_canonicalizes_nested_subtask_ids_immediately() {
        let (_tmp, doc) = doc_with_pending("- [ ] [#tmuxcrash] parent task\n");
        force_pending(|| {
            edit(
                &doc,
                "tmuxcrash",
                "parent task\n  - child dependency\n  - child subtask",
            )
        });

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
        force_pending(|| {
            edit(
                &doc,
                "tmuxcrash",
                "parent task\n  - fresh child\n  - fresh child two",
            )
        });

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
    fn edit_many_batches_multiple_replacements() {
        let (_tmp, doc) = doc_with_pending(concat!(
            "- [ ] [#first] first old text\n",
            "- [ ] [#second] second old text\n",
            "- [ ] [#third] third untouched\n",
        ));
        force_pending(|| {
            edit_many(
                &doc,
                &[
                    (
                        "first".to_string(),
                        "[operator-verify] first new text".to_string(),
                    ),
                    (
                        "second".to_string(),
                        "[operator-verify] second new text".to_string(),
                    ),
                ],
            )
        });

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- [ ] [#first] [operator-verify] first new text"));
        assert!(content.contains("- [ ] [#second] [operator-verify] second new text"));
        assert!(content.contains("- [ ] [#third] third untouched"));
        assert!(!content.contains("first old text"));
        assert!(!content.contains("second old text"));
    }

    #[test]
    fn list_prints_pending_items() {
        let (_tmp, doc) = doc_with_pending("- item one\n- item two");
        with_test_effects(|| list(&doc).unwrap());
        // Just checking it doesn't panic
    }

    #[test]
    fn resolve_gate_flips_typed_items() {
        let (_tmp, doc) = doc_with_pending(
            "- [/release] [#a1b2] Release v1.0\n- [/deploy] [#c3d4] Deploy\n- [/] [#e5f6] Generic",
        );
        force_pending(|| resolve_gate(&doc, "release"));
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("[x]"), "release item should be done");
        assert!(content.contains("[/deploy]"), "deploy should be untouched");
        assert!(content.contains("[/]"), "generic gate should be untouched");
    }

    #[test]
    fn resolve_gate_noop_no_match() {
        let (_tmp, doc) = doc_with_pending("- [/release] [#a1b2] Release");
        with_test_effects(|| resolve_gate(&doc, "deploy").unwrap());
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("[/release]"), "should be unchanged");
    }

    #[test]
    fn set_gate_type_on_gated_item() {
        let (_tmp, doc) = doc_with_pending("- [/] [#a1b2] Release v1.0");
        force_pending(|| set_gate_type(&doc, "a1b2", "release"));
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

        let report = force_pending(|| add_ungate_tasks_for_review(&doc));
        assert_eq!(report.scanned, 2);
        assert_eq!(report.added.len(), 2, "{report:?}");
        assert!(report.skipped.is_empty());

        let after = fs::read_to_string(&doc).unwrap();
        assert!(after.contains("Ungate review item #rev1"), "{after}");
        assert!(after.contains("Ungate review item #rev2"), "{after}");

        // Re-run is idempotent: both already tracked, nothing added.
        let report2 = force_pending(|| add_ungate_tasks_for_review(&doc));
        assert_eq!(report2.scanned, 2);
        assert!(report2.added.is_empty(), "{report2:?}");
        assert_eq!(report2.skipped.len(), 2);
    }

    #[test]
    fn ungate_tasks_noop_when_no_gated_review_items() {
        let (_tmp, doc) = doc_with_pending("- [ ] [#a] open item\n");
        let report = with_test_effects(|| add_ungate_tasks_for_review(&doc).unwrap());
        assert_eq!(report.scanned, 0);
        assert!(report.added.is_empty());
    }

    #[test]
    fn list_review_items_extracts_tags_next_and_filters() {
        let (_tmp, doc) = setup_test_dir();
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#aa11] #foo-tag first item summary. NEXT: do the thing\n",
            "- [/] [#bb22] second item, no next, #bar\n",
            "- [/release] [#cc33] release-gated item\n",
            "<!-- /agent:review -->\n",
        );
        fs::write(&doc, content).unwrap();

        let all =
            with_test_effects(|| list_review_items(&doc, &ReviewListFilter::default()).unwrap());
        assert_eq!(all.len(), 3);

        let aa = all.iter().find(|v| v.id == "aa11").unwrap();
        assert!(aa.tags.contains(&"#foo-tag".to_string()), "{aa:?}");
        assert_eq!(aa.next.as_deref(), Some("do the thing"));

        let bb = all.iter().find(|v| v.id == "bb22").unwrap();
        assert!(bb.next.is_none());
        assert!(bb.tags.contains(&"#bar".to_string()), "{bb:?}");

        // gate-type filter
        let f = ReviewListFilter {
            gate_type: Some("release".into()),
            ..Default::default()
        };
        let rel = with_test_effects(|| list_review_items(&doc, &f).unwrap());
        assert_eq!(rel.len(), 1);
        assert_eq!(rel[0].id, "cc33");

        // has-next filter surfaces the actionable subset
        let f = ReviewListFilter {
            has_next: Some(true),
            ..Default::default()
        };
        let with_next = with_test_effects(|| list_review_items(&doc, &f).unwrap());
        assert_eq!(with_next.len(), 1);
        assert_eq!(with_next[0].id, "aa11");

        // no-next surfaces the stale-to-triage set
        let f = ReviewListFilter {
            has_next: Some(false),
            ..Default::default()
        };
        assert_eq!(
            with_test_effects(|| list_review_items(&doc, &f).unwrap()).len(),
            2
        );

        // tag filter accepts the bare form (without leading `#`)
        let f = ReviewListFilter {
            tag: Some("bar".into()),
            ..Default::default()
        };
        let tagged = with_test_effects(|| list_review_items(&doc, &f).unwrap());
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].id, "bb22");
    }

    fn doc_with_review(items: &str) -> (TempDir, PathBuf) {
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:backlog -->\n- [ ] [#keep1] unrelated open item\n<!-- /agent:backlog -->\n\n<!-- agent:review -->\n{items}\n<!-- /agent:review -->\n"
        );
        let (tmp, doc) = setup_test_dir();
        fs::write(&doc, content).unwrap();
        (tmp, doc)
    }

    #[test]
    fn review_remove_deletes_all_entries_sharing_an_id() {
        // #reviewrm: the duplicate `[/] #saevon` shape an interleaved finalize
        // leaves behind clears in one pass, with the unrelated item preserved.
        let (_tmp, doc) = doc_with_review(concat!(
            "- [/] [#saevon] activate early-ack\n",
            "- [/] [#saevon] activate early-ack\n",
            "- [/] [#other] keep me\n",
        ));
        force_pending(|| review_remove(&doc, "saevon"));
        let after = fs::read_to_string(&doc).unwrap();
        assert!(!after.contains("#saevon"), "{after}");
        assert!(after.contains("[#other]"), "{after}");
        assert!(after.contains("[#keep1]"), "{after}");
    }

    #[test]
    fn review_remove_errors_when_id_absent() {
        let (_tmp, doc) = doc_with_review("- [/] [#aa11] only item\n");
        let err = with_test_effects(|| review_remove(&doc, "nope99").unwrap_err());
        assert!(format!("{err:#}").contains("#nope99"), "{err:#}");
        // Document is untouched on a miss.
        let after = fs::read_to_string(&doc).unwrap();
        assert!(after.contains("[#aa11]"), "{after}");
    }

    #[test]
    fn review_resolve_archives_to_done_and_removes_from_review() {
        let (_tmp, doc) = doc_with_review(concat!(
            "- [/] [#aa11] finished work\n",
            "- [/] [#bb22] still open\n",
        ));
        force_pending(|| review_resolve(&doc, "aa11"));
        let after = fs::read_to_string(&doc).unwrap();
        // Removed from review.
        let review_body = find_review_component_in_content(&after)
            .unwrap()
            .unwrap()
            .content(&after)
            .to_string();
        assert!(!review_body.contains("[#aa11]"), "{review_body}");
        assert!(review_body.contains("[#bb22]"), "{review_body}");
        // Archived as done.
        assert!(after.contains("<!-- agent:done -->"), "{after}");
        let done_body = after
            .split("<!-- agent:done -->")
            .nth(1)
            .unwrap_or_default();
        assert!(done_body.contains("[#aa11]"), "{after}");
        assert!(done_body.contains("finished work"), "{after}");
    }
}

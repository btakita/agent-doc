//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_document::queue_projection::{
    set_in_progress_work_item_markers, strip_in_progress_marker, strip_priority_markers,
    sync_in_progress_marker_regions,
};
use agent_doc_element_backlog::backlog::{
    component_matches_tracked_surface, ensure_no_completed_tracked_items, format_dropped_refs,
    format_shadow_refs, maintenance_surface_label, review_counts, should_reap_already_done_mirrors,
    should_reap_ops_proof_completions, tracked_body_for_reorder,
};
use agent_doc_queue::{
    backlog_sync::AutoBacklogQueueSyncPolicy,
    control_binding::{
        converge_queue_control_binding_content, explicit_queue_go_mode, explicit_queue_start_mode,
        explicit_queue_stop_mode, strip_queue_activation_tokens_in_content,
    },
    free_text_admission::{
        FreeTextAdmissionScope, append_empty_agent_component, collect_actionable_free_text_prompts,
        ensure_queue_priority_attr, goal_command_already_queued, goal_command_for_ids,
        queue_currently_active_for_free_text_admission, queue_entry_is_admitted_free_text,
        queue_free_text_admission_scope,
    },
    queue_response::{free_text_head_answered_by_response, queue_prompt_text_is_free_text},
};
use agent_doc_workflow::preflight_policy::ResolvedFreeTextExecution;

/// Resolve the live finalize-pipeline view surfaced in preflight output
/// (`#fmrunid-wire`). Cycle-state is authoritative; the document
/// `agent_doc_pipeline:` frontmatter block is only a fallback hint when no live
/// cycle-state exists (e.g. a crash that wiped `.agent-doc/state` but left the
/// document mirror behind). Returns `None` when neither is present.
pub(crate) fn resolve_pipeline_state(
    file: &Path,
) -> Result<Option<agent_doc_frontmatter::frontmatter::AgentDocPipeline>> {
    if let Some(state) = crate::cycle_state::load(file)? {
        return Ok(Some(state.to_pipeline()));
    }
    let current = std::fs::read_to_string(file).unwrap_or_default();
    Ok(match agent_doc_frontmatter::frontmatter::parse(&current) {
        Ok((fm, _)) if !fm.pipeline.is_empty() => Some(fm.pipeline),
        _ => None,
    })
}

#[derive(Debug, Clone, Default)]
pub struct PendingMaintenanceReport {
    pub reordered: bool,
    pub backlog_gated_count: usize,
    pub review_count: usize,
    pub review_gated_count: usize,
    pub legacy_gated_in_backlog_count: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct PendingMaintenanceOptions {
    force_disk: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct GateVerifyOptions {
    force_disk: bool,
}

/// Run pending-component maintenance: lazy backfill, reap `[x]`, and reorder detection.
///
/// Any write-through (backfill / reap) is persisted and committed in the same pass.
/// Silent no-op when the document has no tracked-work component.
pub fn run_pending_maintenance(file: &Path) -> Result<PendingMaintenanceReport> {
    run_pending_maintenance_with_options(file, PendingMaintenanceOptions::default())
}

pub(crate) fn run_pending_maintenance_force_disk(file: &Path) -> Result<PendingMaintenanceReport> {
    run_pending_maintenance_with_options(file, PendingMaintenanceOptions { force_disk: true })
}

fn run_pending_maintenance_with_options(
    file: &Path,
    options: PendingMaintenanceOptions,
) -> Result<PendingMaintenanceReport> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(PendingMaintenanceReport::default()),
    };
    let components = match agent_doc_element::element::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(PendingMaintenanceReport::default()),
    };
    let tracked_surfaces: Vec<String> = components
        .iter()
        .filter(|c| is_tracked_work_component(&c.name))
        .map(|c| c.name.clone())
        .collect();
    if tracked_surfaces.is_empty() {
        return Ok(PendingMaintenanceReport::default());
    }

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let doc_id = agent_doc_fs::document_state_hash(&canonical)
        .unwrap_or_else(|_| file.display().to_string());

    let mut current_content = content.clone();
    let mut snapshot_content = snapshot::load(file)?;
    // Reorder detection (step 4) compares the file's backlog order against the
    // snapshot as it was at cycle start. Capture it before the loop re-syncs the
    // snapshot to the file (#pending-gate-snapshot-desync), otherwise the synced
    // snapshot masks a same-cycle reorder.
    let snapshot_at_start = snapshot_content.clone();
    let mut mutated = false;
    // #pending-gate-snapshot-desync: the snapshot may need re-syncing to the
    // file's tracked surfaces even when maintenance itself makes no change —
    // the write phase can apply --pending-gate / --pending-edit / --review-add
    // to the file without those reaching the content_ours snapshot. Tracked
    // separately from `mutated` so the snapshot is re-saved without an
    // unnecessary working-tree rewrite.
    let mut snapshot_mutated = false;
    let mut saw_completed_before = false;
    let project_root = file.canonicalize().ok().and_then(|canonical| {
        agent_doc_fs::find_project_root(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let already_done_ids = collect_agent_done_ids_with_root(&content, project_root.as_deref());

    for surface in &tracked_surfaces {
        let components = agent_doc_element::element::parse(&current_content)
            .with_context(|| format!("failed to parse components while maintaining {}", surface))?;
        let comp = components
            .into_iter()
            .find(|c| component_matches_tracked_surface(&c.name, surface))
            .with_context(|| format!("document is missing the {} component", surface))?;
        let body = comp.content(&current_content);

        let mut current_body = body.to_string();
        let surface_label = maintenance_surface_label(surface);
        saw_completed_before |=
            !agent_doc_element_backlog::backlog::completed_items(&current_body).is_empty();

        let (after_backfill, changed) = agent_doc_element_backlog::backlog::backfill(
            &current_body,
            &doc_id,
            &std::collections::HashSet::new(),
        );
        if changed {
            eprintln!(
                "[preflight] {}: backfilled missing hash ids / checkboxes",
                surface_label
            );
            current_body = after_backfill;
            mutated = true;
        }

        // #reviewrm: collapse identical same-id entries an interleaved finalize
        // can leave behind (the duplicate `[/] #id` pair preflight flags as
        // preset_item_id_collision). Only exact duplicates are removed; distinct
        // items that merely share an id are preserved so the ambiguity warning
        // still surfaces.
        let (after_dedupe, deduped_ids) =
            agent_doc_element_backlog::backlog::op_dedupe_identical_items(&current_body);
        if !deduped_ids.is_empty() {
            eprintln!(
                "[preflight] {}: deduped {} duplicate same-id entr{}: {}",
                surface_label,
                deduped_ids.len(),
                if deduped_ids.len() == 1 { "y" } else { "ies" },
                deduped_ids.join(", ")
            );
            current_body = after_dedupe;
            mutated = true;
        }

        if should_reap_already_done_mirrors(surface) && !already_done_ids.is_empty() {
            let (after_mirror_reap, mirror_items) =
                agent_doc_element_backlog::backlog::op_take_active_items_by_ids(
                    &current_body,
                    &already_done_ids,
                );
            if !mirror_items.is_empty() {
                let removed_ids: Vec<String> = mirror_items.iter().map(|i| i.id.clone()).collect();
                eprintln!(
                    "[preflight] {}: reaped {} already-done mirror item(s): {}",
                    surface_label,
                    mirror_items.len(),
                    removed_ids.join(", ")
                );
                current_body = after_mirror_reap;
                mutated = true;
            }
        }

        let mut removed_items = Vec::new();
        if should_reap_ops_proof_completions(surface) {
            // #opsproof-falsepos: never auto-archive an item that was added this
            // same cycle. A brand-new add is absent from the post-commit snapshot
            // captured at cycle start; such items describe just-landed dependency
            // work and must be closed explicitly, not reaped on the cycle they
            // appear. Only apply the guard when we have a snapshot baseline to
            // compare against (untracked scaffold docs have none).
            let snapshot_baseline = snapshot_at_start
                .as_deref()
                .filter(|s| !s.trim().is_empty());
            let snapshot_ids = snapshot_baseline.map(|snap| {
                agent_doc_element_backlog::ops_proof::surface_pending_ids(snap, surface)
            });
            // `#opsproof-samecycle-add`: the snapshot baseline alone is not enough.
            // In the `write`/`finalize` path the same invocation that adds an item
            // via `--review-add` / `--pending-add*` also re-syncs the on-disk
            // snapshot, so a brand-new same-cycle add is already present in
            // `snapshot_ids` and the snapshot test cannot exclude it. Cross-check
            // the ids cycle-state recorded as added this cycle and never reap them.
            let added_this_cycle = crate::cycle_state::pending_added_ids(file);
            let ops_proof_completions: Vec<
                agent_doc_element_backlog::ops_proof::OpsProofCompletion,
            > = agent_doc_element_backlog::ops_proof::ops_proof_completion_candidates(
                &current_body,
            )
            .into_iter()
            .filter(|candidate| {
                snapshot_ids
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&candidate.id))
            })
            .filter(|candidate| !added_this_cycle.contains(&candidate.id))
            .collect();
            if !ops_proof_completions.is_empty() {
                let evidence_by_id: HashMap<String, String> = ops_proof_completions
                    .iter()
                    .map(|candidate| (candidate.id.clone(), candidate.evidence.clone()))
                    .collect();
                let ids: HashSet<String> = ops_proof_completions
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect();
                let (after_ops_proof_reap, mut ops_proof_items) =
                    agent_doc_element_backlog::backlog::op_take_active_items_by_ids(
                        &current_body,
                        &ids,
                    );
                if !ops_proof_items.is_empty() {
                    let removed_ids: Vec<String> =
                        ops_proof_items.iter().map(|i| i.id.clone()).collect();
                    for item in &mut ops_proof_items {
                        item.state = agent_doc_element_backlog::backlog::PendingState::Done;
                        item.gate_type = None;
                    }
                    eprintln!(
                        "[preflight] {}: auto-completed {} ops-proof item(s): {}",
                        surface_label,
                        ops_proof_items.len(),
                        removed_ids.join(", ")
                    );
                    for item in &ops_proof_items {
                        let evidence = evidence_by_id
                            .get(&item.id)
                            .map(String::as_str)
                            .unwrap_or("ops_proof");
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "auto_complete_ops_proof file={} id={} surface={} evidence={}",
                                file.display(),
                                item.id,
                                surface_label,
                                evidence
                            ),
                        );
                    }
                    let _ = crate::cycle_state::record_pending_done_ids(file, &removed_ids);
                    let _ = crate::cycle_state::record_reaped_pending_ids(file, &removed_ids);
                    let _ = crate::cycle_state::mark_pending_mutations(file);
                    current_body = after_ops_proof_reap;
                    mutated = true;
                    removed_items.extend(ops_proof_items);
                }
            }
        }

        let (after_reap, reaped_items) =
            agent_doc_element_backlog::backlog::reap_with_items(&current_body)?;
        if !reaped_items.is_empty() {
            let removed_ids: Vec<String> = reaped_items.iter().map(|i| i.id.clone()).collect();
            eprintln!(
                "[preflight] {}: reaped {} item(s): {}",
                surface_label,
                reaped_items.len(),
                removed_ids.join(", ")
            );
            let _ = crate::cycle_state::record_reaped_pending_ids(file, &removed_ids);
            current_body = after_reap;
            mutated = true;
        }
        removed_items.extend(reaped_items);

        // Priority sort (#backlog-priority-attribute): when the component marker
        // carries `priority`, stable-sort items by their per-item `priority=<1..9>`
        // token (1 = highest; absent = lowest) so a downstream `agent:queue` sync
        // inherits the prioritized order.
        if comp.attrs.contains_key("priority")
            && let Some(sorted) =
                agent_doc_element_backlog::backlog::sort_by_priority(&current_body)
        {
            eprintln!("[preflight] {}: sorted by priority", surface_label);
            current_body = sorted;
            mutated = true;
        }

        // Re-sync the snapshot's tracked surface to the file's body whenever the
        // two diverge — even if maintenance made no change to it this pass. The
        // write phase persists --pending-gate / --pending-edit / --review-add to
        // the file but saves the content_ours snapshot (baseline + response)
        // before those mutations, so a pure gate/edit/review-add would otherwise
        // leave the snapshot stale and the mutation stranded as post-commit drift
        // (#pending-gate-snapshot-desync). --done already reaches this via reap,
        // which sets `mutated`; this also covers the no-reap mutations.
        if let Some(ref mut snap_content) = snapshot_content {
            let snap_comps = agent_doc_element::element::parse(snap_content).ok();
            let snap_comp = snap_comps
                .and_then(|cs| {
                    cs.into_iter()
                        .find(|c| component_matches_tracked_surface(&c.name, surface))
                })
                .with_context(|| {
                    format!(
                        "pending maintenance: snapshot is missing the {} component",
                        surface
                    )
                })?;
            let snap_body = snap_comp.content(snap_content).to_string();
            if snap_body != current_body {
                *snap_content = snap_comp.replace_content(snap_content, &current_body);
                snapshot_mutated = true;
            }
            if !removed_items.is_empty()
                && let Some(archived) = archive_pending_done(file, snap_content, &removed_items)?
            {
                *snap_content = archived;
                snapshot_mutated = true;
            }
        }

        if current_body == body {
            continue;
        }

        current_content = comp.replace_content(&current_content, &current_body);
        if !removed_items.is_empty()
            && let Some(archived) = archive_pending_done(file, &current_content, &removed_items)?
        {
            current_content = archived;
        }
    }

    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_top_backlog_status_content(
            &current_content,
        )?
    {
        eprintln!("[preflight] status: reconciled stale top-backlog marker");
        current_content = reconciled;
        mutated = true;
    }
    if let Some(ref mut snap_content) = snapshot_content
        && let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_top_backlog_status_content(
                snap_content,
            )?
    {
        *snap_content = reconciled;
        snapshot_mutated = true;
    }

    // `#staleshow` — surface "🔴 (restart/recycle your supervisor)" in the upper status area when the
    // live route-owned supervisor/controller serving this document is mapping a STALE
    // agent-doc binary (a newer build is installed but the running process never
    // recycled onto it). Reuse the existing recycle-staleness signal
    // (`stale_supervisor_warning_for_doc`, the `#fccsupwarn`/`#fccsupwarn2` IO check)
    // so there is one source of truth for "running supervisor is older than installed".
    // Idempotent: the marker is inserted once when stale and removed when fresh.
    let supervisor_binary_is_stale =
        crate::project_controller::stale_supervisor_warning_for_doc(file).is_some();
    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_stale_supervisor_status_content(
            &current_content,
            supervisor_binary_is_stale,
        )?
    {
        if supervisor_binary_is_stale {
            eprintln!("[preflight] status: surfaced stale-supervisor marker");
        } else {
            eprintln!("[preflight] status: cleared stale-supervisor marker");
        }
        current_content = reconciled;
        mutated = true;
    }
    if let Some(ref mut snap_content) = snapshot_content
        && let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_stale_supervisor_status_content(
                snap_content,
                supervisor_binary_is_stale,
            )?
    {
        *snap_content = reconciled;
        snapshot_mutated = true;
    }

    // 3. Persist any mutations to the working tree file and/or the snapshot.
    //    Writing to both (surgically, via component replace) keeps the two in
    //    sync so the upcoming step-2 `git::commit` stages the reaped+archived
    //    snapshot in a single commit. We no longer call `git::commit` here —
    //    see #64mb: calling commit inside maintenance produced a second commit
    //    per preflight whenever anything mutated. The snapshot is saved
    //    independently of the file write so a write-phase pending mutation that
    //    only diverged the snapshot (gate/edit/review-add) is still committed
    //    rather than stranded (#pending-gate-snapshot-desync).
    if mutated {
        // `#fcc0`: converge the reconciled document through the editor IPC when a
        // live JB listener is active so per-cycle pending maintenance never raises a
        // `File Cache Conflict`; `content` is the pre-maintenance on-disk baseline.
        // Falls back to the same plain disk write otherwise. The post-write reap
        // verification below reads `current_content` (not disk), so converging here
        // introduces no read-after-write race.
        persist_pending_maintenance_doc(
            file,
            &content,
            &current_content,
            "pending_maintenance",
            options.force_disk,
        )?;
    }
    if (mutated || snapshot_mutated)
        && let Some(snap_content) = &snapshot_content
        && let Err(e) = snapshot::save(file, snap_content)
    {
        eprintln!("[preflight] pending: snapshot sync warning: {}", e);
    }

    if saw_completed_before {
        let persisted_content = if mutated {
            current_content.clone()
        } else {
            std::fs::read_to_string(file)
                .with_context(|| format!("failed to verify reap in {}", file.display()))?
        };
        ensure_no_completed_tracked_items(&persisted_content, "working tree")?;

        let snapshot_content = snapshot::load(file)?.with_context(|| {
            format!(
                "pending maintenance reaped completed tracked items in {} but the snapshot is missing",
                file.display()
            )
        })?;
        ensure_no_completed_tracked_items(&snapshot_content, "snapshot")?;
    }

    // 4. Reorder detection: compare the cycle-start snapshot's pending component
    //    to the current body. Uses the pre-sync snapshot (`snapshot_at_start`)
    //    rather than re-loading from disk, since step 3 may have re-synced the
    //    on-disk snapshot to the file (#pending-gate-snapshot-desync) which would
    //    otherwise hide a same-cycle reorder.
    let current_body = tracked_body_for_reorder(&current_content);
    let reordered = match snapshot_at_start {
        Some(snap) => {
            let snap_comp = agent_doc_element::element::parse(&snap)
                .ok()
                .and_then(|comps| comps.into_iter().find(|c| is_backlog_component(&c.name)));
            if let (Some(sc), Some(current_body)) = (snap_comp, current_body) {
                let snap_body = &snap[sc.open_end..sc.close_start];
                agent_doc_element_backlog::backlog::detect_reorder(snap_body, current_body)
                    .is_some()
            } else {
                false
            }
        }
        None => false,
    };
    if reordered {
        eprintln!("[preflight] backlog: reorder detected (skill must not reorder this cycle)");
    }

    // 5. Count legacy gated items in backlog and review items in review.
    let backlog_gated_count = current_body
        .map(|body| {
            let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(body);
            items
                .iter()
                .filter(|i| {
                    matches!(
                        i.state,
                        agent_doc_element_backlog::backlog::PendingState::Gated
                    )
                })
                .count()
        })
        .unwrap_or(0);
    if backlog_gated_count > 0 {
        eprintln!("[preflight] backlog: {} gated item(s)", backlog_gated_count);
    }

    let (review_count, review_gated_count) = review_counts(&current_content);
    if review_count > 0 {
        eprintln!(
            "[preflight] review: {} item(s), {} gated",
            review_count, review_gated_count
        );
    }

    Ok(PendingMaintenanceReport {
        reordered,
        backlog_gated_count,
        review_count,
        review_gated_count,
        legacy_gated_in_backlog_count: backlog_gated_count,
    })
}

fn persist_pending_maintenance_doc(
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
    force_disk: bool,
) -> Result<()> {
    if force_disk {
        std::fs::write(file, target)
            .with_context(|| format!("{source}: failed to write {}", file.display()))?;
        crate::write::record_document_write_provenance(file, target);
        crate::ops_log::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                target.len(),
                agent_doc_hash::content_hash(target)
            ),
        );
        return Ok(());
    }

    crate::write::guard_visible_write_idle_and_current(file, source, current)?;
    crate::write::converge_or_disk_write(file, current, target, source)
}

/// Opportunistic gated-review auto-verification (`#optverify` / `#optv3`).
///
/// For each gated `[/]` review item carrying a verify predicate, scan `ops.log`
/// and surface `provable` / `failed` / `pending`. When `autoverify` is true and
/// an item is `provable`, flip it `[/]→[x]` in place (persisting to both the
/// working-tree file and the snapshot, mirroring pending maintenance), so the
/// existing reap pass archives it on a later cycle. Default off — without the
/// opt-in the gate is only surfaced, never silently flipped.
///
/// Returns the per-item results for the preflight output. Best-effort: a missing
/// `ops.log`, no review component, or no predicates yields an empty vector.
pub(crate) fn run_gate_verify(file: &Path, autoverify: bool) -> Result<Vec<GateVerifyResult>> {
    run_gate_verify_with_options(file, autoverify, GateVerifyOptions::default())
}

#[cfg(test)]
fn run_gate_verify_force_disk(file: &Path, autoverify: bool) -> Result<Vec<GateVerifyResult>> {
    run_gate_verify_with_options(file, autoverify, GateVerifyOptions { force_disk: true })
}

fn run_gate_verify_with_options(
    file: &Path,
    autoverify: bool,
    options: GateVerifyOptions,
) -> Result<Vec<GateVerifyResult>> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let components = match agent_doc_element::element::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(review) = components
        .iter()
        .find(|c| is_review_component(&c.name))
        .cloned()
    else {
        return Ok(Vec::new());
    };
    let body = review.content(&content).to_string();
    let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(&body);

    // Gather predicate-bearing gated items.
    let predicates: Vec<(
        String,
        agent_doc_element_backlog::gate_verify::GatePredicate,
    )> = items
        .iter()
        .filter(|item| {
            matches!(
                item.state,
                agent_doc_element_backlog::backlog::PendingState::Gated
            )
        })
        .filter_map(|item| {
            agent_doc_element_backlog::gate_verify::parse_gate_predicate(&item.text)
                .filter(|p| p.is_actionable())
                .map(|p| (item.id.clone(), p))
        })
        .collect();
    if predicates.is_empty() {
        return Ok(Vec::new());
    }

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let ops_log = agent_doc_fs::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
        .and_then(|root| std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).ok())
        .unwrap_or_default();

    let mut results = Vec::new();
    let mut to_resolve: Vec<String> = Vec::new();
    for (id, predicate) in &predicates {
        let outcome = agent_doc_element_backlog::gate_verify::scan_ops_log(predicate, &ops_log);
        let (marker, at) = match &outcome {
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Provable { marker, at } => {
                (Some(marker.clone()), Some(*at))
            }
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Failed { marker, at } => {
                (Some(marker.clone()), Some(*at))
            }
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Pending => (None, None),
        };
        let status = outcome.status_str().to_string();
        let provable = matches!(
            outcome,
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Provable { .. }
        );
        let auto_resolved = autoverify && provable;
        if auto_resolved {
            to_resolve.push(id.clone());
        }
        match &outcome {
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Provable { marker, at } => {
                eprintln!(
                    "[preflight] optverify: review #{} provable (marker {:?} @ {})",
                    id, marker, at
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "optverify review={} status=provable marker={:?} at={} auto_resolved={}",
                        id, marker, at, auto_resolved
                    ),
                );
            }
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Failed { marker, at } => {
                eprintln!(
                    "[preflight] optverify: review #{} FAILED (disproof {:?} @ {}) — file a bug",
                    id, marker, at
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "optverify review={} status=failed marker={:?} at={}",
                        id, marker, at
                    ),
                );
            }
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Pending => {}
        }
        results.push(GateVerifyResult {
            id: id.clone(),
            status,
            marker,
            at,
            auto_resolved,
        });
    }

    // Opt-in transition: flip provable gates [/]→[x] in place, persisting to
    // both the working-tree file and the snapshot.
    if !to_resolve.is_empty() {
        let mut new_body = body.clone();
        for id in &to_resolve {
            new_body = agent_doc_element_backlog::backlog::op_done(&new_body, id)?;
        }
        let new_content = review.replace_content(&content, &new_body);
        persist_pending_maintenance_doc(
            file,
            &content,
            &new_content,
            "optverify_resolve",
            options.force_disk,
        )?;
        // Keep the snapshot in lockstep so the upcoming commit stages the flip.
        if let Some(snap) = snapshot::load(file)?
            && let Ok(snap_comps) = agent_doc_element::element::parse(&snap)
            && let Some(snap_review) = snap_comps.iter().find(|c| is_review_component(&c.name))
        {
            let snap_new = snap_review.replace_content(&snap, &new_body);
            snapshot::save(file, &snap_new)?;
        }
        eprintln!(
            "[preflight] optverify: auto-resolved {} provable gate(s): {}",
            to_resolve.len(),
            to_resolve.join(", ")
        );
    }

    Ok(results)
}

pub(crate) fn enforce_no_shadow_open_backlog(file: &Path) -> Result<()> {
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to inspect backlog shadow state in {}",
            file.display()
        )
    })?;
    let report = agent_doc_element_backlog::backlog::detect_shadow_open_items(&content)?;
    if !report.duplicated_in_live_backlog.is_empty() {
        eprintln!(
            "[preflight] pending shadow warning: open backlog item(s) also appear outside live agent:backlog: {}",
            format_shadow_refs(&report.duplicated_in_live_backlog)
        );
    }
    if !report.shadow_only.is_empty() {
        anyhow::bail!(
            "open backlog item(s) exist only outside live agent:backlog: {}. Move them back into the live backlog or mark them complete before continuing",
            format_shadow_refs(&report.shadow_only)
        );
    }
    Ok(())
}

pub(crate) fn enforce_no_dropped_backlog(file: &Path, rc: &crate::graph::RunContext) -> Result<()> {
    let head_content = match rc.head_content() {
        Some(content) => content,
        None => return Ok(()),
    };
    let current_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to inspect backlog replay state in {}",
            file.display()
        )
    })?;
    let resolved_ids = crate::cycle_state::resolved_pending_ids(file)?;

    let external_done_ids = external_done_archive_ids(file, &current_content)?;
    let report =
        agent_doc_element_backlog::backlog::detect_dropped_from_history_with_extra_current_ids(
            &current_content,
            &head_content,
            &resolved_ids,
            &external_done_ids,
        )?;
    if !report.dropped.is_empty() {
        anyhow::bail!(
            "open backlog item(s) from recent committed history are completely absent from the document: {}. Restore them to the live backlog, move them to icebox, or mark them done before continuing",
            format_dropped_refs(&report.dropped)
        );
    }
    Ok(())
}

/// Queue component state extracted during maintenance.
///
/// Returned by `run_queue_maintenance` for later composition into `PreflightOutput`.
/// The `queue_prompts` are only populated when the queue is active.
#[derive(Debug, Default)]
pub(crate) struct QueueState {
    pub(crate) queue_prompts: Vec<String>,
    pub(crate) selected_queue_prompts: Vec<String>,
    pub(crate) queue_active: Option<bool>,
    pub(crate) queue_deferred: bool,
    pub(crate) queue_start_at: Option<String>,
    pub(crate) queue_trigger: Option<agent_doc_queue::document_queue::QueueTrigger>,
    pub(crate) queue_halted: Option<String>,
    /// `#qpausego`: true when an accepted controller `admin queue pause` is the
    /// effective queue-control state. Surfaced for visibility and consumed by the
    /// supervisor idle-watch auto-injection guard; it does NOT gate
    /// `queue_continuation_required` / `queue_drainable_head_count` (the attended
    /// in-session `/loop` keeps draining real work). Cleared by `admin queue resume`.
    pub(crate) queue_paused: bool,
    /// `#qpausemix`: the controller-recorded pause reason when `queue_paused` is
    /// true (empty string when the pause carried none); `None` when not paused.
    /// Surfaced so the agent can see *why* the queue was paused instead of reading
    /// `queue_paused` + `queue_continuation_required` as a contradictory "mixed
    /// signal". Feeds the pause-aware `queue_continuation_guidance`.
    pub(crate) queue_pause_reason: Option<String>,
    /// `#cleardrainsignal`: count of agent-drainable heads (not deferred/noise) in
    /// the active queue. 0 while `queue_active` is `Some(true)` means a no-op churn
    /// cycle — the agent/auto-loop must NOT loop.
    pub(crate) queue_drainable_head_count: usize,
    /// `#cleardrainsignal`: whether the queue has agent-drainable continuation work
    /// this session. False when inactive OR every remaining head is deferred/noise.
    pub(crate) queue_continuation_required: bool,
    /// `#rt83`: whether the active queue head is drainable in the SUPERVISOR scope
    /// (defers `[operator-verify]`/noise only; `[focused-cycle]`/`[clean-session]`
    /// stay drainable because the supervisor force-`/clear`s + re-dispatches them).
    /// Gates the preflight synthetic queue-head diff: a head that no drainer (neither
    /// the in-session `/loop` nor the supervisor) will act on must NOT synthesize a
    /// phantom `+:pushpin: do [#id]` prompt diff, which previously kept
    /// `no_changes:false` every preflight and sustained the qchurn flood.
    pub(crate) queue_supervisor_drainable: bool,
    pub(crate) synced_queue_ids: Vec<String>,
    pub(crate) warnings: Vec<PreflightWarning>,
}

/// Deduplicate queue item node keys before queue maintenance projects state
/// from the markdown AST.
pub(crate) fn dedup_queue_nodes_by_key(content: &str) -> Result<Option<(String, usize)>> {
    let before_nodes =
        agent_doc_markdown_ast::mutations::item_nodes(content, "queue").map_err(|err| {
            anyhow::anyhow!("queue maintenance: failed to parse queue node keys: {err}")
        })?;
    let updated =
        agent_doc_markdown_ast::mutations::dedup_node_keys(content, "queue").map_err(|err| {
            anyhow::anyhow!("queue maintenance: failed to dedup queue node keys: {err}")
        })?;
    if updated == content {
        return Ok(None);
    }
    let after_nodes =
        agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue").map_err(|err| {
            anyhow::anyhow!("queue maintenance: failed to parse deduped queue node keys: {err}")
        })?;
    let dropped = before_nodes.len().saturating_sub(after_nodes.len());
    Ok(Some((updated, dropped)))
}

fn record_selected_queue_head_state(
    file: &Path,
    content: &str,
    head_text: &str,
    drainable: bool,
) -> Result<()> {
    let canonical = file.canonicalize().with_context(|| {
        format!(
            "queue maintenance: failed to canonicalize {}",
            file.display()
        )
    })?;
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(());
    };
    let Some(node_key) =
        agent_doc_queue::queue_projection::selected_queue_head_node_key(content, head_text)
    else {
        return Ok(());
    };
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let content_hash = agent_doc_hash::content_hash(head_text);
    let event = crate::state_backbone::StateEvent::new(
        format!("queue-head-selected:{document_hash}:{node_key}:0:{content_hash}"),
        crate::state_backbone::StateFact::QueueHeadSelected {
            document_hash: document_hash.clone(),
            node_key: node_key.clone(),
            backlog_id: agent_doc_queue::queue_response::queue_prompt_done_id(head_text),
            prompt_text: Some(head_text.to_string()),
            drainable,
            hosting_epoch: None,
        },
    );
    let inserted = crate::project_controller::append_state_event(&project_root, &event)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_selected_state_event_recorded file={} event_id={} inserted={} document_hash={} node_id={} drainable={}",
            file.display(),
            event.event_id,
            inserted,
            document_hash,
            node_key,
            drainable
        ),
    );
    Ok(())
}

fn record_deferred_queue_head_state(
    file: &Path,
    content: &str,
    head_text: &str,
    reason: &str,
) -> Result<()> {
    let canonical = file.canonicalize().with_context(|| {
        format!(
            "queue maintenance: failed to canonicalize {}",
            file.display()
        )
    })?;
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(());
    };
    let Some(node_key) =
        agent_doc_queue::queue_projection::selected_queue_head_node_key(content, head_text)
    else {
        return Ok(());
    };
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let content_hash = agent_doc_hash::content_hash(head_text);
    let selected_event = crate::state_backbone::StateEvent::new(
        format!("queue-head-deferred-selected:{document_hash}:{node_key}:0:{content_hash}"),
        crate::state_backbone::StateFact::QueueHeadSelected {
            document_hash: document_hash.clone(),
            node_key: node_key.clone(),
            backlog_id: agent_doc_queue::queue_response::queue_prompt_done_id(head_text),
            prompt_text: Some(head_text.to_string()),
            drainable: false,
            hosting_epoch: None,
        },
    );
    let selected_inserted =
        crate::project_controller::append_state_event(&project_root, &selected_event)?;
    let reason_hash = agent_doc_hash::content_hash(reason);
    let deferred_event = crate::state_backbone::StateEvent::new(
        format!("queue-head-deferred:{document_hash}:{node_key}:0:{reason_hash}:{content_hash}"),
        crate::state_backbone::StateFact::QueueHeadDeferred {
            document_hash: document_hash.clone(),
            node_key: node_key.clone(),
            reason: reason.to_string(),
            hosting_epoch: None,
        },
    );
    let deferred_inserted =
        crate::project_controller::append_state_event(&project_root, &deferred_event)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_deferred_state_event_recorded file={} selected_event_id={} selected_inserted={} deferred_event_id={} deferred_inserted={} document_hash={} node_id={} reason={}",
            file.display(),
            selected_event.event_id,
            selected_inserted,
            deferred_event.event_id,
            deferred_inserted,
            document_hash,
            node_key,
            reason
        ),
    );
    Ok(())
}

fn record_queue_worklist_state(
    file: &Path,
    content: &str,
    entries: &[agent_doc_queue::document_queue::QueueEntry],
    active: bool,
) -> Result<()> {
    let canonical = file.canonicalize().with_context(|| {
        format!(
            "queue maintenance: failed to canonicalize {}",
            file.display()
        )
    })?;
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(());
    };
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let queue_hash = agent_doc_queue::queue_projection::queue_worklist_hash(entries);
    let worklist_entries = if active {
        agent_doc_queue::queue_projection::queue_worklist_entries(content, entries)
            .into_iter()
            .map(|entry| crate::state_backbone::QueueWorklistEntry {
                kind: match entry.kind {
                    agent_doc_queue::queue_projection::QueueWorklistEntryKind::Prompt => {
                        crate::state_backbone::QueueWorklistEntryKind::Prompt
                    }
                    agent_doc_queue::queue_projection::QueueWorklistEntryKind::Preset => {
                        crate::state_backbone::QueueWorklistEntryKind::Preset
                    }
                    agent_doc_queue::queue_projection::QueueWorklistEntryKind::Dispatch => {
                        crate::state_backbone::QueueWorklistEntryKind::Dispatch
                    }
                },
                text: entry.text,
                node_key: entry.node_key,
                backlog_id: entry.backlog_id,
                drainable: entry.drainable,
            })
            .collect()
    } else {
        Vec::new()
    };
    let event = crate::state_backbone::StateEvent::new(
        format!("queue-worklist-projected:{document_hash}:{active}:{queue_hash}"),
        crate::state_backbone::StateFact::QueueWorklistProjected {
            document_hash: document_hash.clone(),
            queue_hash: queue_hash.clone(),
            entries: worklist_entries,
            active,
            hosting_epoch: None,
        },
    );
    let inserted = crate::project_controller::append_state_event(&project_root, &event)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_worklist_state_event_recorded file={} event_id={} inserted={} document_hash={} queue_hash={} active={}",
            file.display(),
            event.event_id,
            inserted,
            document_hash,
            queue_hash,
            active
        ),
    );
    Ok(())
}

/// Fold a proven editor-buffer queue deletion into the preflight queue source.
///
/// Queue maintenance normally starts from disk and then converges that queue
/// shape back into a live editor buffer. When the operator deletes queue rows in
/// the editor during a turn, that delete may be unsaved: blindly starting from
/// disk re-pushes the stale rows and makes them "reappear". Only adopt the live
/// queue when the live-buffer classifier says it is a real unsaved buffer and
/// the live queue is a count-wise subset of disk after stripping cosmetic
/// progress/pin markers. That covers deleting one duplicate row or all copies of
/// a row without treating same-cycle queue additions as an implicit merge.
fn adopt_live_buffer_queue_deletions(file: &Path, disk_content: &mut String) -> Result<bool> {
    let Some(file_str) = file.to_str() else {
        return Ok(false);
    };
    let Some(snapshot) =
        agent_doc_debounce::live_buffer_diverges_from_content(file_str, disk_content)
    else {
        return Ok(false);
    };
    let Some(live_content) = snapshot.content.as_deref() else {
        return Ok(false);
    };
    let disk_components = agent_doc_element::element::parse(disk_content)?;
    let live_components = agent_doc_element::element::parse(live_content)?;
    let Some(disk_queue) = disk_components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(false);
    };
    let Some(live_queue) = live_components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(false);
    };
    let disk_body = disk_queue.content(disk_content);
    let live_body = live_queue.content(live_content);
    if disk_body == live_body {
        return Ok(false);
    }
    let Some(disk_counts) = agent_doc_queue::document_queue::queue_delete_counts(disk_body) else {
        return Ok(false);
    };
    let Some(live_counts) = agent_doc_queue::document_queue::queue_delete_counts(live_body) else {
        return Ok(false);
    };
    if !agent_doc_queue::document_queue::queue_counts_have_deletion(&disk_counts, &live_counts)
        || !agent_doc_queue::document_queue::queue_counts_are_subset(&live_counts, &disk_counts)
    {
        return Ok(false);
    }

    let deleted_count: usize = disk_counts
        .iter()
        .map(|(key, disk_count)| {
            disk_count.saturating_sub(live_counts.get(key).copied().unwrap_or(0))
        })
        .sum();
    *disk_content = disk_queue.replace_content(disk_content, live_body);
    eprintln!(
        "[preflight] queue: adopted {deleted_count} live editor queue deletion(s) before maintenance (#qeditdelete)"
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_live_buffer_delete_adopted file={} deleted_count={} buffer_timestamp_ms={} (#qeditdelete)",
            file.display(),
            deleted_count,
            snapshot.timestamp_ms
        ),
    );
    Ok(true)
}

/// Read-only queue inspection for `preflight --probe`.
///
/// This intentionally does not run queue convergence, backlog mirroring,
/// in-progress marker updates, journals, or snapshot/frontmatter writes. It only
/// computes the queue facts needed for preflight JSON from the current document.
pub(crate) fn inspect_queue_state(file: &Path, diff: Option<&str>) -> Result<QueueState> {
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(QueueState::default()),
    };
    let snapshot_content = snapshot::load(file).ok().flatten();
    let (content, _) =
        converge_queue_control_binding_content(&content, snapshot_content.as_deref())?;
    let components = match agent_doc_element::element::parse(&content) {
        Ok(components) => components,
        Err(_) => return Ok(QueueState::default()),
    };
    let comp = match components
        .iter()
        .find(|component| component.name == "queue")
    {
        Some(component) => component,
        None => return Ok(QueueState::default()),
    };

    let body = &content[comp.open_end..comp.close_start];
    let entries = match agent_doc_queue::document_queue::parse(body) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("[preflight] queue probe parse warning: {err}");
            return Ok(QueueState::default());
        }
    };

    let marker_control = agent_doc_queue::document_queue::marker_control(&comp.attrs);
    let marker_stop = matches!(
        marker_control,
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Stop)
    );
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&comp.attrs)
        || matches!(
            marker_control,
            Some(agent_doc_frontmatter::frontmatter::QueueControl::Start)
        );
    let exchange_triggered = diff
        .map(agent_doc_diff::detect_queue_trigger)
        .unwrap_or(false);
    let (fm, _) = frontmatter::parse(&content).unwrap_or_default();
    let persisted_active = fm.queue_active.unwrap_or(false);
    let explicit_stop = explicit_queue_stop_mode(&comp.attrs, fm.queue.as_deref());
    let persisted_activation = persisted_active
        && (explicit_queue_go_mode(&comp.attrs, fm.queue.as_deref())
            || explicit_queue_start_mode(&comp.attrs, fm.queue.as_deref()));

    let mut activation = agent_doc_queue::document_queue::resolve_activation(
        &entries,
        has_auto,
        exchange_triggered,
        persisted_activation,
    );
    if marker_stop && activation.active {
        activation = agent_doc_queue::document_queue::QueueActivation {
            entries_after: activation.entries_after,
            ..Default::default()
        };
    }

    if activation.active
        && agent_doc_queue::document_queue::has_stop_fence_at_head(&activation.entries_after)
    {
        return Ok(QueueState {
            queue_prompts: vec![],
            selected_queue_prompts: vec![],
            queue_active: Some(false),
            queue_deferred: false,
            queue_start_at: None,
            queue_trigger: activation.trigger,
            queue_halted: Some("stop_fence".to_string()),
            queue_paused: false,
            queue_pause_reason: None,
            queue_drainable_head_count: 0,
            queue_continuation_required: false,
            queue_supervisor_drainable: false,
            synced_queue_ids: vec![],
            warnings: vec![],
        });
    }

    if activation.active
        && let Some(start_at) =
            agent_doc_queue::document_queue::time_gate_at_head(&activation.entries_after)
    {
        return Ok(QueueState {
            queue_prompts: vec![],
            selected_queue_prompts: vec![],
            queue_active: None,
            queue_deferred: true,
            queue_start_at: Some(start_at.to_string()),
            queue_trigger: activation.trigger,
            queue_halted: None,
            queue_paused: false,
            queue_pause_reason: None,
            queue_drainable_head_count: 0,
            queue_continuation_required: false,
            queue_supervisor_drainable: false,
            synced_queue_ids: vec![],
            warnings: vec![],
        });
    }

    let queue_prompts = if activation.active {
        agent_doc_queue::document_queue::prompts(&activation.entries_after)
            .iter()
            .map(|prompt| strip_in_progress_marker(&prompt.text))
            .collect()
    } else {
        vec![]
    };
    let queue_pause_reason =
        agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(file);
    let queue_paused = queue_pause_reason.is_some();
    let drainability_content = if activation.active {
        let body = agent_doc_queue::document_queue::render(&activation.entries_after);
        let projected = comp.replace_content(&content, &body);
        frontmatter::merge_queue_state(&projected, true).unwrap_or(projected)
    } else {
        content.clone()
    };
    let queue_drainable_head_count = if activation.active {
        agent_doc_queue::queue_continuation::drainable_head_count(&drainability_content)
    } else {
        0
    };
    let queue_continuation_required = activation.active && queue_drainable_head_count > 0;
    let queue_supervisor_drainable = activation.active
        && agent_doc_queue::queue_continuation::live_drainable_continuation_head(
            &drainability_content,
            agent_doc_queue::queue_continuation::DrainScope::Supervisor,
        )
        .is_some();
    let selected_queue_prompts = if activation.active {
        agent_doc_queue::queue_projection::active_queue_prompt_projection(
            &drainability_content,
            &activation.entries_after,
            &agent_doc_queue::backlog_sync::collect_after_deps(&components, &content),
            agent_doc_queue::queue_projection::in_progress_marker_retarget_requested(
                diff,
                &drainability_content,
                &activation.entries_after,
            ),
        )
        .prompts
    } else {
        Vec::new()
    };

    Ok(QueueState {
        queue_prompts,
        selected_queue_prompts,
        queue_active: if activation.active {
            Some(true)
        } else if activation.deferred {
            None
        } else if persisted_active || explicit_stop {
            Some(false)
        } else {
            None
        },
        queue_deferred: activation.deferred,
        queue_start_at: activation.start_at,
        queue_trigger: activation.trigger,
        queue_halted: None,
        queue_paused,
        queue_pause_reason,
        queue_drainable_head_count,
        queue_continuation_required,
        queue_supervisor_drainable,
        synced_queue_ids: vec![],
        warnings: vec![],
    })
}

pub(crate) fn run_queue_maintenance(file: &Path, diff: Option<&str>) -> Result<QueueState> {
    // #sqedit-race Phase 2: defer ALL queue maintenance mutation while a different,
    // live process holds a fresh queue-edit lease (a direct `queue prune-noise` /
    // `queue consume` in flight). Round-tripping a torn intermediate queue through
    // the #mirrorall mirror / backlog→queue sync / #7r2s pin / dedup re-mangles
    // entries (double-pins, dropped heads). The brief lease makes this a yield, not
    // a stall: the direct edit completes in well under a TTL and the next preflight
    // performs maintenance normally on the settled queue.
    if let Some(holder_pid) =
        agent_doc_queue::queue_edit_owner::foreign_queue_edit_in_flight(&file.to_string_lossy())
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_maintenance_deferred reason=queue_edit_lease holder_pid={holder_pid} (#sqedit-race)"
            ),
        );
        eprintln!(
            "[preflight] queue: deferring maintenance — direct queue edit in flight (pid {holder_pid}; #sqedit-race)"
        );
        return Ok(QueueState::default());
    }
    let mut content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(QueueState::default()),
    };
    let adopted_live_queue_delete =
        adopt_live_buffer_queue_deletions(file, &mut content).unwrap_or(false);
    let mut current_content = content.clone();
    let mut mutated = adopted_live_queue_delete;
    let mut components = match agent_doc_element::element::parse(&current_content) {
        Ok(cs) => cs,
        Err(_) => return Ok(QueueState::default()),
    };
    let exchange_prompt =
        agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content(&current_content);
    if components.iter().all(|c| c.name != "queue")
        && collect_actionable_free_text_prompts(
            exchange_prompt.as_deref(),
            &[],
            &FreeTextAdmissionScope::None,
        )
        .has_work()
    {
        current_content = append_empty_agent_component(&current_content, "queue");
        content = current_content.clone();
        components = match agent_doc_element::element::parse(&current_content) {
            Ok(cs) => cs,
            Err(_) => return Ok(QueueState::default()),
        };
        mutated = true;
        eprintln!("[preflight] queue: created agent:queue for admitted free-text work");
    }
    let mut comp = match components.iter().find(|c| c.name == "queue").cloned() {
        Some(c) => c,
        None => return Ok(QueueState::default()),
    };

    let body = &current_content[comp.open_end..comp.close_start];
    let entries = match agent_doc_queue::document_queue::parse(body) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[preflight] queue parse warning: {}", e);
            return Ok(QueueState::default());
        }
    };

    let mut entries = entries;
    let mut queue_warnings = Vec::new();
    let mut synced_queue_ids = Vec::new();
    let mut source_queue_priority = false;
    let mut queue_tag_attrs_normalized = false;

    // #qprtloss: journal the live queue as soon as it is parsed, before any
    // convergence/normalization branch can return early and erase an
    // uncommitted operator prompt from the visible queue.
    if let Err(err) = crate::queue_journal::record(file, &current_content) {
        eprintln!(
            "[agent-doc] queue_journal: early record failed for {} ({err:#})",
            file.display()
        );
    }
    // `#qftloss` mode-6: also journal any operator queue prompt that lives only in
    // the live editor buffer (reported to `.agent-doc/live-buffer/<hash>` but not
    // yet on disk), so a pre-observation operator add is crash-durable from this
    // cycle on — before any convergence/normalization branch below can erase it.
    if let Err(err) = crate::queue_journal::record_live_buffer(file) {
        eprintln!(
            "[agent-doc] queue_journal: early live-buffer record failed for {} ({err:#})",
            file.display()
        );
    }

    let raw_queue_tag = &current_content[comp.open_start..comp.open_end];
    let normalized_queue_tag =
        agent_doc_queue::document_queue::normalize_queue_tag_attrs(raw_queue_tag);
    if normalized_queue_tag != raw_queue_tag {
        let mut rebuilt = String::with_capacity(current_content.len());
        rebuilt.push_str(&current_content[..comp.open_start]);
        rebuilt.push_str(&normalized_queue_tag);
        rebuilt.push_str(&current_content[comp.open_end..]);
        current_content = rebuilt;
        content = current_content.clone();
        components = agent_doc_element::element::parse(&current_content)?;
        comp = components
            .iter()
            .find(|c| c.name == "queue")
            .context("queue maintenance: queue component vanished after tag normalization")?
            .clone();
        mutated = true;
        queue_tag_attrs_normalized = true;
        eprintln!("[preflight] queue: normalized malformed queue marker attributes");
    }
    let persisted_active_before_binding = frontmatter::parse(&current_content)
        .ok()
        .and_then(|(fm, _)| fm.queue_active)
        .unwrap_or(false);
    let control_snapshot_content = snapshot::load(file).ok().flatten();
    if let (projected, true) = converge_queue_control_binding_content(
        &current_content,
        control_snapshot_content.as_deref(),
    )? {
        current_content = projected;
        content = current_content.clone();
        components = agent_doc_element::element::parse(&current_content)?;
        comp = components
            .iter()
            .find(|c| c.name == "queue")
            .context("queue maintenance: queue component vanished after control binding sync")?
            .clone();
        mutated = true;
        eprintln!("[preflight] queue: synchronized queue marker/frontmatter control binding");
    }

    let project_root = file.canonicalize().ok().and_then(|canonical| {
        agent_doc_fs::find_project_root(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let queue_active_for_free_text =
        queue_currently_active_for_free_text_admission(&current_content, &comp.attrs);
    let snapshot_content = snapshot::load(file).ok().flatten();
    let queue_free_text_scope = queue_free_text_admission_scope(
        &current_content,
        &comp.attrs,
        &entries,
        snapshot_content.as_deref(),
    );
    if let Some(admission) = admit_free_text_work(
        file,
        &current_content,
        &entries,
        project_root.as_deref(),
        &queue_free_text_scope,
        !queue_active_for_free_text,
    )? {
        current_content = admission.content;
        content = current_content.clone();
        components = agent_doc_element::element::parse(&current_content)?;
        comp = components
            .iter()
            .find(|c| c.name == "queue")
            .context("queue maintenance: queue component vanished after free-text admission")?
            .clone();
        let body = &current_content[comp.open_end..comp.close_start];
        entries = agent_doc_queue::document_queue::parse(body)
            .context("queue maintenance: failed to parse queue after free-text admission")?;
        synced_queue_ids.extend(admission.queued_ids);
        queue_warnings.extend(admission.warnings);
        mutated = true;
        eprintln!(
            "[preflight] queue: admitted {} free-text prompt(s) into backlog ({})",
            admission.admitted_count, admission.execution_label
        );
    }

    // `#ynra`: collect `agent:done` ids ONCE up front. The backlog→queue sync
    // below must never re-mint a `do [#id]` whose id is already completed
    // (archived in `agent:done`) — otherwise the strike pass removes it every
    // cycle, the sync re-injects it the next cycle, and the queue churns forever
    // on a completed ref. `agent:done` is not mutated by any queue maintenance
    // step, so this set is valid for both the sync filter and the later strike.
    let done_ids = collect_agent_done_ids_with_root(&content, project_root.as_deref());

    // Backlog→queue sync (#backlog-queue-sync-attr): when an `agent:backlog`
    // component carries a `queue` attribute, regenerate the queue `do [#id]`
    // prompts from its active items BEFORE activation so a freshly synced queue
    // can auto-activate on the same cycle. `agent:icebox` is intentionally not a
    // component-level sync source; parked work must be moved to backlog or
    // explicitly marked for enqueue. Per-item enqueue markers
    // (#queue-enqueue-action) append marked ids without requiring the component
    // attribute.
    if let Some(sync_request) =
        agent_doc_queue::backlog_sync::collect_backlog_queue_sync(&components, &content)
    {
        let mode = sync_request.mode;
        source_queue_priority = sync_request.priority;
        // #provauth2: honor operator queue deletes. An id the operator deleted
        // from the live queue (active in the committed snapshot, now entirely
        // gone — not merely struck/consumed) is tombstoned so the backlog→queue
        // mirror does not resurrect it ("I deleted items but they reappeared").
        // The tombstone self-clears when the operator re-adds the id as an active
        // head. This makes an operator delete authoritative, the same way #ynra
        // keeps *completed* ids out — but for *operator-deleted* uncompleted ids.
        let tombstones = {
            let snapshot_active_ids: std::collections::HashSet<String> =
                crate::snapshot::load(file)
                    .ok()
                    .flatten()
                    .and_then(|snap| {
                        let comps = agent_doc_element::element::parse(&snap).ok()?;
                        let q = comps.iter().find(|c| c.name == "queue")?;
                        let body = &snap[q.open_end..q.close_start];
                        let snap_entries = agent_doc_queue::document_queue::parse(body).ok()?;
                        Some(
                            snap_entries
                                .iter()
                                .filter(|e| {
                                    matches!(
                                        e,
                                        agent_doc_queue::document_queue::QueueEntry::Prompt(_)
                                    )
                                })
                                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                                .collect(),
                        )
                    })
                    .unwrap_or_default();
            let current_all_ids: std::collections::HashSet<String> = entries
                .iter()
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .collect();
            let current_active_ids: std::collections::HashSet<String> = entries
                .iter()
                .filter(|e| matches!(e, agent_doc_queue::document_queue::QueueEntry::Prompt(_)))
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .collect();
            super::queue_tombstone::reconcile_for_file(
                file,
                &snapshot_active_ids,
                &current_all_ids,
                &current_active_ids,
            )
        };
        // #backlog-queue-sync-pending-add-amplification (decision B/C): while the
        // queue is already running (persisted-active auto-loop), do NOT promote
        // freshly-added backlog items into the live queue. Re-mirroring on every
        // cycle injected each new `--pending-add` as a `do [#id]` head, growing
        // the queue unboundedly and tripping pending_done_guard on each finalize.
        // Restrict the sync to ids already present as queue heads so captured
        // follow-ups wait for the NEXT activation instead of joining mid-loop. A
        // fresh activation (queue not yet active) still mirrors the full backlog.
        let incoming_frontmatter = frontmatter::parse(&content).ok().map(|(fm, _)| fm);
        let persisted_active_incoming = incoming_frontmatter
            .as_ref()
            .and_then(|fm| fm.queue_active)
            .unwrap_or(false);
        // `#backlog-queue-empty-active-repopulate`: gate the empty-active-queue
        // repopulation on the queue's explicit `go` control. `go` (frontmatter
        // `queue: go` or a marker-side `go` token) opts
        // into continuous-backlog-loop: when the live queue is fully drained (0
        // un-struck prompts), repopulate from the full active backlog instead of
        // holding. Without `go` (including a plain `queue: start` activation or
        // persisted-active queue), keep the drain-then-stop hold.
        let queue_go_mode = explicit_queue_go_mode(
            &comp.attrs,
            incoming_frontmatter
                .as_ref()
                .and_then(|fm| fm.queue.as_deref()),
        );
        let queue_explicitly_stopped = explicit_queue_stop_mode(
            &comp.attrs,
            incoming_frontmatter
                .as_ref()
                .and_then(|fm| fm.queue.as_deref()),
        );
        let sync_plan = agent_doc_queue::backlog_sync::plan_auto_backlog_queue_sync_ids(
            agent_doc_queue::backlog_sync::AutoBacklogQueueSyncInput {
                requested_ids: &sync_request.ids,
                enqueue_ids: &sync_request.enqueue_ids,
                done_ids: &done_ids,
                tombstones: &tombstones,
                entries: &entries,
                persisted_active_incoming,
                persisted_active_before_binding,
                queue_go_mode,
                queue_explicitly_stopped,
            },
        );
        if sync_plan.completed_excluded_count > 0 {
            eprintln!(
                "[preflight] queue: excluded {} completed id(s) from backlog→queue sync (already in agent:done; #ynra)",
                sync_plan.completed_excluded_count
            );
        }
        if sync_plan.tombstone_suppressed_count > 0 {
            eprintln!(
                "[preflight] queue: suppressed {} operator-deleted id(s) \
                 from backlog→queue mirror (#provauth2 tombstone)",
                sync_plan.tombstone_suppressed_count
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "queue_mirror_tombstone_suppressed file={} count={} (#provauth2)",
                    file.display(),
                    sync_plan.tombstone_suppressed_count
                ),
            );
        }
        match sync_plan.active_policy {
            AutoBacklogQueueSyncPolicy::ExplicitlyStopped if sync_plan.active_held_count > 0 => {
                eprintln!(
                    "[preflight] queue: held {} backlog id(s) out of explicitly stopped queue binding",
                    sync_plan.active_held_count
                );
            }
            AutoBacklogQueueSyncPolicy::HoldFreshIds if sync_plan.active_held_count > 0 => {
                eprintln!(
                    "[preflight] queue: held {} freshly-added backlog id(s) out of the active auto-loop \
                     (they sync at the next activation; #backlog-queue-sync-pending-add-amplification)",
                    sync_plan.active_held_count
                );
            }
            AutoBacklogQueueSyncPolicy::GoModeAppend => {
                eprintln!(
                    "[preflight] queue: go-mode active queue — appending fresh backlog `queue`-attr id(s) \
                     (continuous-backlog-loop; #backlog-queue-attr-populates-in-go-mode)"
                );
            }
            AutoBacklogQueueSyncPolicy::FreshActivation
            | AutoBacklogQueueSyncPolicy::ExplicitlyStopped
            | AutoBacklogQueueSyncPolicy::HoldFreshIds => {}
        }
        let backlog_ids = sync_plan.ids;
        // #goqueuestall: keep agent-undrainable heads out of the auto-drain queue
        // so a `go`-mode queue does not perpetually re-mirror items it cannot run
        // in the current session type. `[operator-verify]` items are always
        // skipped (they need a human); `[clean-session]` items are skipped only
        // while a live editor-IPC listener is active (running them live risks
        // closeout corruption — a clean session re-queues them next cycle).
        {
            let exec_ctxs = agent_doc_queue::queue_continuation::collect_backlog_execution_contexts(
                &components,
                &content,
            );
            if exec_ctxs.values().any(|c| c.is_deferred()) {
                let live_ipc = project_root
                    .as_deref()
                    .map(crate::ipc_socket::is_listener_active)
                    .unwrap_or(false);
                let (_drainable, skipped) =
                    agent_doc_queue::queue_continuation::partition_drainable_backlog_ids(
                        &backlog_ids,
                        &exec_ctxs,
                    );
                if !skipped.is_empty() {
                    let session_label = if live_ipc { "live_ipc" } else { "clean" };
                    for skip in &skipped {
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "go_queue_mirror_deferred id=#{} reason={} session={} (#mirrorall)",
                                skip.id, skip.reason, session_label
                            ),
                        );
                    }
                    // #mirrorall (operator directive 2026-06-18): mirror ALL open
                    // `queue`-attr backlog ids into the queue, INCLUDING `[operator-verify]`
                    // items, so the queue is a complete worklist (an operator-verify head
                    // surfaces the operator instructions carried in the item text). Crucially
                    // `backlog_ids` is NOT narrowed to the drainable subset: `head_is_drainable`
                    // already defers operator-verify ids (via `deferred_backlog_ids`), so
                    // mirroring them does NOT re-arm the in-session auto-drain loop
                    // (`queue_drainable_head_count` still excludes them). The supervisor
                    // idle-watch must apply the same drainability defer before a mirrored queue
                    // is resumed (#rz3a), else operator-verify-only heads re-injection-thrash;
                    // the queue stays operator-paused until that companion lands. This
                    // supersedes the prior #goqueuestall/#qcontdrain queue-skip that kept
                    // operator-verify items out of the queue entirely.
                    eprintln!(
                        "[preflight] queue: mirrored {} operator-verify backlog head(s) into the queue \
                         (deferred from auto-drain; #mirrorall)",
                        skipped.len()
                    );
                }
            }
        }
        if let Some(synced) =
            agent_doc_queue::document_queue::sync_backlog_into_queue(&entries, &backlog_ids, mode)
        {
            let pre_sync_ids = entries
                .iter()
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .collect::<std::collections::HashSet<String>>();
            let mut seen_synced_ids = std::collections::HashSet::new();
            synced_queue_ids = synced
                .iter()
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .filter(|id| !pre_sync_ids.contains(id))
                .filter(|id| seen_synced_ids.insert(id.clone()))
                .collect();
            let new_body = agent_doc_queue::document_queue::render(&synced);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            let pre_sync_prompt_count = entries
                .iter()
                .filter(|e| matches!(e, agent_doc_queue::document_queue::QueueEntry::Prompt(_)))
                .count();
            eprintln!(
                "[preflight] queue: synced backlog → queue ({:?}, {} active id(s))",
                mode,
                backlog_ids.len()
            );
            if pre_sync_prompt_count == 0 {
                queue_warnings.push(PreflightWarning {
                    code: "backlog_queue_sync_pending".to_string(),
                    message: format!(
                        "{}: a backlog/pending queue sync request populated an empty queue. \
                         The binary synced {} item(s) this cycle. \
                         For manual one-shot sync outside binary preflight: `agent-doc queue sync <FILE>`.",
                        file.display(),
                        synced_queue_ids.len()
                    ),
                    document_agent: None,
                    active_harness: None,
                });
            }
            entries = synced;
            mutated = true;
        }
    }

    // Queue priority ordering (#backlog-priority-attribute): when the queue
    // marker carries `priority`, stable-sort its do-prompts by the priority of
    // the matching backlog/icebox item so append-built or manual queues come out
    // prioritized. The backlog itself is priority-sorted earlier in the pipeline
    // by run_pending_maintenance, so the rank map read here is already current.
    // Also runs when the rank map is empty so a `__prioritized__` manual pin
    // (#queue-manual-priority-override) still floats to the top of the queue even
    // when no backlog item carries a `priority` attribute.
    if comp.attrs.contains_key("priority") || source_queue_priority {
        let rank =
            agent_doc_queue::backlog_sync::collect_backlog_priority_ranks(&components, &content);
        let mut operator_authored_identities: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        if let Ok(Some(snap_content)) = snapshot::load(file)
            && let Ok(snap_components) = agent_doc_element::element::parse(&snap_content)
            && let Some(snap_queue) = snap_components.iter().find(|c| c.name == "queue")
        {
            let snap_body = &snap_content[snap_queue.open_end..snap_queue.close_start];
            if let Ok(snap_entries) = agent_doc_queue::document_queue::parse(snap_body) {
                if let Some(pinned) =
                    agent_doc_queue::document_queue::annotate_operator_priority_reorders(
                        &snap_entries,
                        &entries,
                    )
                {
                    let new_body = agent_doc_queue::document_queue::render(&pinned);
                    current_content = {
                        let comps = agent_doc_element::element::parse(&current_content)?;
                        let q = comps.iter().find(|c| c.name == "queue").unwrap();
                        q.replace_content(&current_content, &new_body)
                    };
                    eprintln!(
                        "[preflight] queue: pinned manually reordered prompt(s) with operator priority"
                    );
                    entries = pinned;
                    mutated = true;
                }
                // #7r2s/#qauthorderpin: a brand-new queue line the operator just
                // typed (absent from the snapshot, not one the binary appended
                // from the backlog this cycle) carries no visible pin. Thread its
                // stable identity into the priority/DAG sort below so the authored
                // slot is held without injecting a `:pushpin:`.
                let synced_set: std::collections::HashSet<String> =
                    synced_queue_ids.iter().cloned().collect();
                operator_authored_identities =
                    agent_doc_queue::document_queue::operator_authored_prompt_identities(
                        &snap_entries,
                        &entries,
                        &synced_set,
                    );
                if !operator_authored_identities.is_empty() {
                    eprintln!(
                        "[preflight] queue: preserving {} manually-added prompt slot(s) by stable identity (#qauthorderpin)",
                        operator_authored_identities.len()
                    );
                }
            }
        }
        // `#backlog-queue-append-stable`: a `do [#id]` queue prompt whose id is an
        // active backlog id is backlog-sourced — the binary's `queue` attribute
        // appends it at the tail. The priority sort holds such prompts AFTER the
        // pre-existing unpinned manual / free-text prompts instead of floating them
        // up by backlog rank, so the default `queue` append stays appended even
        // under `priority` ("append, not prepend, even with non-annotated items in
        // the queue"). Operator/agent pins are exempt (a pin is an explicit position
        // signal — `#7r2s` already operator-pins genuinely operator-typed new lines).
        // Keyed off the active backlog id set (durable across cycles), not a
        // new-this-cycle diff, so a previously-synced item does not float up later.
        let backlog_sourced: std::collections::HashSet<String> = components
            .iter()
            .filter(|c| matches!(c.name.as_str(), "backlog" | "pending"))
            .flat_map(|c| {
                agent_doc_element_backlog::backlog::active_item_ids(
                    &content[c.open_end..c.close_start],
                )
            })
            .map(|id| id.to_ascii_lowercase())
            .collect();
        // Auto-dag (#queue-auto-dag-priority): order by `after=#id` dependency
        // graph first (a blocker outranks a pin); fall back to the plain
        // pin+priority sort when there are no dependency edges.
        let deps = agent_doc_queue::backlog_sync::collect_after_deps(&components, &content);
        let sorted = agent_doc_queue::document_queue::sort_prompts_by_dag_with_operator_authored(
            &entries,
            &rank,
            &deps,
            &backlog_sourced,
            &operator_authored_identities,
        )
        .map(|s| ("auto-dag dependency order (blockers + pins)", s))
        .or_else(|| {
            agent_doc_queue::document_queue::sort_prompts_by_priority_with_operator_authored(
                &entries,
                &rank,
                &backlog_sourced,
                &operator_authored_identities,
            )
            .map(|s| ("backlog priority (operator pins position-locked)", s))
        });
        if let Some((how, sorted)) = sorted {
            let sorted = agent_doc_queue::document_queue::annotate_agent_priority_promotions(
                &entries, &sorted,
            )
            .unwrap_or(sorted);
            let new_body = agent_doc_queue::document_queue::render(&sorted);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            eprintln!("[preflight] queue: sorted do-prompts by {how}");
            entries = sorted;
            mutated = true;
        }
    }

    // Read current state. A marker-side queue control (`start`/`go`/`stop`,
    // #queue-state-unify) is the marker spelling of the canonical `queue:`
    // frontmatter control: `start`/`go` are a fresh-activation gesture
    // equivalent to the legacy `auto` attribute (routed through the Auto trigger,
    // not the continuation-only Persisted path), and `stop` forces the queue
    // inactive this cycle. The control token is stripped from the tag below when
    // the queue drains, mirroring `auto`.
    let marker_control = agent_doc_queue::document_queue::marker_control(&comp.attrs);
    let marker_stop = matches!(
        marker_control,
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Stop)
    );
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&comp.attrs)
        || matches!(
            marker_control,
            Some(agent_doc_frontmatter::frontmatter::QueueControl::Start)
        );
    let exchange_triggered = diff
        .map(agent_doc_diff::detect_queue_trigger)
        .unwrap_or(false);
    let (fm, _) = frontmatter::parse(&current_content).unwrap_or_default();
    let persisted_active = fm.queue_active.unwrap_or(false);
    let explicit_stop = explicit_queue_stop_mode(&comp.attrs, fm.queue.as_deref());
    let persisted_activation = persisted_active
        && (explicit_queue_go_mode(&comp.attrs, fm.queue.as_deref())
            || explicit_queue_start_mode(&comp.attrs, fm.queue.as_deref()));

    let mut activation = agent_doc_queue::document_queue::resolve_activation(
        &entries,
        has_auto,
        exchange_triggered,
        persisted_activation,
    );
    // A `stop` marker control forces the queue inactive this cycle regardless of
    // any other activation signal (#queue-state-unify), so the later
    // drain/clear path halts a running queue and strips the control token.
    if marker_stop && activation.active {
        activation = agent_doc_queue::document_queue::QueueActivation {
            entries_after: activation.entries_after,
            ..Default::default()
        };
    }
    let snapshot_was_active = snapshot_proves_queue_was_active(file);

    // Collapse duplicated queue nodes by durable AST node key, never by prompt
    // text. This keeps intentional repeated `do [#id]` prompts executable while
    // preserving a structural cleanup point for true duplicate node-key replay
    // residue from IPC/snapshot drift.
    // #queue-completed-items-escape-below-component: a post-commit CRDT/boundary
    // merge can displace struck queue items past `<!-- /agent:queue -->` into the
    // neighbouring parking-lot comment, where they render invisibly and
    // accumulate as orphaned residue. Drop any such displaced struck-queue line
    // (outside every agent component span) before the rest of queue maintenance.
    if let Some(repaired) =
        agent_doc_template::repair_queue_struck_items_escaped_below_marker(&current_content)
    {
        current_content = repaired;
        mutated = true;
        eprintln!(
            "[preflight] queue: removed displaced struck queue item(s) below the closing marker"
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_escape_repair file={} reason=struck_items_below_close_marker",
                file.display()
            ),
        );
    }

    if let Some((deduped_content, dropped)) = dedup_queue_nodes_by_key(&current_content)? {
        current_content = deduped_content;
        let comps = agent_doc_element::element::parse(&current_content)?;
        if let Some(q) = comps.iter().find(|c| c.name == "queue") {
            let body = &current_content[q.open_end..q.close_start];
            activation.entries_after = agent_doc_queue::document_queue::parse(body)
                .context("queue maintenance: failed to parse AST-deduped queue")?;
        }
        mutated = true;
        eprintln!("[preflight] queue: collapsed {dropped} duplicate queue node-key(s)");
    }

    // Unified, state-machine-driven queue convergence (#queuestatemachine2 /
    // #cgfx). This single pass replaces the former pile of independent dedup
    // normalizers (`dedup_bare_id_reference_heads` / #qdup-bare-id,
    // `dedup_pin_variant_do_heads` / #qdedupsync+#pushpinaccum,
    // `dedup_free_text_heads` / #qauthorder+#rt83qflood). It keys every
    // prompt-bearing entry by its durable head identity
    // (`agent_doc_element_queue::QueueItemIdentity`) and drives each identity's
    // per-item lifecycle SM to its lawful state: re-injecting an identity that
    // already has a lawful representative is a no-op transition, so a
    // stale-CRDT / supervisor re-emit cannot leave a visible duplicate —
    // duplication is structurally impossible rather than patched after the fact.
    // The historical passes survive only as transition guards inside
    // `converge_queue_via_lifecycle` (intentional-twin guard, pin-variant
    // collapse, snapshot-authored multiplicity) and as a thin migration shim
    // (the unit-tested individual functions remain `pub` so external callers and
    // their regression coverage do not break). Position-lock
    // (#queue-operator-pin-position-lock) is preserved: convergence is purely
    // subtractive at each identity's earliest slot.
    let snapshot_queue_entries: Vec<agent_doc_queue::document_queue::QueueEntry> =
        match snapshot::load(file) {
            Ok(Some(snap)) => agent_doc_element::element::parse(&snap)
                .ok()
                .and_then(|comps| {
                    comps
                        .iter()
                        .find(|c| c.name == "queue")
                        .map(|q| snap[q.open_end..q.close_start].to_string())
                })
                .and_then(|body| agent_doc_queue::document_queue::parse(&body).ok())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
    if let Some(converged_entries) = agent_doc_queue::document_queue::converge_queue_via_lifecycle(
        &activation.entries_after,
        &snapshot_queue_entries,
    ) {
        let dropped = activation
            .entries_after
            .len()
            .saturating_sub(converged_entries.len());
        let new_body = agent_doc_queue::document_queue::render(&converged_entries);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = comps
                .iter()
                .find(|c| c.name == "queue")
                .context("queue maintenance: queue component vanished before convergence")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = converged_entries;
        mutated = true;
        eprintln!(
            "[preflight] queue: converged {dropped} duplicate queue head(s) via per-item lifecycle SM (#cgfx)"
        );
    }

    // Consume start fence if needed
    if activation.consumed_start_fence {
        let new_body = agent_doc_queue::document_queue::render(&activation.entries_after);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = comps.iter().find(|c| c.name == "queue").unwrap();
            q.replace_content(&current_content, &new_body)
        };
        mutated = true;
        eprintln!("[preflight] queue: consumed start fence");
    }

    // Auto-strike queue head prompts whose `#id` is already in `agent:done`.
    //
    // Without this, the queue stays wedged on the first done item whenever
    // the cycle's diff does not literally match the queue head text — for
    // example after the user types new prompts into the exchange or after a
    // commit-mode finalize that reaped the backlog item via `--done` but
    // could not advance the queue because the prompt-text did not match
    // verbatim. The `should_consume_queue_prompt_for_diff_content` gate is
    // intentionally strict; this preflight-side maintenance pass is the
    // catch-up path that keeps the auto queue moving across already-resolved
    // items.
    //
    // Fixes the user-reported "queue gets stuck after 1 turn" symptom.
    // `project_root` / `done_ids` were computed once before the backlog→queue
    // sync (above) and reused here — `agent:done` is untouched by queue
    // maintenance, so the set is still current.
    let gated_ids = agent_doc_element_review::collect_gated_review_ids(&current_content);
    let mut eligible_ids: std::collections::HashSet<String> = done_ids.clone();
    for id in &gated_ids {
        eligible_ids.insert(id.clone());
    }
    let mut eligible_id_list: Vec<String> = eligible_ids.iter().cloned().collect();
    eligible_id_list.sort();
    // `activation.entries_after` already reflects start-fence consumption and
    // the duplicate-prompt collapse above, so it is the authoritative current
    // entry set for the strike pass in every branch.
    let entries_for_strike = activation.entries_after.clone();
    if !eligible_id_list.is_empty() {
        let (new_entries, struck) =
            agent_doc_queue::queue_consume::mark_entries_completed_by_done_ids(
                &entries_for_strike,
                &eligible_id_list,
            );
        if !struck.is_empty() {
            let new_body = agent_doc_queue::document_queue::render(&new_entries);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            mutated = true;
            for prompt_text in &struck {
                let source =
                    match agent_doc_queue::queue_response::queue_prompt_done_id(prompt_text) {
                        Some(id) if done_ids.contains(&id) => "done",
                        Some(id) if gated_ids.contains(&id) => "review_gated",
                        _ => "unknown",
                    };
                eprintln!(
                    "[preflight] queue: auto-struck already-resolved head prompt {:?} source={}",
                    prompt_text, source
                );
            }
            // Recompute activation against the rewritten entry list so subsequent
            // halt / step / dispatch maintenance phases see the post-strike head.
            activation.entries_after = new_entries;
            // If the strike consumed the entire live head set, the queue is now
            // drained residue — every queued `do [#id]` was resolved via
            // `agent:done` / review-gate. `resolve_activation` ran on the
            // pre-strike entries (live prompts present) so `active` is stale-true;
            // flip it false here so the drain-cleanup path below clears
            // `queue_active`, strips `auto`, and empties the body. Without this the
            // stale `active: true` either trips the `item_modified` halt (the
            // post-strike head is `None` vs a still-live snapshot head) or leaves
            // the queue reported active with an empty prompt set. (#drained-done-queue-clear)
            if agent_doc_queue::document_queue::prompts(&activation.entries_after).is_empty() {
                activation.active = false;
                activation.trigger = None;
            }
        }
    }

    // `#qheadresidue`: free-text catch-up strike. The per-cycle `#ftstrike`
    // (`strike_answered_free_text_queue_heads`) only matches THIS cycle's
    // `response_body`, so a free-text queue head answered by a PRIOR cycle — its
    // `> **Queue prompt:**` echo lives in committed `agent:exchange` but not in
    // the current response — was never struck. That left "completed residue"
    // that `check_free_text_queue_head_provenance` INTERRUPTs on every closeout
    // while backlog→queue convergence kept re-adding it, churning the go-queue
    // forever (live repro: the `🚧 JB Run Agent Doc` head).
    //
    // This strikes at the QUEUE-ENTRY level (`Prompt` → `Completed`), NOT via
    // `answered_free_text_head_node_keys`/`item_nodes`: the live churning heads
    // are BARE, multi-line operator-pasted blocks (no `- ` bullet, embedded
    // route-error code fences), which the markdown list-item parser does not
    // surface — only `agent_doc_queue::document_queue::parse` represents them (as a multiline
    // `Prompt`). The match reuses the SAME `free_text_head_answered_by_response`
    // predicate the session-check residue guard uses, so the preflight strike set
    // and the session-check INTERRUPT set agree — anything struck here is exactly
    // what would otherwise have INTERRUPTed closeout. A snapshot gate restricts
    // the strike to heads already present in the committed queue (mirroring
    // session-check's `committed_queue_contains_active_free_text_head`), so an
    // in-flight operator edit the convergence just added is never struck.
    let exchange_text = agent_doc_element::element::parse(&current_content)
        .ok()
        .and_then(|comps| {
            comps
                .iter()
                .find(|c| c.name == "exchange")
                .map(|c| c.content(&current_content).to_string())
        })
        .unwrap_or_default();
    if !exchange_text.trim().is_empty() {
        // Normalized prose of every free-text head committed in the snapshot — the
        // in-flight-edit gate. `snapshot_queue_entries` was parsed above.
        let gate_norm = |text: &str| -> String {
            strip_priority_markers(text)
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let committed_free_text: std::collections::HashSet<String> = snapshot_queue_entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Prompt(p) => Some(gate_norm(&p.text)),
                _ => None,
            })
            .collect();
        let mut struck_count = 0usize;
        let new_entries: Vec<agent_doc_queue::document_queue::QueueEntry> = activation
            .entries_after
            .iter()
            .map(|entry| match entry {
                agent_doc_queue::document_queue::QueueEntry::Prompt(p)
                    if queue_prompt_text_is_free_text(&current_content, &p.text)
                        && free_text_head_answered_by_response(&exchange_text, &p.text)
                        && committed_free_text.contains(&gate_norm(&p.text)) =>
                {
                    struck_count += 1;
                    // #qftstuck: a struck head is no longer in progress — drop the
                    // cosmetic `🚧` marker so it does not linger inside the
                    // strikethrough (`set_first_prompt_in_progress` re-applies it to
                    // the genuinely-active next head).
                    let cleaned = strip_in_progress_marker(&p.text);
                    if cleaned == p.text {
                        agent_doc_queue::document_queue::QueueEntry::Completed(p.clone())
                    } else {
                        agent_doc_queue::document_queue::QueueEntry::Completed(
                            agent_doc_queue::document_queue::QueuePrompt {
                                text: cleaned,
                                multiline: p.multiline,
                            },
                        )
                    }
                }
                other => other.clone(),
            })
            .collect();
        if struck_count > 0 {
            let new_body = agent_doc_queue::document_queue::render(&new_entries);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").context(
                    "queue maintenance: queue component vanished before free-text residue strike",
                )?;
                q.replace_content(&current_content, &new_body)
            };
            activation.entries_after = new_entries;
            mutated = true;
            eprintln!(
                "[preflight] queue: auto-struck {struck_count} answered free-text head(s) by committed exchange match (#qheadresidue)"
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "preflight_freetext_residue_strike file={} struck={struck_count}",
                    file.display(),
                ),
            );
            // If the strike emptied the live head set, mirror the id-backed
            // done-strike drain-clear so the queue does not report active with an
            // empty prompt set (#drained-done-queue-clear).
            if agent_doc_queue::document_queue::prompts(&activation.entries_after).is_empty() {
                activation.active = false;
                activation.trigger = None;
            }
        }
    }

    // `#qftbklgstrike`: convergence auto-strike of a LIVE free-text queue head
    // when it is already complete (a matching `agent:done` item exists) OR a
    // backlog item already addresses it (a matching active `agent:backlog` item
    // exists). This complements the `#qheadresidue` strike above (which needs a
    // committed `agent:exchange` ANSWER): here the head may never have been
    // answered, but the deterministic semantic scorer
    // (`semantic_queue_strike_matches`, the strike sibling of the existing
    // `semantic_completion_match` warning) proves the work is captured elsewhere,
    // so the operator prompt is redundant and lingering only churns the queue.
    //
    // SAFETY: only `QueueEntry::Prompt` heads that are free-text (no `#id` — id
    // heads have their own done-strike) are eligible, the match must clear the
    // conservative `QUEUE_STRIKE_THRESHOLD` (set above the `+1.0`
    // substring-contains bonus so an unrelated operator prompt can never reach
    // it), and a committed-snapshot gate (mirroring the `#qheadresidue` gate)
    // restricts the strike to heads already present in the committed queue so an
    // in-flight operator edit convergence just added is never struck. The strike
    // is annotation-only: the head is converted to a `Completed` entry whose text
    // names the matched id + reason, never deleted — preserving the operator's
    // prompt verbatim inside the strikethrough for auditability.
    {
        let gate_norm = |text: &str| -> String {
            strip_priority_markers(text)
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let committed_free_text: std::collections::HashSet<String> = snapshot_queue_entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Prompt(p) => Some(gate_norm(&p.text)),
                _ => None,
            })
            .collect();
        match crate::memory_cmd::semantic_queue_strike_matches(
            file,
            None,
            agent_doc_memory::QUEUE_STRIKE_THRESHOLD,
            16,
        ) {
            Ok(strike_matches) if !strike_matches.is_empty() => {
                // Match by normalized head text rather than parse index: earlier
                // maintenance phases may have mutated `entries_after` so its
                // indices need not align with the on-disk parse order
                // `semantic_queue_strike_matches` scored. Normalized text is the
                // stable free-text head identity (same key the `#qauthorder`
                // dedup uses).
                let mut by_norm: std::collections::HashMap<
                    String,
                    agent_doc_memory::QueueStrikeMatch,
                > = std::collections::HashMap::new();
                for m in strike_matches {
                    by_norm.entry(gate_norm(&m.candidate_text)).or_insert(m);
                }
                let mut struck: Vec<(agent_doc_memory::QueueStrikeMatch, String)> = Vec::new();
                let new_entries: Vec<agent_doc_queue::document_queue::QueueEntry> = activation
                    .entries_after
                    .iter()
                    .map(|entry| match entry {
                        agent_doc_queue::document_queue::QueueEntry::Prompt(p)
                            if queue_prompt_text_is_free_text(&current_content, &p.text)
                                && committed_free_text.contains(&gate_norm(&p.text)) =>
                        {
                            match by_norm.get(&gate_norm(&p.text)) {
                                Some(m) => {
                                    let id = m.matched_id.as_deref().unwrap_or("?");
                                    let reason = match m.matched_kind {
                                        agent_doc_memory::QueueStrikeMatchKind::Done => {
                                            format!("auto-struck: completed by #{id} (#qftbklgstrike)")
                                        }
                                        agent_doc_memory::QueueStrikeMatchKind::Backlog => {
                                            format!(
                                                "auto-struck: tracked by backlog #{id} (#qftbklgstrike)"
                                            )
                                        }
                                    };
                                    // Bake the reason INSIDE the strikethrough so the
                                    // rendered `- ~~<original> — <reason>~~` round-trips
                                    // through `parse_completed_inline` as a stable
                                    // `Completed` entry (a trailing suffix outside the
                                    // `~~` would re-parse as a live Prompt and churn).
                                    // #qftstuck: drop the cosmetic `🚧` in-progress
                                    // marker before baking — a struck head is no longer
                                    // in progress, so the marker must not linger inside
                                    // the strikethrough (and `set_first_prompt_in_progress`
                                    // re-applies it to the genuinely-active next head).
                                    let head_clean = strip_in_progress_marker(p.text.trim_end());
                                    let annotated = format!("{head_clean} — {reason}");
                                    struck.push((m.clone(), annotated.clone()));
                                    agent_doc_queue::document_queue::QueueEntry::Completed(agent_doc_queue::document_queue::QueuePrompt {
                                        text: annotated,
                                        multiline: p.multiline,
                                    })
                                }
                                None => entry.clone(),
                            }
                        }
                        _ => entry.clone(),
                    })
                    .collect();
                if !struck.is_empty() {
                    let new_body = agent_doc_queue::document_queue::render(&new_entries);
                    current_content = {
                        let comps = agent_doc_element::element::parse(&current_content)?;
                        let q = comps.iter().find(|c| c.name == "queue").context(
                            "queue maintenance: queue component vanished before backlog/done strike",
                        )?;
                        q.replace_content(&current_content, &new_body)
                    };
                    activation.entries_after = new_entries;
                    mutated = true;
                    for (m, _annotated) in &struck {
                        let kind = match m.matched_kind {
                            agent_doc_memory::QueueStrikeMatchKind::Done => "done",
                            agent_doc_memory::QueueStrikeMatchKind::Backlog => "backlog",
                        };
                        let display: String = m.candidate_text.chars().take(120).collect();
                        eprintln!(
                            "[preflight] queue: auto-struck free-text head matched={kind} #{} ({:.3}) (#qftbklgstrike): {:?}",
                            m.matched_id.as_deref().unwrap_or("?"),
                            m.score,
                            display,
                        );
                    }
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "preflight_freetext_backlog_done_strike file={} struck={}",
                            file.display(),
                            struck.len(),
                        ),
                    );
                    if agent_doc_queue::document_queue::prompts(&activation.entries_after)
                        .is_empty()
                    {
                        activation.active = false;
                        activation.trigger = None;
                    }
                }
            }
            Ok(_) => {}
            Err(err) => {
                eprintln!(
                    "[preflight] queue: backlog/done strike retrieval unavailable (#qftbklgstrike): {err}"
                );
            }
        }
    }

    // Phase 3: halt detection — stop fences and item modification
    if activation.active {
        // Stop fence at head → halt the queue
        if agent_doc_queue::document_queue::has_stop_fence_at_head(&activation.entries_after) {
            eprintln!("[preflight] queue: halt — stop fence at head");
            // Consume the stop fence
            let after_stop: Vec<agent_doc_queue::document_queue::QueueEntry> =
                activation.entries_after[1..].to_vec();
            let new_body = agent_doc_queue::document_queue::render(&after_stop);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            // Strip ephemeral activation controls and clear queue state.
            current_content = strip_queue_activation_tokens_in_content(&current_content)?;
            if persisted_active {
                current_content = frontmatter::merge_queue_state(&current_content, false)?;
            }
            // Persist to file + snapshot (skip the raw disk write behind a live
            // editor; #fccqueue routes the queue shape through IPC convergence).
            persist_queue_maintenance_doc(
                file,
                &current_content,
                project_root.as_deref(),
                "queue_halt",
            )?;
            if let Ok(Some(snap)) = snapshot::load(file) {
                let mut new_snap = snap.clone();
                if let Ok(sc) = agent_doc_element::element::parse(&new_snap)
                    && let Some(sq) = sc.iter().find(|c| c.name == "queue")
                {
                    new_snap = sq.replace_content(&new_snap, &new_body);
                    new_snap = strip_queue_activation_tokens_in_content(&new_snap)?;
                    if persisted_active
                        && let Ok(m) = frontmatter::merge_queue_state(&new_snap, false)
                    {
                        new_snap = m;
                    }
                    if new_snap != snap
                        && let Err(e) = snapshot::save(file, &new_snap)
                    {
                        eprintln!("[preflight] queue halt: snapshot sync warning: {}", e);
                    }
                }
            }
            record_queue_worklist_state(file, &current_content, &after_stop, false)?;
            if let Some(head) = agent_doc_queue::document_queue::first_prompt(&after_stop) {
                let head_text = strip_in_progress_marker(&head.text);
                record_deferred_queue_head_state(file, &current_content, &head_text, "stop_fence")?;
            }
            return Ok(QueueState {
                queue_prompts: vec![],
                selected_queue_prompts: vec![],
                queue_active: Some(false),
                queue_deferred: false,
                queue_start_at: None,
                queue_trigger: activation.trigger,
                queue_halted: Some("stop_fence".into()),
                queue_paused: false,
                queue_pause_reason: None,
                queue_drainable_head_count: 0,
                queue_continuation_required: false,
                queue_supervisor_drainable: false,
                synced_queue_ids,
                warnings: Vec::new(),
            });
        }

        // Time gate at head → defer if not yet time
        if let Some(dt) =
            agent_doc_queue::document_queue::time_gate_at_head(&activation.entries_after)
        {
            eprintln!("[preflight] queue: deferred — time gate at head: {}", dt);
            record_queue_worklist_state(file, &current_content, &activation.entries_after, false)?;
            if let Some(head) =
                agent_doc_queue::document_queue::first_prompt(&activation.entries_after)
            {
                let head_text = strip_in_progress_marker(&head.text);
                let reason = format!("time_gate:{dt}");
                record_deferred_queue_head_state(file, &current_content, &head_text, &reason)?;
            }
            return Ok(QueueState {
                queue_prompts: vec![],
                selected_queue_prompts: vec![],
                queue_active: None,
                queue_deferred: true,
                queue_start_at: Some(dt.to_string()),
                queue_trigger: activation.trigger,
                queue_halted: None,
                queue_paused: false,
                queue_pause_reason: None,
                queue_drainable_head_count: 0,
                queue_continuation_required: false,
                queue_supervisor_drainable: false,
                synced_queue_ids,
                warnings: Vec::new(),
            });
        }

        // Change detection: compare head prompt between snapshot and file, but
        // only for a queue that was already active. A newly auto/start/request
        // activated queue is operator-authored input for this cycle, not an
        // in-flight queue item edit.
        if snapshot_was_active
            && let Ok(Some(snap_content)) = snapshot::load(file)
            && let Ok(snap_comps) = agent_doc_element::element::parse(&snap_content)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let snap_body = &snap_content[snap_q.open_end..snap_q.close_start];
            if let Ok(snap_entries) = agent_doc_queue::document_queue::parse(snap_body)
                && {
                    // Apply the same done/gated strike to the snapshot's
                    // entries before comparing heads. A cycle that resolved a
                    // leading queue head via `--done` (so the strike pass above
                    // converted it to `Completed`) otherwise reads as a
                    // head-text change vs the still-live snapshot head and
                    // false-halts as `item_modified`, wedging the remaining
                    // live head behind drained residue. Striking both sides
                    // leaves only genuine operator head edits visible.
                    // (#drained-done-queue-clear)
                    let snap_entries_struck = if eligible_id_list.is_empty() {
                        snap_entries
                    } else {
                        let (entries, struck) =
                            agent_doc_queue::queue_consume::mark_entries_completed_by_done_ids(
                                &snap_entries,
                                &eligible_id_list,
                            );
                        if struck.is_empty() {
                            snap_entries
                        } else {
                            entries
                        }
                    };
                    agent_doc_queue::document_queue::detect_head_prompt_modified(
                        &snap_entries_struck,
                        &activation.entries_after,
                    )
                }
            {
                // #queue-no-stall-on-head-edit: a head prompt edit between
                // cycles only pauses the loop while the operator is actively
                // mid-edit. Once the buffer settles, adopt the edited head as
                // the new prompt and keep the queue armed instead of stripping
                // `auto` + forcing queue_active:false (the old behavior stalled
                // the loop on every settled head edit). The pause is retained
                // only while a live typing indicator proves the buffer is still
                // being edited, so we never grab a half-typed head.
                let head_edit_mid_typing = agent_doc_debounce::is_typing_via_file(
                    &file.to_string_lossy(),
                    preflight_debounce_ms(file),
                );
                if !head_edit_mid_typing {
                    eprintln!(
                        "[preflight] queue: head prompt modified but buffer settled — adopting edited head, continuing loop (#queue-no-stall-on-head-edit)"
                    );
                    adopt_edited_queue_head_into_snapshot(file, &current_content);
                    // Fall through to normal active-queue handling below; the
                    // queue stays active with the edited head as the new prompt.
                } else {
                    eprintln!(
                        "[preflight] queue: pause — head prompt modified mid-edit (buffer not settled); not grabbing a half-typed head"
                    );
                    // Strip ephemeral activation controls and clear queue state.
                    current_content = strip_queue_activation_tokens_in_content(&current_content)?;
                    if persisted_active {
                        current_content = frontmatter::merge_queue_state(&current_content, false)?;
                    }
                    persist_queue_maintenance_doc(
                        file,
                        &current_content,
                        project_root.as_deref(),
                        "queue_pause",
                    )?;
                    // Update snapshot
                    if let Ok(Some(snap2)) = snapshot::load(file) {
                        let mut ns = snap2.clone();
                        ns = strip_queue_activation_tokens_in_content(&ns)?;
                        if persisted_active
                            && let Ok(m) = frontmatter::merge_queue_state(&ns, false)
                        {
                            ns = m;
                        }
                        if ns != snap2
                            && let Err(e) = snapshot::save(file, &ns)
                        {
                            eprintln!("[preflight] queue halt: snapshot sync warning: {}", e);
                        }
                    }
                    record_queue_worklist_state(
                        file,
                        &current_content,
                        &activation.entries_after,
                        false,
                    )?;
                    if let Some(head) =
                        agent_doc_queue::document_queue::first_prompt(&activation.entries_after)
                    {
                        let head_text = strip_in_progress_marker(&head.text);
                        record_deferred_queue_head_state(
                            file,
                            &current_content,
                            &head_text,
                            "item_modified",
                        )?;
                    }
                    return Ok(QueueState {
                        queue_prompts: vec![],
                        selected_queue_prompts: vec![],
                        queue_active: Some(false),
                        queue_deferred: false,
                        queue_start_at: None,
                        queue_trigger: activation.trigger,
                        queue_halted: Some("item_modified".into()),
                        queue_paused: false,
                        queue_pause_reason: None,
                        queue_drainable_head_count: 0,
                        queue_continuation_required: false,
                        queue_supervisor_drainable: false,
                        synced_queue_ids,
                        warnings: Vec::new(),
                    });
                }
            }
        }
    }

    // Handle queue drain: if the queue has no remaining prompts, clear
    // queue_active, strip auto, and remove completed/directive residue.
    let queue_has_prompts =
        !agent_doc_queue::document_queue::prompts(&activation.entries_after).is_empty();
    let drained_residue = queue_entries_are_drained_residue(&activation.entries_after);
    let need_sync_newly_activated_queue_snapshot = activation.active && !snapshot_was_active;
    let need_set_active = activation.active && !persisted_active;
    let need_clear_active = !activation.active && persisted_active && !activation.deferred;
    let need_strip_auto = has_auto && !queue_has_prompts;
    let need_clear_non_auto_residue =
        !has_auto && !activation.active && !activation.deferred && drained_residue;
    let need_clear_drained_body =
        (need_strip_auto || need_clear_non_auto_residue) && !activation.deferred;

    if need_clear_drained_body {
        let comps = agent_doc_element::element::parse(&current_content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
        if !q.content(&current_content).trim().is_empty() {
            current_content = q.replace_content(&current_content, "");
            mutated = true;
            eprintln!("[preflight] queue: cleared drained queue body");
        }
    }

    if !activation.active
        && !activation.deferred
        && !activation.entries_after.is_empty()
        && !need_clear_drained_body
    {
        // `inactive_queue_residue` is a per-*edit* signal, not a per-preflight
        // nag. It is useful when the operator just added/changed content in an
        // inactive queue (so a `do [#id]` they expected to run silently will
        // not). It is pure noise when the inactive queue is unchanged from the
        // committed snapshot — exactly the steady state an `item_modified` halt
        // leaves behind, where re-warning on every preflight with no user edit
        // drives the #adoc-queue-ipc-drift loop. Only warn when the inactive
        // queue body actually changed since the snapshot this cycle.
        if inactive_queue_changed_vs_snapshot(file, &activation.entries_after) {
            queue_warnings.push(PreflightWarning {
                code: "inactive_queue_residue".to_string(),
                message: "agent:queue is inactive but still contains directive/item residue; only active queue state is executable priority context".to_string(),
                document_agent: None,
                active_harness: None,
            });
        } else {
            eprintln!(
                "[preflight] queue: inactive with retained entries unchanged from snapshot — stable, not re-flagged as residue"
            );
        }
    }

    // Strip auto attribute from opening tag when queue drains
    // Strip the activation token from the opening tag when the queue drains
    // (`auto`/`go`/`start`) or when a `stop` marker halts it (#queue-state-unify).
    // The token is the ephemeral activation gesture; once consumed it must not
    // re-trigger on the next cycle.
    if need_strip_auto || marker_stop {
        let comps = agent_doc_element::element::parse(&current_content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
        let raw_tag = &current_content[q.open_start..q.open_end];
        let new_tag = agent_doc_queue::document_queue::strip_control_from_tag(
            &agent_doc_queue::document_queue::strip_auto_from_tag(raw_tag),
        );
        if new_tag != raw_tag {
            let mut rebuilt = String::with_capacity(current_content.len());
            rebuilt.push_str(&current_content[..q.open_start]);
            rebuilt.push_str(&new_tag);
            rebuilt.push_str(&current_content[q.open_end..]);
            current_content = rebuilt;
            mutated = true;
            eprintln!(
                "[preflight] queue: stripped activation token ({})",
                if marker_stop { "stop" } else { "drained" }
            );
        }
    }

    // Persist canonical queue activation state to frontmatter (#queue-state-unify
    // phase 4: emit `queue: start`/`queue: stop`, migrating off `queue_active:`).
    if need_set_active {
        current_content = frontmatter::merge_queue_state(&current_content, true)?;
        mutated = true;
        eprintln!("[preflight] queue: set queue: start");
    } else if need_clear_active {
        current_content = frontmatter::merge_queue_state(&current_content, false)?;
        mutated = true;
        eprintln!("[preflight] queue: set queue: stop");
    }

    let mut in_progress_markers_changed = false;
    let active_queue_projection = if activation.active {
        let current_components = agent_doc_element::element::parse(&current_content)?;
        agent_doc_queue::queue_projection::active_queue_prompt_projection(
            &current_content,
            &activation.entries_after,
            &agent_doc_queue::backlog_sync::collect_after_deps(
                &current_components,
                &current_content,
            ),
            agent_doc_queue::queue_projection::in_progress_marker_retarget_requested(
                diff,
                &current_content,
                &activation.entries_after,
            ),
        )
    } else {
        agent_doc_document::queue_projection::ActiveQueuePromptProjection::default()
    };
    if active_queue_projection.retargeted {
        eprintln!(
            "[preflight] queue: honored operator in-progress marker retarget to {} active head(s)",
            active_queue_projection.prompts.len()
        );
    }
    if !active_queue_projection.missing_dependency_ids.is_empty() {
        queue_warnings.push(PreflightWarning {
            code: "queue_retarget_missing_prerequisite".to_string(),
            message: format!(
                "operator-selected queue head has prerequisite id(s) not present as live queue prompts: {}",
                active_queue_projection
                    .missing_dependency_ids
                    .iter()
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            document_agent: None,
            active_harness: None,
        });
    }
    let active_queue_prompt_texts = active_queue_projection.prompts;
    let current_head_ids = active_queue_prompt_texts
        .iter()
        .filter_map(|text| agent_doc_queue::queue_response::queue_prompt_done_id(text))
        .collect::<std::collections::HashSet<_>>();
    if let Some(marked_entries) = agent_doc_queue::document_queue::set_prompts_in_progress(
        &activation.entries_after,
        &active_queue_prompt_texts,
    ) {
        let new_body = agent_doc_queue::document_queue::render(&marked_entries);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = comps
                .iter()
                .find(|c| c.name == "queue")
                .context("queue maintenance: queue component vanished before in-progress marker")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = marked_entries;
        mutated = true;
        in_progress_markers_changed = true;
    }
    let (marked_content, pending_markers_changed) =
        set_in_progress_work_item_markers(&current_content, &current_head_ids)?;
    if pending_markers_changed {
        current_content = marked_content;
        mutated = true;
        in_progress_markers_changed = true;
    }
    let need_sync_active_queue_future_state_snapshot = activation.active
        && snapshot_was_active
        && selected_queue_head_unchanged_in_snapshot(file, &activation.entries_after)
        && queue_region_differs_from_snapshot(file, &current_content);

    // Persist file mutations.
    if mutated {
        persist_queue_maintenance_doc(
            file,
            &current_content,
            project_root.as_deref(),
            "queue_maintenance",
        )?;
    }

    // Persist snapshot mutations. For newly activated queues, sync the queue
    // component from the visible document into the snapshot so later closeout
    // consumption can prove the same head prompt in both places.
    if (mutated
        || need_sync_newly_activated_queue_snapshot
        || need_sync_active_queue_future_state_snapshot)
        && let Ok(Some(snap_content)) = snapshot::load(file)
    {
        let mut new_snap = snap_content.clone();

        if in_progress_markers_changed {
            new_snap = sync_in_progress_marker_regions(&new_snap, &current_content);
        }

        if queue_tag_attrs_normalized
            && let Ok(snap_comps) = agent_doc_element::element::parse(&new_snap)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let raw_tag = &new_snap[snap_q.open_start..snap_q.open_end];
            let normalized_tag =
                agent_doc_queue::document_queue::normalize_queue_tag_attrs(raw_tag);
            if normalized_tag != raw_tag {
                let mut rebuilt = String::with_capacity(new_snap.len());
                rebuilt.push_str(&new_snap[..snap_q.open_start]);
                rebuilt.push_str(&normalized_tag);
                rebuilt.push_str(&new_snap[snap_q.open_end..]);
                new_snap = rebuilt;
            }
        }

        if (need_sync_newly_activated_queue_snapshot
            || need_sync_active_queue_future_state_snapshot)
            && let Ok(current_comps) = agent_doc_element::element::parse(&current_content)
            && let Some(current_q) = current_comps
                .iter()
                .find(|component| component.name == "queue")
            && let Ok(snap_comps) = agent_doc_element::element::parse(&new_snap)
            && let Some(snap_q) = snap_comps
                .iter()
                .find(|component| component.name == "queue")
        {
            let queue_region = &current_content[current_q.open_start..current_q.close_end];
            let mut rebuilt = String::with_capacity(new_snap.len() + queue_region.len());
            rebuilt.push_str(&new_snap[..snap_q.open_start]);
            rebuilt.push_str(queue_region);
            rebuilt.push_str(&new_snap[snap_q.close_end..]);
            new_snap = rebuilt;
        }

        // Apply queue body change to snapshot
        if !need_sync_newly_activated_queue_snapshot
            && (activation.consumed_start_fence || need_strip_auto || need_clear_drained_body)
            && let Ok(snap_comps) = agent_doc_element::element::parse(&new_snap)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let new_body = if need_clear_drained_body {
                String::new()
            } else {
                agent_doc_queue::document_queue::render(&activation.entries_after)
            };
            new_snap = snap_q.replace_content(&new_snap, &new_body);

            if (need_strip_auto || marker_stop)
                && let Ok(snap_comps2) = agent_doc_element::element::parse(&new_snap)
                && let Some(snap_q2) = snap_comps2.iter().find(|c| c.name == "queue")
            {
                let raw_tag = &new_snap[snap_q2.open_start..snap_q2.open_end];
                let new_tag = agent_doc_queue::document_queue::strip_control_from_tag(
                    &agent_doc_queue::document_queue::strip_auto_from_tag(raw_tag),
                );
                if new_tag != raw_tag {
                    let mut rebuilt = String::with_capacity(new_snap.len());
                    rebuilt.push_str(&new_snap[..snap_q2.open_start]);
                    rebuilt.push_str(&new_tag);
                    rebuilt.push_str(&new_snap[snap_q2.open_end..]);
                    new_snap = rebuilt;
                }
            }
        }

        // Apply frontmatter change to snapshot
        if need_set_active && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, true) {
            new_snap = merged;
        } else if need_sync_newly_activated_queue_snapshot
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, true)
        {
            new_snap = merged;
        } else if need_clear_active
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, false)
        {
            new_snap = merged;
        }
        if need_clear_drained_body
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, false)
        {
            new_snap = merged;
        }

        if new_snap != snap_content
            && let Err(e) = snapshot::save(file, &new_snap)
        {
            eprintln!("[preflight] queue: snapshot sync warning: {}", e);
        }
    }

    // Build output
    let queue_prompts: Vec<String> = if activation.active {
        agent_doc_queue::document_queue::prompts(&activation.entries_after)
            .iter()
            .map(|p| strip_in_progress_marker(&p.text))
            .collect()
    } else {
        vec![]
    };

    // `#cleardrainsignal`: count heads the agent can actually drain this session,
    // applying the same `#goqueuestall`/`#goqstall2` deferred/noise filtering the
    // supervisor idle-watch uses. When the queue is active but this is 0, the
    // remaining heads are all `[clean-session]` (under live IPC) / `[operator-verify]`
    // / inert noise — a no-op churn cycle. Surfacing it lets the agent and the
    // Claude Code auto-loop stop without re-deriving drainability from prose, even
    // when the route-owned supervisor predates the idle-watch filter (#qchurn).
    // `#qdurcrash`: durably journal the operator queue prompts the binary
    // observes this cycle so an add survives a supervisor/pane crash+restart
    // (replayed at startup by `queue_journal::replay_missing`). Best-effort;
    // never wedges the cycle on a journal hiccup.
    if let Err(err) = crate::queue_journal::record(file, &content) {
        eprintln!(
            "[agent-doc] queue_journal: record failed for {} ({err:#})",
            file.display()
        );
    }

    // `#qpausego`: an accepted controller `admin queue pause` suppresses the
    // *unattended* supervisor idle-watch auto-injection (the flood this fixes —
    // see `start/idle_watch.rs`) and is surfaced here as `queue_paused` for
    // visibility. It deliberately does NOT drop `queue_continuation_required` or
    // `queue_drainable_head_count`: the attended in-session `/loop` is the
    // legitimate single-owner drain of real queue work and must keep going. A
    // pause stalling the in-session loop strands genuine drainable backlog
    // (`#qdurcrash`, `#733r`, …) — the operator-rejected over-reach. Use
    // `queue: stop` frontmatter / `--- stop` fences to stop the in-session loop.
    let queue_pause_reason =
        agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(file);
    let queue_paused = queue_pause_reason.is_some();
    let queue_drainable_head_count = if activation.active {
        agent_doc_queue::queue_continuation::drainable_head_count(&current_content)
    } else {
        0
    };
    let queue_continuation_required = activation.active && queue_drainable_head_count > 0;
    // `#rt83`: supervisor-scope drainability (defers `[operator-verify]`/noise only).
    // Used to gate the preflight synthetic queue-head diff so an operator-verify-only
    // (or otherwise non-actionable) head stops perpetually reporting `no_changes:false`.
    let queue_supervisor_drainable = activation.active
        && agent_doc_queue::queue_continuation::live_drainable_continuation_head(
            &current_content,
            agent_doc_queue::queue_continuation::DrainScope::Supervisor,
        )
        .is_some();
    record_queue_worklist_state(
        file,
        &current_content,
        &activation.entries_after,
        activation.active,
    )?;
    if activation.active && !active_queue_prompt_texts.is_empty() {
        for head_text in &active_queue_prompt_texts {
            record_selected_queue_head_state(file, &current_content, head_text, true)?;
        }
    } else if activation.deferred
        && let Some(head) = agent_doc_queue::document_queue::first_prompt(&activation.entries_after)
    {
        let head_text = strip_in_progress_marker(&head.text);
        let reason = activation
            .start_at
            .as_deref()
            .map(|start_at| format!("time_gate:{start_at}"))
            .unwrap_or_else(|| "deferred".to_string());
        record_deferred_queue_head_state(file, &current_content, &head_text, &reason)?;
    }

    Ok(QueueState {
        queue_prompts,
        selected_queue_prompts: active_queue_prompt_texts,
        queue_active: if activation.active {
            Some(true)
        } else if activation.deferred {
            None
        } else if persisted_active || explicit_stop {
            Some(false)
        } else {
            None
        },
        queue_deferred: activation.deferred,
        queue_start_at: activation.start_at,
        queue_trigger: activation.trigger,
        queue_halted: None,
        queue_paused,
        queue_pause_reason,
        queue_drainable_head_count,
        queue_continuation_required,
        queue_supervisor_drainable,
        synced_queue_ids,
        warnings: queue_warnings,
    })
}

/// Closeout-side repair for same-cycle backlog capture.
///
/// Preflight's normal backlog→queue sync runs before `finalize` / `write`
/// applies `--pending-add*` mutations, so a go-mode document can commit a fresh
/// backlog item without a matching queue head. This helper runs after closeout
/// queue consumption, appending only ids that were explicitly recorded as
/// same-cycle pending additions. It never applies a full priority/sync recompute,
/// so it cannot move the head that the current response just consumed.
pub(crate) fn sync_same_cycle_pending_adds_into_go_queue(file: &Path) -> Result<Vec<String>> {
    let added_this_cycle = crate::cycle_state::pending_added_ids(file);
    if added_this_cycle.is_empty() {
        return Ok(Vec::new());
    }

    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let components = match agent_doc_element::element::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(queue_component) = components.iter().find(|c| c.name == "queue") else {
        return Ok(Vec::new());
    };
    let queue_body = &content[queue_component.open_end..queue_component.close_start];
    let entries = match agent_doc_queue::document_queue::parse(queue_body) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("[write] queue: same-cycle pending-add sync skipped — parse warning: {e}");
            return Ok(Vec::new());
        }
    };

    let (fm, _) = frontmatter::parse(&content).unwrap_or_default();
    let queue_go_mode = explicit_queue_go_mode(&queue_component.attrs, fm.queue.as_deref());
    let queue_active = fm.queue_active.unwrap_or(false) || queue_go_mode;
    if !queue_active || !queue_go_mode {
        return Ok(Vec::new());
    }

    let backlog_has_queue_attr = components.iter().any(|comp| {
        comp.name == "backlog"
            && comp
                .attrs
                .get("queue")
                .and_then(|value| {
                    agent_doc_queue::document_queue::BacklogQueueSyncMode::parse(value)
                })
                .is_some()
    });
    if !backlog_has_queue_attr {
        return Ok(Vec::new());
    }

    let Some(sync_request) =
        agent_doc_queue::backlog_sync::collect_backlog_queue_sync(&components, &content)
    else {
        return Ok(Vec::new());
    };
    let pending_norm: std::collections::HashSet<String> = added_this_cycle
        .into_iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
        .filter(|id| !id.is_empty())
        .collect();
    let mut backlog_ids: Vec<String> = sync_request
        .ids
        .into_iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
        .filter(|id| pending_norm.contains(id))
        .collect();
    if backlog_ids.is_empty() {
        return Ok(Vec::new());
    }

    let project_root = file.canonicalize().ok().and_then(|canonical| {
        agent_doc_fs::find_project_root(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let done_ids: std::collections::HashSet<String> =
        collect_agent_done_ids_with_root(&content, project_root.as_deref())
            .into_iter()
            .map(|id| id.to_ascii_lowercase())
            .collect();
    if !done_ids.is_empty() {
        backlog_ids.retain(|id| !done_ids.contains(&id.to_ascii_lowercase()));
    }

    let exec_ctxs = agent_doc_queue::queue_continuation::collect_backlog_execution_contexts(
        &components,
        &content,
    );
    if exec_ctxs.values().any(|ctx| ctx.is_deferred()) {
        let (drainable, skipped) =
            agent_doc_queue::queue_continuation::partition_drainable_backlog_ids(
                &backlog_ids,
                &exec_ctxs,
            );
        for skip in skipped {
            crate::ops_log::log_op(
                file,
                &format!(
                    "closeout_queue_skip_same_cycle_pending_add id=#{} skip={}",
                    skip.id, skip.reason
                ),
            );
        }
        backlog_ids = drainable;
    }
    if backlog_ids.is_empty() {
        return Ok(Vec::new());
    }

    let pre_sync_ids = entries
        .iter()
        .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
        .collect::<std::collections::HashSet<String>>();
    let Some(synced) = agent_doc_queue::document_queue::sync_backlog_into_queue(
        &entries,
        &backlog_ids,
        agent_doc_queue::document_queue::BacklogQueueSyncMode::Append,
    ) else {
        return Ok(Vec::new());
    };
    let mut seen = std::collections::HashSet::new();
    let synced_ids: Vec<String> = synced
        .iter()
        .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
        .filter(|id| !pre_sync_ids.contains(id))
        .filter(|id| seen.insert(id.clone()))
        .collect();
    if synced_ids.is_empty() {
        return Ok(Vec::new());
    }

    let new_body = agent_doc_queue::document_queue::render(&synced);
    let current_content = {
        let comps = agent_doc_element::element::parse(&content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
        q.replace_content(&content, &new_body)
    };
    persist_queue_maintenance_doc(
        file,
        &current_content,
        project_root.as_deref(),
        "pending_add_sync",
    )?;
    adopt_edited_queue_head_into_snapshot(file, &current_content);
    eprintln!(
        "[write] queue: appended {} same-cycle pending-add id(s) into active go queue",
        synced_ids.len()
    );
    Ok(synced_ids)
}

struct FreeTextAdmission {
    content: String,
    queued_ids: Vec<String>,
    warnings: Vec<PreflightWarning>,
    admitted_count: usize,
    execution_label: &'static str,
}

fn admit_free_text_work(
    file: &Path,
    content: &str,
    entries: &[agent_doc_queue::document_queue::QueueEntry],
    project_root: Option<&Path>,
    queue_scope: &FreeTextAdmissionScope,
    queue_start_required: bool,
) -> Result<Option<FreeTextAdmission>> {
    let exchange_prompt =
        agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content(content);
    let prompts =
        collect_actionable_free_text_prompts(exchange_prompt.as_deref(), entries, queue_scope);
    if !prompts.has_work() {
        return Ok(None);
    }

    let mut current = if agent_doc_element::element::parse(content)?
        .iter()
        .any(|c| c.name == "backlog")
    {
        content.to_string()
    } else {
        append_empty_agent_component(content, "backlog")
    };
    let mut components = agent_doc_element::element::parse(&current)?;
    let backlog = components
        .iter()
        .find(|c| c.name == "backlog")
        .context("free-text admission: backlog component missing after ensure")?
        .clone();
    let backlog_body = backlog.content(&current);
    let (_, existing_items, _) = agent_doc_element_backlog::backlog::parse_items(backlog_body);
    let mut id_by_text = std::collections::HashMap::new();
    for item in existing_items {
        if item.state != agent_doc_element_backlog::backlog::PendingState::Done {
            let key = agent_doc_queue::queue_response::normalize_for_answer_match(&item.text);
            if !key.is_empty() {
                id_by_text.entry(key).or_insert(item.id);
            }
        }
    }

    let mut prompt_keys = Vec::new();
    let mut texts_to_add = Vec::new();
    for prompt in &prompts.prompts {
        let key = agent_doc_queue::queue_response::normalize_for_answer_match(&prompt.text);
        if !id_by_text.contains_key(&key) {
            texts_to_add.push(prompt.text.clone());
        }
        prompt_keys.push(key);
    }
    if !texts_to_add.is_empty() {
        let doc_id = agent_doc_hash::document_id_for_path(file);
        let outcome = agent_doc_element_backlog::backlog::op_prepend_many_with_outcomes(
            backlog_body,
            &texts_to_add,
            &doc_id,
            false,
        )?;
        current = backlog.replace_content(&current, &outcome.body);
        for (text, item_outcome) in texts_to_add.iter().zip(outcome.outcomes) {
            let key = agent_doc_queue::queue_response::normalize_for_answer_match(text);
            id_by_text.insert(key, item_outcome.id.clone());
        }
    }

    let mut unique_ids = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for key in prompt_keys {
        let Some(id) = id_by_text.get(&key) else {
            continue;
        };
        let normalized = id.trim().to_ascii_lowercase();
        if !normalized.is_empty() && seen_ids.insert(normalized.clone()) {
            unique_ids.push(normalized);
        }
    }
    if unique_ids.is_empty() {
        return Ok(None);
    }

    components = agent_doc_element::element::parse(&current)?;
    let queue = components
        .iter()
        .find(|c| c.name == "queue")
        .context("free-text admission: queue component missing")?
        .clone();
    let mut queue_entries: Vec<agent_doc_queue::document_queue::QueueEntry> = entries
        .iter()
        .filter(|entry| !queue_entry_is_admitted_free_text(entry, queue_scope))
        .cloned()
        .collect();

    let (execution, warnings) =
        resolve_free_text_execution(file, &current, project_root, &unique_ids)?;
    let queued_ids = match execution {
        ResolvedFreeTextExecution::Goal => {
            let command = goal_command_for_ids(&unique_ids);
            if !goal_command_already_queued(&queue_entries, &unique_ids) {
                queue_entries.insert(
                    0,
                    agent_doc_queue::document_queue::QueueEntry::Prompt(
                        agent_doc_queue::document_queue::QueuePrompt {
                            text: command,
                            multiline: false,
                        },
                    ),
                );
                if queue_start_required {
                    queue_entries.insert(
                        0,
                        agent_doc_queue::document_queue::QueueEntry::StartFence(None),
                    );
                }
            }
            Vec::new()
        }
        ResolvedFreeTextExecution::Queue => {
            let before_ids: std::collections::HashSet<String> = queue_entries
                .iter()
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .collect();
            let synced = agent_doc_queue::document_queue::sync_backlog_into_queue(
                &queue_entries,
                &unique_ids,
                agent_doc_queue::document_queue::BacklogQueueSyncMode::Prepend,
            )
            .unwrap_or_else(|| queue_entries.clone());
            queue_entries = synced;
            if queue_start_required
                && !matches!(
                    queue_entries.first(),
                    Some(agent_doc_queue::document_queue::QueueEntry::StartFence(_))
                )
            {
                queue_entries.insert(
                    0,
                    agent_doc_queue::document_queue::QueueEntry::StartFence(None),
                );
            }
            queue_entries
                .iter()
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .filter(|id| !before_ids.contains(id))
                .collect()
        }
    };

    let new_queue_body = agent_doc_queue::document_queue::render(&queue_entries);
    current = queue.replace_content(&current, &new_queue_body);
    if execution == ResolvedFreeTextExecution::Queue {
        current = ensure_queue_priority_attr(&current)?;
    }
    Ok(Some(FreeTextAdmission {
        content: current,
        queued_ids,
        warnings,
        admitted_count: prompts.prompts.len(),
        execution_label: execution.label(),
    }))
}

fn resolve_free_text_execution(
    file: &Path,
    content: &str,
    project_root: Option<&Path>,
    ids: &[String],
) -> Result<(ResolvedFreeTextExecution, Vec<PreflightWarning>)> {
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap_or_default();
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    let global_config = agent_doc_config::load().unwrap_or_default();
    let requested = fm
        .free_text_execution
        .or(project_config.agent_doc_free_text_execution)
        .or(global_config.agent_doc_free_text_execution)
        .unwrap_or_default();
    let harness = agent_doc_harness::HarnessConfig::from_context(&fm, &global_config);
    let goal_available = harness.supports_goal_command(
        agent_doc_harness::opencode_goal_extension_available(file, project_root),
    );
    let (execution, warning) = agent_doc_workflow::preflight_policy::resolve_free_text_execution(
        requested,
        goal_available,
        &file.display().to_string(),
        &harness.binary,
        ids,
    );
    let warnings = warning
        .into_iter()
        .map(|warning| PreflightWarning {
            code: warning.code,
            message: warning.message,
            document_agent: fm.agent.clone(),
            active_harness: Some(harness.binary.clone()),
        })
        .collect();
    Ok((execution, warnings))
}

/// `#fccqueue`: persist a queue-maintenance document mutation without provoking
/// an IntelliJ `File Cache Conflict`.
///
/// When a live JB editor listener owns the document, the queue shape is converged
/// through the editor IPC (`converge_live_buffer_queue_shape` → plugin Document
/// API `setText` + `saveDocument`, no external-modification dialog) and the raw
/// disk write is **skipped**. The prior unconditional `std::fs::write(file, …)`
/// at these queue-maintenance sites bypassed the 08b write-authority routing
/// (`write::atomic_write` → ordered write queue / editor convergence) that the
/// finalize/response path already uses, so every preflight queue-maintenance
/// cycle touched disk behind the open editor buffer and fired the conflict
/// dialog. The pending/review maintenance sites already route through the
/// `#fcc0` converge-or-disk gate; this brings the queue path to the same
/// discipline. With no live listener it writes to disk exactly as before, so
/// non-IDE behavior is byte-identical.
///
/// The caller still owns the private snapshot write (a `.agent-doc/` file, never
/// open in the IDE, so it cannot conflict). Like the convergence it wraps this is
/// best-effort: an active-listener send failure leaves the correct content in the
/// snapshot and the next preflight re-converges — it never falls back to a disk
/// write behind the editor.
pub(crate) fn persist_queue_maintenance_doc(
    file: &Path,
    content: &str,
    project_root: Option<&Path>,
    source: &str,
) -> Result<()> {
    let listener_active = project_root
        .map(crate::ipc_socket::is_listener_active)
        .unwrap_or(false);
    if listener_active {
        converge_live_buffer_queue_shape(file, content, project_root);
        crate::ops_log::log_op(
            file,
            &format!(
                "write_authority action=routed reason=plugin_listener_active \
                 surface=queue_maintenance source={source} len={}",
                content.len()
            ),
        );
    } else {
        std::fs::write(file, content)
            .with_context(|| format!("{source}: failed to write {}", file.display()))?;
    }
    Ok(())
}

/// Converge a live route-owned editor buffer to the queue shape just written to
/// `file` by queue maintenance.
///
/// Queue maintenance writes the corrected queue body, opening-tag `auto`
/// attribute, and `queue:` frontmatter to disk + snapshot. When a live
/// IPC listener owns the document it keeps its own working buffer; without this
/// push it overwrites the disk write on its next flush — re-adding stale queue
/// body lines, `auto`, and `queue_active: true` — and the snapshot/HEAD drift
/// regenerates on every preflight (`#adoc-queue-ipc-buffer-divergence`). A
/// content-only IPC patch cannot converge an opening-tag attribute or
/// frontmatter, so we send a dedicated convergence message carrying the queue
/// body, desired `auto` state, and canonical queue frontmatter. Best-effort: a
/// missing listener or send failure is logged, never fatal — the disk/snapshot
/// write remains the source of truth.
pub(crate) fn converge_live_buffer_queue_shape(
    file: &Path,
    content: &str,
    project_root: Option<&Path>,
) {
    let Some(root) = project_root else {
        return;
    };
    if !crate::ipc_socket::is_listener_active(root) {
        return;
    }
    let (want_auto, queue_body) = match agent_doc_element::element::parse(content) {
        Ok(comps) => comps
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| {
                (
                    agent_doc_queue::document_queue::has_auto_attr(&q.attrs),
                    Some(q.content(content).to_string()),
                )
            })
            .unwrap_or((false, None)),
        Err(e) => {
            eprintln!("[preflight] queue: live convergence skipped — component parse failed: {e}");
            return;
        }
    };
    let queue_active = frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.queue_active)
        .unwrap_or(false);
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    // #queue-active-deprecated-line-stuck: converge with the CANONICAL `queue:`
    // control, never the deprecated `queue_active:` line. Emitting the legacy form
    // here re-introduced `queue_active: true` into the live route-owned buffer on
    // every preflight (the buffer then flushed it back to disk, undoing the
    // repair-step migration that drops it). The canonical key is the sole queue
    // control; readers still fold it onto `queue_active` in memory.
    let fm_yaml = format!("queue: {}", if queue_active { "start" } else { "stop" });
    match crate::ipc_socket::send_queue_convergence(
        root,
        &canonical.to_string_lossy(),
        want_auto,
        Some(&fm_yaml),
        queue_body.as_deref(),
    ) {
        Ok(_) => eprintln!(
            "[preflight] queue: converged live editor buffer (auto={want_auto}, queue_active={queue_active})"
        ),
        Err(e) => {
            eprintln!("[preflight] queue: live buffer convergence send failed (non-fatal): {e}")
        }
    }
}

/// Absorb an operator's edited queue head into the snapshot when the loop adopts
/// it instead of halting (#queue-no-stall-on-head-edit). Copying the live file's
/// queue region into the snapshot makes the adopted head prove the same prompt at
/// closeout queue-consume and keeps the next cycle from re-detecting a spurious
/// `item_modified` edit. Best-effort: a parse/load/save failure is logged, never
/// fatal — the loop still continues with the edited head from the live file.
pub(crate) fn adopt_edited_queue_head_into_snapshot(file: &Path, current_content: &str) {
    let snap_now = match snapshot::load(file) {
        Ok(Some(s)) => s,
        Ok(None) => return,
        Err(e) => {
            eprintln!("[preflight] queue: adopt-head snapshot load warning (non-fatal): {e}");
            return;
        }
    };
    let Ok(cur_comps) = agent_doc_element::element::parse(current_content) else {
        return;
    };
    let Some(cur_q) = cur_comps.iter().find(|c| c.name == "queue") else {
        return;
    };
    let Ok(snap_comps) = agent_doc_element::element::parse(&snap_now) else {
        return;
    };
    let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue") else {
        return;
    };
    let queue_region = &current_content[cur_q.open_start..cur_q.close_end];
    let mut rebuilt = String::with_capacity(snap_now.len() + queue_region.len());
    rebuilt.push_str(&snap_now[..snap_q.open_start]);
    rebuilt.push_str(queue_region);
    rebuilt.push_str(&snap_now[snap_q.close_end..]);
    if rebuilt != snap_now
        && let Err(e) = snapshot::save(file, &rebuilt)
    {
        eprintln!("[preflight] queue: adopt-head snapshot sync warning (non-fatal): {e}");
    }
}

fn selected_queue_head_unchanged_in_snapshot(
    file: &Path,
    current_entries: &[agent_doc_queue::document_queue::QueueEntry],
) -> bool {
    let current_prompts = agent_doc_queue::document_queue::prompts(current_entries);
    let Some(current_head) = current_prompts.first() else {
        return false;
    };
    let Ok(Some(snapshot_content)) = snapshot::load(file) else {
        return false;
    };
    let Ok(snapshot_components) = agent_doc_element::element::parse(&snapshot_content) else {
        return false;
    };
    let Some(snapshot_queue) = snapshot_components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    let snapshot_body = &snapshot_content[snapshot_queue.open_end..snapshot_queue.close_start];
    let Ok(snapshot_entries) = agent_doc_queue::document_queue::parse(snapshot_body) else {
        return false;
    };
    let snapshot_prompts = agent_doc_queue::document_queue::prompts(&snapshot_entries);
    let Some(snapshot_head) = snapshot_prompts.first() else {
        return false;
    };
    strip_priority_markers(&snapshot_head.text) == strip_priority_markers(&current_head.text)
}

fn queue_region_differs_from_snapshot(file: &Path, current_content: &str) -> bool {
    let Ok(Some(snapshot_content)) = snapshot::load(file) else {
        return false;
    };
    let Ok(current_components) = agent_doc_element::element::parse(current_content) else {
        return false;
    };
    let Some(current_queue) = current_components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    let Ok(snapshot_components) = agent_doc_element::element::parse(&snapshot_content) else {
        return false;
    };
    let Some(snapshot_queue) = snapshot_components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    current_content[current_queue.open_start..current_queue.close_end]
        != snapshot_content[snapshot_queue.open_start..snapshot_queue.close_end]
}

/// True when the current inactive-queue entry set differs from the queue body
/// recorded in the snapshot (the committed baseline for this cycle). Used to
/// scope the `inactive_queue_residue` warning to genuine operator edits instead
/// of re-warning every preflight on a stable, already-committed inactive queue
/// (the steady state an `item_modified` halt leaves behind — #adoc-queue-ipc-drift).
///
/// Comparison is normalized through `queue::parse` + `queue::render` so trivial
/// whitespace / boundary churn does not register as a change. A missing or
/// unreadable snapshot, or a snapshot with no queue component, is treated as
/// "changed" so a freshly-populated inactive queue still warns.
pub(crate) fn inactive_queue_changed_vs_snapshot(
    file: &Path,
    current_entries: &[agent_doc_queue::document_queue::QueueEntry],
) -> bool {
    let Ok(Some(snapshot_content)) = snapshot::load(file) else {
        return true;
    };
    let Ok(components) = agent_doc_element::element::parse(&snapshot_content) else {
        return true;
    };
    let Some(snap_queue) = components.iter().find(|c| c.name == "queue") else {
        return true;
    };
    let snap_body = &snapshot_content[snap_queue.open_end..snap_queue.close_start];
    let Ok(snap_entries) = agent_doc_queue::document_queue::parse(snap_body) else {
        return true;
    };
    agent_doc_queue::document_queue::render(&snap_entries)
        != agent_doc_queue::document_queue::render(current_entries)
}

pub(crate) fn queue_entries_are_drained_residue(
    entries: &[agent_doc_queue::document_queue::QueueEntry],
) -> bool {
    !entries.is_empty()
        && entries.iter().all(|entry| {
            matches!(
                entry,
                agent_doc_queue::document_queue::QueueEntry::Completed(_)
                    | agent_doc_queue::document_queue::QueueEntry::Preset(_)
                    | agent_doc_queue::document_queue::QueueEntry::Dispatch(_)
            )
        })
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_document::queue_projection::IN_PROGRESS_MARKER;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;

    fn wait_for_typing_indicator(file: &std::path::Path) {
        let file_str = file.to_string_lossy();
        let debounce_ms = crate::preflight::preflight_debounce_ms(file);
        for _ in 0..50 {
            if agent_doc_debounce::is_typing_via_file(&file_str, debounce_ms) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("typing indicator was not written for {}", file.display());
    }

    fn component_body(content: &str, name: &str) -> String {
        let comps = agent_doc_element::element::parse(content).unwrap();
        let comp = comps.iter().find(|c| c.name == name).unwrap();
        comp.content(content).to_string()
    }

    fn backlog_id_for_text(content: &str, text: &str) -> String {
        let body = component_body(content, "backlog");
        let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(&body);
        items
            .into_iter()
            .find(|item| strip_in_progress_marker(&item.text) == text)
            .map(|item| item.id)
            .unwrap()
    }

    #[test]
    fn inspect_queue_state_simulates_activation_without_persisting() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        let snapshot_before = snapshot::load(&doc).unwrap();

        let state = inspect_queue_state(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_prompts, vec!["do [#alpha]".to_string()]);
        assert_eq!(state.queue_drainable_head_count, 1);
        assert!(state.queue_continuation_required);
        assert!(state.queue_supervisor_drainable);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
        assert_eq!(snapshot::load(&doc).unwrap(), snapshot_before);
    }

    #[test]
    fn run_queue_maintenance_syncs_backlog_into_empty_queue() {
        // #backlog-queue-sync-attr: a backlog carrying `queue=sync` regenerates
        // the (empty) queue with `do [#id]` for active items; gated/done excluded.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#alpha] first\n",
            "- [/] [#gated] blocked\n",
            "- [ ] [#beta] second\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            state.synced_queue_ids,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert!(
            updated.contains("- 🚧 do [#alpha]"),
            "synced queue:\n{updated}"
        );
        assert!(updated.contains("- do [#beta]"));
        assert!(
            !updated.contains("- do [#gated]"),
            "gated item must not be queued:\n{updated}"
        );
        assert!(
            state
                .warnings
                .iter()
                .any(|w| w.code == "backlog_queue_sync_pending"),
            "empty-queue-before-sync must emit backlog_queue_sync_pending warning, got {:?}",
            state.warnings
        );
    }

    #[test]
    fn run_queue_maintenance_admits_exchange_free_text_to_native_goal() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Build the importer\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Build the importer");
        let queue = component_body(&updated, "queue");

        assert!(
            queue.contains(&format!("/goal Implement backlog item(s): #{id}")),
            "native goal-capable harness must queue a /goal command:\n{updated}"
        );
        assert_eq!(
            state.selected_queue_prompts,
            vec![format!("/goal Implement backlog item(s): #{id}")]
        );
    }

    #[test]
    fn run_queue_maintenance_admits_new_active_queue_free_text_to_native_goal() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#existing]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#existing] existing work\n",
            "<!-- /agent:backlog -->\n",
        );
        let content = snapshot_content.replace(
            "- do [#existing]\n",
            "- do [#existing]\n- Implement active queue addition\n",
        );
        std::fs::write(&doc, &content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement active queue addition");
        let queue = component_body(&updated, "queue");

        assert!(
            queue.contains(&format!("/goal Implement backlog item(s): #{id}")),
            "new active queue free text should become a native goal:\n{updated}"
        );
        assert!(
            !queue.contains("Implement active queue addition"),
            "admitted free-text source line should be removed from the active queue:\n{updated}"
        );
        assert!(
            !queue.contains("--- start"),
            "an already-active queue must not receive a redundant start fence:\n{updated}"
        );
        assert_eq!(
            state.selected_queue_prompts.first(),
            Some(&format!("/goal Implement backlog item(s): #{id}"))
        );
    }

    #[test]
    fn run_queue_maintenance_ignores_response_tail_for_free_text_admission() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:head-boundary -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#ov1]\n",
            "- [route] target tmux session: 0\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ov1] [operator-verify] live drive needs a human editor\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue = component_body(&updated, "queue");
        let backlog = component_body(&updated, "backlog");

        assert!(!queue.contains("/goal"), "{updated}");
        assert!(
            !backlog.contains("Done."),
            "assistant closeout text must not become backlog work:\n{updated}"
        );
        assert_eq!(state.queue_drainable_head_count, 0);
        assert!(!state.queue_continuation_required);
    }

    #[test]
    fn run_queue_maintenance_keeps_do_directive_exchange_tail_unadmitted() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:head -->\n",
            "do #autocmp. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(updated, content);
        assert!(state.queue_prompts.is_empty());
        assert_eq!(state.queue_active, None);
    }

    #[test]
    fn run_queue_maintenance_keeps_non_actionable_free_text_queue_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do something\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue = component_body(&updated, "queue");
        let backlog = component_body(&updated, "backlog");

        assert!(queue.contains("do something"), "{updated}");
        assert!(!queue.contains("/goal"), "{updated}");
        assert!(
            !backlog.contains("do something"),
            "non-actionable existing queue text should not be lifted into backlog:\n{updated}"
        );
        assert_eq!(state.selected_queue_prompts, vec!["do something"]);
    }

    #[test]
    fn run_queue_maintenance_frontmatter_can_force_free_text_queue_execution() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "agent_doc_free_text_execution: queue\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement checkout flow\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement checkout flow");
        let queue = component_body(&updated, "queue");

        assert!(
            queue.contains(&format!("do [#{id}]")),
            "queue mode must materialize a do head:\n{updated}"
        );
        assert!(
            !queue.contains("Implement checkout flow"),
            "free-text queue source should be replaced by the backlog-backed head:\n{updated}"
        );
        assert!(
            !queue.contains("/goal"),
            "frontmatter queue mode must not create a /goal command:\n{updated}"
        );
        assert!(state.synced_queue_ids.contains(&id));
    }

    #[test]
    fn run_queue_maintenance_queue_fallback_uses_auto_dag_priority_order() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: opencode\n",
            "agent_doc_free_text_execution: goal\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement checkout shipping after=#setup\n",
            "- Implement checkout setup\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ship] Implement checkout shipping after=#setup\n",
            "- [ ] [#setup] Implement checkout setup\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue = component_body(&updated, "queue");

        assert!(
            updated.contains("<!-- agent:queue priority -->"),
            "queue fallback must opt into the existing priority/auto-DAG path:\n{updated}"
        );
        assert!(
            queue.contains("- 🚧 :round_pushpin: do [#setup]\n- do [#ship]"),
            "auto-DAG fallback must run prerequisites before dependents:\n{updated}"
        );
        assert!(!queue.contains("/goal"), "{updated}");
        assert_eq!(state.queue_active, Some(true));
        assert!(
            state
                .warnings
                .iter()
                .any(|warning| warning.code == "free_text_goal_unavailable"),
            "OpenCode without /goal support must warn about queue fallback: {:?}",
            state.warnings
        );
    }

    #[test]
    fn run_queue_maintenance_opencode_goal_extension_uses_native_goal() {
        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join(".opencode/commands")).unwrap();
        std::fs::write(
            dir.path().join(".opencode/commands/goal.md"),
            "goal command",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: opencode\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement OpenCode goal extension flow\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement OpenCode goal extension flow");
        let queue = component_body(&updated, "queue");

        assert!(
            queue.contains(&format!("/goal Implement backlog item(s): #{id}")),
            "OpenCode with a goal extension should use native /goal:\n{updated}"
        );
        assert!(!queue.contains("do [#"), "{updated}");
        assert!(
            state
                .warnings
                .iter()
                .all(|warning| warning.code != "free_text_goal_unavailable"),
            "goal-capable OpenCode should not warn about fallback: {:?}",
            state.warnings
        );
    }

    #[test]
    fn run_queue_maintenance_project_config_can_force_free_text_queue_execution() {
        let dir = setup_project();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_free_text_execution = \"queue\"\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement import mapping\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement import mapping");
        let queue = component_body(&updated, "queue");

        assert!(queue.contains(&format!("do [#{id}]")), "{updated}");
        assert!(!queue.contains("/goal"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_opencode_goal_config_falls_back_to_queue_without_extension() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: opencode\n",
            "agent_doc_free_text_execution: goal\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement export retry\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement export retry");
        let queue = component_body(&updated, "queue");

        assert!(queue.contains(&format!("do [#{id}]")), "{updated}");
        assert!(!queue.contains("/goal"), "{updated}");
        assert!(
            state
                .warnings
                .iter()
                .any(|warning| warning.code == "free_text_goal_unavailable"),
            "OpenCode without a /goal extension must surface the fallback warning: {:?}",
            state.warnings
        );
    }
    #[test]
    fn run_queue_maintenance_defers_while_foreign_queue_edit_lease_held() {
        // #sqedit-race Phase 2: a backlog `queue=sync` would normally regenerate
        // the empty queue. While a DIFFERENT live process holds a fresh queue-edit
        // lease (a direct `queue prune-noise` / `queue consume` in flight),
        // preflight maintenance must defer entirely — no mutation, no sync — so it
        // never round-trips a torn intermediate queue.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        // pid 1 (init) is always live on Unix and is never this test process →
        // a genuine foreign in-flight queue edit.
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_queue::queue_edit_owner::refresh_queue_edit_owner_lease(&doc_str, 1).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            after, content,
            "queue must be untouched while a foreign edit is in flight"
        );
        assert!(
            state.synced_queue_ids.is_empty(),
            "no backlog→queue sync may run while deferred, got {:?}",
            state.synced_queue_ids
        );
        assert!(
            !after.contains("- do [#alpha]"),
            "deferred maintenance must not mint the queue head:\n{after}"
        );

        // Once the lease clears, the next pass syncs normally (the defer is a
        // yield, not a permanent skip).
        agent_doc_queue::queue_edit_owner::clear_queue_edit_owner_lease(&doc_str);
        let resumed = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(resumed.synced_queue_ids, vec!["alpha".to_string()]);
        assert!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .contains("do [#alpha]")
        );
    }

    #[test]
    fn run_queue_maintenance_adopts_live_buffer_queue_duplicate_delete() {
        // #qeditdelete: the operator deletes one duplicate queue row in the live
        // editor while disk still has both copies. Queue maintenance must start
        // from that editor-authored deletion before it converges queue shape back
        // to the editor, or the stale disk copy reappears.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- :pushpin: do [#dup]\n",
            "- :pushpin: do [#dup]\n",
            "- :pushpin: do [#keep]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let live_content = content.replacen(
            "- :pushpin: do [#dup]\n- :pushpin: do [#dup]\n",
            "- :pushpin: do [#dup]\n",
            1,
        );
        agent_doc_debounce::record_live_buffer_digest_content(
            &doc.to_string_lossy(),
            &live_content,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated.matches("do [#dup]").count(),
            1,
            "the operator-deleted duplicate must not be re-pushed from stale disk:\n{updated}"
        );
        assert!(
            updated.contains("do [#keep]"),
            "unrelated queue rows must survive:\n{updated}"
        );
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap.matches("do [#dup]").count(),
            1,
            "snapshot must adopt the deleted duplicate too:\n{snap}"
        );
    }

    #[test]
    fn run_queue_maintenance_restrikes_snapshot_struck_live_reemit() {
        // #qeditdupguard: a stale editor/live-buffer flush can replay an
        // unstruck copy of a queue row that the committed snapshot already
        // struck. Queue maintenance must converge that row back to `Completed`
        // instead of making the retired work runnable again.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- ~~:pushpin: do [#qeditdup] [#qftloss#qftloss]~~\n",
            "- do [#stillopen]\n",
            "<!-- /agent:queue -->\n",
        );
        let live_stale_reemit = snapshot_content.replace(
            "- ~~:pushpin: do [#qeditdup] [#qftloss#qftloss]~~",
            "- :pushpin: do [#qeditdup] [#qftloss#qftloss]",
        );
        std::fs::write(&doc, live_stale_reemit).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let entries = read_queue_entries(&doc);
        let active: Vec<&str> = entries
            .iter()
            .filter_map(|entry| match entry {
                agent_doc_queue::document_queue::QueueEntry::Prompt(prompt) => {
                    Some(prompt.text.as_str())
                }
                _ => None,
            })
            .collect();
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|entry| match entry {
                agent_doc_queue::document_queue::QueueEntry::Completed(prompt) => {
                    Some(prompt.text.as_str())
                }
                _ => None,
            })
            .collect();

        assert!(
            !active.iter().any(|text| text.contains("#qeditdup")),
            "stale live qeditdup head must not stay runnable: active={active:?}"
        );
        assert!(
            completed.iter().any(|text| text.contains("#qeditdup")),
            "snapshot-struck qeditdup head must remain struck: completed={completed:?}"
        );
        assert!(
            active.iter().any(|text| text.contains("#stillopen")),
            "unrelated live head must stay runnable: active={active:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_mirrors_operator_verify_into_queue_but_keeps_it_nondrainable() {
        // #mirrorall (operator directive 2026-06-18): operator-verify backlog items
        // are mirrored INTO the queue (complete worklist) instead of being skipped,
        // but they stay non-drainable — `drainable_head_count` counts only the
        // actionable head, so the in-session auto-drain loop is not re-armed by the
        // mirrored operator-verify head.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#act] actionable work\n",
            "- [ ] [#opv] [operator-verify] needs a human, do not auto-drain\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#act]"),
            "actionable item mirrored into queue:\n{updated}"
        );
        assert!(
            updated.contains("- do [#opv]"),
            "operator-verify item must ALSO be mirrored into the queue (#mirrorall):\n{updated}"
        );
        // ...but the operator-verify head must NOT count as drainable: the loop is
        // safe because only the actionable head is countable.
        let drainable = agent_doc_queue::queue_continuation::drainable_head_count(&updated);
        assert_eq!(
            drainable, 1,
            "operator-verify head must be deferred from drainability (#mirrorall keeps the loop \
             safe); only #act should count:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_operator_verify_only_queue_is_not_supervisor_drainable() {
        // `#rt83`: a queue whose only active heads are `[operator-verify]` has no
        // drainer (neither the in-session `/loop` nor the supervisor) — so
        // `queue_supervisor_drainable` must be false. The preflight synthetic
        // queue-head diff gates on this flag, so an operator-verify-only head no
        // longer synthesizes a phantom `+:pushpin: do [#id]` add every preflight
        // (the qchurn flood that kept `no_changes:false` forever).
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#opv]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#opv] [operator-verify] needs a human, do not auto-drain\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            !state.queue_supervisor_drainable,
            "operator-verify-only queue must NOT be supervisor-drainable (#rt83): {state:?}"
        );
        assert_eq!(
            state.queue_drainable_head_count, 0,
            "operator-verify head is not in-session drainable either (#rt83): {state:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_focused_cycle_head_is_supervisor_drainable() {
        // `#rt83`: a `[focused-cycle]` head stays supervisor-drainable (the
        // supervisor force-`/clear`s + re-dispatches it), so the synthetic
        // queue-head diff must still fire — suppressing it only for non-drainable
        // heads must NOT strand legitimate supervisor-driven continuation.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#foc]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#foc] [focused-cycle] fix the merge core\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            state.queue_supervisor_drainable,
            "focused-cycle head must stay supervisor-drainable (#rt83): {state:?}"
        );
        assert_eq!(
            state.queue_drainable_head_count, 0,
            "focused-cycle head is deferred for the in-session loop (#qcontdrain): {state:?}"
        );
    }
    #[test]
    fn run_queue_maintenance_enqueue_marker_populates_queue_without_backlog_attr() {
        // #queue-enqueue-action: a single marked backlog item appends to the
        // queue without a component-level `queue` attr. Explicit markers bypass
        // the active-loop fresh-item hold because the user is directly enqueueing
        // that one id.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#running]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] :inbox_tray: queue this now\n",
            "- [ ] [#beta] leave this unqueued\n",
            "- [/] [#gated] :inbox_tray: blocked\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(state.synced_queue_ids, vec!["alpha".to_string()]);
        assert!(
            updated.contains("- 🚧 do [#running]"),
            "running head stays:\n{updated}"
        );
        assert!(
            updated.contains("- do [#alpha]"),
            "marked item should append:\n{updated}"
        );
        assert!(
            !updated.contains("- do [#beta]"),
            "unmarked item must not append:\n{updated}"
        );
        assert!(
            !updated.contains("- do [#gated]"),
            "gated marked item must not append:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_holds_fresh_backlog_item_out_of_active_queue() {
        // #backlog-queue-sync-pending-add-amplification (decision B/C): a backlog
        // item added while the auto-queue is already running (queue_active: true)
        // must NOT be promoted into the live queue this cycle — it waits for the
        // next activation. Prevents unbounded queue growth + pending_done_guard
        // churn when an agent captures follow-ups mid-loop.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] already running\n",
            "- [ ] [#beta] freshly added mid-loop\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#alpha]"),
            "the already-running head stays:\n{updated}"
        );
        assert!(
            !updated.contains("- do [#beta]"),
            "a freshly-added backlog item must NOT be promoted into the active queue mid-loop:\n{updated}"
        );
        assert!(
            !state.synced_queue_ids.contains(&"beta".to_string()),
            "beta must not be a newly-synced queue id while the loop is active: {:?}",
            state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_go_mode_repopulates_drained_active_queue() {
        // #backlog-queue-empty-active-repopulate: with the `go` control
        // (`queue: go`, continuous-backlog-loop) and a fully drained live queue
        // (0 un-struck prompts), the amplification hold is skipped and the full
        // active backlog repopulates the queue so the loop keeps working it.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] first\n",
            "- [ ] [#beta] second\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#alpha]"),
            "go-mode must repopulate a drained active queue:\n{updated}"
        );
        assert!(
            updated.contains("- do [#beta]"),
            "go-mode must repopulate ALL open backlog ids:\n{updated}"
        );
        assert!(
            state.synced_queue_ids.contains(&"alpha".to_string())
                && state.synced_queue_ids.contains(&"beta".to_string()),
            "both ids must be newly synced under go-mode repopulation: {:?}",
            state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_controller_pause_surfaces_flag_without_stalling_continuation() {
        // `#qpausego`: an accepted controller `admin queue pause` surfaces
        // `queue_paused` (for visibility + the idle-watch auto-injection guard)
        // but must NOT drop `queue_continuation_required` / drainable head count:
        // the attended in-session `/loop` keeps draining real queue work. Stalling
        // the in-session loop on a pause strands genuine backlog (operator-rejected
        // over-reach). `resume` clears the flag; continuation is unaffected by both.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        // Baseline: an active go-mode queue with a live head requires continuation.
        let state = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            !state.queue_paused,
            "no controller pause means queue_paused is false"
        );
        assert!(
            state.queue_pause_reason.is_none(),
            "no controller pause means no pause reason is surfaced"
        );
        assert!(
            state.queue_continuation_required && state.queue_drainable_head_count > 0,
            "active go queue with a live head must require continuation before pause"
        );

        // Accepted controller pause must be surfaced without halting continuation.
        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        let scope_id = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_sqlite::state_store::upsert_queue_control_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &scope_id,
                state: "paused",
                reason: Some("operator pause"),
                operation_receipt_id: None,
            },
        )
        .unwrap();

        let paused = run_queue_maintenance(&doc, None).unwrap();
        assert!(paused.queue_paused, "accepted pause must set queue_paused");
        assert_eq!(
            paused.queue_pause_reason.as_deref(),
            Some("operator pause"),
            "accepted pause must surface its recorded reason"
        );
        assert!(
            paused.queue_continuation_required,
            "controller pause must NOT stall the in-session loop continuation"
        );
        assert!(
            paused.queue_drainable_head_count > 0,
            "controller pause must NOT zero the drainable head count for the in-session loop"
        );

        // Resume clears the flag; continuation is unaffected by either state.
        agent_doc_sqlite::state_store::upsert_queue_control_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &scope_id,
                state: "resumed",
                reason: Some("operator resume"),
                operation_receipt_id: None,
            },
        )
        .unwrap();

        let resumed = run_queue_maintenance(&doc, None).unwrap();
        assert!(!resumed.queue_paused, "resume must clear queue_paused");
        assert!(
            resumed.queue_pause_reason.is_none(),
            "resume must clear the surfaced pause reason"
        );
        assert!(
            resumed.queue_continuation_required && resumed.queue_drainable_head_count > 0,
            "resume keeps continuation for the active go-mode queue"
        );
    }

    #[test]
    fn run_queue_maintenance_go_mode_appends_fresh_backlog_into_nondrained_queue() {
        // #backlog-queue-attr-populates-in-go-mode: with the `go` control and a
        // NON-drained live queue, a freshly-added backlog `queue`-attr item still
        // appends to the queue immediately (the operator opted into the
        // continuous-backlog-loop, so the `queue` attribute must populate it).
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] already running\n",
            "- [ ] [#beta] freshly added mid-loop\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let updated_state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#alpha]"),
            "the running head stays:\n{updated}"
        );
        assert!(
            updated.contains("- do [#beta]"),
            "go-mode must append a fresh backlog `queue`-attr item even when the queue is not drained:\n{updated}"
        );
        assert!(
            updated_state.synced_queue_ids.contains(&"beta".to_string()),
            "beta must be a newly-synced queue id under go-mode: {:?}",
            updated_state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_queue_start_without_go_holds_fresh_backlog() {
        // `queue: start` is the durable active-state spelling for a normal queue
        // run, not a continuous-backlog-loop opt-in. Only explicit `go` should
        // append freshly-added backlog `queue` items into an already-running queue.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority start preset=\"#spec-test-build-install-commit-push\" -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] already running\n",
            "- [ ] [#beta] freshly added mid-loop\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let updated_state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            !updated.contains("do [#beta]"),
            "queue:start without marker/frontmatter go must hold fresh backlog ids:\n{updated}"
        );
        assert!(
            updated_state.synced_queue_ids.is_empty(),
            "non-go queue must not report newly synced ids: {:?}",
            updated_state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_routes_through_ipc_without_disk_write_when_listener_active() {
        // #fccqueue: with a live JB editor listener owning the document, queue
        // maintenance must NOT raw-write the session doc to disk (the every-cycle
        // source of the IntelliJ `File Cache Conflict`). It routes the queue shape
        // through the editor IPC convergence instead and records the routed
        // write-authority decision in ops.log.
        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] already running\n",
            "- [ ] [#beta] freshly added mid-loop\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        // Fake editor listener that acks patches but never writes the file, so any
        // change to the on-disk doc could only have come from the binary itself.
        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());

        let state = run_queue_maintenance(&doc, None).unwrap();

        // The mutation still happened logically (beta synced into the queue) ...
        assert!(
            state.synced_queue_ids.contains(&"beta".to_string()),
            "beta must still be synced under go-mode with a listener active: {:?}",
            state.synced_queue_ids
        );
        // ... but the binary must NOT have raw-written the doc to disk behind the
        // editor: the fake listener never writes the file, so disk stays as-is.
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            content,
            "queue maintenance must not raw-write the session doc while a JB listener is active"
        );
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            log.contains("write_authority action=routed")
                && log.contains("surface=queue_maintenance"),
            "active-listener queue maintenance must record the routed write-authority decision:\n{log}"
        );
    }
    #[test]
    fn run_queue_maintenance_holds_future_not_before_backlog_item_out_of_queue() {
        // #backlog-not-before: a backlog item with a future `not-before=` date
        // precondition is NOT synced into the queue (operator: "if items have
        // preconditions that are not met such as a date in the future, do not add
        // the backlog item into the queue"). A ready item still syncs.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#ready] do now\n",
            "- [ ] [#later] not-before=2999-12-31 scheduled for the future\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#ready]"),
            "the ready item must sync into the queue:\n{updated}"
        );
        assert!(
            !updated.contains("do [#later]"),
            "a future not-before item must be held out of the queue:\n{updated}"
        );
        assert!(
            !state.synced_queue_ids.contains(&"later".to_string()),
            "future-dated id must not be a synced queue id: {:?}",
            state.synced_queue_ids
        );
    }
    #[test]
    fn closeout_sync_appends_same_cycle_pending_add_in_go_mode() {
        // #pendingaddqueuesync: pending-add writes happen after preflight queue
        // maintenance, so closeout appends recorded same-cycle ids once the
        // current queue head has been consumed.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#head] already running\n",
            "- [ ] [#fresh] same-cycle follow-up\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::record_pending_added_ids(&doc, &["fresh".to_string()]).unwrap();

        let synced = sync_same_cycle_pending_adds_into_go_queue(&doc).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(synced, vec!["fresh".to_string()]);
        let head = updated.find("do [#head]").unwrap();
        let fresh = updated.find("do [#fresh]").unwrap();
        assert!(
            head < fresh,
            "same-cycle add must append behind the current queue head:\n{updated}"
        );
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("- do [#fresh]"),
            "snapshot queue region must include the appended closeout head:\n{snap}"
        );
    }
    #[test]
    fn closeout_sync_holds_same_cycle_pending_add_without_go_mode() {
        // The old amplification guard still applies to a plain persisted-active
        // queue: same-cycle captures wait for a later activation unless the
        // operator opted into explicit `go` continuous backlog drain. `queue:
        // start` alone is just the durable active-state spelling.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:queue priority -->\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#head] already running\n",
            "- [ ] [#fresh] same-cycle follow-up\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::record_pending_added_ids(&doc, &["fresh".to_string()]).unwrap();

        let synced = sync_same_cycle_pending_adds_into_go_queue(&doc).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(synced.is_empty());
        assert!(
            !updated.contains("do [#fresh]"),
            "non-go active queue must not append same-cycle pending add:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_no_go_keeps_drain_then_stop_on_empty_active_queue() {
        // #backlog-queue-empty-active-repopulate: WITHOUT the `go` control, a
        // drained persisted-active queue stays drained (drain-then-stop). The
        // amplification hold drops every backlog id because none are already
        // live queue heads, so nothing repopulates.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] first\n",
            "- [ ] [#beta] second\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            !updated.contains("- do [#alpha]") && !updated.contains("- do [#beta]"),
            "without `go`, a drained active queue must stay drained:\n{updated}"
        );
        assert!(
            state.synced_queue_ids.is_empty(),
            "no ids may be synced into a drained active queue without `go`: {:?}",
            state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_no_warning_when_queue_already_synced() {
        // When the queue already matches the backlog, no backlog_queue_sync_pending
        // warning should fire (sync_backlog_into_queue returns None → no warning path).
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert!(
            !state
                .warnings
                .iter()
                .any(|w| w.code == "backlog_queue_sync_pending"),
            "already-synced queue must NOT emit backlog_queue_sync_pending warning, got {:?}",
            state.warnings
        );
    }
    #[test]
    fn run_queue_maintenance_marker_go_activates_like_auto() {
        // #queue-state-unify: a `go`/`start` marker control freshly activates the
        // queue through the Auto trigger, identical to the legacy `auto` attribute.
        for token in ["go", "start"] {
            let dir = setup_project();
            let doc = dir.path().join("session.md");
            let content = format!(
                concat!(
                    "---\nagent_doc_session: test\nagent_doc_format: template\n",
                    "agent_doc_write: crdt\n---\n\n",
                    "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
                    "<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue {} -->\n- please do the thing\n<!-- /agent:queue -->\n",
                ),
                token
            );
            std::fs::write(&doc, &content).unwrap();
            snapshot::save(&doc, &content).unwrap();

            let state = run_queue_maintenance(&doc, None).unwrap();
            assert_eq!(
                state.queue_active,
                Some(true),
                "marker `{token}` must activate the queue"
            );
            assert_eq!(
                state.queue_trigger,
                Some(agent_doc_queue::document_queue::QueueTrigger::Auto)
            );
            let updated = std::fs::read_to_string(&doc).unwrap();
            let expected_queue = if token == "go" {
                "queue: go"
            } else {
                "queue: start"
            };
            assert!(
                updated.contains(expected_queue),
                "marker `{token}` must persist queue_active:\n{updated}"
            );
        }
    }
    #[test]
    fn run_queue_maintenance_marker_stop_halts_active_queue() {
        // #queue-state-unify: a `stop` marker control forces an otherwise-active
        // queue inactive and clears persisted queue_active.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue stop -->\n- please do the thing\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state.queue_active,
            Some(false),
            "marker `stop` must halt the active queue"
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("queue: stop"),
            "marker `stop` must clear queue_active:\n{updated}"
        );
        assert!(
            !updated.contains("agent:queue stop"),
            "marker `stop` token must be stripped after halt:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_removed_marker_go_stops_frontmatter_queue() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: go\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        let current_content =
            snapshot_content.replace("<!-- agent:queue go -->", "<!-- agent:queue -->");
        std::fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(false));
        assert!(!state.queue_continuation_required);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: stop"), "{updated}");
        assert!(updated.contains("<!-- agent:queue -->"), "{updated}");
        assert!(!updated.contains("agent:queue go"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_frontmatter_go_adds_marker_go() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: stop\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        let current_content = snapshot_content.replace("queue: stop", "queue: go");
        std::fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"), "{updated}");
        assert!(updated.contains("<!-- agent:queue go -->"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_frontmatter_stop_removes_marker_go() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: go\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        let current_content = snapshot_content.replace("queue: go", "queue: stop");
        std::fs::write(&doc, current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(false));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: stop"), "{updated}");
        assert!(updated.contains("<!-- agent:queue -->"), "{updated}");
        assert!(!updated.contains("agent:queue go"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_stop_fence_records_typed_deferred_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n--- stop\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_halted.as_deref(), Some("stop_fence"));
        assert_eq!(state.queue_active, Some(false));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("--- stop"),
            "stop fence should be consumed from halted queue:\n{updated}"
        );
        let node_key = agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue")
            .unwrap()
            .into_iter()
            .find(|node| !node.item.struck)
            .expect("deferred queue head should retain a node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let ledger = crate::project_controller::load_state_event_ledger(dir.path()).unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("deferred queue state should project for document");
        assert_eq!(projection.queue.active_head, None);
        let head = projection
            .queue
            .heads
            .get(&node_key)
            .expect("deferred queue head should be present in projection");
        assert_eq!(head.phase, crate::state_backbone::QueueHeadPhase::Deferred);
        assert_eq!(head.backlog_id.as_deref(), Some("alpha"));
        assert_eq!(head.prompt_text.as_deref(), Some("do [#alpha]"));
        assert_eq!(head.defer_reason.as_deref(), Some("stop_fence"));
        assert!(!head.drainable);

        record_selected_queue_head_state(&doc, &updated, "do [#alpha]", true).unwrap();
        let ledger = crate::project_controller::load_state_event_ledger(dir.path()).unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("reselected queue state should project for document");
        assert_eq!(
            projection.queue.active_head.as_deref(),
            Some(node_key.as_str())
        );
        let head = projection
            .queue
            .heads
            .get(&node_key)
            .expect("reselected queue head should be present in projection");
        assert_eq!(head.phase, crate::state_backbone::QueueHeadPhase::Selected);
        assert_eq!(head.defer_reason, None);
        assert!(head.drainable);
    }

    #[test]
    fn run_queue_maintenance_excludes_done_ids_from_backlog_sync() {
        // #ynra: a lingering active backlog `[ ]` bullet whose id is also archived
        // in `agent:done` must NOT be re-minted into the queue (it would be struck
        // every cycle and re-injected the next → forever churn). The fresh active
        // id is still minted.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#na3x] completed-but-lingering\n",
            "- [ ] [#fresh] genuinely open\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "- 2026-06-01 [#na3x] completed-but-lingering\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("[#na3x]") || !updated.contains("do [#na3x]"),
            "completed id must not be minted into the queue:\n{updated}"
        );
        assert!(
            !updated.contains("do [#na3x]"),
            "completed id must not appear as a queue do-prompt:\n{updated}"
        );
        assert!(
            updated.contains("do [#fresh]"),
            "fresh active id must still be queued:\n{updated}"
        );
        assert_eq!(state.synced_queue_ids, vec!["fresh".to_string()]);
    }
    #[test]
    fn run_queue_maintenance_excludes_external_archive_done_ids() {
        // #ynra (external-archive variant): a completed id reaped to the EXTERNAL
        // `agent:done archive=<file>` (not inline) must also be excluded from the
        // backlog→queue sync and struck from the queue. Done-id collection reads
        // the archive file, so the queue must not churn on an externally-archived
        // completed ref.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let archive_rel = "session.done.md";
        std::fs::write(
            dir.path().join(archive_rel),
            "# Done\n\n- 2026-06-01 [#extdone] archived externally\n",
        )
        .unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#extdone]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#extdone] lingering active dup of an externally-archived id\n",
            "- [ ] [#fresh] genuinely open\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=session.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("- do [#extdone]"),
            "externally-archived completed ref must be struck/excluded, not left live:\n{updated}"
        );
        assert!(
            updated.contains("do [#fresh]"),
            "fresh active id must still be queued:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_strikes_external_archive_done_queue_prompt() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(
            dir.path().join("session.done.md"),
            "# Done\n\n- 2026-06-01 [#extdone] archived externally\n",
        )
        .unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#extdone]\n",
            "- do [#fresh]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:done archive=session.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- ~~do [#extdone]~~"),
            "externally-archived live queue mirror must be struck:\n{updated}"
        );
        assert!(
            updated.contains("- do [#fresh]"),
            "fresh live queue prompt must remain:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_backlog_sync_is_idempotent() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            state.synced_queue_ids.is_empty(),
            "idempotent sync should not report freshly-added ids"
        );
        assert_eq!(
            updated.matches("- do [#alpha]").count(),
            1,
            "append must not duplicate an already-queued id:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_records_only_newly_synced_ids() {
        // The existing queue head must stay outside the synced-id exclusion set
        // so pending_done_guard still requires the consumed `do [#worked]` item
        // to be done/gated.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#worked]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=prepend -->\n",
            "- [ ] [#worked] the real queue head\n",
            "- [ ] [#alpha] freshly synced\n",
            "- [ ] [#beta] freshly synced\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.synced_queue_ids,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        let open_backlog: std::collections::HashSet<String> = ["worked", "alpha", "beta"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let synced_queue_ids = state
            .synced_queue_ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<String>>();
        let result = agent_doc_queue::queue_directive::filter_expect_done_or_gate_ids(
            &[
                "worked".to_string(),
                "alpha".to_string(),
                "beta".to_string(),
            ],
            &open_backlog,
            &synced_queue_ids,
        );
        assert_eq!(result, vec!["worked".to_string()]);
    }
    #[test]
    fn run_queue_maintenance_backlog_queue_priority_sorts_and_marks_promoted_item() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync priority -->\n",
            "- [ ] [#slow] slower follow-up priority=9\n",
            "- [ ] [#fast] fast follow-up priority=1\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 :round_pushpin: do [#fast]\n- do [#slow]"),
            "backlog `queue priority` must sort synced queue prompts and mark the promoted item:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_marks_current_queue_and_work_items_in_progress() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#alpha]\n",
            "- 🚧 do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] active work\n",
            "- [ ] 🚧 [#beta] stale work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] 🚧 [#cold] stale parked work\n",
            "<!-- /agent:icebox -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_prompts[0], "do [#alpha]");
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 do [#alpha]\n- do [#beta]"),
            "queue marker must move to active head:\n{updated}"
        );
        assert!(updated.contains("- [ ] 🚧 [#alpha] active work"));
        assert!(updated.contains("- [ ] [#beta] stale work"));
        assert!(updated.contains("- [ ] [#cold] stale parked work"));

        let node_key = agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue")
            .unwrap()
            .into_iter()
            .find(|node| !node.item.struck)
            .expect("active queue head should have a node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let ledger = crate::project_controller::load_state_event_ledger(dir.path()).unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("selected queue state should project for document");
        assert_eq!(
            projection.queue.active_head.as_deref(),
            Some(node_key.as_str())
        );
        let head = projection
            .queue
            .heads
            .get(&node_key)
            .expect("selected queue head should be present in projection");
        assert_eq!(head.phase, crate::state_backbone::QueueHeadPhase::Selected);
        assert_eq!(head.backlog_id.as_deref(), Some("alpha"));
        assert_eq!(head.prompt_text.as_deref(), Some("do [#alpha]"));
        assert!(head.drainable);
        assert!(projection.queue.worklist_active);
        assert_eq!(projection.queue.worklist.len(), 2);
        assert_eq!(
            projection.queue.worklist[0].kind,
            crate::state_backbone::QueueWorklistEntryKind::Prompt
        );
        assert_eq!(projection.queue.worklist[0].text, "do [#alpha]");
        assert_eq!(
            projection.queue.worklist[1].kind,
            crate::state_backbone::QueueWorklistEntryKind::Prompt
        );
        assert_eq!(projection.queue.worklist[1].text, "do [#beta]");
        assert!(projection.queue.worklist_queue_hash.is_some());
    }

    #[test]
    fn run_queue_maintenance_marks_first_in_session_drainable_head_in_progress() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#verify]\n",
            "- do [#focused]\n",
            "- do [#ready]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#verify] [operator-verify] needs a human\n",
            "- [ ] [#focused] [focused-cycle] needs a clean turn\n",
            "- [ ] [#ready] active work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_drainable_head_count, 1);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- do [#verify]\n- do [#focused]\n- 🚧 do [#ready]"),
            "in-progress marker must project the first in-session drainable head:\n{updated}"
        );
        assert!(updated.contains("- [ ] [#verify] [operator-verify] needs a human"));
        assert!(updated.contains("- [ ] [#focused] [focused-cycle] needs a clean turn"));
        assert!(updated.contains("- [ ] 🚧 [#ready] active work"));

        let ready_node_key = agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue")
            .unwrap()
            .into_iter()
            .find(|node| node.item.text.contains("#ready"))
            .expect("ready queue head should have a node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let ledger = crate::project_controller::load_state_event_ledger(dir.path()).unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("selected queue state should project for document");
        assert_eq!(
            projection.queue.active_head.as_deref(),
            Some(ready_node_key.as_str())
        );
        let head = projection
            .queue
            .heads
            .get(&ready_node_key)
            .expect("ready queue head should be present in projection");
        assert_eq!(head.backlog_id.as_deref(), Some("ready"));
        assert_eq!(head.prompt_text.as_deref(), Some("do [#ready]"));
        assert!(head.drainable);
    }

    #[test]
    fn run_queue_maintenance_honors_marker_retarget_with_auto_dag_prerequisites() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#ops]\n",
            "- 🚧 do [#ship]\n",
            "- do [#setup]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority -->\n",
            "- [ ] [#ops] priority=9 independent work\n",
            "- [ ] [#ship] priority=2 after=#setup selected dependent work\n",
            "- [ ] [#setup] priority=1 prerequisite work\n",
            "<!-- /agent:backlog -->\n",
        );
        let snapshot_content = content.replace(
            "- do [#ops]\n- 🚧 do [#ship]",
            "- 🚧 do [#ops]\n- do [#ship]",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, &snapshot_content).unwrap();

        let diff = concat!(
            "@@ queue @@\n",
            "-- 🚧 do [#ops]\n",
            "+- do [#ops]\n",
            "-- do [#ship]\n",
            "+- 🚧 do [#ship]\n",
        );
        let state = run_queue_maintenance(&doc, Some(diff)).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(
            state.selected_queue_prompts,
            vec![
                ":round_pushpin: do [#setup]".to_string(),
                "do [#ship]".to_string()
            ]
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 :round_pushpin: do [#setup]\n- 🚧 do [#ship]\n- do [#ops]"),
            "operator retarget must project the selected head and its auto-DAG prerequisite as active:\n{updated}"
        );
        assert!(updated.contains("- [ ] 🚧 [#setup] priority=1 prerequisite work"));
        assert!(
            updated.contains("- [ ] 🚧 [#ship] priority=2 after=#setup selected dependent work")
        );
        assert!(updated.contains("- [ ] [#ops] priority=9 independent work"));

        let node_keys = agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue").unwrap();
        let setup_node_key = node_keys
            .iter()
            .find(|node| node.item.text.contains("#setup"))
            .expect("setup queue head should have a node key")
            .node_key
            .clone();
        let ship_node_key = node_keys
            .iter()
            .find(|node| node.item.text.contains("#ship"))
            .expect("ship queue head should have a node key")
            .node_key
            .clone();
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let ledger = crate::project_controller::load_state_event_ledger(dir.path()).unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("selected queue state should project for document");
        assert!(projection.queue.active_heads.contains(&setup_node_key));
        assert!(projection.queue.active_heads.contains(&ship_node_key));
        assert_eq!(
            projection.queue.active_head.as_deref(),
            Some(ship_node_key.as_str())
        );
    }

    #[test]
    fn run_queue_maintenance_removes_in_progress_from_completed_items() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#alpha]\n",
            "- ~~🚧 do [#done]~~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] active work\n",
            "- [x] 🚧 [#done] finished work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [x] 🚧 [#reviewdone] finished review\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [x] 🚧 [#cold] finished parked work\n",
            "<!-- /agent:icebox -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_prompts[0], "do [#alpha]");
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 do [#alpha]\n- ~~do [#done]~~"),
            "queue marker must move to the active head and clear struck items:\n{updated}"
        );
        assert!(
            updated.contains("- [ ] 🚧 [#alpha] active work"),
            "active backlog items must be marked:\n{updated}"
        );
        assert!(
            updated.contains("- [x] [#done] finished work"),
            "done backlog items must not keep in-progress markers:\n{updated}"
        );
        assert!(
            updated.contains("- [x] [#reviewdone] finished review"),
            "done review items must not keep in-progress markers:\n{updated}"
        );
        assert!(
            updated.contains("- [x] [#cold] finished parked work"),
            "done icebox items must not keep in-progress markers:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_pins_operator_moved_priority_queue_item() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue priority auto -->\n",
            "- do [#fast]\n",
            "- do [#medium]\n",
            "- do [#slow]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority -->\n",
            "- [ ] [#fast] priority=1 first by rank\n",
            "- [ ] [#medium] priority=5 middle by rank\n",
            "- [ ] [#slow] priority=9 operator moved this up\n",
            "<!-- /agent:backlog -->\n",
        );
        let current_content = snapshot_content.replace(
            "- do [#fast]\n- do [#medium]\n- do [#slow]",
            "- do [#slow]\n- do [#fast]\n- do [#medium]",
        );
        std::fs::write(&doc, &current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 :pushpin: do [#slow]\n- do [#fast]\n- do [#medium]"),
            "operator-moved queue prompt should become sticky with :pushpin::\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_auto_dag_intersperses_blocker_with_pinned_batch() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue priority auto -->\n",
            "- :pushpin: do [#ops]\n",
            "- :pushpin: do [#ship]\n",
            "- :pushpin: do [#notify]\n",
            "- :round_pushpin: do [#setup]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority -->\n",
            "- [ ] [#ops] priority=5 independent operator-pinned task\n",
            "- [ ] [#ship] priority=1 after=#setup depends on setup\n",
            "- [ ] [#notify] priority=2 after=#ship depends on ship\n",
            "- [ ] [#setup] priority=9 required setup work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains(
                "- 🚧 :pushpin: do [#ops]\n\
                 - :round_pushpin: do [#setup]\n\
                 - :pushpin: do [#ship]\n\
                 - :pushpin: do [#notify]"
            ),
            "auto-dag must let dependency blockers intersperse a pinned batch:\n{updated}"
        );
    }
    #[test]
    fn preflight_new_auto_queue_from_inactive_snapshot_does_not_halt_on_changed_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#oldhead]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#newhead] Run the newly queued head.\n",
            "- [ ] [#nexthead] Run the next queued item.\n",
            "<!-- /agent:backlog -->\n"
        );
        let current_content = snapshot_content
            .replace("<!-- agent:queue -->", "<!-- agent:queue auto -->")
            .replace("- do [#oldhead]", "- do [#newhead]\n- do [#nexthead]");
        std::fs::write(&doc, &current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_halted, None);
        assert_eq!(
            state.queue_prompts,
            vec!["do [#newhead]".to_string(), "do [#nexthead]".to_string()]
        );

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: start"));
        assert!(updated.contains("<!-- agent:queue auto start -->"));
        assert!(updated.contains("- 🚧 do [#newhead]"));
        assert!(!updated.contains("- do [#oldhead]"));

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("queue: start")
                && snap.contains("<!-- agent:queue auto start -->")
                && snap.contains("do [#newhead]")
                && !snap.contains("- do [#oldhead]"),
            "newly activated queue must be snapshotted as the closeout baseline:\n{snap}"
        );

        let done_ids = vec!["newhead".to_string()];
        let outcome = crate::write::consume_queue_prompts_for_done_ids_force_disk_with_outcome(
            &doc, &done_ids,
        )
        .unwrap()
        .expect("newly activated queue head should be consumable");
        assert_eq!(outcome.consumed_count, 1);
        assert_eq!(outcome.remaining, 1);

        let consumed = std::fs::read_to_string(&doc).unwrap();
        assert!(consumed.contains("- ~~do [#newhead]~~"));
        assert!(consumed.contains("- do [#nexthead]"));
    }
    #[test]
    fn queue_maintenance_drains_all_done_queue_without_item_modified_halt() {
        // #drained-done-queue-clear: a fully resolved auto-queue (every `do
        // [#id]` already in agent:done) plus a batch dispatch directive must
        // drain — not false-halt as `item_modified`. Before the fix the
        // strike pass converted every live head to Completed, leaving the
        // post-strike head `None` vs a still-live snapshot head, which
        // detect_head_prompt_modified read as an edit and halted before the
        // drain-cleanup path ran. The Corky live-repro shape: template doc,
        // dispatch preset, multiple bracketed `do [#id]` prompts, no diff.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#alpha]\n",
            "- do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#alpha] First done.\n",
            "- [x] [#beta] Second done.\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_halted, None,
            "fully-resolved queue must drain, not halt as item_modified"
        );
        assert_eq!(state.queue_active, Some(false));
        assert!(state.queue_prompts.is_empty());

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: stop"), "file: {updated}");
        assert!(
            !updated.contains("agent:queue auto"),
            "auto must be stripped on drain: {updated}"
        );
        assert!(
            !updated.contains("- do [#alpha]") && !updated.contains("- do [#beta]"),
            "drained queue body must be cleared: {updated}"
        );

        // Snapshot matches the drained file so the closeout commit boundary
        // does not strand the maintenance mutation.
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(snap.contains("queue: stop"));
        assert!(!snap.contains("agent:queue auto"));
        assert!(!snap.contains("- do [#alpha]"));
    }
    #[test]
    fn queue_maintenance_partial_done_strike_advances_to_live_head_without_halt() {
        // #drained-done-queue-clear (partial case): a leading queue head that
        // is already done must be struck and the queue advanced to the next
        // live head — without false-halting as item_modified. The snapshot is
        // struck the same way before the head-modified comparison so only a
        // genuine operator head edit can halt.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "- do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#alpha] First done.\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_halted, None,
            "striking a done head must not halt while a live head remains"
        );
        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_prompts, vec!["do [#beta]".to_string()]);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- ~~do [#alpha]~~"),
            "done head struck to completed: {updated}"
        );
        assert!(updated.contains("- 🚧 do [#beta]"));
        assert!(updated.contains("agent:queue auto"));
        assert!(updated.contains("queue: start"));
    }
    #[test]
    fn queue_maintenance_converges_live_ipc_buffer_on_item_modified_halt() {
        // SimWorld repro for #adoc-queue-ipc-buffer-divergence (root cause #2):
        // a live route-owned IPC listener owns the document. When an
        // already-active auto-queue's head prompt changes between cycles, queue
        // maintenance halts (item_modified), strips `auto`, and clears
        // `queue_active` on disk + snapshot. Without convergence the live editor
        // buffer would re-add `auto`/`queue_active: true` on its next flush and
        // the snapshot/HEAD drift loop regenerates every preflight. This test
        // proves maintenance pushes a queue-tag + frontmatter convergence message
        // to the listener, and that a follow-up maintenance pass is idempotent
        // (no second divergence, no second convergence send).
        use std::sync::{Arc, Mutex};

        let dir = setup_project();
        let root = dir.path().canonicalize().unwrap();
        let doc = root.join("session.md");

        let received: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();
        let listener_root = root.clone();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg) {
                    // #fccqueue: under the editor-safe contract the binary no longer
                    // raw-writes the converged queue shape to disk while a listener is
                    // active — it routes the convergence through us. Mirror the real JB
                    // plugin (`setText` + `saveDocument`) so disk catches up to the
                    // converged shape synchronously (before this ack), which is what
                    // makes the next preflight idempotent. A content-only patch cannot
                    // change the opening-tag `auto` token or frontmatter, so we apply
                    // the queue body + `queue_auto` strip + `frontmatter` here just like
                    // the plugin's convergence handler does.
                    if v.get("queue_auto").is_some()
                        && let Some(file_str) = v.get("file").and_then(|x| x.as_str())
                        && let Ok(mut doc) = std::fs::read_to_string(file_str)
                    {
                        if let Some(body) = v
                            .get("patches")
                            .and_then(|p| p.get(0))
                            .and_then(|p| p.get("content"))
                            .and_then(|c| c.as_str())
                            && let Ok(comps) = agent_doc_element::element::parse(&doc)
                            && let Some(q) = comps.iter().find(|c| c.name == "queue")
                        {
                            doc = q.replace_content(&doc, body);
                        }
                        if v["queue_auto"] == serde_json::json!(false)
                            && let Ok(comps) = agent_doc_element::element::parse(&doc)
                            && let Some(q) = comps.iter().find(|c| c.name == "queue")
                        {
                            let raw = doc[q.open_start..q.open_end].to_string();
                            let new_tag =
                                agent_doc_queue::document_queue::strip_auto_from_tag(&raw);
                            if new_tag != raw {
                                let mut rebuilt = String::with_capacity(doc.len());
                                rebuilt.push_str(&doc[..q.open_start]);
                                rebuilt.push_str(&new_tag);
                                rebuilt.push_str(&doc[q.open_end..]);
                                doc = rebuilt;
                            }
                        }
                        if v.get("frontmatter") == Some(&serde_json::json!("queue: stop"))
                            && let Ok(merged) = frontmatter::merge_queue_state(&doc, false)
                        {
                            doc = merged;
                        }
                        let _ = std::fs::write(file_str, &doc);
                    }
                    received_clone.lock().unwrap().push(v);
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        std::thread::sleep(std::time::Duration::from_millis(150));

        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#oldhead]\n",
            "- do [#nexthead]\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
        std::fs::write(&doc, &current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        // A live editor is actively mid-edit on the head prompt, so the loop
        // must still pause/halt rather than adopt a half-typed head
        // (#queue-no-stall-on-head-edit gates adopt on a settled buffer).
        agent_doc_debounce::document_changed(&doc.to_string_lossy());
        wait_for_typing_indicator(&doc);

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.queue_halted, Some("item_modified".into()));
        assert_eq!(state.queue_active, Some(false));

        // Disk converged to the inactive shape.
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(updated.contains("queue: stop"));

        // Listener received exactly one queue convergence message carrying the
        // queue body plus the tag + frontmatter shape that a content-only patch
        // cannot deliver.
        std::thread::sleep(std::time::Duration::from_millis(100));
        {
            let msgs = received.lock().unwrap();
            let convergences: Vec<&serde_json::Value> = msgs
                .iter()
                .filter(|m| m.get("queue_auto").is_some())
                .collect();
            assert_eq!(
                convergences.len(),
                1,
                "expected exactly one queue convergence message, got: {msgs:?}"
            );
            let conv = convergences[0];
            assert_eq!(conv["queue_auto"], serde_json::json!(false));
            // #queue-active-deprecated-line-stuck: convergence carries the
            // canonical `queue:` control, never the deprecated `queue_active:`.
            assert_eq!(conv["frontmatter"], serde_json::json!("queue: stop"));
            assert_eq!(conv["patches"][0]["component"], serde_json::json!("queue"));
            assert_eq!(
                conv["patches"][0]["content"],
                serde_json::json!(
                    agent_doc_element::element::parse(&updated)
                        .unwrap()
                        .iter()
                        .find(|c| c.name == "queue")
                        .unwrap()
                        .content(&updated)
                )
            );
        }

        // Idempotency: a follow-up maintenance pass on the converged document
        // mutates nothing and sends no further convergence.
        let state2 = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state2.queue_halted, None);
        std::thread::sleep(std::time::Duration::from_millis(100));
        {
            let msgs = received.lock().unwrap();
            let convergences = msgs
                .iter()
                .filter(|m| m.get("queue_auto").is_some())
                .count();
            assert_eq!(
                convergences, 1,
                "follow-up maintenance must not re-diverge / re-send convergence"
            );
        }

        let _ = std::fs::remove_file(crate::ipc_socket::socket_path(&root));
        drop(server);
    }
    #[test]
    fn preflight_pauses_when_active_queue_head_changes_mid_edit() {
        // #queue-no-stall-on-head-edit (pause case): while a live editor is
        // actively mid-edit on the head prompt, the loop must still pause/halt
        // rather than grab a half-typed head. The settled-buffer adopt path is
        // covered separately by
        // `preflight_adopts_edited_queue_head_when_buffer_settled`.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#oldhead]\n",
            "- do [#nexthead]\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
        std::fs::write(&doc, &current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        // Mark the document as actively being typed so the head edit reads as
        // a half-typed buffer.
        agent_doc_debounce::document_changed(&doc.to_string_lossy());
        wait_for_typing_indicator(&doc);

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(false));
        assert_eq!(state.queue_halted.as_deref(), Some("item_modified"));
        assert!(state.queue_prompts.is_empty());

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: stop"));
        assert!(updated.contains("<!-- agent:queue -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(updated.contains("- do [#newhead]"));

        let missing = crate::queue_journal::replay_missing(&doc, snapshot_content);
        assert!(
            missing.iter().any(|entry| entry.text == "do [#newhead]"),
            "early-return queue maintenance must journal the live edited head before convergence: {:?}",
            missing
        );
    }
    #[test]
    fn preflight_adopts_edited_queue_head_when_buffer_settled() {
        // #queue-no-stall-on-head-edit (adopt case): when an already-active
        // auto-queue's head prompt changes between cycles and the buffer is
        // settled (no live typing indicator), the loop must adopt the edited
        // head as the new prompt and stay armed — NOT strip `auto` / force
        // queue_active:false. The snapshot must absorb the edited head so
        // closeout queue-consume proves the same prompt and the next cycle sees
        // no spurious item_modified edit.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#oldhead]\n",
            "- do [#nexthead]\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
        std::fs::write(&doc, &current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();
        // No typing indicator written → buffer is settled.

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_halted, None,
            "settled head edit must adopt + continue, not halt"
        );
        assert_eq!(state.queue_active, Some(true));
        assert_eq!(
            state.queue_prompts,
            vec!["do [#newhead]".to_string(), "do [#nexthead]".to_string()],
            "loop continues with the edited head as the new prompt"
        );

        // File keeps the armed auto-queue with the edited head.
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("agent:queue auto"),
            "auto preserved: {updated}"
        );
        assert!(
            updated.contains("queue: start"),
            "active preserved: {updated}"
        );
        assert!(updated.contains("- 🚧 do [#newhead]"));

        // Snapshot absorbed the edited head so a follow-up pass is idempotent
        // (no spurious item_modified on the now-converged head).
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("do [#newhead]"),
            "snapshot must absorb the adopted head: {snap}"
        );
        assert!(
            !snap.contains("- do [#oldhead]"),
            "snapshot must drop the stale head: {snap}"
        );
        let state2 = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state2.queue_halted, None,
            "converged head must not re-halt on the next pass"
        );
        assert_eq!(state2.queue_active, Some(true));
    }
    #[test]
    fn preflight_preserves_intentional_duplicate_tracked_queue_prompt() {
        // #queue-dedup-destroys-intentional-duplicates / #md-ast-document-model:
        // duplicate `do [#id]` text can be intentional user queue intent. Preflight
        // must not collapse it by raw prompt/id matching; only duplicate AST node
        // keys are eligible for cleanup.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test-build-install-commit-push\n",
            "- ~do [#adoc-sqlite-seam]~\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated.matches("do [#adoc-orch-shim-cleanup]").count(),
            2,
            "duplicate tracked prompts must remain executable queue intent:\n{updated}"
        );
        assert_eq!(
            state.queue_prompts,
            vec![
                "do [#adoc-orch-shim-cleanup]".to_string(),
                "do [#adoc-orch-shim-cleanup]".to_string()
            ],
            "duplicate tracked prompts should remain queued: {state:?}"
        );
        // Re-running maintenance on the converged doc is a no-op (stable).
        let before = std::fs::read_to_string(&doc).unwrap();
        let _ = run_queue_maintenance(&doc, None).unwrap();
        let after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            before, after,
            "queue maintenance must be idempotent after dedup"
        );
    }
    #[test]
    fn preflight_keeps_intentional_duplicate_free_text_prompt() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do deploy\n",
            "- do deploy\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state.queue_prompts,
            vec!["do deploy".to_string(), "do deploy".to_string()],
            "intentional duplicate free-text prompts should remain queued: {state:?}"
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated.matches("do deploy").count(),
            2,
            "maintenance should preserve intentional duplicate free-text prompts:\n{updated}"
        );
    }
    #[test]
    fn preflight_does_not_reflag_stable_inactive_queue_as_residue() {
        // #adoc-queue-ipc-drift root cause #1: after an `item_modified` halt the
        // queue goes inactive (queue_active: false, no `auto`) with a retained
        // live tail, and the halt synced that shape into the snapshot. On the
        // NEXT preflight the inactive queue is unchanged from the snapshot, so
        // re-emitting `inactive_queue_residue` every cycle (with no user edit)
        // is pure loop noise and must be suppressed.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        // Snapshot == file: a stable, already-committed inactive queue with a
        // retained tail (the post-halt steady state).
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- ~do [#first-done]~\n",
            "- do [#second-live]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert!(
            !state
                .warnings
                .iter()
                .any(|w| w.code == "inactive_queue_residue"),
            "stable inactive queue (unchanged vs snapshot) must not re-warn residue: {:?}",
            state.warnings
        );
        // The retained tail is preserved, and maintenance is idempotent.
        let before = std::fs::read_to_string(&doc).unwrap();
        assert!(before.contains("- do [#second-live]"));
        let _ = run_queue_maintenance(&doc, None).unwrap();
        let after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(before, after, "stable inactive queue must not be mutated");
    }
    #[test]
    fn pending_maintenance_clears_stale_supervisor_marker_when_fresh() {
        // `#staleshow`: with no live route-owned supervisor in the test environment,
        // `stale_supervisor_warning_for_doc` reads NOT stale, so a pre-seeded
        // "🔴 (restart/recycle your supervisor)" marker must be removed from the status
        // component (file + snapshot) by the preflight maintenance pass.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "🔴 (restart/recycle your supervisor)\n",
            "Session ready.\n",
            "<!-- /agent:status -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_pending_maintenance_force_disk(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !file_after
                .contains(agent_doc_document::status_projection::STALE_SUPERVISOR_STATUS_MARKER),
            "fresh supervisor must clear the stale marker from the file: {file_after}"
        );
        assert!(
            file_after.contains("Session ready."),
            "other status content must be preserved: {file_after}"
        );

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !snapshot_after
                .contains(agent_doc_document::status_projection::STALE_SUPERVISOR_STATUS_MARKER),
            "fresh supervisor must clear the stale marker from the snapshot: {snapshot_after}"
        );
    }

    #[test]
    fn pending_maintenance_reaps_completed_items_from_file_and_snapshot() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#reap1] Reap me\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let report = run_pending_maintenance_force_disk(&doc).unwrap();
        assert!(!report.reordered);
        assert_eq!(report.backlog_gated_count, 0);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_backlog_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_backlog_after.contains("[#reap1]"));
        assert!(file_after.contains("[#keep1]"));
        assert!(file_after.contains("## Completed / Reaped"));
        assert!(file_after.contains("<!-- agent:done -->"));
        assert!(file_after.contains("[#reap1] Reap me"));

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        let snapshot_backlog_after = agent_doc_element::element::parse(&snapshot_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        assert!(!snapshot_backlog_after.contains("[#reap1]"));
        assert!(snapshot_after.contains("[#keep1]"));
        assert!(snapshot_after.contains("## Completed / Reaped"));
        assert!(snapshot_after.contains("<!-- agent:done -->"));
        assert!(snapshot_after.contains("[#reap1] Reap me"));
    }
    #[test]
    fn pending_maintenance_auto_reaps_ops_proof_done_items() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#doneci] #agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): supervisor idle-queue watch self-heals stale busy state\n",
            "- [ ] [#partial] #agent-doc-bug PARTIAL SHIPPED 9df1244f: committed first slice. REMAINING: live proof gate\n",
            "- [ ] [#reopened] #agent-doc-bug REOPENED false closeout: previous closeout DONE 1234567 (CI 1 green)\n",
            "- [ ] [#noproof] DONE: lacks deterministic proof\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#reviewdone] SHIPPED abcdef1 (CI 2 passed): review-gated shipped marker\n",
            "- [/] [#reviewkeep] Needs release review\n",
            "<!-- /agent:review -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let report = run_pending_maintenance_force_disk(&doc).unwrap();
        assert_eq!(report.backlog_gated_count, 0);
        assert_eq!(report.review_count, 1);
        assert_eq!(report.review_gated_count, 1);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let backlog_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&file_after)
            .to_string();
        let review_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| is_review_component(&c.name))
            .unwrap()
            .content(&file_after)
            .to_string();

        assert!(!backlog_after.contains("[#doneci]"));
        assert!(!review_after.contains("[#reviewdone]"));
        assert!(backlog_after.contains("[#partial]"));
        assert!(backlog_after.contains("[#reopened]"));
        assert!(backlog_after.contains("[#noproof]"));
        assert!(review_after.contains("[#reviewkeep]"));
        assert!(file_after.contains("## Completed / Reaped"));
        assert!(file_after.contains("[#doneci] #agent-doc-bug DONE 7b60fcdc"));
        assert!(file_after.contains("[#reviewdone] SHIPPED abcdef1"));

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        assert!(!snapshot_after.contains("- [ ] [#doneci]"));
        assert!(!snapshot_after.contains("- [/] [#reviewdone]"));
        assert!(snapshot_after.contains("[#partial]"));
        assert!(snapshot_after.contains("[#reopened]"));
        assert!(snapshot_after.contains("[#noproof]"));
        assert!(snapshot_after.contains("[#reviewkeep]"));

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("auto_complete_ops_proof"));
        assert!(log.contains("id=doneci"));
        assert!(log.contains("id=reviewdone"));
    }
    #[test]
    fn ops_proof_does_not_reap_same_cycle_added_gated_item() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#freshgate] operator live-verify the destructive path. Code SHIPPED 1edb20d2; this is the live gate only\n",
            "<!-- /agent:review -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        // The snapshot already contains the item — this models the finalize path
        // where the same invocation's --review-add re-synced the snapshot, so the
        // snapshot-only guard cannot tell this is a brand-new add.
        snapshot::save(&doc, content).unwrap();
        // cycle_state records #freshgate as added this cycle.
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::record_pending_added_ids(&doc, &["freshgate".to_string()]).unwrap();

        run_pending_maintenance_force_disk(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        // The freshly added gated item survives — not reaped on its first cycle.
        assert!(
            file_after.contains("[#freshgate]"),
            "same-cycle-added gated item must not be ops-proof reaped: {file_after}"
        );
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("auto_complete_ops_proof"),
            "no ops-proof auto-completion should fire for a same-cycle add"
        );
    }
    #[test]
    fn ops_proof_does_not_reap_cited_dependency_marker() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#citeddep] wire the predicate into dispatch. The predicate already shipped in 600797b3 and is unit-tested\n",
            "- [ ] [#leadstatus] DONE 7b60fcdc: wired the predicate into dispatch\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_pending_maintenance_force_disk(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let backlog_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&file_after)
            .to_string();

        // Cited-dependency marker stays open; leading-status marker is reaped.
        assert!(
            backlog_after.contains("[#citeddep]"),
            "cited-dependency item must not be reaped: {backlog_after}"
        );
        assert!(!backlog_after.contains("[#leadstatus]"));
        assert!(file_after.contains("[#leadstatus] DONE 7b60fcdc"));
    }
    #[test]
    fn ops_proof_does_not_reap_live_verify_gate_on_commit_hash() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#ktw8] [live-verify gate] destructive auto-/clear between queue turns. ",
            "Code SHIPPED 1edb20d2; a shipped commit is NOT proof, an operator drive is. ",
            "PASS = a genuine anchored ops.log line; current verdict UNDRIVEN.\n",
            "<!-- /agent:review -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_pending_maintenance_force_disk(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            file_after.contains("[#ktw8]"),
            "live-verify gate must not be ops-proof reaped on a cited commit hash: {file_after}"
        );
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("auto_complete_ops_proof"),
            "no ops-proof auto-completion should fire for a live-verify gate"
        );
    }
    #[test]
    fn pending_maintenance_does_not_reap_same_cycle_add() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        // Snapshot baseline: an existing leading-status done item + a keeper.
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#leadstatus] DONE 7b60fcdc: wired the predicate\n",
            "- [ ] [#keep] keep this open item\n",
            "<!-- /agent:backlog -->\n"
        );
        // File adds a brand-new same-cycle item with a leading-status marker that
        // would normally reap — but it is absent from the snapshot.
        let file_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#freshdone] DONE abc1234: just landed this cycle\n",
            "- [ ] [#leadstatus] DONE 7b60fcdc: wired the predicate\n",
            "- [ ] [#keep] keep this open item\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, file_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        run_pending_maintenance_force_disk(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let backlog_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&file_after)
            .to_string();

        // Same-cycle add survives; pre-existing leading-status item is reaped.
        assert!(
            backlog_after.contains("[#freshdone]"),
            "same-cycle add must not be reaped: {backlog_after}"
        );
        assert!(backlog_after.contains("[#keep]"));
        assert!(!backlog_after.contains("[#leadstatus]"));
    }
    #[test]
    fn pending_maintenance_reaps_inline_done_backlog_and_review_mirrors() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#done1] stale backlog mirror\n",
            "- [ ] [#keep1] keep backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#done2] stale review mirror\n",
            "- [/] [#keep2] keep review\n",
            "<!-- /agent:review -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#done1] already archived backlog\n",
            "- [x] [#done2] already archived review\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let report = run_pending_maintenance_force_disk(&doc).unwrap();
        assert_eq!(report.backlog_gated_count, 0);
        assert_eq!(report.review_count, 1);
        assert_eq!(report.review_gated_count, 1);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_components = agent_doc_element::element::parse(&file_after).unwrap();
        let file_backlog = file_components
            .iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&file_after)
            .to_string();
        let file_review = file_components
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_backlog.contains("[#done1]"));
        assert!(file_backlog.contains("[#keep1] keep backlog"));
        assert!(!file_review.contains("[#done2]"));
        assert!(file_review.contains("[#keep2] keep review"));
        assert_eq!(file_after.matches("[#done1]").count(), 1);
        assert_eq!(file_after.matches("[#done2]").count(), 1);

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        let snapshot_components = agent_doc_element::element::parse(&snapshot_after).unwrap();
        let snapshot_backlog = snapshot_components
            .iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        let snapshot_review = snapshot_components
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        assert!(!snapshot_backlog.contains("[#done1]"));
        assert!(!snapshot_review.contains("[#done2]"));
        assert_eq!(snapshot_after.matches("[#done1]").count(), 1);
        assert_eq!(snapshot_after.matches("[#done2]").count(), 1);
    }
    #[test]
    fn pending_maintenance_reaps_external_done_archive_backlog_and_review_mirrors() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let archive_rel = "session.done.md";
        let archive_path = dir.path().join(archive_rel);
        let archive_content = concat!(
            "# Done\n\n",
            "- [x] [#extdone1] externally archived backlog\n",
            "- [x] [#extdone2] externally archived review\n",
        );
        std::fs::write(&archive_path, archive_content).unwrap();
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#extdone1] stale backlog mirror\n",
            "- [ ] [#fresh1] fresh backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#extdone2] stale review mirror\n",
            "- [/] [#fresh2] fresh review\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:done archive=session.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let report = run_pending_maintenance_force_disk(&doc).unwrap();
        assert_eq!(report.review_count, 1);
        assert_eq!(report.review_gated_count, 1);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_components = agent_doc_element::element::parse(&file_after).unwrap();
        let file_backlog = file_components
            .iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&file_after)
            .to_string();
        let file_review = file_components
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_backlog.contains("[#extdone1]"));
        assert!(file_backlog.contains("[#fresh1] fresh backlog"));
        assert!(!file_review.contains("[#extdone2]"));
        assert!(file_review.contains("[#fresh2] fresh review"));
        assert_eq!(
            std::fs::read_to_string(&archive_path).unwrap(),
            archive_content
        );

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        assert!(!snapshot_after.contains("stale backlog mirror"));
        assert!(!snapshot_after.contains("stale review mirror"));
        assert!(snapshot_after.contains("[#fresh1] fresh backlog"));
        assert!(snapshot_after.contains("[#fresh2] fresh review"));
    }
    #[test]
    fn preflight_allows_user_marked_done_item_reaped_in_same_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [/] [#done1] Waiting on manual validation\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, baseline).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "baseline", "--no-verify"])
            .output()
            .unwrap();

        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Waiting on manual validation\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(current)).unwrap();

        let report = run_pending_maintenance_force_disk(&doc).unwrap();
        assert!(!report.reordered);
        assert_eq!(report.backlog_gated_count, 0);
        let rc = crate::graph::RunContext::new(doc.clone());
        enforce_no_dropped_backlog(&doc, &rc)
            .expect("same-cycle reap should count as intentional completion");
    }
    #[test]
    fn pending_maintenance_reaps_completed_icebox_items_from_file_and_snapshot() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Icebox\n\n",
            "<!-- agent:icebox -->\n",
            "- [x] [#ice01] Reap me from icebox\n",
            "- [ ] [#keep2] Keep me parked\n",
            "<!-- /agent:icebox -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let report = run_pending_maintenance_force_disk(&doc).unwrap();
        assert!(!report.reordered);
        assert_eq!(report.backlog_gated_count, 0);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_icebox_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "icebox")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_icebox_after.contains("[#ice01]"));
        assert!(file_after.contains("[#keep2]"));
        assert!(file_after.contains("## Completed / Reaped"));
        assert!(file_after.contains("[#ice01] Reap me from icebox"));

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        let snapshot_icebox_after = agent_doc_element::element::parse(&snapshot_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "icebox")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        assert!(!snapshot_icebox_after.contains("[#ice01]"));
        assert!(snapshot_after.contains("[#keep2]"));
        assert!(snapshot_after.contains("## Completed / Reaped"));
        assert!(snapshot_after.contains("[#ice01] Reap me from icebox"));
    }
    #[test]
    fn pending_maintenance_syncs_snapshot_for_write_phase_gate_without_reap() {
        // #pending-gate-snapshot-desync: the write phase moved #g1 from backlog
        // to review (a --pending-gate) on the FILE, but the content_ours snapshot
        // still shows #g1 in backlog and an empty review. Maintenance makes no
        // reap/backfill change, yet it must re-sync the snapshot's tracked
        // surfaces to the file so the upcoming commit stages the gate instead of
        // stranding it as post-commit drift.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let file_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#g1] Gated, awaiting review\n",
            "<!-- /agent:review -->\n"
        );
        // Snapshot lags the file: #g1 still in backlog, review empty (the
        // baseline+response content_ours saved before the gate mutation).
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "- [ ] [#g1] Gated, awaiting review\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "<!-- /agent:review -->\n"
        );
        std::fs::write(&doc, file_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        run_pending_maintenance_force_disk(&doc).unwrap();

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        let comps = agent_doc_element::element::parse(&snapshot_after).unwrap();
        let snap_backlog = comps
            .iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        let snap_review = comps
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        // Snapshot now matches the file: #g1 gated into review, gone from backlog.
        assert!(
            !snap_backlog.contains("[#g1]"),
            "snapshot backlog must drop the gated item: {snap_backlog}"
        );
        assert!(
            snap_review.contains("[/] [#g1]"),
            "snapshot review must carry the gated item: {snap_review}"
        );
        assert!(snap_backlog.contains("[#keep1]"));
    }
    #[test]
    fn gate_verify_surfaces_provable_without_flipping_when_optin_off() {
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some("early_ack_pending".to_string()),
                disproof: Some("false ack-timeout".to_string()),
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(&dir, "[150] early_ack_pending emitted ok\n");

        let results = run_gate_verify(&doc, false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "saev");
        assert_eq!(results[0].status, "provable");
        assert!(
            !results[0].auto_resolved,
            "opt-in off must not flip the gate"
        );

        // The document still shows the gated item — never silently flipped.
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(after.contains("- [/] [#saev]"), "gate must remain: {after}");
    }
    #[test]
    fn gate_verify_auto_resolves_provable_when_optin_on() {
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some("early_ack_pending".to_string()),
                disproof: None,
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(&dir, "[150] early_ack_pending emitted ok\n");

        let results = run_gate_verify_force_disk(&doc, true).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "provable");
        assert!(results[0].auto_resolved);

        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            after.contains("[x] [#saev]"),
            "gate must be flipped: {after}"
        );
        // Snapshot kept in lockstep for the upcoming commit.
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("[x] [#saev]"),
            "snapshot must flip too: {snap}"
        );
    }
    #[test]
    fn gate_verify_failed_never_auto_resolves_even_with_optin() {
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some("early_ack_pending".to_string()),
                disproof: Some("manual cleanup".to_string()),
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(
            &dir,
            "[150] early_ack_pending emitted\n[160] looks like a manual cleanup\n",
        );

        let results = run_gate_verify_force_disk(&doc, true).unwrap();
        assert_eq!(results[0].status, "failed", "disproof wins");
        assert!(!results[0].auto_resolved);
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            after.contains("- [/] [#saev]"),
            "failed gate must remain: {after}"
        );
    }
    #[test]
    fn gate_verify_empty_without_predicate() {
        let dir = setup_project();
        let doc = write_optverify_doc(&dir, "");
        write_ops_log(&dir, "[150] early_ack_pending emitted\n");
        let results = run_gate_verify_force_disk(&doc, true).unwrap();
        assert!(results.is_empty(), "no predicate → no results");
    }
    #[test]
    fn gate_verify_ignores_marker_quoted_in_content_logging_lines() {
        // #gng8: queue_diff_active_prompt_differs embeds document prose via
        // {:?}; a gate must not auto-prove from its own backlog description.
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some("early_ack_pending".to_string()),
                disproof: None,
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(
            &dir,
            "[150] queue_diff_active_prompt_differs file=doc.md prompt_changes=[\"expect early_ack_pending emitted before apply\"] queue_head=\"[#saev]\"\n",
        );

        let results = run_gate_verify(&doc, true).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "pending", "quoted prose must not prove");
        assert!(!results[0].auto_resolved);
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(after.contains("- [/] [#saev]"), "gate must remain: {after}");
    }
    #[test]
    fn gate_verify_s760_builtin_ignores_queue_diff_prose_only() {
        // #ktw8: the destructive clear gate is proven only by an anchored
        // structured [s760] line, never by prose embedded in queue_diff logs.
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some(
                    agent_doc_element_backlog::gate_verify::S760_CLEAR_DECISION_CLEAR_TRUE_MARKER
                        .to_string(),
                ),
                disproof: None,
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(
            &dir,
            "[150] queue_diff_active_prompt_differs file=doc.md prompt_changes=[\"PASS requires [s760] clear-decision optIn=true threshold=50 pct=50.0 clear=true\"] queue_head=\"[#ktw8]\"\n",
        );

        let results = run_gate_verify(&doc, true).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "pending", "quoted prose must not prove");
        assert!(!results[0].auto_resolved);
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(after.contains("- [/] [#saev]"), "gate must remain: {after}");
    }
    #[test]
    fn gate_verify_s760_builtin_auto_resolves_on_anchored_clear_true() {
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some(
                    agent_doc_element_backlog::gate_verify::S760_CLEAR_DECISION_CLEAR_TRUE_MARKER
                        .to_string(),
                ),
                disproof: None,
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(
            &dir,
            "[150] [s760] clear-decision optIn=true threshold=50 pct=50.0 clear=true\n",
        );

        let results = run_gate_verify_force_disk(&doc, true).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "provable");
        assert!(results[0].auto_resolved);
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            after.contains("[x] [#saev]"),
            "gate must be flipped: {after}"
        );
    }
    #[test]
    fn pending_maintenance_fails_closed_when_snapshot_backlog_cannot_be_synced() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let file_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#reap1] Reap me\n",
            "<!-- /agent:backlog -->\n"
        );
        let snapshot_content =
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\nNo backlog here.\n";
        std::fs::write(&doc, file_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let err = run_pending_maintenance(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("snapshot is missing the backlog component")
        );
    }
    #[test]
    fn run_pending_maintenance_sorts_backlog_by_priority() {
        // #backlog-priority-attribute: a backlog carrying `priority` stable-sorts
        // items by their per-item priority token each cycle.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:backlog priority -->\n",
            "- [ ] [#low] priority=5 later\n",
            "- [ ] [#high] priority=1 first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_pending_maintenance_force_disk(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let high = updated.find("[#high]").unwrap();
        let low = updated.find("[#low]").unwrap();
        assert!(
            high < low,
            "priority=1 item must sort before priority=5:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_orders_synced_queue_by_priority() {
        // #backlog-priority-attribute + #backlog-queue-sync-attr: a priority queue
        // synced from a priority backlog comes out prioritized.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue priority -->\n",
            "- do [#low]\n",
            "- do [#high]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#low] priority=5 later\n",
            "- [ ] [#high] priority=1 first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let q = updated.find("<!-- agent:queue").unwrap();
        let qend = updated[q..].find("<!-- /agent:queue").unwrap() + q;
        let queue_region = &updated[q..qend];
        let high = queue_region.find("do [#high]").unwrap();
        let low = queue_region.find("do [#low]").unwrap();
        assert!(
            queue_region.contains(":round_pushpin: do [#high]"),
            "auto-promoted queue item should carry an agent-priority marker:\n{queue_region}"
        );
        assert!(
            high < low,
            "priority=1 must sort before priority=5 in queue:\n{queue_region}"
        );
    }
    #[test]
    fn resolve_pipeline_state_none_without_cycle_or_frontmatter() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();
        assert!(resolve_pipeline_state(&doc).unwrap().is_none());
    }
    #[test]
    fn resolve_pipeline_state_falls_back_to_frontmatter_block() {
        // No cycle-state on disk → read the document `agent_doc_pipeline:` mirror.
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_pipeline:\n  run_id: cycle-77\n  step: write_applied\n---\n\nbody\n",
        )
        .unwrap();
        let p = resolve_pipeline_state(&doc)
            .unwrap()
            .expect("frontmatter fallback");
        assert_eq!(p.run_id.as_deref(), Some("cycle-77"));
        assert_eq!(p.step.as_deref(), Some("write_applied"));
    }
    #[test]
    fn resolve_pipeline_state_cycle_state_wins_over_frontmatter() {
        // Cycle-state is authoritative; a stale frontmatter block must not override it.
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_pipeline:\n  run_id: stale-mirror\n  step: committed\n---\n\nbody\n",
        )
        .unwrap();
        let state = crate::cycle_state::start_preflight_with_task(
            &doc,
            Some("snap"),
            Some("body"),
            Some("#fmrunid-wire"),
            Some("#fmrunid-wire"),
        )
        .unwrap();

        let p = resolve_pipeline_state(&doc)
            .unwrap()
            .expect("cycle-state present");
        assert_eq!(p.run_id.as_deref(), Some(state.cycle_id.as_str()));
        assert_eq!(p.step.as_deref(), Some("preflight_started"));
        assert_eq!(p.turn_id.as_deref(), Some("#fmrunid-wire"));
        assert_ne!(p.run_id.as_deref(), Some("stale-mirror"));
    }

    #[test]
    fn run_queue_maintenance_strikes_prior_cycle_answered_free_text_head() {
        // #qheadresidue: a free-text queue head answered by a PRIOR cycle (its
        // `> **Queue prompt:**` echo is in committed `agent:exchange`, but the
        // current response does not re-quote it) was never struck by the
        // per-cycle #ftstrike, so it stayed active as "completed residue" that
        // session-check INTERRUPTs on every closeout while go-mode convergence
        // re-adds it — the live queue-churn root cause. The preflight catch-up
        // strike must remove it.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: agent switch isn't reactive — opus-4-8\n\n",
            "> **Queue prompt:** JB Run Agent Doc on sampleportal after switching from codex to opencode any agent change has this issue\n\n",
            "Diagnosed: a paused stale parent supervisor. Restart it.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "🚧 :pushpin: JB Run Agent Doc on sampleportal after switching from codex to opencode any agent change has this issue\n",
            "```\n",
            "[route] target tmux session: 0\n",
            "Error: authoritative actor record deferring to boundary agent restart\n",
            "```\n",
            "---\n",
            "- :pushpin: do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#beta] second item still open\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue_body = agent_doc_element::element::parse(&updated)
            .unwrap()
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| updated[q.open_end..q.close_start].to_string())
            .unwrap();
        let entries = agent_doc_queue::document_queue::parse(&queue_body).unwrap();
        // The answered bare multi-line head is now a `Completed` (struck) entry,
        // not an active `Prompt` — session-check's residue guard keys off active
        // heads, so this is exactly what clears the churn.
        let active: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Prompt(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Completed(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            !active
                .iter()
                .any(|t| t.contains("JB Run Agent Doc on sampleportal")),
            "answered head must NOT remain an active Prompt:\nactive={active:?}"
        );
        assert!(
            completed
                .iter()
                .any(|t| t.contains("JB Run Agent Doc on sampleportal")),
            "answered head must be moved to a Completed (struck) entry:\ncompleted={completed:?}\n{updated}"
        );
        // The unanswered id-backed head survives the strike pass, still active.
        assert!(
            active.iter().any(|t| t.contains("do [#beta]")),
            "unanswered id-backed head must remain an active Prompt:\nactive={active:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_keeps_unanswered_free_text_head_active() {
        // #qheadresidue guard: the catch-up strike must NOT strike a free-text
        // head the exchange does not answer — only genuine completed residue is
        // removed, never a live operator report still awaiting a response.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: something unrelated — opus-4-8\n\n",
            "An answer about a completely different topic entirely.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- :pushpin: Still getting JB File Cache Conflict dialogs on every save\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue_body = agent_doc_element::element::parse(&updated)
            .unwrap()
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| updated[q.open_end..q.close_start].to_string())
            .unwrap();
        let entries = agent_doc_queue::document_queue::parse(&queue_body).unwrap();
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert!(
            active
                .iter()
                .any(|t| t.contains("Still getting JB File Cache Conflict dialogs")),
            "unanswered free-text head must stay active (not falsely struck):\n{active:?}"
        );
        assert!(
            !updated.contains("auto-struck"),
            "no head should be struck when the exchange answers none of them:\n{updated}"
        );
    }

    /// Test helper: read the queue entries from the on-disk document.
    fn read_queue_entries(doc: &Path) -> Vec<agent_doc_queue::document_queue::QueueEntry> {
        let updated = std::fs::read_to_string(doc).unwrap();
        let queue_body = agent_doc_element::element::parse(&updated)
            .unwrap()
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| updated[q.open_end..q.close_start].to_string())
            .unwrap();
        agent_doc_queue::document_queue::parse(&queue_body).unwrap()
    }

    #[test]
    fn run_queue_maintenance_strikes_free_text_head_completed_by_done_item() {
        // #qftbklgstrike case (a): a LIVE free-text queue head (never answered in
        // the exchange) that restates a completed `agent:done` item is struck in
        // place, annotated "completed by #<id>".
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- Fix JB File Cache Conflict dialogs on every save\n",
            "- do [#stillopen]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#stillopen] unrelated open work item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        // A second active id-backed head keeps the queue from fully draining, so
        // the struck residue stays in the body (the drain-clear that wipes a
        // fully-emptied queue is existing convergence behavior, not part of this
        // strike). The free-text head is now Completed + annotated.
        let entries = read_queue_entries(&doc);
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Completed(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert!(
            !active
                .iter()
                .any(|t| t.contains("Fix JB File Cache Conflict dialogs")),
            "struck head must no longer be an active Prompt:\nactive={active:?}"
        );
        assert!(
            completed.iter().any(|t| {
                t.contains("Fix JB File Cache Conflict dialogs")
                    && t.contains("auto-struck: completed by #jbcache (#qftbklgstrike)")
            }),
            "head must be struck + annotated 'completed by #jbcache':\ncompleted={completed:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_clears_in_progress_marker_when_free_text_head_struck() {
        // #qftstuck: an in-progress (🚧-marked) free-text queue head that is then
        // struck by the #qftbklgstrike backlog/done convergence must NOT keep the
        // 🚧 marker baked inside the strikethrough, and the marker must not be
        // stranded on an unrelated head either. The genuinely-active next head
        // (`do [#stillopen]`) DOES keep 🚧 (it is now the in-progress head).
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- 🚧 Fix JB File Cache Conflict dialogs on every save\n",
            "- do [#stillopen]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#stillopen] unrelated open work item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let entries = read_queue_entries(&doc);
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Completed(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        // The struck head retains its prose + annotation but NOT the 🚧 marker.
        assert!(
            completed.iter().any(|t| {
                t.contains("Fix JB File Cache Conflict dialogs")
                    && t.contains("auto-struck: completed by #jbcache (#qftbklgstrike)")
                    && !t.contains(IN_PROGRESS_MARKER)
            }),
            "struck head must be annotated with NO 🚧 marker:\ncompleted={completed:?}"
        );
        // The 🚧 marker is not stranded on any struck/completed entry.
        assert!(
            !completed.iter().any(|t| t.contains(IN_PROGRESS_MARKER)),
            "no completed entry may carry 🚧:\ncompleted={completed:?}"
        );
        // The newly-promoted active head is the in-progress head now.
        assert!(
            active
                .iter()
                .filter(|t| t.contains(IN_PROGRESS_MARKER))
                .count()
                == 1,
            "exactly one active head should carry 🚧 (the new in-progress head):\nactive={active:?}"
        );
        assert!(
            active
                .iter()
                .any(|t| t.contains("[#stillopen]") && t.contains(IN_PROGRESS_MARKER)),
            "the genuinely-active next head should be the 🚧 in-progress head:\nactive={active:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_strikes_free_text_head_tracked_by_backlog_item() {
        // #qftbklgstrike case (b): a LIVE free-text queue head that restates an
        // active `agent:backlog` item is struck in place, annotated "tracked by
        // backlog #<id>".
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- Opencode responses are reverse ordering numeric lists\n",
            "- do [#stillopen]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#revlist] Opencode responses are reverse ordering numeric lists\n",
            "- [ ] [#stillopen] unrelated open work item\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let entries = read_queue_entries(&doc);
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Completed(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            completed.iter().any(|t| {
                t.contains("Opencode responses are reverse ordering numeric lists")
                    && t.contains("auto-struck: tracked by backlog #revlist (#qftbklgstrike)")
            }),
            "head must be struck + annotated 'tracked by backlog #revlist':\ncompleted={completed:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_does_not_strike_unrelated_operator_prompt() {
        // #qftbklgstrike false-strike safety: an unrelated operator prompt that is
        // NOT a restatement of any done/backlog item stays an active Prompt and is
        // never annotated/struck, even with done + backlog items present.
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- Please add a dark mode toggle to the settings panel\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#revlist] Opencode responses are reverse ordering numeric lists\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let entries = read_queue_entries(&doc);
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert!(
            active
                .iter()
                .any(|t| t.contains("Please add a dark mode toggle")),
            "unrelated operator prompt must stay active (NEVER silently buried):\n{active:?}"
        );
        assert!(
            !updated.contains("#qftbklgstrike"),
            "no #qftbklgstrike annotation should appear for an unrelated prompt:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_qftbklgstrike_leaves_id_backed_heads_untouched() {
        // #qftbklgstrike: id-backed heads have their own done-strike path; the
        // free-text strike must not annotate them with the #qftbklgstrike marker.
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#open1] still-open work\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#open1] still-open work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("#qftbklgstrike"),
            "id-backed head must not be touched by the #qftbklgstrike free-text path:\n{updated}"
        );
        let entries = read_queue_entries(&doc);
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert!(
            active.iter().any(|t| t.contains("do [#open1]")),
            "unanswered id-backed head must remain active:\n{active:?}"
        );
    }
}

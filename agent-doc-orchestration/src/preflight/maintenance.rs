//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

/// Resolve the live finalize-pipeline view surfaced in preflight output
/// (`#fmrunid-wire`). Cycle-state is authoritative; the document
/// `agent_doc_pipeline:` frontmatter block is only a fallback hint when no live
/// cycle-state exists (e.g. a crash that wiped `.agent-doc/state` but left the
/// document mirror behind). Returns `None` when neither is present.
pub(crate) fn resolve_pipeline_state(file: &Path) -> Result<Option<crate::frontmatter::AgentDocPipeline>> {
    if let Some(state) = crate::cycle_state::load(file)? {
        return Ok(Some(state.to_pipeline()));
    }
    let current = std::fs::read_to_string(file).unwrap_or_default();
    Ok(match crate::frontmatter::parse(&current) {
        Ok((fm, _)) if !fm.pipeline.is_empty() => Some(fm.pipeline),
        _ => None,
    })
}

#[derive(Debug, Clone, Default)]
pub struct PendingMaintenanceReport {
    pub reordered: bool,
    pub pending_gated_count: usize,
    pub review_count: usize,
    pub review_gated_count: usize,
    pub legacy_gated_in_backlog_count: usize,
}

/// Run pending-component maintenance: lazy backfill, reap `[x]`, and reorder detection.
///
/// Any write-through (backfill / reap) is persisted and committed in the same pass.
/// Silent no-op when the document has no tracked-work component.
pub fn run_pending_maintenance(file: &Path) -> Result<PendingMaintenanceReport> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(PendingMaintenanceReport::default()),
    };
    let components = match crate::component::parse(&content) {
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
    let doc_id = snapshot::doc_hash(&canonical).unwrap_or_else(|_| file.display().to_string());

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
        snapshot::find_project_root(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let already_done_ids = collect_agent_done_ids_with_root(&content, project_root.as_deref());

    for surface in &tracked_surfaces {
        let components = crate::component::parse(&current_content)
            .with_context(|| format!("failed to parse components while maintaining {}", surface))?;
        let comp = components
            .into_iter()
            .find(|c| component_matches_tracked_surface(&c.name, surface))
            .with_context(|| format!("document is missing the {} component", surface))?;
        let body = comp.content(&current_content);

        let mut current_body = body.to_string();
        let surface_label = maintenance_surface_label(surface);
        saw_completed_before |= !completed_pending_items(&current_body).is_empty();

        let (after_backfill, changed) =
            crate::pending::backfill(&current_body, &doc_id, &std::collections::HashSet::new());
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
        let (after_dedupe, deduped_ids) = crate::pending::op_dedupe_identical_items(&current_body);
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
                crate::pending::op_take_active_items_by_ids(&current_body, &already_done_ids);
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
            let snapshot_ids = snapshot_baseline.map(|snap| surface_pending_ids(snap, surface));
            // `#opsproof-samecycle-add`: the snapshot baseline alone is not enough.
            // In the `write`/`finalize` path the same invocation that adds an item
            // via `--review-add` / `--pending-add*` also re-syncs the on-disk
            // snapshot, so a brand-new same-cycle add is already present in
            // `snapshot_ids` and the snapshot test cannot exclude it. Cross-check
            // the ids cycle-state recorded as added this cycle and never reap them.
            let added_this_cycle = crate::cycle_state::pending_added_ids(file);
            let ops_proof_completions: Vec<OpsProofCompletion> =
                ops_proof_completion_candidates(&current_body)
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
                    crate::pending::op_take_active_items_by_ids(&current_body, &ids);
                if !ops_proof_items.is_empty() {
                    let removed_ids: Vec<String> =
                        ops_proof_items.iter().map(|i| i.id.clone()).collect();
                    for item in &mut ops_proof_items {
                        item.state = crate::pending::PendingState::Done;
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

        let (after_reap, reaped_items) = crate::pending::reap_with_items(&current_body)?;
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
            && let Some(sorted) = crate::pending::sort_by_priority(&current_body)
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
            let snap_comps = crate::component::parse(snap_content).ok();
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
        crate::status_cmd::reconcile_top_backlog_status_content(&current_content)?
    {
        eprintln!("[preflight] status: reconciled stale top-backlog marker");
        current_content = reconciled;
        mutated = true;
    }
    if let Some(ref mut snap_content) = snapshot_content
        && let Some(reconciled) =
            crate::status_cmd::reconcile_top_backlog_status_content(snap_content)?
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
        std::fs::write(file, &current_content)
            .with_context(|| format!("failed to write pending updates to {}", file.display()))?;
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
            let snap_comp = crate::component::parse(&snap)
                .ok()
                .and_then(|comps| comps.into_iter().find(|c| is_backlog_component(&c.name)));
            if let (Some(sc), Some(current_body)) = (snap_comp, current_body) {
                let snap_body = &snap[sc.open_end..sc.close_start];
                crate::pending::detect_reorder(snap_body, current_body).is_some()
            } else {
                false
            }
        }
        None => false,
    };
    if reordered {
        eprintln!("[preflight] pending: reorder detected (skill must not reorder this cycle)");
    }

    // 5. Count legacy gated items in backlog and review items in review.
    let pending_gated_count = current_body
        .map(|body| {
            let (_, items, _) = crate::pending::parse_items(body);
            items
                .iter()
                .filter(|i| matches!(i.state, crate::pending::PendingState::Gated))
                .count()
        })
        .unwrap_or(0);
    if pending_gated_count > 0 {
        eprintln!("[preflight] pending: {} gated item(s)", pending_gated_count);
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
        pending_gated_count,
        review_count,
        review_gated_count,
        legacy_gated_in_backlog_count: pending_gated_count,
    })
}

pub(crate) fn component_matches_tracked_surface(name: &str, surface: &str) -> bool {
    if is_backlog_component(surface) {
        is_backlog_component(name)
    } else {
        name == surface
    }
}

pub(crate) fn maintenance_surface_label(surface: &str) -> &str {
    if is_backlog_component(surface) {
        "pending"
    } else if is_review_component(surface) {
        "review"
    } else {
        "icebox"
    }
}

pub(crate) fn should_reap_already_done_mirrors(surface: &str) -> bool {
    is_backlog_component(surface) || is_review_component(surface)
}

pub(crate) fn should_reap_ops_proof_completions(surface: &str) -> bool {
    is_backlog_component(surface) || is_review_component(surface)
}

pub(crate) struct OpsProofCompletion {
    id: String,
    evidence: String,
}

/// Pending item ids present in `surface` within `content`. Used to detect
/// brand-new same-cycle adds (absent from the cycle-start snapshot) so ops-proof
/// auto-completion never reaps an item on the cycle it first appears.
pub(crate) fn surface_pending_ids(content: &str, surface: &str) -> HashSet<String> {
    crate::component::parse(content)
        .ok()
        .and_then(|comps| {
            comps
                .into_iter()
                .find(|c| component_matches_tracked_surface(&c.name, surface))
        })
        .map(|comp| {
            let (_, items, _) = crate::pending::parse_items(comp.content(content));
            items
                .into_iter()
                .map(|item| item.id)
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn ops_proof_completion_candidates(body: &str) -> Vec<OpsProofCompletion> {
    let (_, items, _) = crate::pending::parse_items(body);
    items
        .iter()
        .filter(|item| !matches!(item.state, crate::pending::PendingState::Done))
        .filter_map(|item| {
            classify_ops_proof_completion(item).map(|evidence| OpsProofCompletion {
                id: item.id.clone(),
                evidence,
            })
        })
        .collect()
}

pub(crate) fn classify_ops_proof_completion(item: &crate::pending::PendingItem) -> Option<String> {
    if item.id.is_empty() {
        return None;
    }
    let text = format!("{} {}", item.text, item.continuation);
    let upper = text.to_ascii_uppercase();
    if !has_ops_completion_marker(&upper) || has_ops_completion_blocker(&upper) {
        return None;
    }

    // #opsproofgate: a live-verify / operator-drive gate must NEVER be
    // auto-completed on `evidence=commit`. A shipped commit is not proof for
    // these items — only an anchored `^[epoch] <marker>` line in ops.log
    // (driven live by the operator) is. The `#optverify` log-arbiter path
    // (`run_gate_verify`) closes them on a genuine structured emission; this
    // commit/CI prose scan must stay out of their way, or a submodule hash
    // cited in the gate text falsely archives an UNDRIVEN gate to done.
    if is_live_verify_gate(&upper) {
        return None;
    }

    // #opsproof-falsepos: an open (non-gated) actionable item must NOT be reaped
    // just because its prose cites already-landed dependency work ("the predicate
    // already shipped in abc1234"). The completion marker must be the item's own
    // leading status verb. Gated items were deliberately code-completed by the
    // agent, so a proven marker anywhere in their text legitimately closes them.
    let is_gated = matches!(item.state, crate::pending::PendingState::Gated);
    if !is_gated && !marker_is_leading_status(&upper) {
        return None;
    }

    let has_commit = contains_commit_hash(&text);
    let has_ci = contains_successful_ci_proof(&upper);
    if !has_commit && !has_ci {
        return None;
    }

    Some(
        match (has_commit, has_ci) {
            (true, true) => "commit+ci",
            (true, false) => "commit",
            (false, true) => "ci",
            (false, false) => unreachable!(),
        }
        .to_string(),
    )
}

pub(crate) fn has_ops_completion_marker(upper: &str) -> bool {
    ["DONE", "SHIPPED", "IMPLEMENTED", "COMPLETE", "COMPLETED"]
        .iter()
        .any(|marker| contains_ascii_word(upper, marker))
}

/// Max number of leading words (after skipping `#hashtag` tokens) that count as
/// the item's status prefix for ops-proof auto-completion.
pub(crate) const LEADING_STATUS_WORDS: usize = 4;

/// True when an ops-completion marker is the item's leading status verb rather
/// than a marker buried in a cited dependency clause. The leading status segment
/// is the prefix before the first clause break (`: ` or `. `), further capped to
/// the first [`LEADING_STATUS_WORDS`] words after skipping leading `#hashtag`
/// tokens. `upper` must already be ASCII-uppercased.
pub(crate) fn marker_is_leading_status(upper: &str) -> bool {
    has_ops_completion_marker(&leading_status_segment(upper))
}

pub(crate) fn leading_status_segment(upper: &str) -> String {
    let mut cut = upper.len();
    for sep in [": ", ". "] {
        if let Some(idx) = upper.find(sep) {
            cut = cut.min(idx);
        }
    }
    upper[..cut]
        .split_whitespace()
        .filter(|word| !word.starts_with('#'))
        .take(LEADING_STATUS_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn has_ops_completion_blocker(upper: &str) -> bool {
    const BLOCKER_PHRASES: &[&str] = &[
        "COULD NOT",
        "CAN NOT",
        "CANNOT",
        "CAN'T",
        "FALSE CLOSEOUT",
        "FOLLOW-UP",
        "FOLLOW UP",
        "FOLLOWUPS",
        "NOT DONE",
        "NOT SHIPPED",
        "NOT IMPLEMENTED",
        "SUB-PART",
        "SUBPART",
    ];
    const BLOCKER_WORDS: &[&str] = &[
        "PARTIAL",
        "REMAINING",
        "REOPENED",
        "DEFERRED",
        "BLOCKED",
        "BLOCKER",
        "TODO",
        "WIP",
        "PARTLY",
        "FAILING",
        "FAILED",
    ];

    BLOCKER_PHRASES.iter().any(|phrase| upper.contains(phrase))
        || BLOCKER_WORDS
            .iter()
            .any(|word| contains_ascii_word(upper, word))
}

/// True when an item is a live-verify / operator-drive gate whose only valid
/// completion proof is an anchored structured ops.log marker driven live by the
/// operator — never a cited commit/CI reference (`#opsproofgate`). `upper` must
/// already be ASCII-uppercased.
pub(crate) fn is_live_verify_gate(upper: &str) -> bool {
    const LIVE_VERIFY_PHRASES: &[&str] = &[
        "LIVE-VERIFY GATE",
        "LIVE-VERIFY ONLY",
        "LIVE VERIFY GATE",
        "LIVE VERIFY ONLY",
        "OPERATOR-DRIVE",
        "OPERATOR DRIVE",
        "OPERATOR DRIVES",
        "OPERATOR LIVE-VERIFY",
        "OPERATOR LIVE VERIFY",
    ];
    LIVE_VERIFY_PHRASES
        .iter()
        .any(|phrase| upper.contains(phrase))
}

pub(crate) fn contains_successful_ci_proof(upper: &str) -> bool {
    contains_ascii_word(upper, "CI")
        && ["GREEN", "PASSED", "PASSING", "SUCCESS", "SUCCEEDED"]
            .iter()
            .any(|word| contains_ascii_word(upper, word))
}

pub(crate) fn contains_commit_hash(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| {
            (7..=40).contains(&token.len())
                && token.chars().all(|c| c.is_ascii_hexdigit())
                && token.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F'))
        })
}

pub(crate) fn contains_ascii_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(idx, _)| {
        let before = idx
            .checked_sub(1)
            .and_then(|pos| haystack.as_bytes().get(pos).copied());
        let after = haystack.as_bytes().get(idx + needle.len()).copied();
        before.is_none_or(|b| !b.is_ascii_alphanumeric())
            && after.is_none_or(|b| !b.is_ascii_alphanumeric())
    })
}

pub(crate) fn tracked_body_for_reorder(content: &str) -> Option<&str> {
    crate::component::parse(content).ok().and_then(|comps| {
        comps
            .into_iter()
            .find(|component| is_backlog_component(&component.name))
            .map(|component| component.content(content))
    })
}

pub(crate) fn review_counts(content: &str) -> (usize, usize) {
    let Some(body) = crate::component::parse(content).ok().and_then(|comps| {
        comps
            .into_iter()
            .find(|component| is_review_component(&component.name))
            .map(|component| component.content(content).to_string())
    }) else {
        return (0, 0);
    };
    let (_, items, _) = crate::pending::parse_items(&body);
    let review_items: Vec<_> = items.into_iter().filter(|item| !item.is_done()).collect();
    let gated = review_items
        .iter()
        .filter(|item| matches!(item.state, crate::pending::PendingState::Gated))
        .count();
    (review_items.len(), gated)
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
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let components = match crate::component::parse(&content) {
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
    let (_, items, _) = crate::pending::parse_items(&body);

    // Gather predicate-bearing gated items.
    let predicates: Vec<(String, crate::gate_verify::GatePredicate)> = items
        .iter()
        .filter(|item| matches!(item.state, crate::pending::PendingState::Gated))
        .filter_map(|item| {
            crate::gate_verify::parse_gate_predicate(&item.text)
                .filter(|p| p.is_actionable())
                .map(|p| (item.id.clone(), p))
        })
        .collect();
    if predicates.is_empty() {
        return Ok(Vec::new());
    }

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let ops_log = snapshot::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
        .and_then(|root| std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).ok())
        .unwrap_or_default();

    let mut results = Vec::new();
    let mut to_resolve: Vec<String> = Vec::new();
    for (id, predicate) in &predicates {
        let outcome = crate::gate_verify::scan_ops_log(predicate, &ops_log);
        let (marker, at) = match &outcome {
            crate::gate_verify::VerifyOutcome::Provable { marker, at } => {
                (Some(marker.clone()), Some(*at))
            }
            crate::gate_verify::VerifyOutcome::Failed { marker, at } => {
                (Some(marker.clone()), Some(*at))
            }
            crate::gate_verify::VerifyOutcome::Pending => (None, None),
        };
        let status = outcome.status_str().to_string();
        let provable = matches!(outcome, crate::gate_verify::VerifyOutcome::Provable { .. });
        let auto_resolved = autoverify && provable;
        if auto_resolved {
            to_resolve.push(id.clone());
        }
        match &outcome {
            crate::gate_verify::VerifyOutcome::Provable { marker, at } => {
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
            crate::gate_verify::VerifyOutcome::Failed { marker, at } => {
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
            crate::gate_verify::VerifyOutcome::Pending => {}
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
            new_body = crate::pending::op_done(&new_body, id)?;
        }
        let new_content = review.replace_content(&content, &new_body);
        std::fs::write(file, &new_content)
            .with_context(|| format!("failed to write {} after optverify", file.display()))?;
        // Keep the snapshot in lockstep so the upcoming commit stages the flip.
        if let Some(snap) = snapshot::load(file)?
            && let Ok(snap_comps) = crate::component::parse(&snap)
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

pub(crate) fn ensure_no_completed_tracked_items(content: &str, surface: &str) -> Result<()> {
    let components = crate::component::parse(content).with_context(|| {
        format!("failed to parse {surface} components during pending reap check")
    })?;
    let completed: Vec<crate::pending::PendingItem> = components
        .into_iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| completed_pending_items(component.content(content)))
        .collect();
    if completed.is_empty() {
        return Ok(());
    }

    let refs = completed
        .into_iter()
        .map(|item| {
            if item.id.is_empty() {
                format!("<missing-id> {}", item.text)
            } else {
                format!("#{}", item.id)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!("pending maintenance left completed tracked items in the {surface}: {refs}");
}

pub(crate) fn completed_pending_items(body: &str) -> Vec<crate::pending::PendingItem> {
    let (_, items, _) = crate::pending::parse_items(body);
    items
        .into_iter()
        .filter(crate::pending::PendingItem::is_done)
        .collect()
}

pub(crate) fn enforce_no_shadow_open_backlog(file: &Path) -> Result<()> {
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to inspect backlog shadow state in {}",
            file.display()
        )
    })?;
    let report = crate::pending::detect_shadow_open_items(&content)?;
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

pub(crate) fn format_shadow_refs(items: &[crate::pending::ShadowPendingItem]) -> String {
    items
        .iter()
        .map(crate::pending::ShadowPendingItem::reference)
        .collect::<Vec<_>>()
        .join(", ")
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
    let report = crate::pending::detect_dropped_from_history_with_extra_current_ids(
        &current_content,
        &head_content,
        &resolved_ids,
        &external_done_ids,
    )?;
    if !report.dropped.is_empty() {
        let refs = report
            .dropped
            .iter()
            .map(crate::pending::DroppedBacklogItem::reference)
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "open backlog item(s) from recent committed history are completely absent from the document: {}. Restore them to the live backlog, move them to icebox, or mark them done before continuing",
            refs
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
    pub(crate) queue_active: Option<bool>,
    pub(crate) queue_deferred: bool,
    pub(crate) queue_start_at: Option<String>,
    pub(crate) queue_trigger: Option<crate::queue::QueueTrigger>,
    pub(crate) queue_halted: Option<String>,
    pub(crate) synced_queue_ids: Vec<String>,
    pub(crate) warnings: Vec<PreflightWarning>,
}

/// Run queue component maintenance: resolve activation, consume start fences,
/// persist `queue_active` state, and emit queue prompts for the skill.
///
/// Mutations (consumed start fences, `queue_active` changes) are persisted to
/// BOTH the working tree file and the snapshot, same as pending maintenance.
///
/// The `diff` parameter is optional — only needed for detecting exchange-level
/// `do queue`/`run queue` triggers. Pass `None` on the first call (before diff
/// computation) and the exchange trigger will be resolved in a later step.
/// Collect the backlog→queue sync request from `agent:backlog`
/// (and the legacy `pending` alias) components carrying a `queue` attribute
/// (`#backlog-queue-sync-attr`). Returns the effective mode (the first
/// queue-tagged component's mode wins) and the active item ids from every
/// queue-tagged source component, in document order. Returns `None` when no
/// source component carries a recognized `queue` attribute. Icebox items are
/// intentionally excluded from component-level sync so a drained backlog cannot
/// auto-promote parked work; explicit per-item enqueue markers still work.
/// Narrow the raw `do [#id]` directive target ids to the set that must reach a
/// `--done`/`--pending-gate` lifecycle outcome this cycle: ids still open in the
/// live backlog, minus any id that the backlog→queue sync auto-populated this
/// cycle (`#queue-sync-auto-pending-done-guard-misfire`). Synced ids are agent
/// queue maintenance, not user directives, so demanding they be resolved in the
/// populating cycle is a false-closed misfire.
pub(crate) fn filter_expect_done_or_gate_ids(
    directive_ids: &[String],
    open_backlog_ids: &std::collections::HashSet<String>,
    synced_queue_ids: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    directive_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| open_backlog_ids.contains(id))
        .filter(|id| !synced_queue_ids.contains(id))
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

pub(crate) fn queue_entry_do_id(entry: &crate::queue::QueueEntry) -> Option<String> {
    match entry {
        crate::queue::QueueEntry::Prompt(prompt) | crate::queue::QueueEntry::Completed(prompt) => {
            queue_prompt_done_id(&prompt.text)
        }
        _ => None,
    }
}

pub(crate) struct BacklogQueueSyncRequest {
    pub(crate) mode: crate::queue::BacklogQueueSyncMode,
    pub(crate) ids: Vec<String>,
    pub(crate) enqueue_ids: Vec<String>,
    pub(crate) priority: bool,
}

pub(crate) fn collect_backlog_queue_sync(
    components: &[crate::component::Component],
    content: &str,
) -> Option<BacklogQueueSyncRequest> {
    let mut mode: Option<crate::queue::BacklogQueueSyncMode> = None;
    let mut ids: Vec<String> = Vec::new();
    let mut enqueue_ids: Vec<String> = Vec::new();
    let mut priority = false;
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        enqueue_ids.extend(crate::pending::active_enqueue_item_ids(body));
        if comp.name == "icebox" {
            continue;
        }
        let Some(value) = comp.attrs.get("queue") else {
            continue;
        };
        priority |= comp.attrs.contains_key("priority");
        let Some(comp_mode) = crate::queue::BacklogQueueSyncMode::parse(value) else {
            continue;
        };
        if mode.is_none() {
            mode = Some(comp_mode);
        }
        ids.extend(crate::pending::active_item_ids(body));
    }
    if mode.is_none() && !enqueue_ids.is_empty() {
        mode = Some(crate::queue::BacklogQueueSyncMode::Append);
    }
    ids.extend(enqueue_ids.iter().cloned());
    mode.map(|m| BacklogQueueSyncRequest {
        mode: m,
        ids,
        enqueue_ids,
        priority,
    })
}

/// Build an id→priority-rank map from active `agent:backlog` / `agent:icebox`
/// items (`#backlog-priority-attribute`) for ordering a synced `agent:queue`.
/// First-seen rank wins on duplicate ids across components.
pub(crate) fn collect_backlog_priority_ranks(
    components: &[crate::component::Component],
    content: &str,
) -> std::collections::HashMap<String, u8> {
    let mut rank = std::collections::HashMap::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, r) in crate::pending::active_item_priorities(body) {
            rank.entry(id).or_insert(r);
        }
    }
    rank
}

/// Build an id→`after=#id` dependency map from active `agent:backlog` /
/// `agent:icebox` items for auto-dag queue ordering (`#queue-auto-dag-priority`).
/// First-seen deps win on duplicate ids across components; items with no
/// dependency tokens are omitted.
pub(crate) fn collect_after_deps(
    components: &[crate::component::Component],
    content: &str,
) -> std::collections::HashMap<String, Vec<String>> {
    let mut deps = std::collections::HashMap::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, d) in crate::pending::active_item_after_deps(body) {
            if !d.is_empty() {
                deps.entry(id).or_insert(d);
            }
        }
    }
    deps
}

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

pub(crate) fn run_queue_maintenance(file: &Path, diff: Option<&str>) -> Result<QueueState> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(QueueState::default()),
    };
    let components = match crate::component::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(QueueState::default()),
    };
    let comp = match components.iter().find(|c| c.name == "queue") {
        Some(c) => c,
        None => return Ok(QueueState::default()),
    };

    let body = &content[comp.open_end..comp.close_start];
    let entries = match crate::queue::parse(body) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[preflight] queue parse warning: {}", e);
            return Ok(QueueState::default());
        }
    };

    let mut entries = entries;
    let mut mutated = false;
    let mut current_content = content.clone();
    let mut queue_warnings = Vec::new();
    let mut synced_queue_ids = Vec::new();
    let mut source_queue_priority = false;
    let mut queue_tag_attrs_normalized = false;

    let raw_queue_tag = &current_content[comp.open_start..comp.open_end];
    let normalized_queue_tag = crate::queue::normalize_queue_tag_attrs(raw_queue_tag);
    if normalized_queue_tag != raw_queue_tag {
        let mut rebuilt = String::with_capacity(current_content.len());
        rebuilt.push_str(&current_content[..comp.open_start]);
        rebuilt.push_str(&normalized_queue_tag);
        rebuilt.push_str(&current_content[comp.open_end..]);
        current_content = rebuilt;
        mutated = true;
        queue_tag_attrs_normalized = true;
        eprintln!("[preflight] queue: normalized malformed queue marker attributes");
    }

    // `#ynra`: collect `agent:done` ids ONCE up front. The backlog→queue sync
    // below must never re-mint a `do [#id]` whose id is already completed
    // (archived in `agent:done`) — otherwise the strike pass removes it every
    // cycle, the sync re-injects it the next cycle, and the queue churns forever
    // on a completed ref. `agent:done` is not mutated by any queue maintenance
    // step, so this set is valid for both the sync filter and the later strike.
    let project_root = file.canonicalize().ok().and_then(|canonical| {
        snapshot::find_project_root(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let done_ids = collect_agent_done_ids_with_root(&content, project_root.as_deref());

    // Backlog→queue sync (#backlog-queue-sync-attr): when an `agent:backlog`
    // component carries a `queue` attribute, regenerate the queue `do [#id]`
    // prompts from its active items BEFORE activation so a freshly synced queue
    // can auto-activate on the same cycle. `agent:icebox` is intentionally not a
    // component-level sync source; parked work must be moved to backlog or
    // explicitly marked for enqueue. Per-item enqueue markers
    // (#queue-enqueue-action) append marked ids without requiring the component
    // attribute.
    if let Some(sync_request) = collect_backlog_queue_sync(&components, &content) {
        let mode = sync_request.mode;
        source_queue_priority = sync_request.priority;
        let enqueue_ids: std::collections::HashSet<String> = sync_request
            .enqueue_ids
            .iter()
            .map(|id| id.trim().to_ascii_lowercase())
            .collect();
        let mut backlog_ids = sync_request.ids;
        // Drop ids already in `agent:done` so completed refs are never
        // re-injected into the queue (#ynra). A lingering active backlog `[ ]`
        // bullet whose id is also archived in `agent:done` would otherwise be
        // minted → struck → minted on every cycle.
        if !done_ids.is_empty() {
            let done_lower: std::collections::HashSet<String> =
                done_ids.iter().map(|id| id.to_ascii_lowercase()).collect();
            let before = backlog_ids.len();
            backlog_ids.retain(|id| !done_lower.contains(&id.trim().to_ascii_lowercase()));
            let excluded = before - backlog_ids.len();
            if excluded > 0 {
                eprintln!(
                    "[preflight] queue: excluded {excluded} completed id(s) from backlog→queue sync (already in agent:done; #ynra)"
                );
            }
        }
        // #backlog-queue-sync-pending-add-amplification (decision B/C): while the
        // queue is already running (persisted-active auto-loop), do NOT promote
        // freshly-added backlog items into the live queue. Re-mirroring on every
        // cycle injected each new `--pending-add` as a `do [#id]` head, growing
        // the queue unboundedly and tripping pending_done_guard on each finalize.
        // Restrict the sync to ids already present as queue heads so captured
        // follow-ups wait for the NEXT activation instead of joining mid-loop. A
        // fresh activation (queue not yet active) still mirrors the full backlog.
        let persisted_active_incoming = frontmatter::parse(&content)
            .map(|(fm, _)| fm.queue_active.unwrap_or(false))
            .unwrap_or(false);
        // `#backlog-queue-empty-active-repopulate`: gate the empty-active-queue
        // repopulation on the queue's `go` control. `go` (frontmatter `queue: go`
        // or a marker-side `go`/`start` token, both → `QueueControl::Start`) opts
        // into continuous-backlog-loop: when the live queue is fully drained (0
        // un-struck prompts), repopulate from the full active backlog instead of
        // holding. Without `go` (a plain persisted-active queue), keep the
        // drain-then-stop hold. Amplification can't occur with 0 live prompts, and
        // `active_item_ids` returns only Open `[ ]` items, so processed items
        // (marked `[/]`/`[x]` per the `do #id` closeout rule) drop out and the
        // loop converges when no Open backlog item remains.
        let queue_go_mode = matches!(
            crate::queue::marker_control(&comp.attrs),
            Some(agent_doc_core::frontmatter::QueueControl::Start)
        ) || frontmatter::parse(&content)
            .ok()
            .and_then(|(fm, _)| fm.queue)
            .and_then(|q| agent_doc_core::frontmatter::QueueControl::parse(&q))
            .map(|c| matches!(c, agent_doc_core::frontmatter::QueueControl::Start))
            .unwrap_or(false);
        // `#backlog-queue-attr-populates-in-go-mode`: a plain persisted-active
        // queue (no `go`/`start`) still holds freshly-added backlog ids out of the
        // running loop to avoid mid-loop amplification. But a `go`-mode queue
        // (`queue: go`/`start`) is an explicit continuous-backlog-loop opt-in: the
        // `queue` backlog attribute is *supposed* to populate the queue, so fresh
        // backlog ids append immediately (not only when the queue fully drains).
        // Append/Prepend stay idempotent (existing + struck `Completed` ids are
        // never re-added) and processed items drop out of `active_item_ids` once
        // marked `[/]`/`[x]`, so the queue stays bounded by the open backlog.
        if persisted_active_incoming && !queue_go_mode {
            let existing_queue_ids: std::collections::HashSet<String> = entries
                .iter()
                .filter_map(queue_entry_do_id)
                .map(|id| id.to_ascii_lowercase())
                .collect();
            let before = backlog_ids.len();
            backlog_ids.retain(|id| {
                let key = id.trim().to_ascii_lowercase();
                existing_queue_ids.contains(&key) || enqueue_ids.contains(&key)
            });
            let held = before - backlog_ids.len();
            if held > 0 {
                eprintln!(
                    "[preflight] queue: held {held} freshly-added backlog id(s) out of the active auto-loop \
                     (they sync at the next activation; #backlog-queue-sync-pending-add-amplification)"
                );
            }
        } else if persisted_active_incoming && queue_go_mode {
            eprintln!(
                "[preflight] queue: go-mode active queue — appending fresh backlog `queue`-attr id(s) \
                 (continuous-backlog-loop; #backlog-queue-attr-populates-in-go-mode)"
            );
        }
        if let Some(synced) = crate::queue::sync_backlog_into_queue(&entries, &backlog_ids, mode) {
            let pre_sync_ids = entries
                .iter()
                .filter_map(queue_entry_do_id)
                .collect::<std::collections::HashSet<String>>();
            let mut seen_synced_ids = std::collections::HashSet::new();
            synced_queue_ids = synced
                .iter()
                .filter_map(queue_entry_do_id)
                .filter(|id| !pre_sync_ids.contains(id))
                .filter(|id| seen_synced_ids.insert(id.clone()))
                .collect();
            let new_body = crate::queue::render(&synced);
            current_content = {
                let comps = crate::component::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            let pre_sync_prompt_count = entries
                .iter()
                .filter(|e| matches!(e, crate::queue::QueueEntry::Prompt(_)))
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
        let rank = collect_backlog_priority_ranks(&components, &content);
        if let Ok(Some(snap_content)) = snapshot::load(file)
            && let Ok(snap_components) = crate::component::parse(&snap_content)
            && let Some(snap_queue) = snap_components.iter().find(|c| c.name == "queue")
        {
            let snap_body = &snap_content[snap_queue.open_end..snap_queue.close_start];
            if let Ok(snap_entries) = crate::queue::parse(snap_body) {
                if let Some(pinned) =
                    crate::queue::annotate_operator_priority_reorders(&snap_entries, &entries)
                {
                    let new_body = crate::queue::render(&pinned);
                    current_content = {
                        let comps = crate::component::parse(&current_content)?;
                        let q = comps.iter().find(|c| c.name == "queue").unwrap();
                        q.replace_content(&current_content, &new_body)
                    };
                    eprintln!(
                        "[preflight] queue: pinned manually reordered prompt(s) with operator priority"
                    );
                    entries = pinned;
                    mutated = true;
                }
                // #7r2s: a brand-new queue line the operator just typed (absent from
                // the snapshot, not one the binary appended from the backlog this
                // cycle) carries no pin, so the priority sort below would sink it
                // under `queue`-attr backlog items. Auto-pin it with operator
                // priority so it stays at its authored slot.
                let synced_set: std::collections::HashSet<String> =
                    synced_queue_ids.iter().cloned().collect();
                if let Some(pinned_new) = crate::queue::annotate_manual_queue_additions(
                    &snap_entries,
                    &entries,
                    &synced_set,
                ) {
                    let new_body = crate::queue::render(&pinned_new);
                    current_content = {
                        let comps = crate::component::parse(&current_content)?;
                        let q = comps.iter().find(|c| c.name == "queue").unwrap();
                        q.replace_content(&current_content, &new_body)
                    };
                    eprintln!(
                        "[preflight] queue: auto-pinned manually-added prompt(s) with operator priority (#7r2s)"
                    );
                    entries = pinned_new;
                    mutated = true;
                }
            }
        }
        // Auto-dag (#queue-auto-dag-priority): order by `after=#id` dependency
        // graph first (a blocker outranks a pin); fall back to the plain
        // pin+priority sort when there are no dependency edges.
        let deps = collect_after_deps(&components, &content);
        let sorted = crate::queue::sort_prompts_by_dag(&entries, &rank, &deps)
            .map(|s| ("auto-dag dependency order (blockers + pins)", s))
            .or_else(|| {
                crate::queue::sort_prompts_by_priority(&entries, &rank)
                    .map(|s| ("backlog priority (operator pins position-locked)", s))
            });
        if let Some((how, sorted)) = sorted {
            let sorted = crate::queue::annotate_agent_priority_promotions(&entries, &sorted)
                .unwrap_or(sorted);
            let new_body = crate::queue::render(&sorted);
            current_content = {
                let comps = crate::component::parse(&current_content)?;
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
    let marker_control = crate::queue::marker_control(&comp.attrs);
    let marker_stop = matches!(
        marker_control,
        Some(agent_doc_core::frontmatter::QueueControl::Stop)
    );
    let has_auto = crate::queue::has_auto_attr(&comp.attrs)
        || matches!(
            marker_control,
            Some(agent_doc_core::frontmatter::QueueControl::Start)
        );
    let exchange_triggered = diff.map(crate::diff::detect_queue_trigger).unwrap_or(false);
    let (fm, _) = frontmatter::parse(&current_content).unwrap_or_default();
    let persisted_active = fm.queue_active.unwrap_or(false);

    let mut activation =
        crate::queue::resolve_activation(&entries, has_auto, exchange_triggered, persisted_active);
    // A `stop` marker control forces the queue inactive this cycle regardless of
    // any other activation signal (#queue-state-unify), so the later
    // drain/clear path halts a running queue and strips the control token.
    if marker_stop && activation.active {
        activation = crate::queue::QueueActivation {
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
        crate::template::repair_queue_struck_items_escaped_below_marker(&current_content)
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
        let comps = crate::component::parse(&current_content)?;
        if let Some(q) = comps.iter().find(|c| c.name == "queue") {
            let body = &current_content[q.open_end..q.close_start];
            activation.entries_after = crate::queue::parse(body)
                .context("queue maintenance: failed to parse AST-deduped queue")?;
        }
        mutated = true;
        eprintln!("[preflight] queue: collapsed {dropped} duplicate queue node-key(s)");
    }

    // Consume start fence if needed
    if activation.consumed_start_fence {
        let new_body = crate::queue::render(&activation.entries_after);
        current_content = {
            let comps = crate::component::parse(&current_content)?;
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
    let gated_ids = collect_agent_review_gated_ids(&current_content);
    let mut eligible_ids: std::collections::HashSet<String> = done_ids.clone();
    for id in &gated_ids {
        eligible_ids.insert(id.clone());
    }
    // `activation.entries_after` already reflects start-fence consumption and
    // the duplicate-prompt collapse above, so it is the authoritative current
    // entry set for the strike pass in every branch.
    let entries_for_strike = activation.entries_after.clone();
    if !eligible_ids.is_empty()
        && let Some((new_entries, struck)) =
            strike_done_queue_head_prompts(&entries_for_strike, &eligible_ids)
    {
        let new_body = crate::queue::render(&new_entries);
        current_content = {
            let comps = crate::component::parse(&current_content)?;
            let q = comps.iter().find(|c| c.name == "queue").unwrap();
            q.replace_content(&current_content, &new_body)
        };
        mutated = true;
        for prompt in &struck {
            let source = match queue_prompt_done_id(&prompt.text) {
                Some(id) if done_ids.contains(&id) => "done",
                Some(id) if gated_ids.contains(&id) => "review_gated",
                _ => "unknown",
            };
            eprintln!(
                "[preflight] queue: auto-struck already-resolved head prompt {:?} source={}",
                prompt.text, source
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
        if crate::queue::prompts(&activation.entries_after).is_empty() {
            activation.active = false;
            activation.trigger = None;
        }
    }

    // Phase 3: halt detection — stop fences and item modification
    if activation.active {
        // Stop fence at head → halt the queue
        if crate::queue::has_stop_fence_at_head(&activation.entries_after) {
            eprintln!("[preflight] queue: halt — stop fence at head");
            // Consume the stop fence
            let after_stop: Vec<crate::queue::QueueEntry> = activation.entries_after[1..].to_vec();
            let new_body = crate::queue::render(&after_stop);
            current_content = {
                let comps = crate::component::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            // Strip auto and clear queue_active
            if has_auto {
                let comps = crate::component::parse(&current_content)?;
                if let Some(q) = comps.iter().find(|c| c.name == "queue") {
                    let raw = &current_content[q.open_start..q.open_end];
                    let new_tag = crate::queue::strip_auto_from_tag(raw);
                    if new_tag != raw {
                        let mut rebuilt = String::with_capacity(current_content.len());
                        rebuilt.push_str(&current_content[..q.open_start]);
                        rebuilt.push_str(&new_tag);
                        rebuilt.push_str(&current_content[q.open_end..]);
                        current_content = rebuilt;
                    }
                }
            }
            if persisted_active {
                current_content = frontmatter::merge_queue_state(&current_content, false)?;
            }
            // Persist to file + snapshot
            std::fs::write(file, &current_content)
                .with_context(|| format!("queue halt: failed to write {}", file.display()))?;
            converge_live_buffer_queue_shape(file, &current_content, project_root.as_deref());
            if let Ok(Some(snap)) = snapshot::load(file) {
                let mut new_snap = snap.clone();
                if let Ok(sc) = crate::component::parse(&new_snap)
                    && let Some(sq) = sc.iter().find(|c| c.name == "queue")
                {
                    new_snap = sq.replace_content(&new_snap, &new_body);
                    if has_auto
                        && let Ok(sc2) = crate::component::parse(&new_snap)
                        && let Some(sq2) = sc2.iter().find(|c| c.name == "queue")
                    {
                        let raw = &new_snap[sq2.open_start..sq2.open_end];
                        let new_tag = crate::queue::strip_auto_from_tag(raw);
                        if new_tag != raw {
                            let mut rebuilt = String::with_capacity(new_snap.len());
                            rebuilt.push_str(&new_snap[..sq2.open_start]);
                            rebuilt.push_str(&new_tag);
                            rebuilt.push_str(&new_snap[sq2.open_end..]);
                            new_snap = rebuilt;
                        }
                    }
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
            return Ok(QueueState {
                queue_prompts: vec![],
                queue_active: Some(false),
                queue_deferred: false,
                queue_start_at: None,
                queue_trigger: activation.trigger,
                queue_halted: Some("stop_fence".into()),
                synced_queue_ids,
                warnings: Vec::new(),
            });
        }

        // Time gate at head → defer if not yet time
        if let Some(dt) = crate::queue::time_gate_at_head(&activation.entries_after) {
            eprintln!("[preflight] queue: deferred — time gate at head: {}", dt);
            return Ok(QueueState {
                queue_prompts: vec![],
                queue_active: None,
                queue_deferred: true,
                queue_start_at: Some(dt.to_string()),
                queue_trigger: activation.trigger,
                queue_halted: None,
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
            && let Ok(snap_comps) = crate::component::parse(&snap_content)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let snap_body = &snap_content[snap_q.open_end..snap_q.close_start];
            if let Ok(snap_entries) = crate::queue::parse(snap_body)
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
                    let snap_entries_struck = if eligible_ids.is_empty() {
                        snap_entries
                    } else {
                        strike_done_queue_head_prompts(&snap_entries, &eligible_ids)
                            .map(|(entries, _)| entries)
                            .unwrap_or(snap_entries)
                    };
                    crate::queue::detect_head_prompt_modified(
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
                let head_edit_mid_typing = crate::debounce::is_typing_via_file(
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
                    // Strip auto and clear queue_active
                    if has_auto {
                        let comps = crate::component::parse(&current_content)?;
                        if let Some(q) = comps.iter().find(|c| c.name == "queue") {
                            let raw = &current_content[q.open_start..q.open_end];
                            let new_tag = crate::queue::strip_auto_from_tag(raw);
                            if new_tag != raw {
                                let mut rebuilt = String::with_capacity(current_content.len());
                                rebuilt.push_str(&current_content[..q.open_start]);
                                rebuilt.push_str(&new_tag);
                                rebuilt.push_str(&current_content[q.open_end..]);
                                current_content = rebuilt;
                            }
                        }
                    }
                    if persisted_active {
                        current_content = frontmatter::merge_queue_state(&current_content, false)?;
                    }
                    std::fs::write(file, &current_content).with_context(|| {
                        format!("queue halt: failed to write {}", file.display())
                    })?;
                    converge_live_buffer_queue_shape(
                        file,
                        &current_content,
                        project_root.as_deref(),
                    );
                    // Update snapshot
                    if let Ok(Some(snap2)) = snapshot::load(file) {
                        let mut ns = snap2.clone();
                        if has_auto
                            && let Ok(sc) = crate::component::parse(&ns)
                            && let Some(sq) = sc.iter().find(|c| c.name == "queue")
                        {
                            let raw = &ns[sq.open_start..sq.open_end];
                            let new_tag = crate::queue::strip_auto_from_tag(raw);
                            if new_tag != raw {
                                let mut rebuilt = String::with_capacity(ns.len());
                                rebuilt.push_str(&ns[..sq.open_start]);
                                rebuilt.push_str(&new_tag);
                                rebuilt.push_str(&ns[sq.open_end..]);
                                ns = rebuilt;
                            }
                        }
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
                    return Ok(QueueState {
                        queue_prompts: vec![],
                        queue_active: Some(false),
                        queue_deferred: false,
                        queue_start_at: None,
                        queue_trigger: activation.trigger,
                        queue_halted: Some("item_modified".into()),
                        synced_queue_ids,
                        warnings: Vec::new(),
                    });
                }
            }
        }
    }

    // Handle queue drain: if the queue has no remaining prompts, clear
    // queue_active, strip auto, and remove completed/directive residue.
    let queue_has_prompts = !crate::queue::prompts(&activation.entries_after).is_empty();
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
        let comps = crate::component::parse(&current_content)?;
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
        let comps = crate::component::parse(&current_content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
        let raw_tag = &current_content[q.open_start..q.open_end];
        let new_tag =
            crate::queue::strip_control_from_tag(&crate::queue::strip_auto_from_tag(raw_tag));
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

    // Persist file mutations.
    if mutated {
        std::fs::write(file, &current_content)
            .with_context(|| format!("failed to write queue updates to {}", file.display()))?;
        converge_live_buffer_queue_shape(file, &current_content, project_root.as_deref());
    }

    // Persist snapshot mutations. For newly activated queues, sync the queue
    // component from the visible document into the snapshot so later closeout
    // consumption can prove the same head prompt in both places.
    if (mutated || need_sync_newly_activated_queue_snapshot)
        && let Ok(Some(snap_content)) = snapshot::load(file)
    {
        let mut new_snap = snap_content.clone();

        if queue_tag_attrs_normalized
            && let Ok(snap_comps) = crate::component::parse(&new_snap)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let raw_tag = &new_snap[snap_q.open_start..snap_q.open_end];
            let normalized_tag = crate::queue::normalize_queue_tag_attrs(raw_tag);
            if normalized_tag != raw_tag {
                let mut rebuilt = String::with_capacity(new_snap.len());
                rebuilt.push_str(&new_snap[..snap_q.open_start]);
                rebuilt.push_str(&normalized_tag);
                rebuilt.push_str(&new_snap[snap_q.open_end..]);
                new_snap = rebuilt;
            }
        }

        if need_sync_newly_activated_queue_snapshot
            && let Ok(current_comps) = crate::component::parse(&current_content)
            && let Some(current_q) = current_comps
                .iter()
                .find(|component| component.name == "queue")
            && let Ok(snap_comps) = crate::component::parse(&new_snap)
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
            && let Ok(snap_comps) = crate::component::parse(&new_snap)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let new_body = if need_clear_drained_body {
                String::new()
            } else {
                crate::queue::render(&activation.entries_after)
            };
            new_snap = snap_q.replace_content(&new_snap, &new_body);

            if need_strip_auto
                && let Ok(snap_comps2) = crate::component::parse(&new_snap)
                && let Some(snap_q2) = snap_comps2.iter().find(|c| c.name == "queue")
            {
                let raw_tag = &new_snap[snap_q2.open_start..snap_q2.open_end];
                let new_tag = crate::queue::strip_auto_from_tag(raw_tag);
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
        crate::queue::prompts(&activation.entries_after)
            .iter()
            .map(|p| p.text.clone())
            .collect()
    } else {
        vec![]
    };

    Ok(QueueState {
        queue_prompts,
        queue_active: if activation.active {
            Some(true)
        } else if activation.deferred {
            None
        } else if persisted_active {
            Some(false)
        } else {
            None
        },
        queue_deferred: activation.deferred,
        queue_start_at: activation.start_at,
        queue_trigger: activation.trigger,
        queue_halted: None,
        synced_queue_ids,
        warnings: queue_warnings,
    })
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
pub(crate) fn converge_live_buffer_queue_shape(file: &Path, content: &str, project_root: Option<&Path>) {
    let Some(root) = project_root else {
        return;
    };
    if !crate::ipc_socket::is_listener_active(root) {
        return;
    }
    let (want_auto, queue_body) = match crate::component::parse(content) {
        Ok(comps) => comps
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| {
                (
                    crate::queue::has_auto_attr(&q.attrs),
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
    let Ok(cur_comps) = crate::component::parse(current_content) else {
        return;
    };
    let Some(cur_q) = cur_comps.iter().find(|c| c.name == "queue") else {
        return;
    };
    let Ok(snap_comps) = crate::component::parse(&snap_now) else {
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
    current_entries: &[crate::queue::QueueEntry],
) -> bool {
    let Ok(Some(snapshot_content)) = snapshot::load(file) else {
        return true;
    };
    let Ok(components) = crate::component::parse(&snapshot_content) else {
        return true;
    };
    let Some(snap_queue) = components.iter().find(|c| c.name == "queue") else {
        return true;
    };
    let snap_body = &snapshot_content[snap_queue.open_end..snap_queue.close_start];
    let Ok(snap_entries) = crate::queue::parse(snap_body) else {
        return true;
    };
    crate::queue::render(&snap_entries) != crate::queue::render(current_entries)
}

pub(crate) fn queue_entries_are_drained_residue(entries: &[crate::queue::QueueEntry]) -> bool {
    !entries.is_empty()
        && entries.iter().all(|entry| {
            matches!(
                entry,
                crate::queue::QueueEntry::Completed(_)
                    | crate::queue::QueueEntry::Preset(_)
                    | crate::queue::QueueEntry::Dispatch(_)
            )
        })
}

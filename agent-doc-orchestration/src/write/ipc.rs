//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_document::singleton_repair::repair_duplicate_singleton_components;
use agent_doc_document_realtime::write_policy::{
    WholeBufferAuthority, WholeBufferAuthorityFacts, WholeBufferDelivery,
    WholeBufferDeliveryAction, ack_content_contains_latest_response, decide_whole_buffer_delivery,
    dropped_prompt_lines_after_content_ours, first_response_heading,
    ipc_snapshot_would_absorb_live_prompt_drift_after_preflight, new_agent_response_headings,
    response_already_in_current, response_converged_in_visible_target,
    response_target_disjoint_from_user_edit,
};
#[cfg(test)]
use agent_doc_element_exchange::extract_post_commit_normalization_targets;
#[cfg(test)]
use agent_doc_element_exchange::verify_sidecar_normalization;
use agent_doc_element_exchange::{
    duplicate_prompt_line_count, normalize_exchange_prefixes_for_targets, user_prompt_count_growth,
};
use agent_doc_ipc_protocol::{
    AlreadyAppliedSnapshotOutcome, EditorBadStateFingerprint, FullContentRepairRedelivery,
    IpcDiskRepairReason, IpcRepairDecision, IpcSnapshotSource,
};
use agent_doc_template::response_materialization::{
    extract_response_headings_from_patches, response_materialization_probe_from_response,
};
use agent_doc_turn::response_replay::materialize_response_in_current_exchange;
use agent_doc_write_converge_io::{
    ack_content_disk_write_proof, ipc_repair_decision_from_sidecar,
    mark_ack_content_live_buffer_synced_after_write, poll_ack_content_sidecar,
    reconcile_ack_snapshot_to_newer_operator_buffer, write_ack_content_through_to_disk,
};
#[cfg(test)]
use agent_doc_write_converge_io::{
    ipc_direct_disk_degraded, read_ack_content_sidecar, record_ipc_socket_ack_timeout,
};

/// Return `true` when every `### Re:` response heading carried in the
/// incoming patches is already present in the document's `HEAD` content.
///
/// Used inside the late-fallback gate (see `#adoc-compact-during-turn-response-loss`)
/// to distinguish:
/// - "cycle committed because this response already landed" (skip apply), and
/// - "cycle committed by an unrelated mid-turn operation, but the response
///   is still waiting to be written" (rotate the cycle, apply the patch).
///
/// Returns `true` when there are no headings to check (no patches), which
/// preserves the gate's previous conservative behavior for empty patch lists.
/// Returns `false` if `git show HEAD:<file>` fails — the caller treats that
/// the same as "not in HEAD" and rotates the cycle, which is fail-safe for
/// the mid-turn race.
pub(crate) fn patch_response_headings_already_in_head(
    file: &Path,
    patches: &[agent_doc_template::PatchBlock],
) -> bool {
    let headings = extract_response_headings_from_patches(patches);
    if headings.is_empty() {
        return true;
    }
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
    let Some(head) = rc.head_content() else {
        return false;
    };
    headings.iter().all(|h| head.contains(h.as_str()))
}

#[cfg(test)]
pub(crate) fn content_ours_with_pending_from_disk(file: &Path, content_ours: &str) -> String {
    match std::fs::read_to_string(file) {
        Ok(on_disk_content) => {
            let spliced = splice_pending_component(content_ours, &on_disk_content);
            if let Some(warning) = spliced.warning.as_ref() {
                log_splice_pending_component_warning(warning);
            }
            spliced.content
        }
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to read {} while preserving pending mutations during normalization fallback: {}",
                file.display(),
                e
            );
            content_ours.to_string()
        }
    }
}

#[cfg(test)]
pub(crate) fn content_ours_merged_with_disk_edits(
    file: &Path,
    baseline: Option<&str>,
    content_ours: &str,
) -> String {
    let Some(base) = baseline else {
        return content_ours_with_pending_from_disk(file, content_ours);
    };
    let Ok(on_disk_content) = std::fs::read_to_string(file) else {
        return content_ours_with_pending_from_disk(file, content_ours);
    };
    if strip_boundary_for_dedup(&on_disk_content) == strip_boundary_for_dedup(content_ours) {
        return content_ours.to_string();
    }
    if response_already_in_current(base, content_ours, &on_disk_content) {
        eprintln!(
            "[write] normalization fallback: response delta already in current file; adopting current content"
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_adopted_current_delta file={} delta=response_contained",
                file.display()
            ),
        );
        return on_disk_content;
    }

    let base_state = match agent_doc_snapshot_io::crdt_merge_base_state_with(
        file,
        base,
        agent_doc_op_capture_io::has_pending_editor_ops,
        agent_doc_ops_log_io::log_op,
    ) {
        Ok(base) => base.state,
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to load overlay CRDT merge base, falling back to baseline text: {}",
                e
            );
            agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state()
        }
    };
    match agent_doc_merge_io::merge_contents_crdt_with_ops(
        file,
        Some(&base_state),
        content_ours,
        &on_disk_content,
        agent_doc_ops_log_io::log_op,
    ) {
        Ok((merged, _)) => merged,
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to merge current disk edits into normalization fallback: {}",
                e
            );
            content_ours_with_pending_from_disk(file, content_ours)
        }
    }
}

#[cfg(test)]
pub(crate) fn normalized_content_ours_fallback(
    file: &Path,
    baseline: Option<&str>,
    content_ours: &str,
    normalize_prefix_lines: &[String],
) -> String {
    let fallback = content_ours_merged_with_disk_edits(file, baseline, content_ours);
    let normalized = normalize_exchange_prefixes_for_targets(&fallback, normalize_prefix_lines);
    agent_doc_element_exchange_io::repair_duplicate_prompt_artifacts_with_log(
        &normalized,
        file,
        DuplicatePromptRepairOptions::new("normalization_fallback")
            .with_before(baseline)
            .preserving(baseline)
            .without_residue_guard(),
        agent_doc_ops_log_io::log_op,
        log_duplicate_prompt_residue_guard,
    )
    .map(|(repaired, _)| repaired)
    .unwrap_or(normalized)
}

/// `#smconv` (`#semmerge-converge-adapter`, Phase 2): attempt a node-keyed
/// semantic merge of `base`, `candidate` (the agent's response snapshot =
/// `ours_agent`), and `content_ours` (the editor buffer = `theirs_operator`),
/// returning the merged document ONLY when it is *safely applicable*.
///
/// This is the convergence path that replaces dropping the agent's work via a
/// `content_ours` adoption: when the operator and agent edited disjoint nodes
/// (the common case) both change-sets land; on a true same-node conflict the
/// operator wins. The merge is applied only when ALL conservative gates hold —
/// this is the commit path, so when in doubt the caller falls through to the
/// existing line-based `#fintol2` / `content_ours` carry-forward UNCHANGED:
///
/// 1. The reconstructed `merged_doc` is non-empty and re-parses cleanly
///    (`element::structural_corruption_reason` is `None`).
/// 2. The merge preserves the agent's response: it must NOT drop any agent
///    exchange/queue/backlog content that `content_ours` would have dropped —
///    both `dropped_prompt_lines_after_content_ours` and
///    `dropped_queue_prompt_lines_after_content_ours` against `merged_doc` are
///    empty. This is the critical safety check.
/// 3. The AST model actually applies: `base`, `candidate`, and `content_ours`
///    each parse to at least one component, so node-keyed merge is meaningful.
fn try_semantic_merge_convergence(
    base: &str,
    candidate: &str,
    content_ours: &str,
) -> Option<agent_doc_merge::semantic_merge::SemanticMerge> {
    // Gate 3 first (cheapest, no allocation of the merged doc): the AST model
    // must apply to all three sides for a node-keyed merge to be meaningful.
    if agent_doc_markdown_ast::overlay::components(base).is_empty()
        || agent_doc_markdown_ast::overlay::components(candidate).is_empty()
        || agent_doc_markdown_ast::overlay::components(content_ours).is_empty()
    {
        return None;
    }

    // #msn6 / #smturnactive (semantic_merge Phase 6): the turn-active area is the
    // `exchange` tail (the in-flight prompt + its response). Scope ack emission to
    // it so a same-node operator↔agent collision OUTSIDE exchange — the operator
    // editing the queue or a backlog item while the agent writes its response —
    // auto-resolves to the operator value with no ack noise; only an
    // exchange-area collision raises an AckRequest. The merged document is
    // identical to the unscoped merge (operator always wins), so this only
    // narrows ack noise, never content.
    let active = agent_doc_merge::semantic_merge::ActiveNodes::new().active_component("exchange");
    let sm = agent_doc_merge::semantic_merge::semantic_merge_scoped(
        base,
        candidate,
        content_ours,
        &active,
    );

    // Gate 1: non-empty and structurally clean (same guard used for `ours`).
    if sm.merged_doc.is_empty() {
        return None;
    }
    if element::structural_corruption_reason(&sm.merged_doc).is_some() {
        return None;
    }

    // Gate 2 (critical): the merge must not silently drop agent content. If it
    // would, decline and let the caller fall through to the existing path, which
    // records the dropped-prompt evidence before adopting `content_ours`.
    if !dropped_prompt_lines_after_content_ours(base, candidate, &sm.merged_doc).is_empty() {
        return None;
    }
    if !dropped_queue_prompt_lines_after_content_ours(base, candidate, &sm.merged_doc).is_empty() {
        return None;
    }
    // Gate 2b (critical, agent response): the merge must preserve every NEW
    // `### Re:` response heading the agent authored this cycle. The shipped
    // semantic_merge reconstructs component bodies from list *items* and keeps
    // only the operator skeleton's non-item prose (documented assumption), so a
    // heading-prose exchange turn can be dropped silently — which is exactly the
    // data-loss class this phase fixes. If any agent-added response heading is
    // absent from `merged_doc`, decline and fall through so the existing path
    // records the dropped evidence instead of losing the agent's turn.
    for heading in new_agent_response_headings(base, candidate) {
        if !sm.merged_doc.contains(&heading) {
            return None;
        }
    }

    Some(sm)
}

pub(crate) fn guard_ipc_snapshot_adoption_against_live_prompt_drift(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    decision: &mut IpcRepairDecision,
) -> bool {
    if decision.snap_source == IpcSnapshotSource::ContentOurs {
        return false;
    }
    let (Some(base), Some(ours)) = (baseline, content_ours) else {
        return false;
    };
    // #dupcontent: never adopt a structurally-corrupt `content_ours` buffer (a
    // bad prior CRDT merge with duplicate singleton component blocks or a
    // split/unterminated attribute). Refuse the adoption so the clean candidate
    // snapshot stays as the base — the corrupt buffer never reaches disk, where
    // the lint-gate could only flag it after the fact.
    if let Some(reason) = element::structural_corruption_reason(ours) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "content_ours_adoption_refused_structural file={} source={} patch_id={} reason={} content_ours_len={} content_ours_hash={}",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                reason,
                ours.len(),
                agent_doc_hash::content_hash(ours),
            ),
        );
        return false;
    }
    if !ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
        base,
        &decision.snapshot_content,
        ours,
    ) {
        return false;
    }
    if let Some(stale_message) = stale_supervisor_content_ours_adoption_warning(file) {
        log_content_ours_adoption_refused_stale_supervisor(
            file,
            source,
            patch_id,
            "live_prompt_drift",
            ours,
            &stale_message,
        );
        let _ = agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(file);
        return false;
    }

    let prior_source = decision.snap_source.label();
    agent_doc_flow_io::log_flow_event(
        file,
        agent_doc_flow::types::FlowEvent::new(
            agent_doc_flow::types::FlowName::DocumentMutation,
            agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
            agent_doc_flow::types::FlowOutcome::Blocked,
        )
        .with_reason("live_prompt_drift_after_preflight"),
        agent_doc_ops_log_io::log_op,
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_blocked file={} source={} patch_id={} snap_source={} reason=live_prompt_drift_after_preflight candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            prior_source,
            decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&decision.snapshot_content),
            ours.len(),
            agent_doc_hash::content_hash(ours)
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        patch_id,
        "live_prompt_drift_after_preflight",
        "visible_repair_required",
        &format!(
            "snap_source={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            prior_source,
            decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&decision.snapshot_content),
            ours.len(),
            agent_doc_hash::content_hash(ours)
        ),
    );
    let _ = agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(file);

    let candidate = decision.snapshot_content.clone();
    // #qdelipc: live queue deletion in the IPC candidate is not proof of
    // operator intent; stale editor/ack content can present an old empty queue.
    // Preserve content_ours and log the ignored deletion count. Normal
    // queue-consume / done-id paths remain the only closeout-time deletion proof.
    let (queue_reconciled_ours, ignored_queue_deletions) =
        preserve_content_ours_over_live_queue_deletions(base, &candidate, ours);
    if !ignored_queue_deletions.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_live_deletion_ignored file={} source={} patch_id={} count={} reason=unproven_ipc_candidate_queue_deletion",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                ignored_queue_deletions.len()
            ),
        );
    }

    // #smconv: node-keyed semantic merge — apply BOTH change-sets when the
    // operator and agent edited disjoint nodes (the common case), operator-wins on
    // same-node conflicts, instead of dropping the agent's changes by adopting
    // content_ours. Falls through to the line-based #fintol2 / content_ours path
    // only when the AST merge is not safely applicable (structural corruption,
    // would still drop agent content, or the AST model does not apply).
    if let Some(sm) = try_semantic_merge_convergence(base, &candidate, &queue_reconciled_ours) {
        let merged_doc = sm.merged_doc.clone();
        let outcome_count = sm.outcomes.len();
        let ack_count = sm.requires_ack.len();
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "live_prompt_drift_semantic_merged file={} source={} patch_id={} base_len={} base_hash={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={} merged_len={} merged_hash={} outcomes={} acks={} reason=node_keyed_semantic_merge",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                base.len(),
                agent_doc_hash::content_hash(base),
                candidate.len(),
                agent_doc_hash::content_hash(&candidate),
                queue_reconciled_ours.len(),
                agent_doc_hash::content_hash(&queue_reconciled_ours),
                merged_doc.len(),
                agent_doc_hash::content_hash(&merged_doc),
                outcome_count,
                ack_count,
            ),
        );
        if ack_count > 0 {
            // #semmerge-ack-turn (Phase 4): operator-wins is already encoded in
            // `merged_doc`, but the agent's non-applied change must be acknowledged
            // in an exchange turn the NEXT cycle. Persist the acks to cycle_state so
            // `start_preflight` carries them forward and preflight surfaces them as
            // `semantic_merge_acks`. Also log the proof marker so a forensic reader
            // (or `session-check`) can recover the reasons.
            let reasons: Vec<String> = sm
                .requires_ack
                .iter()
                .map(|ack| format!("{}:{}:{}", ack.component, ack.id, ack.reason.token()))
                .collect();
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "semantic_merge_ack_pending file={} source={} patch_id={} ack_count={} reasons={}",
                    file.display(),
                    source,
                    patch_id.unwrap_or("-"),
                    ack_count,
                    reasons.join(","),
                ),
            );
            if let Err(e) =
                agent_doc_cycle_state_io::record_semantic_merge_acks(file, &sm.requires_ack)
            {
                eprintln!(
                    "[write] warning: failed to record semantic_merge acks for carry-forward: {e}"
                );
            }
        }
        // #adoc-live-prompt-drift-operator-edit: the reconciled `merged_doc` is the
        // operator-visible converged state (semantic merge, operator-wins on the
        // response node). It is proven — no editor redelivery required — as soon as
        // it presents this cycle's response heading(s) with a body, even if the
        // operator edited the response text. Requiring the agent's exact bytes here
        // is what wedged the closeout on an operator body-edit.
        let visible_repair_required =
            !response_converged_in_visible_target(base, &candidate, &merged_doc);
        decision.replace_snapshot_with_content_ours_for_live_prompt_drift(
            &merged_doc,
            visible_repair_required,
        );
        return true;
    }

    // #fintol2 — forward-merge tolerance for an independent concurrent edit. When
    // the user's concurrent edit is a DISJOINT, plain content edit outside
    // `exchange` (proven by `response_target_disjoint_from_user_edit`: confined
    // outside the response component, carrying no prompt/directive, and yielding a
    // conflict-free union that preserves both sides), commit that union so the
    // response lands AND the user's edit is preserved this cycle. Anything
    // prompt-/directive-bearing, in-`exchange`, or colliding returns false and
    // falls through to a visible-repair-required content_ours adoption. The
    // caller must prove the editor/worktree accepted that repaired state before
    // closeout can commit.
    if response_target_disjoint_from_user_edit(
        base,
        &queue_reconciled_ours,
        &candidate,
        |base, ours, theirs| agent_doc_merge_io::merge_contents(base, ours, theirs).ok(),
    ) && let Ok(union) =
        agent_doc_merge_io::merge_contents(base, &queue_reconciled_ours, &candidate)
        && !union.contains("<<<<<<<")
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "live_prompt_drift_forward_merged file={} source={} patch_id={} candidate_len={} candidate_hash={} union_len={} union_hash={} reason=independent_concurrent_edit",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                candidate.len(),
                agent_doc_hash::content_hash(&candidate),
                union.len(),
                agent_doc_hash::content_hash(&union),
            ),
        );
        // #adoc-live-prompt-drift-operator-edit: same operator-edit tolerance for the
        // forward-merge union — proven once it presents this cycle's response
        // heading(s) with a body, regardless of operator body-edits.
        let visible_repair_required =
            !response_converged_in_visible_target(base, &candidate, &union);
        decision.replace_snapshot_with_content_ours_for_live_prompt_drift(
            &union,
            visible_repair_required,
        );
        return true;
    }

    // #exchange-prompt-dropped-on-merge: persist the dropped user prompt lines
    // now, while the divergent candidate still carries them. The post-commit
    // session-check disk diff cannot win the race against an editor that
    // overwrites disk with the converged content_ours buffer, so the dropped
    // prompt guard reads this persisted evidence to fail closed instead.
    let dropped = dropped_prompt_lines_after_content_ours(base, &candidate, ours);
    if !dropped.is_empty() {
        if let Err(e) = agent_doc_cycle_state_io::record_dropped_exchange_prompts(file, &dropped) {
            eprintln!(
                "[write] warning: failed to record dropped exchange prompt(s) for {}: {}",
                file.display(),
                e
            );
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "dropped_exchange_prompt_recorded file={} source={} patch_id={} count={}",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                dropped.len()
            ),
        );
    }
    // #queue-user-edit-overwrite: same silent-loss race for user-authored queue
    // edits. Record the dropped `do [#id]` queue lines now; session-check
    // filters them against committed HEAD (preserved or consumed → cleared,
    // silently deleted → fail closed).
    let dropped_queue =
        dropped_queue_prompt_lines_after_content_ours(base, &candidate, &queue_reconciled_ours);
    if !dropped_queue.is_empty() {
        if let Err(e) = agent_doc_cycle_state_io::record_dropped_queue_prompts(file, &dropped_queue)
        {
            eprintln!(
                "[write] warning: failed to record dropped queue prompt(s) for {}: {}",
                file.display(),
                e
            );
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "dropped_queue_prompt_recorded file={} source={} patch_id={} count={}",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                dropped_queue.len()
            ),
        );
    }
    let live_candidate_contains_response =
        ack_content_contains_latest_response(&candidate, &queue_reconciled_ours);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "live_prompt_drift_agent_target_not_snapshot_authority file={} source={} patch_id={} live_candidate_contains_response={} candidate_len={} candidate_hash={} agent_target_len={} agent_target_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            live_candidate_contains_response,
            candidate.len(),
            agent_doc_hash::content_hash(&candidate),
            queue_reconciled_ours.len(),
            agent_doc_hash::content_hash(&queue_reconciled_ours),
        ),
    );
    true
}

pub(crate) fn guard_ipc_snapshot_adoption_against_prompt_duplication(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    content_ours: Option<&str>,
    decision: &mut IpcRepairDecision,
) -> bool {
    if decision.snap_source == IpcSnapshotSource::ContentOurs {
        return false;
    }
    let Some(ours) = content_ours else {
        return false;
    };
    // #dupcontent: same fail-closed refusal on the prompt-duplication path — a
    // structurally-corrupt `content_ours` buffer must never become the snapshot.
    if let Some(reason) = element::structural_corruption_reason(ours) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "content_ours_adoption_refused_structural file={} source={} patch_id={} reason={} guard=prompt_duplication content_ours_len={} content_ours_hash={}",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                reason,
                ours.len(),
                agent_doc_hash::content_hash(ours),
            ),
        );
        return false;
    }
    let duplicate_count = user_prompt_count_growth(ours, &decision.snapshot_content);
    if duplicate_count == 0 {
        return false;
    }
    if let Some(stale_message) = stale_supervisor_content_ours_adoption_warning(file) {
        log_content_ours_adoption_refused_stale_supervisor(
            file,
            source,
            patch_id,
            "prompt_duplication",
            ours,
            &stale_message,
        );
        let _ = agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(file);
        return false;
    }

    let prior_source = decision.snap_source.label();
    let bad_state = decision.snapshot_content.clone();
    agent_doc_flow_io::log_flow_event(
        file,
        agent_doc_flow::types::FlowEvent::new(
            agent_doc_flow::types::FlowName::DocumentMutation,
            agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
            agent_doc_flow::types::FlowOutcome::Blocked,
        )
        .with_reason("prompt_duplication_in_ack_content"),
        agent_doc_ops_log_io::log_op,
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_blocked file={} source={} patch_id={} snap_source={} reason=prompt_duplication_in_ack_content duplicate_prompt_count={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            prior_source,
            duplicate_count,
            decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&decision.snapshot_content),
            ours.len(),
            agent_doc_hash::content_hash(ours)
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        patch_id,
        "prompt_duplication_in_ack_content",
        "content_ours_snapshot_and_visible_repair",
        &format!(
            "snap_source={} duplicate_prompt_count={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            prior_source,
            duplicate_count,
            decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&decision.snapshot_content),
            ours.len(),
            agent_doc_hash::content_hash(ours)
        ),
    );
    let _ = agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(file);
    decision.replace_snapshot_with_content_ours_for_prompt_duplication(ours, bad_state);
    true
}

/// Emit a diagnostic for every IPC snapshot adoption that the two fail-closed
/// guards did NOT block. Blocked adoptions already log richly; allowed ones were
/// previously silent, so a corruption that slips through as "allowed" left no
/// trace. This symmetric `ipc_snapshot_adoption_allowed` line records the final
/// snapshot shape plus an independent drift/dup re-check (both must be benign on
/// an allowed path — a non-benign re-check here flags a guard-coverage gap).
pub(crate) fn log_ipc_snapshot_adoption_allowed(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    decision: &IpcRepairDecision,
    was_blocked: bool,
) {
    if was_blocked {
        return;
    }
    let drift_recheck = match (baseline, content_ours) {
        (Some(base), Some(ours)) => ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
            base,
            &decision.snapshot_content,
            ours,
        ),
        _ => false,
    };
    let dup_recheck = content_ours
        .map(|ours| user_prompt_count_growth(ours, &decision.snapshot_content))
        .unwrap_or(0);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_allowed file={} source={} patch_id={} snap_source={} snapshot_len={} snapshot_hash={} content_ours_len={} content_ours_hash={} drift_recheck={} dup_growth_recheck={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            decision.snap_source.label(),
            decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&decision.snapshot_content),
            content_ours.map(|o| o.len()).unwrap_or(0),
            content_ours
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            drift_recheck,
            dup_recheck,
        ),
    );
}

/// #ipcfullprompt-recur2 — default-on forensic capture. The fail-closed snapshot
/// guards above protect what gets *committed*, but a full-document editor-side
/// IPC mutation (e.g. `PatchWatcher.setText`) can still corrupt the
/// editor-visible buffer — deleting or duplicating a previously-committed
/// `### Re:` response block — while the user types a live prompt. This records
/// every such occurrence to `ops.log` and preserves the candidate buffer, so the
/// bug (which is not reliably reproducible) is captured the next time it happens
/// without any manual editor debug opt-in. Detection only: it never changes the
/// adoption decision — the guards above own that.
///
/// `candidate` must be the live editor buffer as received (capture it before the
/// guards replace `decision.snapshot_content`).
pub(crate) fn log_ipcfullprompt_corruption_if_any(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    candidate: &str,
) {
    // Scaffold duplication is a self-check on the candidate (the full-tail
    // duplication shape — two `<!-- /agent:exchange -->` markers — captured live in
    // brandon-cinquegrana.md), so it runs even when no baseline is available.
    let mut findings =
        agent_doc_document_realtime::ipc_corruption::detect_duplicated_scaffold(candidate);
    // Response-block delete/duplicate needs the prior committed baseline.
    if let Some(base) = baseline {
        findings.extend(
            agent_doc_document_realtime::ipc_corruption::detect_response_block_corruption(
                base, candidate,
            ),
        );
    }
    if findings.is_empty() {
        return;
    }
    let base = baseline.unwrap_or("");
    let summary = agent_doc_document_realtime::ipc_corruption::summarize_findings(&findings);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipcfullprompt_corruption_suspected file={} source={} patch_id={} candidate_len={} candidate_hash={} baseline_len={} baseline_hash={} {}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            candidate.len(),
            agent_doc_hash::content_hash(candidate),
            base.len(),
            agent_doc_hash::content_hash(base),
            summary,
        ),
    );
    let _ = agent_doc_ipc_forensics_io::preserve_ipcfullprompt_forensic(
        file, patch_id, base, candidate,
    );
}

pub(crate) fn materialize_missing_response_for_socket_ack_drift(
    file: &Path,
    patch_id: Option<&str>,
    content_ours: Option<&str>,
    expected_response: &str,
    drift_fired: bool,
    decision: &mut IpcRepairDecision,
) -> bool {
    if !drift_fired || decision.snap_source != IpcSnapshotSource::AckContentSidecar {
        return false;
    }
    // #ackdriftprefixmaterialize: a prefix-normalization / dedupe repair
    // (`PrefixDivergence`, `IpcDedupe`, `PrefixDivergenceThenIpcDedupe`) only
    // rewrites operator-visible prefixes or removes duplicate blocks — it never
    // restores the agent response. When such a repair leaves a drifted ACK
    // snapshot still missing the response, this missing-response materialization
    // is exactly the intended rescue, so those reasons must NOT gate it off. The
    // old blanket `disk_repair_reason.is_some()` bail dead-ended a stale
    // prefix-diverged ACK into a `retry_without_disk_write` spin until supervisor
    // supersession. Only a repair that already replaced the snapshot with a
    // response-bearing buffer (`LivePromptDrift`, which also sets
    // `snap_source == ContentOurs` and is excluded above) skips the rescue.
    if matches!(
        decision.disk_repair_reason,
        Some(IpcDiskRepairReason::LivePromptDrift)
    ) {
        return false;
    }
    let response = response_materialization_probe_from_response(expected_response);
    if response.trim().is_empty()
        || response_materialized_in_content(&response, &decision.snapshot_content)
    {
        return false;
    }
    let Some(ours) = content_ours else {
        return false;
    };
    if !response_materialized_in_content(&response, ours) {
        return false;
    }
    if first_response_heading(&response).is_some_and(|heading| {
        decision
            .snapshot_content
            .lines()
            .any(|line| line.trim() == heading)
    }) {
        return false;
    }
    let Some(repaired) =
        materialize_response_in_current_exchange(&decision.snapshot_content, &response)
    else {
        return false;
    };
    if repaired == decision.snapshot_content
        || !response_materialized_in_content(&response, &repaired)
    {
        return false;
    }

    let pre_materialize = decision.snapshot_content.clone();
    decision.snapshot_content = repaired;
    decision.disk_repair_reason = Some(IpcDiskRepairReason::LivePromptDrift);
    // Preserve an already-recorded editor bad state (e.g. from a prior prefix
    // repair whose normalized content was never delivered to the editor) so
    // redelivery still verifies against what the editor actually holds; only fall
    // back to the pre-materialize snapshot when none was recorded.
    if decision.editor_bad_state.is_none() {
        decision.editor_bad_state = Some(EditorBadStateFingerprint::new(pre_materialize));
    }
    decision.redeliver_editor = true;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_ack_drift_missing_response_materialized file={} patch_id={} repaired_len={} repaired_hash={} response_sha256={}",
            file.display(),
            patch_id.unwrap_or("-"),
            decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&decision.snapshot_content),
            agent_doc_hash::content_hash(&response),
        ),
    );
    true
}

struct StaleAckContentContext<'a> {
    file: &'a Path,
    source: &'a str,
    patch_id: Option<&'a str>,
    baseline: Option<&'a str>,
    content_ours: Option<&'a str>,
    expected_response: &'a str,
}

fn visible_content_supersedes_ack_content(
    context: &StaleAckContentContext<'_>,
    ack_content: &str,
    visible_content: &str,
) -> bool {
    if strip_boundary_for_dedup(ack_content) == strip_boundary_for_dedup(visible_content) {
        return false;
    }
    if context.expected_response.trim().is_empty() {
        return false;
    }
    let response_present =
        response_materialized_in_content(context.expected_response, visible_content)
            || match (context.baseline, context.content_ours) {
                (Some(base), Some(ours)) => {
                    response_already_in_current(base, ours, visible_content)
                }
                _ => false,
            };
    if !response_present {
        return false;
    }
    let prompt_drift = match (context.baseline, context.content_ours) {
        (Some(base), Some(ours)) => {
            ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(base, visible_content, ours)
        }
        _ => false,
    };
    agent_doc_ops_log_io::log_op(
        context.file,
        &format!(
            "{source}_ack_content_stale_visible_adopted file={} patch_id={} visible_len={} visible_hash={} ack_len={} ack_hash={} response_present=true prompt_drift={}",
            context.file.display(),
            context.patch_id.unwrap_or("-"),
            visible_content.len(),
            agent_doc_hash::content_hash(visible_content),
            ack_content.len(),
            agent_doc_hash::content_hash(ack_content),
            prompt_drift,
            source = context.source
        ),
    );
    true
}

pub(crate) fn prefer_visible_content_over_stale_ack_content(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    expected_response: &str,
    decision: &mut IpcRepairDecision,
) -> bool {
    if decision.snap_source != IpcSnapshotSource::AckContentSidecar {
        return false;
    }
    if decision.disk_repair_reason.is_some() {
        return false;
    }
    let Ok(visible_content) = std::fs::read_to_string(file) else {
        return false;
    };
    let context = StaleAckContentContext {
        file,
        source,
        patch_id,
        baseline,
        content_ours,
        expected_response,
    };
    if !visible_content_supersedes_ack_content(
        &context,
        &decision.snapshot_content,
        &visible_content,
    ) {
        return false;
    }
    *decision = IpcRepairDecision::file_read(visible_content);
    true
}

pub(crate) fn persist_already_applied_socket_content_ours_snapshot(
    file: &Path,
    patch_id: &str,
    editor_id: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
    expected_response: &str,
) -> Result<AlreadyAppliedSnapshotOutcome> {
    let Some(ours) = content_ours else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_socket_already_applied_no_content_ours_snapshot file={} patch_id={}",
                file.display(),
                patch_id
            ),
        );
        return Ok(AlreadyAppliedSnapshotOutcome::Persisted);
    };

    let ack_content = if !patch_id.is_empty() {
        file.canonicalize().ok().and_then(|canonical| {
            let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
            poll_ack_content_sidecar(
                &project_root,
                patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )
            .ok()
            .flatten()
        })
    } else {
        None
    };
    let current_source = if ack_content.is_some() {
        IpcSnapshotSource::AckContentSidecar
    } else {
        IpcSnapshotSource::FileRead
    };
    let current = ack_content.or_else(|| std::fs::read_to_string(file).ok());
    let mut repair_decision = IpcRepairDecision::content_ours(ours.to_string());
    if let Some(current) = current.as_deref()
        && strip_boundary_for_dedup(current) != strip_boundary_for_dedup(ours)
    {
        let response_present = response_materialized_in_content(expected_response, current)
            || baseline.is_some_and(|base| response_already_in_current(base, ours, current));
        let prompt_drift = baseline.is_some_and(|base| {
            ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(base, current, ours)
        });
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_socket_already_applied_live_buffer_diverged file={} patch_id={} response_present={} current_len={} current_hash={} content_ours_len={} content_ours_hash={} prompt_drift={}",
                file.display(),
                patch_id,
                response_present,
                current.len(),
                agent_doc_hash::content_hash(current),
                ours.len(),
                agent_doc_hash::content_hash(ours),
                prompt_drift
            ),
        );
        // #6cmx/#wy0y verification marker: an explicit, greppable record that the
        // operator typed into the document while finalize was writing (the live
        // buffer diverged from our content with prompt drift). `typed_delta_bytes`
        // is the live-vs-ours byte delta (their keystrokes); `response_present`
        // confirms the assistant response is still materialized in the buffer, so
        // grepping `finalize_typing_during_write` verifies a typing-during-finalize
        // run was exercised and whether the response survived intact.
        if prompt_drift {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "finalize_typing_during_write file={} patch_id={} typed_delta_bytes={} response_present={} resolution=content_ours_adopted",
                    file.display(),
                    patch_id,
                    current.len() as i64 - ours.len() as i64,
                    response_present
                ),
            );
        }

        if !response_present {
            if let Some(repaired_current) =
                materialize_response_in_current_exchange(current, expected_response)
            {
                log_ipc_proof_failure(
                    file,
                    "socket_already_applied",
                    Some(patch_id),
                    "disk_missing_response_probe",
                    "content_ours_snapshot_visible_response_repair",
                    &format!(
                        "response_sha256={} current_len={} current_hash={} repaired_len={} repaired_hash={}",
                        agent_doc_hash::content_hash(expected_response),
                        current.len(),
                        agent_doc_hash::content_hash(current),
                        repaired_current.len(),
                        agent_doc_hash::content_hash(&repaired_current)
                    ),
                );
                // #stale-already-applied — the visible-buffer repair is only safe
                // when the buffer is idle and matches `current`. If the guard
                // defers (buffer still changing) do NOT hard-error finalize: that
                // wedges the cycle at `response_captured` and can leave a scrambled
                // partial write on disk (a stale/dead IPC endpoint — e.g. an exited
                // `sync` process or a 0-byte socket — that keeps oscillating a
                // phantom buffer never goes idle, so "retry after typing stops"
                // never converges). Fall back to file-IPC, which writes the
                // response from the authoritative base instead of a phantom buffer.
                // A real live editor mid-type also lands here and is served by the
                // file-IPC transport / retry rather than a hard failure.
                if let Err(defer) = guard_visible_write_idle_and_current(
                    file,
                    "socket_already_applied_missing_disk_response",
                    current,
                ) {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "ipc_socket_already_applied_visible_not_idle_file_fallback file={} patch_id={} reason={}",
                            file.display(),
                            patch_id,
                            defer.to_string().replace('\n', " ")
                        ),
                    );
                    return Ok(AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
                }
                atomic_write_pub(file, &repaired_current)?;
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_missing_disk_response_repaired file={} patch_id={} visible_len={} visible_hash={} content_ours_len={} content_ours_hash={}",
                        file.display(),
                        patch_id,
                        repaired_current.len(),
                        agent_doc_hash::content_hash(&repaired_current),
                        ours.len(),
                        agent_doc_hash::content_hash(ours)
                    ),
                );
                repair_decision = IpcRepairDecision::file_read(repaired_current);
            } else {
                log_ipc_proof_failure(
                    file,
                    "socket_already_applied",
                    Some(patch_id),
                    "disk_missing_response_probe",
                    "file_ipc_fallback",
                    &format!(
                        "response_sha256={} current_len={} current_hash={}",
                        agent_doc_hash::content_hash(expected_response),
                        current.len(),
                        agent_doc_hash::content_hash(current)
                    ),
                );
                return Ok(AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
            }
        } else {
            repair_decision = match current_source {
                IpcSnapshotSource::AckContentSidecar => {
                    IpcRepairDecision::ack_content(current.to_string())
                }
                IpcSnapshotSource::FileRead | IpcSnapshotSource::ContentOurs => {
                    IpcRepairDecision::file_read(current.to_string())
                }
            };
            if let Some(lines) = normalize_prefix_lines
                && !lines.is_empty()
            {
                let normalized = normalize_exchange_prefixes_for_targets(
                    &repair_decision.snapshot_content,
                    lines,
                );
                if normalized != repair_decision.snapshot_content {
                    repair_decision = IpcRepairDecision::file_read_prefix_repair(
                        normalized,
                        current.to_string(),
                        lines,
                    );
                }
            }

            let before_response_dedupe = repair_decision.snapshot_content.clone();
            let response_deduped =
                dedupe_consecutive_response_blocks(&repair_decision.snapshot_content, file);
            if response_deduped != repair_decision.snapshot_content {
                repair_decision =
                    repair_decision.apply_ipc_dedupe(response_deduped, before_response_dedupe);
            }

            let pre_dedupe_snap = repair_decision.snapshot_content.clone();
            let (effective_snap, dedupe_repair) = dedupe_ipc_snapshot_content(
                file,
                baseline,
                &repair_decision.snapshot_content,
                "socket_already_applied_disk",
            )?;
            if dedupe_repair {
                repair_decision = repair_decision.apply_ipc_dedupe(effective_snap, pre_dedupe_snap);
            } else {
                repair_decision.snapshot_content = effective_snap;
            }
        }
    } else if let Some(current) = current.as_deref() {
        repair_decision = match current_source {
            IpcSnapshotSource::AckContentSidecar => {
                IpcRepairDecision::ack_content(current.to_string())
            }
            IpcSnapshotSource::FileRead | IpcSnapshotSource::ContentOurs => {
                IpcRepairDecision::file_read(current.to_string())
            }
        };
    }

    prefer_visible_content_over_stale_ack_content(
        file,
        "already_applied",
        Some(patch_id),
        baseline,
        Some(ours),
        expected_response,
        &mut repair_decision,
    );

    if expected_response.trim().is_empty() {
        log_ipc_proof_failure(
            file,
            "socket_already_applied",
            Some(patch_id),
            "already_applied_empty_response_probe",
            "file_ipc_fallback",
            &format!(
                "snapshot_len={} snapshot_hash={} content_ours_len={} content_ours_hash={}",
                repair_decision.snapshot_content.len(),
                agent_doc_hash::content_hash(&repair_decision.snapshot_content),
                ours.len(),
                agent_doc_hash::content_hash(ours)
            ),
        );
        return Ok(AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
    }

    if repair_decision.snap_source == IpcSnapshotSource::ContentOurs {
        log_ipc_proof_failure(
            file,
            "socket_already_applied",
            Some(patch_id),
            "already_applied_unproven_content_ours",
            "file_ipc_fallback",
            &format!(
                "snapshot_len={} snapshot_hash={} content_ours_len={} content_ours_hash={}",
                repair_decision.snapshot_content.len(),
                agent_doc_hash::content_hash(&repair_decision.snapshot_content),
                ours.len(),
                agent_doc_hash::content_hash(ours)
            ),
        );
        return Ok(AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
    }

    let response_present_in_snapshot =
        response_materialized_in_content(expected_response, &repair_decision.snapshot_content)
            || baseline.is_some_and(|base| {
                response_already_in_current(base, ours, &repair_decision.snapshot_content)
            });
    if !response_present_in_snapshot {
        log_ipc_proof_failure(
            file,
            "socket_already_applied",
            Some(patch_id),
            "already_applied_snapshot_missing_response",
            "file_ipc_fallback",
            &format!(
                "response_sha256={} snapshot_len={} snapshot_hash={} content_ours_len={} content_ours_hash={}",
                agent_doc_hash::content_hash(expected_response),
                repair_decision.snapshot_content.len(),
                agent_doc_hash::content_hash(&repair_decision.snapshot_content),
                ours.len(),
                agent_doc_hash::content_hash(ours)
            ),
        );
        return Ok(AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
    }

    repair_ipc_decision_visible_state(file, &repair_decision, Some(patch_id))?;
    if repair_decision.snap_source.is_ack_content_proven() {
        let proof =
            ack_content_disk_write_proof(file, editor_id, &repair_decision.snapshot_content);
        let disk_synced = write_ack_content_through_to_disk(
            &super::WRITE_CONVERGENCE_EFFECTS,
            file,
            patch_id,
            &repair_decision.snapshot_content,
            proof,
        )?;
        if !disk_synced {
            return Ok(AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
        }
        mark_ack_content_live_buffer_synced_after_write(
            file,
            patch_id,
            editor_id,
            &repair_decision.snapshot_content,
        );
    }
    agent_doc_snapshot_io::save(
        file,
        &repair_decision.snapshot_content,
        agent_doc_ops_log_io::log_op,
    )?;
    let crdt_doc = agent_doc_merge::crdt::CrdtDoc::from_text(&repair_decision.snapshot_content);
    agent_doc_merge_io::save_document_crdt(
        file,
        &crdt_doc.encode_state(),
        &repair_decision.snapshot_content,
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_already_applied_snapshot file={} patch_id={} snap_source={} snap_len={} snap_hash={}",
            file.display(),
            patch_id,
            repair_decision.snap_source.label(),
            repair_decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&repair_decision.snapshot_content)
        ),
    );
    Ok(AlreadyAppliedSnapshotOutcome::Persisted)
}

pub(crate) fn redeliver_full_content_repair_to_editor(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    kind: FullContentRepairRedelivery,
    source_patch_id: Option<&str>,
) -> bool {
    let current_content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "[write] WARNING: {} editor repair skipped because {} could not be read: {}",
                kind.label(),
                file.display(),
                e
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{}_editor_redelivery_skipped file={} patch_id={} skip=read_failed error={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    e
                ),
            );
            return false;
        }
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{}_editor_redelivery_proof file={} patch_id={} proof_source=bad_editor_state expected_len={} expected_hash={} current_len={} current_hash={} redeliver={}",
            kind.label(),
            file.display(),
            source_patch_id.unwrap_or("-"),
            expected_bad_state.len(),
            agent_doc_hash::content_hash(expected_bad_state),
            current_content.len(),
            agent_doc_hash::content_hash(&current_content),
            current_content == expected_bad_state
        ),
    );
    let source_buffer_matches = current_content == expected_bad_state;
    let authority = if source_buffer_matches
        && agent_doc_write_converge_io::redelivery_missing_operator_text_authority(
            file,
            expected_bad_state,
            kind.label(),
            source_patch_id,
        ) {
        WholeBufferAuthority::None
    } else {
        WholeBufferAuthority::OperatorTextAuthority
    };
    let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
        delivery: WholeBufferDelivery::EditorRepairRedelivery,
        authority,
        source_buffer_matches,
        scope_rejection: None,
        enabled: true,
    });
    if decision.action != WholeBufferDeliveryAction::Apply {
        if !source_buffer_matches {
            eprintln!(
                "[write] {} editor repair skipped: visible buffer no longer matches the bad state",
                kind.label()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{}_editor_redelivery_skipped file={} patch_id={} skip=stale_bad_state expected_len={} expected_hash={} current_len={} current_hash={} table_reason={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    expected_bad_state.len(),
                    agent_doc_hash::content_hash(expected_bad_state),
                    current_content.len(),
                    agent_doc_hash::content_hash(&current_content),
                    decision.reason
                ),
            );
        } else if decision.reason != "missing_operator_text_authority" {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{}_editor_redelivery_skipped file={} patch_id={} skip=authority_table action={} reason={} authority={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    decision.action.as_str(),
                    decision.reason,
                    authority.as_str()
                ),
            );
        }
        return false;
    }

    // #clearexchstale / #dvre: the disk check above proves DISK still matches the
    // bad state, but the live editor buffer can diverge from disk while disk lags
    // (the plugin applies edits to the in-memory Document first). When the operator
    // has freshly edited the buffer — e.g. cleared the exchange or deleted a typed
    // prompt — redelivering the stale snapshot over it REVIVES the deleted content.
    // Fail closed on a proven live-buffer divergence from the bad state: the
    // operator's unsaved edits win over a stale repair. `live_buffer_diverges_from_content`
    // is provenance-aware (it ignores a buffer digest that merely lags a newer disk
    // write), so this only suppresses genuine unsaved editor edits ahead of disk.
    let indicator_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    if let Some(live) =
        agent_doc_debounce::live_buffer_diverges_from_content(&indicator_path, expected_bad_state)
    {
        eprintln!(
            "[write] {} editor repair skipped: live editor buffer has unsaved edits ahead of the bad state",
            kind.label()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{}_editor_redelivery_skipped file={} patch_id={} skip=live_buffer_diverges expected_len={} expected_hash={} live_len={} live_hash={}",
                kind.label(),
                file.display(),
                source_patch_id.unwrap_or("-"),
                expected_bad_state.len(),
                agent_doc_hash::content_hash(expected_bad_state),
                live.len,
                live.hash
            ),
        );
        return false;
    }

    match try_ipc_full_content_response_fallback_from_source(
        file,
        repaired_content,
        expected_bad_state,
    ) {
        Ok(true) => {
            eprintln!("{}", kind.success_message());
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{}_redelivered_editor file={} patch_id={} bytes={} expected_bad_len={} expected_bad_hash={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    repaired_content.len(),
                    expected_bad_state.len(),
                    agent_doc_hash::content_hash(expected_bad_state)
                ),
            );
            true
        }
        Ok(false) => {
            eprintln!("{}", kind.not_consumed_message());
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{}_editor_repair_not_consumed file={} patch_id={} bytes={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    repaired_content.len()
                ),
            );
            false
        }
        Err(e) => {
            eprintln!("{}", kind.failed_message(&e));
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{}_editor_repair_failed file={} patch_id={} error={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    e
                ),
            );
            false
        }
    }
}

#[cfg(test)]
pub(crate) fn redeliver_ipc_dedupe_to_editor(
    file: &Path,
    content: &str,
    expected_bad_state: &str,
) -> bool {
    redeliver_full_content_repair_to_editor(
        file,
        content,
        expected_bad_state,
        FullContentRepairRedelivery::IpcDedupe,
        None,
    )
}

pub(crate) fn repair_ipc_decision_visible_state(
    file: &Path,
    decision: &IpcRepairDecision,
    patch_id: Option<&str>,
) -> Result<()> {
    let Some(reason) = decision.disk_repair_reason else {
        return Ok(());
    };
    let bad_len = decision
        .editor_bad_state
        .as_ref()
        .map(|state| state.len)
        .unwrap_or(0);
    let bad_hash = decision
        .editor_bad_state
        .as_ref()
        .map(|state| state.hash.as_str())
        .unwrap_or("-");
    let current = std::fs::read_to_string(file).ok();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_repair_decision file={} patch_id={} snap_source={} repair_reason={} redeliver_editor={} bad_len={} bad_hash={} repaired_len={} repaired_hash={} current_len={} current_hash={} normalize_targets={} duplicate_prompt_count={}",
            file.display(),
            patch_id.unwrap_or("-"),
            decision.snap_source.label(),
            reason.label(),
            decision.redeliver_editor,
            bad_len,
            bad_hash,
            decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&decision.snapshot_content),
            current.as_deref().map(str::len).unwrap_or(0),
            current
                .as_deref()
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            decision.normalize_prefix_lines.len(),
            duplicate_prompt_line_count(
                decision
                    .editor_bad_state
                    .as_ref()
                    .map(EditorBadStateFingerprint::content)
                    .unwrap_or(&decision.snapshot_content)
            )
        ),
    );

    if decision.redeliver_editor
        && let Some(expected_bad_state) = decision.editor_bad_state.as_ref()
        && match reason {
            IpcDiskRepairReason::PrefixDivergence => {
                agent_doc_write_converge_io::redeliver_normalization_fallback_to_editor(
                    file,
                    &decision.snapshot_content,
                    expected_bad_state.content(),
                    &decision.normalize_prefix_lines,
                    patch_id,
                    |file, repaired_content, expected_bad_state, source_patch_id| {
                        redeliver_full_content_repair_to_editor(
                            file,
                            repaired_content,
                            expected_bad_state,
                            FullContentRepairRedelivery::NormalizationFallback,
                            source_patch_id,
                        )
                    },
                )
            }
            IpcDiskRepairReason::IpcDedupe
            | IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe
            | IpcDiskRepairReason::LivePromptDrift => redeliver_full_content_repair_to_editor(
                file,
                &decision.snapshot_content,
                expected_bad_state.content(),
                reason.redelivery_kind(),
                patch_id,
            ),
        }
    {
        return Ok(());
    }

    // #ipcvisredeliver-incycle: the disk-keyed redelivery above requires the
    // on-disk buffer to still equal the bad editor state. Under a live
    // JetBrains/VSCode editor the disk LAGS the in-memory buffer (the plugin
    // applies edits to the Document first), so a `live_prompt_drift` repaired
    // target — a merge of the operator buffer + the agent response — is skipped
    // as `stale_bad_state` on every finalize and only converges on a LATER
    // cycle's commit path via `try_auto_recover_live_prompt_drift` (which keys
    // off the editor IPC listener, not disk). That is the recurring
    // `ipc_proof_insufficient` / `retry_without_disk_write` churn: several
    // finalizes each fail before the commit-path auto-recovery lands.
    //
    // Collapse the churn: when an editor IPC listener is active, attempt the
    // SAME editor-IPC convergence in-cycle now, keyed off the live buffer rather
    // than disk. `try_auto_recover_live_prompt_drift` reuses the proven-safe
    // convergence (dropped-prompt guard, `live_prompt_drift_auto_recovery_safe`,
    // editor-wins-outside-response reconciliation) and, with a listener active,
    // NEVER takes its disk-fallback branch — it either proves the editor
    // converged (returns `Some`) or fails closed (returns `None`). On success
    // the visible editor state is proven and this cycle can commit; on
    // `None`/error we fall through to the existing fail-closed bail below, so the
    // no-disk-write invariant is preserved unchanged.
    if matches!(reason, IpcDiskRepairReason::LivePromptDrift) {
        let listener_active = file
            .canonicalize()
            .ok()
            .map(|canonical| agent_doc_project_root_io::resolve_ipc_project_root(&canonical))
            .as_deref()
            .map(agent_doc_ipc_io::is_listener_active)
            .unwrap_or(false);
        if listener_active
            && let Ok(file_content) = std::fs::read_to_string(file)
            && let Ok(Some(_recovered)) =
                agent_doc_write_converge_io::try_auto_recover_live_prompt_drift(
                    &super::WRITE_CONVERGENCE_EFFECTS,
                    file,
                    &decision.snapshot_content,
                    &file_content,
                )
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "ipc_visible_repair_incycle_editor_converged file={} patch_id={} repair_reason={} redeliver_editor={} bad_len={} bad_hash={} repaired_len={} repaired_hash={} transport=editor_ipc",
                    file.display(),
                    patch_id.unwrap_or("-"),
                    reason.label(),
                    decision.redeliver_editor,
                    bad_len,
                    bad_hash,
                    decision.snapshot_content.len(),
                    agent_doc_hash::content_hash(&decision.snapshot_content),
                ),
            );
            return Ok(());
        }
    }

    log_ipc_proof_failure(
        file,
        "ipc_visible_repair",
        patch_id,
        reason.label(),
        "retry_without_disk_write",
        &format!(
            "redeliver_editor={} bad_len={} bad_hash={} repaired_len={} repaired_hash={}",
            decision.redeliver_editor,
            bad_len,
            bad_hash,
            decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&decision.snapshot_content)
        ),
    );
    let detail = format!(
        "redeliver_editor={} bad_len={} bad_hash={} repaired_len={} repaired_hash={}",
        decision.redeliver_editor,
        bad_len,
        bad_hash,
        decision.snapshot_content.len(),
        agent_doc_hash::content_hash(&decision.snapshot_content)
    );
    if let Err(err) = agent_doc_cycle_state_io::record_editor_convergence_required(
        file,
        "ipc_visible_repair",
        reason.label(),
        patch_id,
        Some(&detail),
    ) {
        eprintln!(
            "[write] WARNING: failed to record IPC repair blocked closeout for {}: {err}",
            file.display()
        );
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_visible_repair_retry_required_no_disk_write file={} patch_id={} repair_reason={} recovery=retry_without_disk_write",
            file.display(),
            patch_id.unwrap_or("-"),
            reason.label()
        ),
    );
    anyhow::bail!(
        "editor IPC repair did not prove visible state for {}; pending response retained for retry; refusing direct document write",
        file.display()
    );
}

pub fn dedupe_ipc_snapshot_content(
    file: &Path,
    before: Option<&str>,
    content: &str,
    source: &str,
) -> Result<(String, bool)> {
    let singleton_repair = repair_duplicate_singleton_components(before, content);
    let singleton_changed = singleton_repair.is_some();
    let singleton_repaired = singleton_repair
        .as_ref()
        .map(|repair| repair.content.as_str())
        .unwrap_or(content);
    let (deduped, report) =
        agent_doc_element_exchange_io::repair_duplicate_prompt_artifacts_with_log(
            singleton_repaired,
            file,
            DuplicatePromptRepairOptions::new(source)
                .with_before(before)
                .preserving(before),
            agent_doc_ops_log_io::log_op,
            log_duplicate_prompt_residue_guard,
        )?;
    let changed = singleton_changed || deduped != content;
    if let Some(repair) = &singleton_repair {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "duplicate_singleton_component_repaired file={} source={} groups={} removed={} canonical_source=before before_commit=true",
                file.display(),
                source,
                repair.groups.join(","),
                repair.removed
            ),
        );
    }
    if singleton_changed {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_snapshot_singleton_components_deduped file={} source={} before_commit=true",
                file.display(),
                source
            ),
        );
    }
    if singleton_changed || report.changed() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_snapshot_deduped file={} source={} before_commit=true",
                file.display(),
                source
            ),
        );
    }
    Ok((deduped, changed))
}

fn stale_supervisor_content_ours_adoption_warning(file: &Path) -> Option<String> {
    #[cfg(test)]
    if TEST_FORCE_STALE_SUPERVISOR_CONTENT_OURS_REFUSAL.with(|flag| flag.get()) {
        return Some("test supervisor_binary_stale".to_string());
    }

    #[cfg(test)]
    {
        let _ = file;
        None
    }

    #[cfg(not(test))]
    agent_doc_controller_io::project_controller::stale_supervisor_warning_for_doc(file)
}

fn log_content_ours_adoption_refused_stale_supervisor(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    guard: &str,
    content_ours: &str,
    stale_message: &str,
) {
    let stale_message = stale_message.replace('\n', " ");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "content_ours_adoption_refused_stale_supervisor file={} source={} patch_id={} guard={} reason=supervisor_binary_stale content_ours_len={} content_ours_hash={} warning={:?}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            guard,
            content_ours.len(),
            agent_doc_hash::content_hash(content_ours),
            stale_message
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        patch_id,
        "supervisor_binary_stale",
        "candidate_snapshot_kept",
        &format!(
            "guard={} content_ours_len={} content_ours_hash={}",
            guard,
            content_ours.len(),
            agent_doc_hash::content_hash(content_ours)
        ),
    );
}

#[cfg(test)]
thread_local! {
    static TEST_FORCE_STALE_SUPERVISOR_CONTENT_OURS_REFUSAL: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn with_test_stale_supervisor_content_ours_refusal<T>(f: impl FnOnce() -> T) -> T {
    TEST_FORCE_STALE_SUPERVISOR_CONTENT_OURS_REFUSAL.with(|flag| flag.set(true));
    let result = f();
    TEST_FORCE_STALE_SUPERVISOR_CONTENT_OURS_REFUSAL.with(|flag| flag.set(false));
    result
}

/// Result of an IPC write attempt, including the patch_id used.
///
/// The `patch_id` is returned so callers can report/retry the same logical
/// response — the plugin tracks applied patch_ids and skips duplicates,
/// preventing double-apply when both socket and file IPC fire.
#[derive(Debug)]
pub struct IpcResult {
    /// Whether the plugin successfully consumed the patch.
    pub success: bool,
    /// The patch_id used for this write attempt. Reuse in fallback writes
    /// so the plugin can deduplicate.
    pub patch_id: String,
    /// True when IPC was intentionally skipped because the current cycle has
    /// already reached the terminal committed state.
    pub skipped_committed_cycle: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIpcRepositionResult {
    Queued,
    DeferredExistingPatch,
    Unavailable,
}

/// Remove leftover fallback patch files for a document after closeout commits.
/// Prevents late file-watcher or plugin recovery from re-applying a stale patch
/// to an already-committed document.
pub fn cleanup_fallback_patch_files(file: &Path) {
    agent_doc_flow_io::closeout::cleanup_fallback_patch_files(file);
}

/// Check if the current cycle for `file` is already in Committed phase.
/// Returns `Some(cycle_id)` if committed, `None` if no cycle or cycle is open.
pub(crate) fn cycle_already_committed(file: &Path) -> Option<String> {
    agent_doc_flow_io::closeout::cycle_already_committed(file)
}

pub(crate) fn write_claimed_patch_sentinel(project_root: &Path, patch_id: &str) {
    agent_doc_flow_io::closeout::write_claimed_patch_sentinel(project_root, patch_id);
}

mod transport;
pub use transport::*;

#[cfg(test)]
mod ack_content_snapshot_tests {
    use super::*;
    use tempfile::TempDir;

    fn redeliver_normalization_fallback_for_test(
        file: &Path,
        repaired_content: &str,
        expected_bad_state: &str,
        normalize_prefix_lines: &[String],
        source_patch_id: Option<&str>,
    ) -> bool {
        agent_doc_write_converge_io::redeliver_normalization_fallback_to_editor(
            file,
            repaired_content,
            expected_bad_state,
            normalize_prefix_lines,
            source_patch_id,
            |file, repaired_content, expected_bad_state, source_patch_id| {
                redeliver_full_content_repair_to_editor(
                    file,
                    repaired_content,
                    expected_bad_state,
                    FullContentRepairRedelivery::NormalizationFallback,
                    source_patch_id,
                )
            },
        )
    }

    fn wait_until_typing_indicator_active(file: &str) {
        for _ in 0..100 {
            if agent_doc_debounce::is_typing_via_file(file, 2_000) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("typing indicator did not become active for {file}");
    }

    #[test]
    fn test_ack_content_sidecar_read() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "test-patch-abc123";

        let ack_dir = project_root.join(".agent-doc/ack-content");
        std::fs::create_dir_all(&ack_dir).unwrap();
        let sidecar = ack_dir.join(format!("{patch_id}.md"));
        std::fs::write(&sidecar, "applied content from plugin").unwrap();

        let result = read_ack_content_sidecar(&project_root, patch_id).unwrap();
        assert_eq!(result, Some("applied content from plugin".to_string()));
        assert!(!sidecar.exists(), "sidecar should be deleted after read");
    }

    #[test]
    fn ack_content_disk_write_proof_waits_for_typing_indicator_before_trusting_live_buffer() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/typing")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let editor_id = "jetbrains:test";
        let ack_content = "before\n### Re: done\n";
        let newer_editor_content = "before\n### Re: done\noperator typed after ack\n";

        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            ack_content,
            editor_id,
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();
        agent_doc_debounce::document_changed(&doc_str);
        wait_until_typing_indicator_active(&doc_str);

        let updater_doc = doc_str.clone();
        let updater = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(25));
            agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                &updater_doc,
                newer_editor_content,
                editor_id,
                "jetbrains",
                "test",
                &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
            )
            .unwrap();
        });

        let proof = ack_content_disk_write_proof(&doc, Some(editor_id), ack_content);
        updater.join().unwrap();

        assert_eq!(proof.authority, WholeBufferAuthority::OperatorTextAuthority);
        assert!(
            !proof.source_buffer_matches,
            "ACK-content must not remain authoritative after active typing publishes a newer editor buffer"
        );
        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ack_content_disk_write_proof_typing_settle"),
            "typing-settle proof should be auditable:\n{log}"
        );
    }

    #[test]
    fn reconcile_ack_snapshot_forward_adopts_newer_operator_buffer_presenting_response() {
        // #adoc-live-prompt-drift-operator-edit: the ack snapshot the closeout is
        // about to persist is stale (agent's exact body) relative to the operator's
        // newer live buffer (edited body, response still present). Reconcile forward
        // to the newer operator-authoritative buffer instead of wedging.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let editor_id = "jetbrains:test";
        let stale_ack = "<!-- agent:exchange -->\n❯ fold it in\n### Re: folded — opus\n\nAgent exact body.\n<!-- /agent:exchange -->\n";
        let newer = "<!-- agent:exchange -->\n❯ fold it in\n### Re: folded — opus\n\nOperator reworded body, happy 4th.\n<!-- /agent:exchange -->\n";

        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            newer,
            editor_id,
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let mut decision = IpcRepairDecision::ack_content(stale_ack.to_string());
        let reconciled =
            reconcile_ack_snapshot_to_newer_operator_buffer(&doc, Some(editor_id), &mut decision);

        assert!(
            reconciled,
            "newer operator buffer presenting the response must reconcile forward"
        );
        assert_eq!(
            decision.snapshot_content, newer,
            "snapshot must adopt the operator's newer buffer so disk/snapshot/CRDT stay consistent"
        );
        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ack_content_snapshot_reconciled_forward"),
            "forward reconcile must be auditable:\n{log}"
        );
    }

    #[test]
    fn reconcile_ack_snapshot_forward_declines_when_newer_buffer_dropped_the_response() {
        // Fail closed: the newer buffer no longer carries the response, so the stale
        // ack must NOT reconcile forward (the existing proof stays authoritative).
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/live-buffer")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "before\n").unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let editor_id = "jetbrains:test";
        let stale_ack = "<!-- agent:exchange -->\n❯ fold it in\n### Re: folded — opus\n\nAgent exact body.\n<!-- /agent:exchange -->\n";
        let newer_without_response =
            "<!-- agent:exchange -->\n❯ fold it in\n<!-- /agent:exchange -->\n";

        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            newer_without_response,
            editor_id,
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let mut decision = IpcRepairDecision::ack_content(stale_ack.to_string());
        let reconciled =
            reconcile_ack_snapshot_to_newer_operator_buffer(&doc, Some(editor_id), &mut decision);

        assert!(
            !reconciled,
            "must not reconcile forward when the response was dropped"
        );
        assert_eq!(
            decision.snapshot_content, stale_ack,
            "snapshot unchanged on decline"
        );
    }

    // --- #dupcontent: structurally-corrupt content_ours is never adopted ---

    const DC_BASELINE: &str = "<!-- agent:status -->\nA\n<!-- /agent:status -->\n<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n";
    const DC_CANDIDATE: &str = "<!-- agent:status -->\nB\n<!-- /agent:status -->\n<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n";

    #[test]
    fn guard_refuses_structurally_corrupt_content_ours() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        // The live #dupcontent corruption: two agent:queue blocks ingested from
        // a bad CRDT merge — must never become the snapshot base.
        let corrupt_ours = "<!-- agent:status -->\nC\n<!-- /agent:status -->\n\
<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n\
<!-- agent:queue -->\n- a\n<!-- /agent:queue -->\n\
<!-- agent:queue -->\n- b\n<!-- /agent:queue -->\n";
        let mut decision = IpcRepairDecision::file_read(DC_CANDIDATE.to_string());
        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("p1"),
            Some(DC_BASELINE),
            Some(corrupt_ours),
            &mut decision,
        );
        assert!(!adopted, "corrupt content_ours must be refused");
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::FileRead,
            "decision must keep the clean candidate, not adopt the corrupt buffer"
        );
        assert_eq!(
            decision.snapshot_content, DC_CANDIDATE,
            "snapshot content must remain the clean candidate"
        );
    }

    #[test]
    fn guard_keeps_live_ack_candidate_when_agent_target_would_absorb_drift() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let baseline = concat!("<!-- agent:exchange -->\n", "<!-- /agent:exchange -->\n");
        let live_ack_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ operator typed while closeout was running\n",
            "<!-- /agent:exchange -->\n"
        );
        let agent_target = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: queued prompt — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision = IpcRepairDecision::ack_content(live_ack_content.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "socket_ack_content",
            Some("p-live-no-snapshot"),
            Some(baseline),
            Some(agent_target),
            &mut decision,
        );

        assert!(
            adopted,
            "live prompt drift should still be classified and logged"
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::AckContentSidecar,
            "the binary agent target must not replace the live editor ACK as snapshot authority"
        );
        assert_eq!(decision.snapshot_content, live_ack_content);
        assert_eq!(decision.disk_repair_reason, None);
        assert!(!decision.redeliver_editor);
    }

    #[test]
    fn guard_live_prompt_drift_requires_visible_repair() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let baseline = concat!("<!-- agent:exchange -->\n", "<!-- /agent:exchange -->\n");
        let editor_ack_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ operator typed while closeout was running\n",
            "<!-- /agent:exchange -->\n"
        );
        let response_target = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: queued prompt — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision = IpcRepairDecision::ack_content(editor_ack_content.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "socket_ack_content",
            Some("p-live"),
            Some(baseline),
            Some(response_target),
            &mut decision,
        );

        assert!(
            adopted,
            "live editor ACK drift should be classified and logged"
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::AckContentSidecar,
            "the turn path must not promote the agent target to snapshot authority without realtime proof"
        );
        assert_eq!(decision.snapshot_content, editor_ack_content);
        assert!(
            !decision.redeliver_editor,
            "the turn path must fail closed/retry rather than repair the live editor from the agent target"
        );
        assert_eq!(decision.disk_repair_reason, None);
        assert_eq!(decision.editor_bad_state, None);
    }

    #[test]
    fn guard_live_prompt_drift_accepts_ack_visible_union() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let baseline = concat!("<!-- agent:exchange -->\n", "<!-- /agent:exchange -->\n");
        let response_target = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: queued prompt — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let editor_ack_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ operator typed while closeout was running\n",
            "### Re: queued prompt — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision = IpcRepairDecision::ack_content(editor_ack_content.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "socket_ack_content",
            Some("p-union"),
            Some(baseline),
            Some(response_target),
            &mut decision,
        );

        assert!(adopted, "ACK-visible union should be classified and logged");
        assert_eq!(decision.snap_source, IpcSnapshotSource::AckContentSidecar);
        assert_eq!(decision.snapshot_content, editor_ack_content);
        assert!(
            !decision.redeliver_editor,
            "ACK content already contains the response delta, so no turn-local repair is required"
        );
        assert_eq!(decision.disk_repair_reason, None);
        assert_eq!(decision.editor_bad_state, None);
    }

    #[test]
    fn socket_ack_drift_missing_response_materializes_only_with_content_ours_proof() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let ack_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = "### Re: queued prompt - gpt-5\n\nAnswered.\n";
        let content_ours = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "### Re: queued prompt - gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision = IpcRepairDecision::ack_content(ack_content.to_string());

        let repaired = materialize_missing_response_for_socket_ack_drift(
            &file,
            Some("p-response"),
            Some(content_ours),
            response,
            true,
            &mut decision,
        );

        assert!(
            repaired,
            "socket ACK drift should repair only when content_ours proves the exact response"
        );
        assert_eq!(decision.snap_source, IpcSnapshotSource::AckContentSidecar);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::LivePromptDrift)
        );
        assert!(decision.redeliver_editor);
        assert_eq!(
            decision
                .editor_bad_state
                .as_ref()
                .expect("bad state fingerprint")
                .content(),
            ack_content
        );
        assert!(decision.snapshot_content.contains("❯ queued prompt"));
        assert!(
            response_materialized_in_content(response, &decision.snapshot_content),
            "repaired snapshot must contain the exact response body:\n{}",
            decision.snapshot_content
        );
    }

    #[test]
    fn socket_ack_drift_missing_response_materializes_over_prefix_repair() {
        // #ackdriftprefixmaterialize regression: a stale ACK sidecar that went
        // through a prefix-divergence repair (`disk_repair_reason =
        // Some(PrefixDivergence)`) but is still missing the agent response must
        // NOT be blocked from the missing-response materialization rescue. Before
        // the fix, the blanket `disk_repair_reason.is_some()` bail left the stale
        // snapshot in place and the cycle dead-ended into a
        // `retry_without_disk_write` spin until supervisor supersession.
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let original_bad_state = concat!(
            "<!-- agent:exchange -->\n",
            "queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        // Prefix-normalized snapshot (the `❯ ` marker restored) but still no
        // response body — exactly what the sidecar prefix repair produces.
        let prefix_repaired = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = "### Re: queued prompt - gpt-5\n\nAnswered.\n";
        let content_ours = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "### Re: queued prompt - gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let prefix_lines = vec!["❯ queued prompt".to_string()];
        let mut decision = IpcRepairDecision::ack_content_prefix_repair(
            prefix_repaired.to_string(),
            original_bad_state.to_string(),
            &prefix_lines,
        );
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergence)
        );

        let repaired = materialize_missing_response_for_socket_ack_drift(
            &file,
            Some("p-prefix"),
            Some(content_ours),
            response,
            true,
            &mut decision,
        );

        assert!(
            repaired,
            "a prefix-diverged ACK still missing the response must materialize it from content_ours proof"
        );
        assert_eq!(decision.snap_source, IpcSnapshotSource::AckContentSidecar);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::LivePromptDrift)
        );
        assert!(decision.redeliver_editor);
        // The originally-recorded bad state (what the editor actually holds — the
        // un-normalized buffer) must survive so redelivery verifies against it,
        // not against the undelivered prefix-repaired image.
        assert_eq!(
            decision
                .editor_bad_state
                .as_ref()
                .expect("bad state fingerprint")
                .content(),
            original_bad_state
        );
        assert!(
            response_materialized_in_content(response, &decision.snapshot_content),
            "repaired snapshot must contain the exact response body:\n{}",
            decision.snapshot_content
        );
    }

    #[test]
    fn socket_ack_drift_missing_response_refuses_partial_heading() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let partial_ack_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "### Re: queued prompt - gpt-5\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = "### Re: queued prompt - gpt-5\n\nAnswered.\n";
        let content_ours = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "### Re: queued prompt - gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision = IpcRepairDecision::ack_content(partial_ack_content.to_string());

        let repaired = materialize_missing_response_for_socket_ack_drift(
            &file,
            Some("p-partial"),
            Some(content_ours),
            response,
            true,
            &mut decision,
        );

        assert!(
            !repaired,
            "partial response headings must fail closed instead of appending a second body"
        );
        assert_eq!(decision.snapshot_content, partial_ack_content);
        assert_eq!(decision.disk_repair_reason, None);
        assert!(!decision.redeliver_editor);
    }

    #[test]
    fn socket_ack_drift_missing_response_refuses_without_content_ours_response() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let ack_content = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = "### Re: queued prompt - gpt-5\n\nAnswered.\n";
        let content_ours_without_response = concat!(
            "<!-- agent:exchange -->\n",
            "❯ queued prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut decision = IpcRepairDecision::ack_content(ack_content.to_string());

        let repaired = materialize_missing_response_for_socket_ack_drift(
            &file,
            Some("p-no-proof"),
            Some(content_ours_without_response),
            response,
            true,
            &mut decision,
        );

        assert!(
            !repaired,
            "missing content_ours response proof must fail closed"
        );
        assert_eq!(decision.snapshot_content, ack_content);
        assert_eq!(decision.disk_repair_reason, None);
        assert!(!decision.redeliver_editor);
    }

    #[test]
    fn guard_refuses_stale_supervisor_content_ours_on_drift() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let clean_ours = "<!-- agent:status -->\nC\n<!-- /agent:status -->\n<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n";
        let mut decision = IpcRepairDecision::file_read(DC_CANDIDATE.to_string());

        let adopted = with_test_stale_supervisor_content_ours_refusal(|| {
            guard_ipc_snapshot_adoption_against_live_prompt_drift(
                &file,
                "test",
                Some("p-stale"),
                Some(DC_BASELINE),
                Some(clean_ours),
                &mut decision,
            )
        });

        assert!(
            !adopted,
            "stale supervisor content_ours must be refused even when it would absorb drift"
        );
        assert_eq!(decision.snap_source, IpcSnapshotSource::FileRead);
        assert_eq!(decision.snapshot_content, DC_CANDIDATE);
    }

    #[test]
    fn guard_refuses_stale_supervisor_content_ours_on_prompt_duplication() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let content_ours = "<!-- agent:exchange -->\n❯ do [#x]\n<!-- /agent:exchange -->\n";
        let duplicate_candidate =
            "<!-- agent:exchange -->\n❯ do [#x]\n❯ do [#x]\n<!-- /agent:exchange -->\n";
        let mut decision = IpcRepairDecision::file_read(duplicate_candidate.to_string());

        let adopted = with_test_stale_supervisor_content_ours_refusal(|| {
            guard_ipc_snapshot_adoption_against_prompt_duplication(
                &file,
                "test",
                Some("p-stale-dup"),
                Some(content_ours),
                &mut decision,
            )
        });

        assert!(
            !adopted,
            "stale supervisor content_ours must not become the repair snapshot"
        );
        assert_eq!(decision.snap_source, IpcSnapshotSource::FileRead);
        assert_eq!(decision.snapshot_content, duplicate_candidate);
    }

    // --- #smconv: node-keyed semantic-merge convergence on live drift ---
    //
    // The shipped Phase-1 `semantic_merge` models exchange turns (and all
    // component content) as list *items* keyed by id; it reconstructs each
    // operator-skeleton component body from items and keeps only the operator's
    // non-item prose (documented assumption). These fixtures therefore use the
    // list-item exchange representation (`- re [#id] ...`) that the primitive
    // supports — the heading-prose (`### Re:`) form is exercised separately by
    // `smconv_declines_when_heading_prose_response_would_drop`, which proves the
    // conservative decline path. See the report for the representation finding.

    // base: queue:start + head [#cf-txn-email] + a prior exchange turn item.
    const SM_BASE: &str = concat!(
        "---\n",
        "session: test\n",
        "queue: start\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "- re [#cf-txn-email] prior turn\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- do [#cf-txn-email]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [#bk1] original backlog text\n",
        "<!-- /agent:backlog -->\n",
    );

    // candidate (AGENT): head struck, a NEW exchange turn appended, and the
    // backlog item edited. All node-DISJOINT from the operator's edits below.
    const SM_AGENT: &str = concat!(
        "---\n",
        "session: test\n",
        "queue: start\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "- re [#cf-txn-email] prior turn\n",
        "- re [#new-turn] implemented the cf-txn-email change and verified it\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- ~~do [#cf-txn-email]~~\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [#bk1] edited backlog text by the agent\n",
        "<!-- /agent:backlog -->\n",
    );

    // ours (OPERATOR): frontmatter flipped to queue:stop + an unrelated queue
    // line added. Disjoint from the agent's exchange/strike/backlog edits.
    const SM_OPERATOR: &str = concat!(
        "---\n",
        "session: test\n",
        "queue: stop\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "- re [#cf-txn-email] prior turn\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- do [#cf-txn-email]\n",
        "- do [#operator-unrelated]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [#bk1] original backlog text\n",
        "<!-- /agent:backlog -->\n",
    );

    #[test]
    fn smconv_disjoint_drift_merges_both_change_sets() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(SM_AGENT.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("smconv"),
            Some(SM_BASE),
            Some(SM_OPERATOR),
            &mut decision,
        );

        assert!(
            adopted,
            "node-disjoint live drift must converge via semantic merge"
        );
        let merged = &decision.snapshot_content;
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::ContentOurs,
            "the merged result is installed via the content_ours snapshot slot"
        );
        assert!(
            merged.contains("[#new-turn]"),
            "merged result must preserve the agent's new exchange turn (the case that used to drop it); got:\n{merged}"
        );
        assert!(
            merged.contains("~~do [#cf-txn-email]~~"),
            "merged result must preserve the agent's queue strike; got:\n{merged}"
        );
        assert!(
            merged.contains("edited backlog text by the agent"),
            "merged result must preserve the agent's backlog edit; got:\n{merged}"
        );
        assert!(
            merged.contains("queue: stop"),
            "merged result must preserve the operator's queue: stop frontmatter flip; got:\n{merged}"
        );
        assert!(
            merged.contains("[#operator-unrelated]"),
            "merged result must preserve the operator's added queue line; got:\n{merged}"
        );
        assert!(
            element::structural_corruption_reason(merged).is_none(),
            "merged result must re-parse cleanly"
        );
    }

    #[test]
    fn smconv_preserves_freetext_fenced_queue_head_on_drift() {
        // `#qdup-freetext` (root cause of the persistent live_prompt_drift churn):
        // when the queue carries a multi-line free-text fenced head (a pasted-
        // console bug report — not a `- ` list item), the per-node reconstruction
        // used to DROP it, trip `dropped_queue_prompt_lines_after_content_ours`,
        // and decline the merge on EVERY cycle, blocking every IPC write. It must
        // now converge AND preserve the head verbatim.
        let head = concat!(
            "---\n",
            ":pushpin: JB `Run Agent Doc` did not submit.\n",
            "\n",
            "```\n",
            "claude exited cleanly.\n",
            "[agent-doc] idle-queue watch: reconciled stale busy actor to ready\n",
            "```\n",
            "---\n",
        );
        let base = format!(
            "---\nsession: test\nqueue: start\n---\n\n\
<!-- agent:exchange -->\n- re [#prior] prior turn\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n{head}- do [#a]\n<!-- /agent:queue -->\n"
        );
        // candidate (live editor buffer): head intact, agent struck the queue item
        // (an outside-exchange change, so the drift guard engages as in the real case).
        let candidate = format!(
            "---\nsession: test\nqueue: start\n---\n\n\
<!-- agent:exchange -->\n- re [#prior] prior turn\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n{head}- ~~do [#a]~~\n<!-- /agent:queue -->\n"
        );
        // content_ours: head intact, operator flipped frontmatter (disjoint node).
        let content_ours = format!(
            "---\nsession: test\nqueue: stop\n---\n\n\
<!-- agent:exchange -->\n- re [#prior] prior turn\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n{head}- do [#a]\n<!-- /agent:queue -->\n"
        );

        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(candidate.clone());
        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("fthead"),
            Some(&base),
            Some(&content_ours),
            &mut decision,
        );
        assert!(
            adopted,
            "free-text-head drift must converge via semantic merge, not block forever"
        );
        let merged = &decision.snapshot_content;
        assert!(
            merged.contains(":pushpin: JB `Run Agent Doc` did not submit."),
            "free-text head line lost in merge:\n{merged}"
        );
        assert!(
            merged.contains("[agent-doc] idle-queue watch: reconciled stale busy actor to ready"),
            "fenced head body lost in merge:\n{merged}"
        );
        assert!(
            element::structural_corruption_reason(merged).is_none(),
            "merged result must re-parse cleanly"
        );
    }

    #[test]
    fn smconv_merges_heading_prose_response_preserving_both_changesets() {
        // The real-session `### Re:` heading-prose exchange turn is now modeled by
        // semantic_merge as an append-only node (#semmerge-owner heading-prose
        // extension), so a live drift no longer drops the agent's response: the
        // node-disjoint merge applies BOTH the agent's new `### Re:` turn AND the
        // operator's concurrent frontmatter/queue edits. This is the root-cause
        // fix for the `content_ours`-drops-the-response transition.
        let base = concat!(
            "---\nsession: test\nqueue: start\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #cf-txn-email\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#cf-txn-email]\n<!-- /agent:queue -->\n",
        );
        let agent = concat!(
            "---\nsession: test\nqueue: start\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #cf-txn-email\n",
            "### Re: do #cf-txn-email — opus-4-8\n\n",
            "Implemented the cf-txn-email change and verified it end to end. This\n",
            "response body is comfortably over the stale-drift threshold so the\n",
            "live drift guard genuinely engages.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- ~~do [#cf-txn-email]~~\n<!-- /agent:queue -->\n",
        );
        let operator = concat!(
            "---\nsession: test\nqueue: stop\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #cf-txn-email\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#cf-txn-email]\n- do [#op]\n<!-- /agent:queue -->\n",
        );
        // The merge now SUCCEEDS: the agent's heading-prose turn is appended.
        let merged = try_semantic_merge_convergence(base, agent, operator)
            .expect("semantic merge must converge a heading-prose response turn now");
        let doc = &merged.merged_doc;
        assert!(
            doc.contains("### Re: do #cf-txn-email — opus-4-8")
                && doc.contains("Implemented the cf-txn-email change"),
            "merged result must preserve the agent's `### Re:` heading-prose turn; got:\n{doc}"
        );
        assert!(
            doc.contains("queue: stop"),
            "merged result must preserve the operator's frontmatter flip; got:\n{doc}"
        );
        assert!(
            doc.contains("[#op]"),
            "merged result must preserve the operator's added queue line; got:\n{doc}"
        );
        assert!(
            element::structural_corruption_reason(doc).is_none(),
            "merged result must re-parse cleanly; got:\n{doc}"
        );

        // End-to-end through the guard: it now converges via semantic merge
        // (snapshot installed from the merged doc) instead of dropping the turn.
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(agent.to_string());
        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("smconv-heading"),
            Some(base),
            Some(operator),
            &mut decision,
        );
        assert!(adopted, "the guard resolves the drift");
        assert!(
            decision
                .snapshot_content
                .contains("### Re: do #cf-txn-email — opus-4-8"),
            "the installed snapshot must carry the agent's response turn; got:\n{}",
            decision.snapshot_content
        );
        assert!(
            decision.snapshot_content.contains("queue: stop"),
            "the installed snapshot must carry the operator's frontmatter flip; got:\n{}",
            decision.snapshot_content
        );
    }

    #[test]
    fn smconv_same_node_conflict_is_safe() {
        // Operator DELETED the queue item the agent struck, and operator edited
        // the backlog node the agent also edited (same-node conflict → operator
        // wins). Assert no agent exchange content (the new turn) is lost: either
        // it converges via semantic merge (preferred) or it falls through to
        // content_ours. Both are acceptable; the invariant is "no silent loss of
        // agent content" and "no panic / clean re-parse".
        let operator_conflict = concat!(
            "---\n",
            "session: test\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "- re [#cf-txn-email] prior turn\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [#bk1] operator-rewritten backlog text\n",
            "<!-- /agent:backlog -->\n",
        );
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(SM_AGENT.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("smconv-conflict"),
            Some(SM_BASE),
            Some(operator_conflict),
            &mut decision,
        );

        assert!(adopted, "the conflict case must still resolve (no panic)");
        let merged = &decision.snapshot_content;
        assert!(
            element::structural_corruption_reason(merged).is_none(),
            "resolved result must re-parse cleanly"
        );
        // The agent's new exchange turn is a list item, so whichever path runs it
        // must NOT silently lose it: converged merges keep it, and the content_ours
        // fallback would only run after recording the dropped evidence.
        if decision.snap_source != IpcSnapshotSource::ContentOurs || merged != operator_conflict {
            assert!(
                merged.contains("[#new-turn]"),
                "converged merge must preserve the agent's new exchange turn; got:\n{merged}"
            );
        }
    }

    #[test]
    fn smconv_declines_on_structurally_corrupt_ours_falls_through() {
        // A structurally-corrupt operator buffer (duplicate singleton queue
        // component) must make the semantic merge decline AND the existing
        // content_ours structural-refusal guard run — the corrupt buffer never
        // becomes the snapshot, the clean candidate is kept.
        let corrupt_ours = concat!(
            "---\n",
            "session: test\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #cf-txn-email\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#cf-txn-email]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#dup]\n",
            "<!-- /agent:queue -->\n",
        );
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        let mut decision = IpcRepairDecision::file_read(SM_AGENT.to_string());

        let adopted = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &file,
            "test",
            Some("smconv-corrupt"),
            Some(SM_BASE),
            Some(corrupt_ours),
            &mut decision,
        );

        assert!(
            !adopted,
            "a structurally-corrupt operator buffer must be refused (semantic merge declines, content_ours guard refuses)"
        );
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::FileRead,
            "decision must keep the clean candidate, not adopt the corrupt buffer"
        );
        assert_eq!(decision.snapshot_content, SM_AGENT);
    }

    #[test]
    fn test_poll_sidecar_present_immediately() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-immediate";

        let ack_dir = project_root.join(".agent-doc/ack-content");
        std::fs::create_dir_all(&ack_dir).unwrap();
        std::fs::write(ack_dir.join(format!("{patch_id}.md")), "immediate content").unwrap();

        let result = poll_ack_content_sidecar(
            &project_root,
            patch_id,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(result, Some("immediate content".to_string()));
    }

    #[test]
    fn test_poll_sidecar_appears_after_delay() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-delayed";

        let ack_dir = project_root.join(".agent-doc/ack-content");
        std::fs::create_dir_all(&ack_dir).unwrap();

        // Spawn a thread that writes the sidecar after 50ms using atomic
        // rename to avoid the poll reading a partially-written file.
        let sidecar_path = ack_dir.join(format!("{patch_id}.md"));
        let tmp_path = ack_dir.join(format!("{patch_id}.md.tmp"));
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::fs::write(&tmp_path, "delayed content").unwrap();
            std::fs::rename(&tmp_path, &sidecar_path).unwrap();
        });

        let result = poll_ack_content_sidecar(
            &project_root,
            patch_id,
            std::time::Duration::from_millis(500),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(result, Some("delayed content".to_string()));
    }

    #[test]
    fn test_poll_sidecar_timeout() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-timeout";

        // Don't create the sidecar — poll should timeout
        std::fs::create_dir_all(project_root.join(".agent-doc/ack-content")).unwrap();

        let start = std::time::Instant::now();
        let result = poll_ack_content_sidecar(
            &project_root,
            patch_id,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(25),
        )
        .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result, None);
        assert!(
            elapsed >= std::time::Duration::from_millis(100),
            "should wait at least the timeout"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(300),
            "should not wait much longer than timeout"
        );
    }

    #[test]
    fn normalization_fallback_fails_closed_when_sidecar_missing_prefix_without_editor_proof() {
        // When the sidecar is missing a ❯ prefix expected by normalize_prefix_lines,
        // try_ipc must not fall back to content_ours for the snapshot (#jbpfx2).
        // Simulates the IntelliJ exact-match failure: plugin wrote sidecar without
        // the ❯ prefix. The repaired candidate is derived from the ACK sidecar and
        // must be proven in the editor before closeout can persist it.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "---\nsession: test\n---\n\n<!-- agent:exchange -->\ndo #jbpfx2\n<!-- agent:boundary:test-bnd-001 -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, original).unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "agent response");

        // content_ours has the ❯ prefix, but it is only an agent-owned candidate.
        let content_ours = "---\nsession: test\n---\n\n<!-- agent:exchange -->\n❯ do #jbpfx2\nagent response\n<!-- /agent:exchange -->\n";
        let normalize_prefix_lines = vec!["do #jbpfx2".to_string()];

        // Simulate plugin: reads patch_id, writes sidecar WITHOUT prefix (bug), ACKs
        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            // Write sidecar WITHOUT ❯ prefix (plugin failure)
                            let bad_sidecar = "---\nsession: test\n---\n\n<!-- agent:exchange -->\ndo #jbpfx2\nagent response\n<!-- /agent:exchange -->\n";
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let err = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("refusing direct document write"),
            "normalization fallback must fail closed instead of repairing disk: {err}"
        );

        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            original,
            "missing-prefix sidecar must not trigger a direct document rewrite"
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "unproven normalization fallback must not save a snapshot"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback")
                && ops_log.contains("recovery=retry_without_disk_write"),
            "ops log should record retry-only normalization fallback:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_divergence_repair_decision_keeps_ack_sidecar_authoritative() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "disk state\n").unwrap();
        let sidecar = "\
<!-- agent:exchange patch=append -->
do #sidecar
operator sidecar text
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #sidecar
agent-owned response
<!-- /agent:exchange -->
";
        let lines = vec!["do #sidecar".to_string()];

        let decision = ipc_repair_decision_from_sidecar(
            &doc,
            Some("patch-1"),
            None,
            sidecar.to_string(),
            Some(content_ours),
            Some(lines.as_slice()),
        );

        assert_eq!(decision.snap_source, IpcSnapshotSource::AckContentSidecar);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergence)
        );
        assert!(
            decision.snapshot_content.contains("❯ do #sidecar"),
            "normalization retry may add the missing prefix to the ACK sidecar"
        );
        assert!(
            decision.snapshot_content.contains("operator sidecar text"),
            "operator-visible sidecar text must be preserved"
        );
        assert!(
            !decision.snapshot_content.contains("agent-owned response"),
            "normalization retry must not adopt content_ours as repair authority"
        );
    }

    #[test]
    fn normalization_fallback_retries_missing_prompt_prefix_from_ack_sidecar() {
        // Regression for #bppfxstrip: if sidecar verification rejects the plugin
        // snapshot, normalization retry may add normalize_prefix_lines only to the
        // ACK sidecar candidate and must fail closed without editor proof.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, original).unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let normalize_prefix_lines =
            vec!["do #bppfxstrip. spec-test-build-install-commit-push".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
                            let _ = std::fs::write(&doc_for_watcher, bad_sidecar);
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let err = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("refusing direct document write"),
            "normalization fallback must fail closed instead of repairing disk: {err}"
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("do #bppfxstrip. spec-test-build-install-commit-push")
                && !disk.contains("❯ do #bppfxstrip. spec-test-build-install-commit-push"),
            "unproven normalization fallback must leave the editor-visible sidecar state untouched; got: {}",
            disk
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "unproven normalization fallback must not save a snapshot"
        );
    }

    #[test]
    fn normfallback_records_repaired_working_tree_when_sidecar_strips_prompt_prefix() {
        // Regression for #normfallback: the observed ops-log signal should be
        // backed by deterministic coverage. A plugin sidecar that drops a
        // required prompt prefix must be rejected, and an unproven editor repair
        // must not rewrite the live file behind the editor.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("agent-doc-bugs2.md");
        let original = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
do [#normfallback]
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, original).unwrap();

        let patch = agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: #normfallback — gpt-5\n\nCovered.",
        );
        let content_ours = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do [#normfallback]
### Re: #normfallback — gpt-5

Covered.
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let normalize_prefix_lines = vec!["do [#normfallback]".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
do [#normfallback]
### Re: #normfallback — gpt-5

Covered.
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
                            let _ = std::fs::write(&doc_for_watcher, bad_sidecar);
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let err = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("refusing direct document write"),
            "normalization fallback must fail closed instead of repairing disk: {err}"
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("do [#normfallback]") && !disk.contains("❯ do [#normfallback]"),
            "unproven normalization fallback must leave the stripped editor state untouched: {disk}"
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "unproven normalization fallback must not save a snapshot"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback")
                && ops_log.contains("reason=prefix_divergence"),
            "ops log should record why the primary sidecar snapshot was rejected:\n{ops_log}"
        );
        assert!(
            ops_log.contains("ipc_visible_repair_retry_required_no_disk_write"),
            "ops log should record retry without direct working-tree repair:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_redelivers_narrow_patch_before_full_content() {
        // A disk-only fallback can leave an editor buffer stale. If the rejected
        // editor state differs only by prompt-prefix normalization, the repair
        // should converge the editor with a narrow normalization patch.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #sidecar-diverge. spec-test-build-install-commit-push
### Re: #sidecar-diverge — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let repaired = "\
---
session: test
---

<!-- agent:exchange patch=append -->
❯ do #sidecar-diverge. spec-test-build-install-commit-push
### Re: #sidecar-diverge — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();
        let normalize_prefix_lines =
            vec!["do #sidecar-diverge. spec-test-build-install-commit-push".to_string()];

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen_repair_payloads =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let listener_root = dir.path().to_path_buf();
        let listener_doc = doc.clone();
        let listener_count = call_count.clone();
        let listener_repair_payloads = seen_repair_payloads.clone();
        std::fs::create_dir_all(listener_root.join(".agent-doc")).unwrap();
        let _listener = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                listener_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(full_content) = v.get("fullContent").and_then(|value| value.as_str()) {
                    let _ = std::fs::write(&listener_doc, full_content);
                    listener_repair_payloads.lock().unwrap().push(v.clone());
                    return Some(serde_json::json!({"type": "ack"}).to_string());
                }

                let patches_empty = v
                    .get("patches")
                    .and_then(|value| value.as_array())
                    .is_none_or(|patches| patches.is_empty());
                if patches_empty
                    && let Some(lines) = v.get("normalize_prefix_lines").and_then(|value| {
                        value.as_array().map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                    })
                {
                    let current = std::fs::read_to_string(&listener_doc).ok()?;
                    let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                    let _ = std::fs::write(&listener_doc, repaired);
                    listener_repair_payloads.lock().unwrap().push(v.clone());
                    return Some(serde_json::json!({"type": "ack"}).to_string());
                }

                Some(serde_json::json!({"type": "ack"}).to_string())
            });
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(dir.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(dir.path()),
            "fake socket listener did not start"
        );

        let result = redeliver_normalization_fallback_for_test(
            &doc,
            repaired,
            bad_state,
            &normalize_prefix_lines,
            Some("source-patch"),
        );
        assert!(result, "narrow normalization repair should be delivered");

        assert!(
            call_count.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "fallback should send a narrow IPC repair"
        );
        let repair_payloads = seen_repair_payloads.lock().unwrap();
        assert_eq!(
            repair_payloads.len(),
            1,
            "expected exactly one narrow repair payload"
        );
        assert!(
            repair_payloads[0].get("fullContent").is_none(),
            "eligible prefix repair should avoid fullContent payloads: {}",
            repair_payloads[0]
        );
        assert_eq!(
            repair_payloads[0]["normalize_prefix_lines"][0],
            "do #sidecar-diverge. spec-test-build-install-commit-push"
        );
        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #sidecar-diverge. spec-test-build-install-commit-push"),
            "editor narrow repair should leave disk/editor content normalized: {disk}"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repaired_editor"),
            "ops log should record the narrow editor repair:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_file_ipc_queues_narrow_patch_before_full_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #sidecar-file. spec-test-build-install-commit-push
### Re: #sidecar-file — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let repaired = "\
---
session: test
---

<!-- agent:exchange patch=append -->
❯ do #sidecar-file. spec-test-build-install-commit-push
### Re: #sidecar-file — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();
        let normalize_prefix_lines =
            vec!["do #sidecar-file. spec-test-build-install-commit-push".to_string()];
        let patch_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let patch_file = agent_doc_dir
            .join("patches")
            .join(format!("{patch_hash}.json"));

        let seen_repair_payloads =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let watcher_doc = doc.clone();
        let watcher_patch_file = patch_file.clone();
        let watcher_ack_dir = agent_doc_dir.join("ack-content");
        let watcher_repair_payloads = seen_repair_payloads.clone();
        let watcher = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(3) {
                if !watcher_patch_file.exists() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                let payload_text = match std::fs::read_to_string(&watcher_patch_file) {
                    Ok(text) => text,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                };
                let payload: serde_json::Value = match serde_json::from_str(&payload_text) {
                    Ok(payload) => payload,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                };
                watcher_repair_payloads
                    .lock()
                    .unwrap()
                    .push(payload.clone());
                let patch_id = payload
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap()
                    .to_string();
                let lines = payload
                    .get("normalize_prefix_lines")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let current = std::fs::read_to_string(&watcher_doc).unwrap();
                let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                std::fs::write(&watcher_doc, &repaired).unwrap();
                std::fs::write(watcher_ack_dir.join(format!("{patch_id}.md")), repaired).unwrap();
                std::fs::remove_file(&watcher_patch_file).unwrap();
                return true;
            }
            false
        });

        let result = redeliver_normalization_fallback_for_test(
            &doc,
            repaired,
            bad_state,
            &normalize_prefix_lines,
            Some("source-patch-file"),
        );
        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");
        assert!(result, "file IPC narrow normalization repair should apply");

        let repair_payloads = seen_repair_payloads.lock().unwrap();
        assert_eq!(
            repair_payloads.len(),
            1,
            "expected exactly one file IPC repair payload"
        );
        let payload = &repair_payloads[0];
        assert!(
            payload.get("fullContent").is_none(),
            "eligible file IPC prefix repair should avoid fullContent payloads: {payload}"
        );
        assert_eq!(payload["patches"].as_array().unwrap().len(), 0);
        assert_eq!(payload["unmatched"], "");
        assert_eq!(payload["reposition_boundary"], true);
        assert_eq!(payload["preserve_head"], true);
        assert_eq!(
            payload["normalize_prefix_lines"][0],
            "do #sidecar-file. spec-test-build-install-commit-push"
        );
        assert_eq!(payload["expected_content_len"], bad_state.len());
        assert_eq!(
            payload["expected_content_hash"],
            agent_doc_hash::content_hash(bad_state)
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #sidecar-file. spec-test-build-install-commit-push"),
            "file IPC narrow repair should leave disk/editor content normalized: {disk}"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repaired_editor")
                && ops_log.contains("transport=file"),
            "ops log should record the file IPC narrow editor repair:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("sidecar_normalization_fallback_redelivered_editor"),
            "file IPC normalization-only repair should not fall back to fullContent:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_redelivery_skips_when_bad_state_is_stale() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
<!-- agent:exchange patch=append -->
do #stale. spec-test-build-install-commit-push
### Re: #stale — gpt-5

Done.
<!-- /agent:exchange -->
";
        let live_state = "\
<!-- agent:exchange patch=append -->
do #stale. spec-test-build-install-commit-push
live prompt typed after sidecar fallback
<!-- /agent:exchange -->
";
        let repaired = "\
<!-- agent:exchange patch=append -->
❯ do #stale. spec-test-build-install-commit-push
### Re: #stale — gpt-5

Done.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, live_state).unwrap();

        let delivered = redeliver_normalization_fallback_for_test(
            &doc,
            repaired,
            bad_state,
            &["do #stale. spec-test-build-install-commit-push".to_string()],
            Some("source-patch"),
        );

        assert!(
            !delivered,
            "normalization fallback redelivery must skip stale bad-state proof"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), live_state);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repair_skipped")
                && ops_log.contains("skip=stale_bad_state")
                && ops_log.contains("sidecar_normalization_fallback_editor_redelivery_skipped"),
            "stale proof skip should be logged for narrow and full-content fallback:\n{ops_log}"
        );
    }

    #[test]
    fn redelivery_skips_when_live_buffer_diverges_from_bad_state() {
        // #clearexchstale: disk still equals the bad state (so the disk-divergence
        // guard passes), but the operator has freshly cleared/edited the live editor
        // buffer (a smaller cleared exchange reported via the live-buffer sidecar).
        // Redelivering the stale snapshot would REVIVE the cleared content, so the
        // redeliver must fail closed on the proven live-buffer divergence.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Stale response that the operator cleared.
<!-- /agent:exchange -->
";
        // Disk still holds the bad state (the redeliver's disk check will pass).
        std::fs::write(&doc, bad_state).unwrap();

        // The operator cleared the exchange in the editor — the live buffer diverges
        // from the bad state and from disk. Record it via the live-buffer sidecar the
        // plugin maintains, using the same canonicalized path the guard consults.
        let indicator_path = doc
            .canonicalize()
            .unwrap_or_else(|_| doc.clone())
            .to_string_lossy()
            .to_string();
        let cleared_buffer = "\
<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->
";
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &indicator_path,
            cleared_buffer,
            "jetbrains-capable-diverged",
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let repaired = bad_state; // the stale snapshot the repair would re-apply
        let delivered = redeliver_ipc_dedupe_to_editor(&doc, repaired, bad_state);

        assert!(
            !delivered,
            "redelivery must skip when the live editor buffer has unsaved edits ahead of the bad state"
        );
        // Disk must be left untouched (the guard returns before any write).
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), bad_state);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("skip=live_buffer_diverges"),
            "live-buffer divergence skip should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_redelivery_blocks_capability_unknown_live_editor_before_ipc() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
<!-- agent:exchange patch=append -->
do #old-editor. spec-test-build-install-commit-push
### Re: #old-editor — gpt-5

Done.
<!-- /agent:exchange -->
";
        let repaired = "\
<!-- agent:exchange patch=append -->
❯ do #old-editor. spec-test-build-install-commit-push
### Re: #old-editor — gpt-5

Done.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();

        let indicator_path = doc
            .canonicalize()
            .unwrap_or_else(|_| doc.clone())
            .to_string_lossy()
            .to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &indicator_path,
            bad_state,
            Some("jetbrains-old-editor"),
        )
        .unwrap();

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let listener_root = dir.path().to_path_buf();
        let listener_doc = doc.clone();
        let listener_count = call_count.clone();
        let captured_clone = captured.clone();
        std::fs::create_dir_all(listener_root.join(".agent-doc")).unwrap();
        let _listener = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
                listener_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                *captured_clone.lock().unwrap() = Some(v.clone());
                if let Some(lines) = v.get("normalize_prefix_lines").and_then(|value| {
                    value.as_array().map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                }) {
                    let current = std::fs::read_to_string(&listener_doc).ok()?;
                    let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                    let _ = std::fs::write(&listener_doc, repaired);
                }
                Some(serde_json::json!({"type": "ack"}).to_string())
            });
        });
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(dir.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            agent_doc_ipc_io::is_listener_active(dir.path()),
            "fake socket listener did not start"
        );

        let delivered = redeliver_normalization_fallback_for_test(
            &doc,
            repaired,
            bad_state,
            &["do #old-editor. spec-test-build-install-commit-push".to_string()],
            Some("source-patch-old-editor"),
        );

        assert!(
            !delivered,
            "capability-unknown editor must not receive normalization repair"
        );
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "capability guard may only send one read-only authority refresh before blocking repair IPC"
        );
        let msg = captured
            .lock()
            .unwrap()
            .clone()
            .expect("listener should receive the authority refresh");
        assert_eq!(msg["type"], "publish_live_buffer");
        assert_eq!(msg["file"], indicator_path);
        assert!(
            msg.get("content").is_none()
                && msg.get("patches").is_none()
                && msg.get("normalize_prefix_lines").is_none(),
            "authority refresh must not carry repair or document mutation payload: {msg}"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), bad_state);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("skip=editor_capability_missing")
                && ops_log.contains(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY),
            "missing-capability redelivery skip should be logged:\n{ops_log}"
        );
        assert!(
            ops_log
                .contains("sidecar_normalization_fallback_narrow_repair_editor_authority_refresh")
                && ops_log.contains("action=publish_live_buffer"),
            "missing-capability redelivery should log the read-only authority refresh:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("sidecar_normalization_fallback_narrow_repair_attempt"),
            "guard must run before repair IPC attempt:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_dedupes_already_applied_editor_response() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
do #duppb. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let editor_already_applied = "\
<!-- agent:exchange patch=append -->
do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, editor_already_applied).unwrap();

        let fallback = normalized_content_ours_fallback(
            &doc,
            Some(baseline),
            content_ours,
            &["do #duppb. spec-test-build-install-commit-push".to_string()],
        );

        assert_eq!(
            fallback.matches("### Re: #duppb — gpt-5").count(),
            1,
            "fallback full-content repair must not redeliver duplicate responses: {fallback}"
        );
        assert!(fallback.contains("❯ do #duppb. spec-test-build-install-commit-push"));
    }

    #[test]
    fn normalization_fallback_adopts_ack_content_response_delta_before_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
do #ackdelta
<!-- agent:boundary:base -->
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #ackdelta
### Re: ack delta — gpt-5

Done.
<!-- agent:boundary:ours -->
<!-- /agent:exchange -->
";
        let disk_after_ack_content = "\
<!-- agent:exchange patch=append -->
do #ackdelta
while typing next prompt
### Re: ack delta — gpt-5

Done.
<!-- agent:boundary:current -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, disk_after_ack_content).unwrap();

        let fallback = normalized_content_ours_fallback(
            &doc,
            Some(baseline),
            content_ours,
            &["do #ackdelta".to_string()],
        );

        assert_eq!(
            fallback.matches("### Re: ack delta — gpt-5").count(),
            1,
            "ack-content normalization fallback must not replay an already-applied response: {fallback}"
        );
        assert!(
            fallback.contains("while typing next prompt"),
            "ack-content fallback should preserve concurrent disk edits: {fallback}"
        );
        assert!(
            fallback.contains("❯ do #ackdelta"),
            "ack-content fallback should still normalize the prompt prefix: {fallback}"
        );
    }

    #[test]
    fn normalization_fallback_splices_pending_mutations_from_disk() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
        let on_disk_with_pending = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] [#keepme] Preserve pending add from disk
<!-- /agent:backlog -->
";
        std::fs::write(&doc, on_disk_with_pending).unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange -->
❯ do #splpend
agent response
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
        let normalize_prefix_lines = vec!["do #splpend".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
agent response
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let err = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("refusing direct document write"),
            "normalization fallback must fail closed instead of repairing disk: {err}"
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            disk, on_disk_with_pending,
            "unproven normalization fallback must leave pending disk mutations untouched"
        );
        assert!(
            agent_doc_snapshot_io::load(&doc).unwrap().is_none(),
            "unproven normalization fallback must not save a snapshot"
        );
    }

    #[test]
    fn normalization_sidecar_retry_preserves_concurrent_comment_deletion() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange -->
do #commentdel
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!--
The tmux focus should be snappy.
-->
";
        std::fs::write(&doc, original).unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange -->
❯ do #commentdel
agent response
<!-- /agent:exchange -->

<!--
The tmux focus should be snappy.
-->
";
        let normalize_prefix_lines = vec!["do #commentdel".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange -->
do #commentdel
agent response
<!-- /agent:exchange -->
";
                            let _ = std::fs::write(&doc_for_watcher, bad_sidecar);
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap();
        assert!(result.success);

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("do #commentdel"),
            "ACK-sidecar-derived retry should preserve the sidecar prompt text: {disk}"
        );
        assert!(
            disk.contains("agent response"),
            "agent response from the ACK sidecar should be preserved: {disk}"
        );
        assert!(
            !disk.contains("The tmux focus should be snappy."),
            "operator-visible deletion from the sidecar must not be resurrected from content_ours: {disk}"
        );
        let snapshot = agent_doc_snapshot_io::load(&doc)
            .unwrap()
            .expect("snapshot should be saved");
        assert!(
            !snapshot.contains("The tmux focus should be snappy."),
            "snapshot must not resurrect content_ours-only comment text: {snapshot}"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("snap_source=ack_content_sidecar")
                && !ops_log.contains("snap_source=content_ours"),
            "normalization retry must not promote content_ours as snapshot authority:\n{ops_log}"
        );
    }

    #[test]
    fn verify_sidecar_normalization_requires_duplicate_occurrences() {
        let sidecar = "\
---
session: test
---

<!-- agent:exchange -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5

Done.

❯ follow-up
spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let normalize_prefix_lines = vec![
            "do [#dup]. Are repeated presets handled?".to_string(),
            "spec-test-build-install-commit-push".to_string(),
            "follow-up".to_string(),
            "spec-test-build-install-commit-push".to_string(),
        ];

        assert!(
            !verify_sidecar_normalization(sidecar, &normalize_prefix_lines),
            "one earlier prefixed preset line must not mask a later bare duplicate"
        );
    }

    #[test]
    fn extract_post_commit_normalization_targets_preserves_duplicate_missing_lines() {
        let committed = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5

Done.

❯ Why follow up?
❯ spec-test-build-install-commit-push
<!-- agent:boundary:committed -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5 (HEAD)

Done.

❯ Why follow up?
spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert_eq!(
            targets,
            vec!["spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_repairs_late_duplicate_occurrence() {
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5 (HEAD)

Done.

❯ Why follow up?
spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[String::from("spec-test-build-install-commit-push")],
        );

        assert_eq!(
            repaired
                .matches("❯ spec-test-build-install-commit-push")
                .count(),
            2,
            "repair should prefix the later bare duplicate without losing the earlier one"
        );
        assert!(
            !repaired.contains("\n❯ ❯ spec-test-build-install-commit-push"),
            "repair must not double-prefix existing matches"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_assistant_verification_lists() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Implemented.

Verification:
- Passed focused tests:
  - `cargo test normalize_prefix`
- `cargo test` is still red on a pre-existing failure.
<!-- agent:boundary:previous -->
do #verfpfx. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "- Passed focused tests:".to_string(),
                "  - `cargo test normalize_prefix`".to_string(),
                "- `cargo test` is still red on a pre-existing failure.".to_string(),
                "do #verfpfx. spec-test-build-install-commit-push".to_string(),
            ],
        );

        assert!(
            repaired.contains("Verification:\n- Passed focused tests:\n  - `cargo test normalize_prefix`\n- `cargo test` is still red on a pre-existing failure."),
            "assistant verification list must stay unprefixed:\n{repaired}"
        );
        assert!(
            repaired.contains("\n❯ do #verfpfx. spec-test-build-install-commit-push\n"),
            "real prompt after the response boundary must still be repaired:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ - Passed focused tests:")
                && !repaired.contains("\n❯   - `cargo test normalize_prefix`")
                && !repaired.contains("\n❯ - `cargo test` is still red on a pre-existing failure."),
            "assistant list items must not receive prompt prefixes:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_requires_targeted_prompt_start_after_response() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Why did this keep happening?
spec-test-build-install-commit-push
<!-- agent:boundary:previous -->
do #spfxnorm. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "spec-test-build-install-commit-push".to_string(),
                "do #spfxnorm. spec-test-build-install-commit-push".to_string(),
            ],
        );

        assert!(
            repaired
                .contains("\nWhy did this keep happening?\nspec-test-build-install-commit-push\n"),
            "assistant question and preset-looking prose must stay bare:\n{repaired}"
        );
        assert!(
            repaired.contains("\n❯ do #spfxnorm. spec-test-build-install-commit-push\n"),
            "real prompt after the boundary must still be repaired:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ spec-test-build-install-commit-push\n"),
            "a stale target inside assistant prose must not be enough to start repair:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_assistant_commit_label() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #done — gpt-5
Verification:
- `cargo test`

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let repaired =
            normalize_exchange_prefixes_for_targets(working, &[String::from("Commit / push:")]);

        assert!(
            repaired.contains("\nCommit / push:\n"),
            "assistant commit evidence label must stay unprefixed:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Commit / push:\n"),
            "assistant commit evidence label must not receive prompt prefix:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_later_assistant_commit_label_after_stale_target()
     {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

Commit / push:
- `old-sha`
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `new-sha`
<!-- agent:boundary:abc --> (HEAD)
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[String::from("Commit / push:"), String::from("- `old-sha`")],
        );

        assert!(
            repaired.contains("\nCommit / push:\n- `new-sha`\n"),
            "later assistant commit label/list must stay bare:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Commit / push:\n- `new-sha`\n"),
            "later assistant commit label must not become a prompt:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_treats_prefixed_response_heading_as_assistant_boundary()
     {
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#done]. spec-test-build-install-commit-push
❯ ### Re: #done — gpt-5

Implemented.

Verification:
- `cargo test normalize_prefix`

Commit / push:
- `abc123`
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "Implemented.".to_string(),
                "Verification:".to_string(),
                "- `cargo test normalize_prefix`".to_string(),
                "Commit / push:".to_string(),
                "- `abc123`".to_string(),
            ],
        );

        assert!(
            repaired.contains("\n❯ ### Re: #done — gpt-5\n\nImplemented.\n"),
            "prefixed response heading must still start an assistant block:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Implemented.")
                && !repaired.contains("\n❯ Verification:")
                && !repaired.contains("\n❯ - `cargo test normalize_prefix`")
                && !repaired.contains("\n❯ Commit / push:")
                && !repaired.contains("\n❯ - `abc123`"),
            "assistant response body after a prefixed heading must not be prompt-prefixed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_patch_content_skips_assistant_commit_label() {
        let patch = "\
### Re: #done — gpt-5
Verification:
- `cargo test`

Commit / push:
- `git push` returned `Everything up-to-date`.
";

        let normalized = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch,
            &[String::from("Commit / push:")],
        );

        assert!(
            normalized.contains("\nCommit / push:\n"),
            "assistant commit evidence label must stay unprefixed:\n{normalized}"
        );
        assert!(
            !normalized.contains("\n❯ Commit / push:\n"),
            "assistant commit evidence label must not receive prompt prefix:\n{normalized}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_assistant_commit_label() {
        let committed = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

❯ Commit / push:
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

❯ Commit / push:
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc --> (HEAD)
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            !targets.iter().any(|target| target == "Commit / push:"),
            "assistant evidence label must not become a prefix repair target: {targets:?}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_assistant_prose_before_next_heading() {
        let committed = "\
<!-- agent:exchange patch=append -->
### Re: sync latency — gpt-5

❯ The current tree has already started making this accountable.
### Re: closeout guard — gpt-5

Done.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
### Re: sync latency — gpt-5

The current tree has already started making this accountable.
### Re: closeout guard — gpt-5 (HEAD)

Done.
<!-- agent:boundary:def -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            targets.is_empty(),
            "a stale prefixed assistant sentence must not become a repair target: {targets:?}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_markdown_lists() {
        let committed = "\
<!-- agent:exchange patch=append -->
❯ Please compare these options:
❯ - keep this bullet bare
❯   - keep this nested bullet bare
❯ 1. keep this ordered bullet bare
### Re: options — gpt-5
Done.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
❯ Please compare these options:
- keep this bullet bare
  - keep this nested bullet bare
1. keep this ordered bullet bare
### Re: options — gpt-5 (HEAD)
Done.
<!-- agent:boundary:def -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            targets.is_empty(),
            "stale prefixed markdown list items must not become repair targets: {targets:?}"
        );
    }

    #[test]
    fn verify_sidecar_normalization_rejects_assistant_list_prefix_substitute() {
        let sidecar = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Verification:
❯ - Passed focused tests:
<!-- agent:boundary:previous -->
do #verfpfx. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        assert!(
            !verify_sidecar_normalization(sidecar, &["- Passed focused tests:".to_string()]),
            "a prefixed assistant list item must not satisfy prompt-prefix sidecar verification"
        );
    }
}

#[cfg(test)]
mod core_tests {
    #![allow(unused_imports)]
    use super::*;
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn ipc_live_prompt_drift_keeps_live_ack_candidate_and_records_queue_proof() {
        let dir = tempfile::tempdir().unwrap();
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ original prompt\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#deleted]\n",
            "<!-- /agent:queue -->\n",
        );
        let content_ours = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ original prompt\n",
            "### Re: original prompt — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#deleted]\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ original prompt\n",
            "❯ live prompt after preflight\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#manual]\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n",
        );
        let doc = agent_doc_test_support::init_repo_with_doc(dir.path(), "session.md", baseline);
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut decision = IpcRepairDecision::ack_content(candidate.to_string());

        let blocked = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            &doc,
            "test",
            Some("patch-q"),
            Some(baseline),
            Some(content_ours),
            &mut decision,
        );

        assert!(blocked);
        assert_eq!(
            decision.snap_source,
            IpcSnapshotSource::AckContentSidecar,
            "turn closeout must not promote the agent target to snapshot authority"
        );
        assert!(
            !decision
                .snapshot_content
                .contains("### Re: original prompt — gpt-5"),
            "the missing response remains unproven; the caller must retry instead of saving a snapshot:\n{}",
            decision.snapshot_content
        );
        assert!(
            decision
                .snapshot_content
                .contains("❯ live prompt after preflight")
        );
        assert!(
            decision.snapshot_content.contains("do [#manual]"),
            "operator-visible queue additions stay in the live ACK candidate:\n{}",
            decision.snapshot_content
        );
        assert!(
            !decision.snapshot_content.contains("do [#deleted]"),
            "the live ACK candidate is preserved as observed; no turn-local repair is applied:\n{}",
            decision.snapshot_content
        );
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            log.contains("queue_live_deletion_ignored")
                && log.contains("reason=unproven_ipc_candidate_queue_deletion")
                && log.contains("dropped_queue_prompt_recorded"),
            "queue deletion must be ignored while queue additions still leave dropped-edit proof:\n{log}"
        );
    }
    #[test]
    fn patch_response_headings_already_in_head_true_when_no_patches() {
        // Empty patch list — conservatively preserve the existing late-fallback
        // gate behavior (reject when no response evidence is present).
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "doc body\n").unwrap();
        assert!(patch_response_headings_already_in_head(&doc, &[]));
    }
    #[test]
    fn patch_response_headings_already_in_head_true_when_heading_in_head() {
        let dir = TempDir::new().unwrap();
        let doc = agent_doc_test_support::init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: shipped — opus-4-7\n\nbody\n",
        );
        let patch = agent_doc_test_support::patch_with_heading("### Re: shipped — opus-4-7");
        assert!(patch_response_headings_already_in_head(&doc, &[patch]));
    }
    #[test]
    fn patch_response_headings_already_in_head_false_when_heading_missing_from_head() {
        // Mid-turn rotation signature: HEAD has been advanced by a different
        // operation (compact, sibling commit) and does not yet contain the
        // response we're about to apply. The late-fallback gate must allow
        // the patch through.
        let dir = TempDir::new().unwrap();
        let doc = agent_doc_test_support::init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: prior cycle — opus-4-7\n\nold\n",
        );
        let patch = agent_doc_test_support::patch_with_heading("### Re: new response — opus-4-7");
        assert!(
            !patch_response_headings_already_in_head(&doc, &[patch]),
            "mid-turn rotation must allow the patch (response not in HEAD)"
        );
    }
    #[test]
    fn patch_response_headings_already_in_head_false_when_any_heading_missing() {
        let dir = TempDir::new().unwrap();
        let doc = agent_doc_test_support::init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: first — opus-4-7\n\nbody\n",
        );
        let patches = vec![
            agent_doc_test_support::patch_with_heading("### Re: first — opus-4-7"),
            agent_doc_test_support::patch_with_heading("### Re: second — opus-4-7"),
        ];
        assert!(
            !patch_response_headings_already_in_head(&doc, &patches),
            "all headings must be in HEAD for the gate to skip"
        );
    }
    #[test]
    fn patch_response_headings_already_in_head_false_when_file_not_in_git() {
        // No git repo → show_head returns Ok(None). Fail-safe: treat as not
        // in HEAD so the late-fallback gate rotates the cycle rather than
        // rejecting the patch.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "no git\n").unwrap();
        let patch = agent_doc_test_support::patch_with_heading("### Re: something — opus-4-7");
        assert!(!patch_response_headings_already_in_head(&doc, &[patch]));
    }
    #[test]
    fn ipc_ack_timeouts_degrade_current_session_to_file_ipc_retry() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test-session\n---\n\ncontent").unwrap();

        assert!(
            !record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap(),
            "first timeout should only record health state"
        );
        assert!(
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap(),
            "second consecutive timeout should mark the listener degraded"
        );
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "current session should now bypass IPC"
        );

        fs::write(&doc, "---\nsession: next-session\n---\n\ncontent").unwrap();
        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a new session id must not inherit the old session's degraded marker"
        );
    }
    #[test]
    fn degraded_latch_self_heals_when_listener_recovers() {
        // `#ipc-degrade-self-heal`: the degrade latch is a circuit breaker, not
        // a permanent session verdict. Once a recovered plugin socket is
        // accepting connections again, `ipc_direct_disk_degraded` must clear the
        // marker and resume the reliable IPC path instead of staying disk-only
        // until session restart.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: heal-session\n---\n\ncontent").unwrap();

        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "two timeouts with no live listener should stay degraded"
        );

        // Bring a live socket listener up (the recovered plugin).
        let root_clone = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root_clone, |_msg| {
                Some(r#"{"type":"ack","id":"x"}"#.to_string())
            });
        });
        std::thread::sleep(std::time::Duration::from_millis(150));

        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a recovered live listener must self-heal the degrade latch"
        );
        let marker = dir.path().join(".agent-doc/ipc-degraded").join(format!(
            "{}.json",
            agent_doc_fs::document_state_hash(&doc).unwrap()
        ));
        assert!(
            !marker.exists(),
            "self-heal must remove the degraded marker"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(dir.path()));
        drop(server);
    }
    #[test]
    fn degraded_latch_does_not_self_heal_when_listener_connects_without_ack() {
        // A wedged editor plugin can leave ipc.sock connectable while its accept
        // / apply path no longer returns acks. The degraded latch must not clear
        // on connect-only evidence; otherwise the next write re-enters the bad
        // socket path instead of preferring file IPC.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: wedged-session\n---\n\ncontent").unwrap();

        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();

        let root_clone = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root_clone, |_msg| None);
        });
        agent_doc_test_support::wait_for_live_prompt_drift_listener(dir.path());

        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "connectable but non-acking listener must remain degraded"
        );
        let marker = dir.path().join(".agent-doc/ipc-degraded").join(format!(
            "{}.json",
            agent_doc_fs::document_state_hash(&doc).unwrap()
        ));
        assert!(
            marker.exists(),
            "non-acking listener must not clear the degraded marker"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_degraded_self_heal_probe_failed")
                && log.contains("IPC_ack_timeout"),
            "failed self-heal probe must be observable:\n{log}"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(dir.path()));
        drop(server);
    }
    #[test]
    fn try_ipc_prefers_file_ipc_when_socket_degraded() {
        // `#ipc-degraded-prefers-file-ipc`: a latched-degraded socket must NOT
        // jump straight to a raw disk write. The write routes through the
        // file-IPC patch queue (plugin file watcher applies via Document API).
        // With no plugin consuming the patch, file IPC times out and returns
        // `false` so the caller can fall back to disk as the LAST resort — but
        // the degraded write still attempted file IPC first.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "new content");
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();

        assert!(
            !result.success,
            "degraded file-IPC with no plugin should report not consumed for retry"
        );
        // The file-IPC poll leaves the unconsumed patch for editor retry.
        let leftover: Vec<_> = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !leftover.is_empty(),
            "file-IPC timeout must leave the unconsumed patch queued"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_socket_degraded_prefer_file_ipc")
                && ops_log.contains("transport=try_ipc"),
            "degraded socket should log the prefer-file-IPC routing decision:\n{ops_log}"
        );
        assert!(
            ops_log.contains("ipc_write_attempt"),
            "degraded write must still attempt the file-IPC patch queue, not bypass it:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("ipc_listener_degraded_direct_disk"),
            "degraded write must NOT take the old direct-disk bypass:\n{ops_log}"
        );
    }
    #[test]
    fn try_ipc_degraded_succeeds_via_file_ipc_when_plugin_consumes() {
        // `#ipc-degraded-prefers-file-ipc`: even with the socket latched
        // degraded, a live plugin file watcher consuming the file-IPC patch
        // makes the degraded write succeed through the plugin (Document API) —
        // no raw disk write, so no manufactured File Cache Conflict.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "two timeouts with no live listener should latch degraded"
        );

        // Simulate the plugin file watcher applying then deleting the patch.
        let watcher_dir = agent_doc_dir.join("patches");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            let _ = fs::write(
                                &doc_for_watcher,
                                "---\nsession: test\n---\n\n<!-- agent:exchange -->\nnew content\n<!-- /agent:exchange -->\n",
                            );
                            let _ = fs::remove_file(entry.path());
                            return;
                        }
                    }
                }
            }
        });

        let patch = agent_doc_template::PatchBlock::new("exchange", "new content");
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "degraded write must succeed through the file-IPC patch queue when the plugin consumes it"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_socket_degraded_prefer_file_ipc"),
            "degraded socket should log the prefer-file-IPC routing decision:\n{ops_log}"
        );
    }
    #[test]
    fn try_editor_converge_skips_wedged_socket_then_uses_detached_disk_when_editorless() {
        // `#fcc0e`: once the de-wedge latch trips degraded (repeated socket ack
        // timeouts) and no live listener can be re-probed, the converger must skip
        // the wedged socket, try the plugin-owned file-IPC queue, then use the
        // guarded detached-disk path when no live editor sidecar owns the document.
        // It must not take the old raw disk-fallback bypass.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        // Trip the degraded latch (threshold = 2 distinct ack timeouts), mirroring
        // the existing dewedge tests. No live listener exists, so the self-heal
        // re-probe in `ipc_direct_disk_degraded` cannot clear it.
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "queue_consume").unwrap();
        let degraded =
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "queue_consume").unwrap();
        assert!(
            degraded,
            "two distinct ack timeouts must trip the degraded latch"
        );

        let written = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(written, "editorless degraded socket should converge");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            target,
            "editorless degraded socket should write through guarded detached disk"
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_degraded_prefer_file_ipc")
                && log.contains("transport=queue_consume"),
            "the degraded skip must prefer file IPC before detached disk:\n{log}"
        );
        assert!(
            log.contains("queue_consume_writeback")
                && log.contains("transport=disk_detached")
                && log.contains("reason=listener_degraded_editor_detached"),
            "the degraded editorless write must be source-labelled as detached disk:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "the degraded converger must not take the old direct-disk bypass:\n{log}"
        );
        assert!(
            !log.contains("reason=no_listener"),
            "the degraded check must short-circuit before the no_listener check:\n{log}"
        );
    }

    #[test]
    fn try_editor_converge_degraded_socket_succeeds_via_file_ipc_when_plugin_consumes() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = agent_doc_test_support::queue_consume_convergence_source();
        let target = agent_doc_test_support::queue_consume_convergence_target();
        fs::write(&doc, &source).unwrap();

        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "queue_consume").unwrap();
        let degraded =
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "queue_consume").unwrap();
        assert!(
            degraded,
            "two distinct ack timeouts must trip the degraded latch"
        );

        let watcher_dir = agent_doc_dir.join("patches");
        let watcher_ack_dir = agent_doc_dir.join("ack-content");
        let watcher_doc = doc.clone();
        let watcher_target = target.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().is_none_or(|e| e != "json") {
                            continue;
                        }
                        let payload_text = fs::read_to_string(&path).unwrap();
                        let payload: serde_json::Value =
                            serde_json::from_str(&payload_text).unwrap();
                        let patch_id = payload
                            .get("patch_id")
                            .and_then(|value| value.as_str())
                            .unwrap()
                            .to_string();
                        fs::write(&watcher_doc, &watcher_target).unwrap();
                        fs::write(
                            watcher_ack_dir.join(format!("{patch_id}.md")),
                            &watcher_target,
                        )
                        .unwrap();
                        fs::remove_file(path).unwrap();
                        return true;
                    }
                }
            }
            false
        });

        let converged = try_editor_converge(&doc, &target, &source, "queue_consume").unwrap();
        assert!(
            converged,
            "degraded convergence must succeed through file IPC when the plugin consumes it"
        );
        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_degraded_prefer_file_ipc")
                && log.contains("queue_consume_file_ipc_convergence_attempt")
                && log.contains("transport=file_ipc")
                && log.contains("degraded_cause=listener_degraded"),
            "degraded convergence should be auditable as file IPC:\n{log}"
        );
        assert!(
            !log.contains("transport=disk_fallback"),
            "degraded convergence must not raw-write behind the plugin:\n{log}"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), target);
    }
    #[test]
    fn ipc_snapshot_adoption_allowed_logs_benign_recheck() {
        // Every adoption that the fail-closed guards did NOT block must still leave
        // a diagnostic so a corruption slipping through as "allowed" is traceable.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Q\n",
            "<!-- /agent:exchange -->\n",
        );
        let content_ours = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Q\n### Re: Q — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -->\n",
        );
        let decision = IpcRepairDecision::content_ours(content_ours.to_string());

        log_ipc_snapshot_adoption_allowed(
            &doc,
            "socket_ack_content",
            Some("pid-allowed"),
            Some(baseline),
            Some(content_ours),
            &decision,
            false,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_snapshot_adoption_allowed")
                && log.contains("source=socket_ack_content")
                && log.contains("patch_id=pid-allowed")
                && log.contains("drift_recheck=false")
                && log.contains("dup_growth_recheck=0"),
            "allowed adoption must log a benign re-check:\n{log}"
        );
    }
    #[test]
    fn ipc_snapshot_adoption_allowed_is_silent_when_blocked() {
        // Blocked adoptions log their own rich diagnostic; the allowed line must not
        // also fire (it would falsely report an unguarded adoption).
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let decision = IpcRepairDecision::content_ours("snapshot".to_string());
        log_ipc_snapshot_adoption_allowed(
            &doc,
            "file_ipc",
            Some("pid-blocked"),
            None,
            None,
            &decision,
            true,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("ipc_snapshot_adoption_allowed"),
            "allowed diagnostic must stay silent once a guard fired:\n{log}"
        );
    }
    #[test]
    fn ipcfullprompt_corruption_logged_on_deleted_response() {
        // #ipcfullprompt-recur2: a live editor buffer (candidate) that dropped a
        // previously-committed `### Re:` block must leave a forensic ops.log line
        // and preserve the baseline + candidate for analysis — default-on capture,
        // no manual editor debug opt-in required.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "### Re: second — opus-4-8\nA2.\n",
            "<!-- /agent:exchange -->\n",
        );
        // candidate dropped the second response block.
        let candidate = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "<!-- /agent:exchange -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "socket_ack_content",
            Some("pid-corrupt"),
            Some(baseline),
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipcfullprompt_corruption_suspected")
                && log.contains("source=socket_ack_content")
                && log.contains("patch_id=pid-corrupt")
                && log.contains("deleted=1")
                && log.contains("response_deleted(### Re: second — opus-4-8:1->0)"),
            "deleted prior response must be captured:\n{log}"
        );
        let forensic_dir = agent_doc_dir.join("logs/ipcfullprompt");
        let preserved: Vec<_> = fs::read_dir(&forensic_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            preserved.iter().any(|n| n.ends_with(".baseline.md"))
                && preserved.iter().any(|n| n.ends_with(".candidate.md")),
            "forensic baseline + candidate must be preserved: {preserved:?}"
        );
    }
    #[test]
    fn ipcfullprompt_scaffold_duplication_logged_without_baseline() {
        // The brandon-cinquegrana.md shape: a full-tail duplication leaves two
        // `<!-- /agent:exchange -->` markers around an in-progress prompt. This is
        // a self-check on the candidate, so it must fire even with no baseline.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let candidate = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: prior — opus-4-8\nAnswer.\n",
            "<!-- agent:boundary:709a41ae -->\n",
            "Is the issue still happening?\nCan it be re\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
            "Can it be rep11ro\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "socket_ack_content",
            Some("pid-x"),
            None,
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipcfullprompt_corruption_suspected")
                && log.contains("scaffold_duplicated=")
                && log.contains("scaffold_duplicated(<!-- /agent:exchange -->:1->2)"),
            "full-tail scaffold duplication must be captured without a baseline:\n{log}"
        );
    }
    #[test]
    fn ipcfullprompt_corruption_silent_on_clean_candidate() {
        // A candidate that only *adds* a new response (expected growth) must not
        // be flagged — no false positive on normal cycles.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "<!-- /agent:exchange -->\n",
        );
        let candidate = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "### Re: second — opus-4-8\nA2.\n",
            "<!-- /agent:exchange -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "file_ipc",
            Some("pid-clean"),
            Some(baseline),
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("ipcfullprompt_corruption_suspected"),
            "clean growth must not be flagged as corruption:\n{log}"
        );
    }
}

#[cfg(test)]
mod committed_blob_authority_autoheal_tests {
    //! `#rtheal-write` — the WRITE/FINALIZE capability gate must UNBLOCK (heal +
    //! return false) when the stale editor buffer byte-matches a recent committed
    //! blob (a previously-saved, recoverable state with no unsaved work), and must
    //! STILL REFUSE (return true) when the buffer matches no committed blob (could
    //! be unsaved-ahead operator work).
    use tempfile::TempDir;

    /// Build a temp git repo with `doc.md`, committing each snapshot in order, and
    /// leave the working tree at the LAST snapshot (== HEAD). Also creates the
    /// `.agent-doc/logs` dir the ops-log writer expects.
    fn temp_git_doc(commits: &[&str]) -> (TempDir, std::path::PathBuf, String) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        let file = root.join("doc.md");
        for (i, body) in commits.iter().enumerate() {
            std::fs::write(&file, body).unwrap();
            git(&["add", "doc.md"]);
            git(&["commit", "-m", &format!("commit {i}")]);
        }
        let canonical = std::fs::canonicalize(&file)
            .unwrap()
            .to_string_lossy()
            .to_string();
        (dir, file, canonical)
    }

    const OLD: &str = concat!(
        "# Doc\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Older committed answer.\n",
        "<!-- /agent:exchange -->\n",
    );
    const NEW: &str = concat!(
        "# Doc\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Older committed answer.\n",
        "### Re: next — gpt-5\n\n",
        "Newer committed answer.\n",
        "<!-- /agent:exchange -->\n",
    );

    #[test]
    fn unblocks_when_stale_buffer_matches_a_committed_blob() {
        // disk == HEAD == NEW; the editor still holds committed ancestor OLD, and
        // its live-buffer sidecar lacks the operator-text-authority capability.
        let (_dir, file, canonical) = temp_git_doc(&[OLD, NEW]);
        // Missing-capability sidecar holding the OLD committed blob.
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &canonical,
            OLD,
            Some("vscode-committed-blob"),
        )
        .unwrap();

        // The stale buffer (expected_bad_state) IS a committed blob → proceed.
        let refuse = agent_doc_write_converge_io::redelivery_missing_operator_text_authority(
            &file,
            OLD,
            "write",
            Some("p1"),
        );
        assert!(
            !refuse,
            "a stale buffer equal to a committed blob holds no unsaved work — the write must proceed (return false)"
        );
        // The autoheal marker is emitted; the capability-missing skip is NOT.
        let ops_log =
            std::fs::read_to_string(file.parent().unwrap().join(".agent-doc/logs/ops.log"))
                .unwrap();
        assert!(
            ops_log.contains("write_editor_authority_autoheal")
                && ops_log.contains("reason=stale_behind_committed_blob"),
            "the committed-blob autoheal must be auditable:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("skip=editor_capability_missing"),
            "the capability-missing refusal must NOT fire for a committed blob:\n{ops_log}"
        );
    }

    #[test]
    fn still_refuses_when_stale_buffer_matches_no_committed_blob() {
        // SAFETY INVARIANT: disk == HEAD == NEW, but the editor holds a genuine
        // unsaved edit that matches NO commit. The gate MUST still refuse.
        let (_dir, file, canonical) = temp_git_doc(&[OLD, NEW]);
        let unsaved = format!("{NEW}<!-- operator note -->\nGENUINELY UNSAVED WORK\n");
        agent_doc_debounce::record_live_buffer_digest_content_for_editor(
            &canonical,
            &unsaved,
            Some("vscode-unsaved-ahead"),
        )
        .unwrap();

        let refuse = agent_doc_write_converge_io::redelivery_missing_operator_text_authority(
            &file,
            &unsaved,
            "write",
            Some("p2"),
        );
        assert!(
            refuse,
            "a buffer matching no committed blob may hold unsaved operator work — the gate MUST still refuse (return true)"
        );
        let ops_log =
            std::fs::read_to_string(file.parent().unwrap().join(".agent-doc/logs/ops.log"))
                .unwrap();
        assert!(
            ops_log.contains("skip=editor_capability_missing"),
            "the capability-missing refusal must fire for a non-committed buffer:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("write_editor_authority_autoheal"),
            "no autoheal may fire when the buffer is not a proven committed blob:\n{ops_log}"
        );
    }
}

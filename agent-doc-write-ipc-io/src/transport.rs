//! Write IPC transport: PID-scoped delivery to the registered Lazily replica.

use crate::{
    IpcResult, build_ipc_patches_json, ipc_document_content, patch_response_bodies_already_in_head,
    projected_cycle_id,
};
use agent_doc_document_realtime::write_policy::{
    EditorDeliveryAdmission, EditorDeliveryAdmissionFacts, decide_editor_delivery_admission,
};
use agent_doc_element_exchange::{
    exchange_has_live_user_edit, exchange_prompt_prefix_count, exchange_prompt_text_duplicated,
};
use agent_doc_flow::types::{FlowOutcome, FlowStage};
use agent_doc_flow_io::closeout::cycle_already_committed;
use agent_doc_ipc_io::editor_target::target_payload_to_editor;
use agent_doc_ipc_protocol::{
    AlreadyAppliedSnapshotOutcome, FullContentIpcMode, build_ipc_node_patches_json,
    effective_unmatched_for_patch_payload, is_already_applied_receipt_error_message,
    is_socket_receipt_timeout_error,
};
use agent_doc_template as template;
use agent_doc_template::stale_baseline::patch_touches_exchange;
use agent_doc_turn::op_log::OpsLogEvent;
use agent_doc_write_converge_io::{
    AlreadyAppliedSocketSnapshotContext, checkpoint_ipc_baseline_nonfatal,
    clear_ipc_socket_ack_timeouts, dedupe_ipc_snapshot_content, full_content_ipc_scope_allows,
    guard_ipc_snapshot_adoption_against_live_prompt_drift,
    guard_ipc_snapshot_adoption_against_prompt_duplication, ipc_repair_decision_from_visible_write,
    log_full_content_ipc_disabled, log_ipc_snapshot_adoption_allowed,
    log_ipcfullprompt_corruption_if_any, mark_visible_write_live_buffer_synced_after_write,
    materialize_missing_response_for_socket_visible_write_drift,
    persist_already_applied_socket_content_ours_snapshot,
    poll_visible_write_content_lazily_event_or_projection,
    prefer_visible_content_over_stale_visible_write_snapshot,
    reconcile_visible_write_snapshot_to_newer_operator_buffer, record_ipc_socket_ack_timeout,
    visible_write_disk_proof, write_visible_write_through_to_disk,
};

fn registered_editor_delivery_target(
    file: &Path,
) -> Option<agent_doc_reliable_sync_io::liveness::EditorRegistration> {
    agent_doc_controller_io::project_controller::live_editor_registration_for_file(file)
        .ok()
        .flatten()
}
use anyhow::Result;
use std::path::Path;

fn log_closeout_guard(
    file: &Path,
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: agent_doc_turn::closeout_guard::CloseoutGuardReason,
) {
    agent_doc_flow_io::closeout::log_closeout_guard_event(file, stage, outcome, reason);
}

fn ipc_response_materialized_or_fallback(
    file: &Path,
    source: &str,
    response: &str,
    content: &str,
) -> bool {
    agent_doc_template_io::ipc_response_materialized_or_fallback_with_recycle(
        file,
        source,
        response,
        content,
        |_, _| {},
    )
}

fn fold_visible_write_into_canonical(
    file: &Path,
    patch_id: &str,
    content: &str,
    source: &str,
) -> Result<()> {
    let changed =
        agent_doc_document_realtime_io::adopt_verified_editor_text_through_relay_authority(
            file, content, source,
        )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_visible_write_canonical_adopt file={} patch_id={} source={} changed={} content_hash={}",
            file.display(),
            patch_id,
            source,
            changed
                .map(|value| value.to_string())
                .unwrap_or_else(|| "editor_absent".to_string()),
            agent_doc_hash::content_hash(content),
        ),
    );
    Ok(())
}

fn log_ipc_proof_failure(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    invariant: &str,
    recovery: &str,
    detail: &str,
) {
    agent_doc_template_io::log_ipc_proof_failure_with_recycle(
        file,
        source,
        patch_id,
        invariant,
        recovery,
        detail,
        |_, _| {},
    );
}

fn missing_lazily_receipt_detail(file: &Path, detail: &str) -> String {
    use agent_doc_crdt_relay_io::CurrentText;
    let current =
        agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
            file,
            "missing_lazily_receipt_detail",
        );
    let (current_state, transition_pending) = match current {
        Ok(None | Some(CurrentText::Detached)) => ("detached", false),
        Ok(Some(CurrentText::Current {
            delivery_converged: true,
            ..
        })) => ("lazily_current", false),
        Ok(Some(CurrentText::Current { .. })) => ("delivery_pending", true),
        Ok(Some(CurrentText::EditorAttachedMissingReplica)) => ("missing_replica", true),
        Ok(Some(CurrentText::EditorSyncPending)) => ("current_pending", true),
        Err(_) => ("authority_unavailable", true),
    };
    format!(
        "{} lazily_current_state={} current_transition_pending={}",
        detail, current_state, transition_pending
    )
}

fn log_partial_response_materialization_for_retry(
    file: &Path,
    source: &str,
    response: &str,
) -> Result<()> {
    agent_doc_template_io::log_partial_response_materialization_for_retry(file, source, response)
}

#[allow(clippy::too_many_arguments)]
fn log_exchange_write_diagnostic(
    file: &Path,
    source: &str,
    write_mode: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    before: &str,
    after: &str,
    patches: &[template::PatchBlock],
    unmatched: &str,
) {
    let before_exchange = agent_doc_element_exchange::exchange_content(before);
    let after_exchange = agent_doc_element_exchange::exchange_content(after);
    let touches_exchange =
        before_exchange != after_exchange || patch_touches_exchange(patches, unmatched);
    if !touches_exchange {
        return;
    }

    let before_hash = agent_doc_hash::content_hash(before);
    let after_hash = agent_doc_hash::content_hash(after);
    let live_exchange_edited = exchange_has_live_user_edit(baseline, before);
    let prompt_text_duplicated = exchange_prompt_text_duplicated(before, after);
    let before_prefix_count = before_exchange
        .map(exchange_prompt_prefix_count)
        .unwrap_or(0);
    let after_prefix_count = after_exchange
        .map(exchange_prompt_prefix_count)
        .unwrap_or(0);
    let normalized_prefix_delta = after_prefix_count.saturating_sub(before_prefix_count);
    let prompt_text_normalized = normalized_prefix_delta > 0;
    let cycle_id = projected_cycle_id(file).unwrap_or_else(|| "-".to_string());
    let writer_pid = std::process::id();
    let writer_exe = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "-".to_string());

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "exchange_write_diagnostic file={} writer_pid={} writer_exe={} source={} write_mode={} patch_id={} cycle_id={} before_hash={} after_hash={} live_exchange_edited={} prompt_text_duplicated={} prompt_text_normalized={} normalized_prefix_delta={} patches={} unmatched_len={}",
            file.display(),
            writer_pid,
            writer_exe,
            source,
            write_mode,
            patch_id.unwrap_or("-"),
            cycle_id,
            before_hash,
            after_hash,
            live_exchange_edited,
            prompt_text_duplicated,
            prompt_text_normalized,
            normalized_prefix_delta,
            patches.len(),
            unmatched.trim().len()
        ),
    );
}

#[allow(clippy::too_many_arguments)]
pub fn try_ipc_with_effects(
    effects: &dyn agent_doc_write_converge_io::EditorConvergenceEffects,
    file: &Path,
    patches: &[agent_doc_template::PatchBlock],
    unmatched: &str,
    frontmatter_yaml: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
    reuse_patch_id: Option<&str>,
) -> Result<IpcResult> {
    try_ipc_inner(
        effects,
        file,
        patches,
        unmatched,
        frontmatter_yaml,
        baseline,
        content_ours,
        normalize_prefix_lines,
        reuse_patch_id,
    )
}

#[allow(clippy::too_many_arguments)]
fn try_ipc_inner(
    effects: &dyn agent_doc_write_converge_io::EditorConvergenceEffects,
    file: &Path,
    patches: &[agent_doc_template::PatchBlock],
    unmatched: &str,
    frontmatter_yaml: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
    reuse_patch_id: Option<&str>,
) -> Result<IpcResult> {
    let canonical = file.canonicalize()?;
    let hash = agent_doc_fs::document_state_hash(file)?;
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    let patch_id = reuse_patch_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let ipc_before_content = ipc_document_content(file, "try_ipc_before_content").ok();

    // Guard: if the cycle is already committed, reject the patch to prevent
    // a late fallback from re-dirtying the document.
    //
    // Exception (#adoc-compact-during-turn-response-loss): when a binary-owned
    // commit lands mid-turn (for example a JetBrains-initiated
    // `agent-doc compact exchange` between this turn's preflight and finalize),
    // the cycle state's `Committed` phase belongs to that other operation —
    // not to the response we are about to apply. Detect that case by checking
    // whether the response headings carried in the incoming patches are
    // already present in HEAD. If they are, the gate is correct (skip).
    // If they are not, the "committed" cycle is unrelated to this response:
    // rotate the cycle state to start fresh and let the patch flow continue.
    if let Some(ref cycle_id) = cycle_already_committed(file) {
        let response_in_head = patch_response_bodies_already_in_head(file, patches);
        if !response_in_head {
            eprintln!(
                "[write] mid-turn cycle rotation detected for {}: cycle {} marked committed \
                 but the incoming response body is absent from HEAD — starting a fresh \
                 cycle instead of rejecting (see #adoc-compact-during-turn-response-loss)",
                file.display(),
                cycle_id
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "mid_turn_cycle_rotation file={} prior_cycle={} patch_id={} action=fresh_cycle",
                    file.display(),
                    cycle_id,
                    patch_id
                ),
            );
            let snapshot_content = agent_doc_snapshot_io::load_document_baseline(file)?;
            let file_content_for_state =
                ipc_document_content(file, "try_ipc_mid_turn_cycle_state_current").ok();
            let _ = agent_doc_cycle_state_io::start_preflight(
                file,
                snapshot_content.as_deref(),
                file_content_for_state.as_deref(),
            );
        } else {
            eprintln!(
                "[write] rejecting late fallback patch: cycle {} already committed for {}",
                cycle_id,
                file.display()
            );
            log_closeout_guard(
                file,
                agent_doc_flow::types::FlowStage::TerminalGuard,
                agent_doc_flow::types::FlowOutcome::Blocked,
                agent_doc_turn::closeout_guard::CloseoutGuardReason::AlreadyCommitted,
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{} file={} cycle_id={} patch_id={} reason=already_committed",
                    OpsLogEvent::LateFallbackPatchRejected,
                    file.display(),
                    cycle_id,
                    patch_id
                ),
            );
            return Ok(IpcResult {
                success: false,
                patch_id,
                skipped_committed_cycle: true,
            });
        }
    }

    let editor_delivery_target = registered_editor_delivery_target(file);
    let reliable_editor_live = agent_doc_crdt_relay_io::reliable_sync_editor_live_for_file(file);
    match decide_editor_delivery_admission(EditorDeliveryAdmissionFacts {
        reliable_editor_live,
        registration_available: editor_delivery_target.is_some(),
    }) {
        EditorDeliveryAdmission::DeliverToLiveEditor => {}
        EditorDeliveryAdmission::Detached => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "ipc_editor_delivery_skipped file={} reliable_editor_live=false registration_available=false action=detached_fallback",
                    file.display()
                ),
            );
            return Ok(IpcResult {
                success: false,
                patch_id,
                skipped_committed_cycle: false,
            });
        }
        EditorDeliveryAdmission::RefuseIncompleteRegistration => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "ipc_editor_delivery_refused file={} reliable_editor_live={} registration_available={} action=fail_closed_before_payload",
                    file.display(),
                    reliable_editor_live,
                    editor_delivery_target.is_some(),
                ),
            );
            anyhow::bail!(
                "refused editor IPC for {} because reliable document liveness has no matching editor registration (reliable_editor_live={}, registration_available={}); refresh or update the editor integration before retrying closeout",
                file.display(),
                reliable_editor_live,
                editor_delivery_target.is_some(),
            );
        }
    }
    let editor_delivery_target =
        editor_delivery_target.expect("delivery admission proves a live targeted editor endpoint");

    // Delivery is a single targeted state-machine transition. The endpoint pid
    // comes from the same Lazily registration as the editor id; unavailable or
    // slow endpoints retain the transition for retry and never create a second
    // filesystem delivery head.
    if agent_doc_ipc_io::is_listener_active_for_pid(&project_root, editor_delivery_target.pid) {
        // Seed the boundary from patch_id so the socket patch and any later file /
        // run_stream fallback rebuild share an IDENTICAL boundary — otherwise a
        // late socket apply + file apply land the response twice
        // (#finalize-visible-buffer-ipc-timeout-race).
        let ipc_patches_json = build_ipc_patches_json(
            file,
            patches,
            unmatched,
            normalize_prefix_lines,
            Some(&patch_id),
        )?;
        let ipc_node_patches_json =
            build_ipc_node_patches_json(baseline.or(ipc_before_content.as_deref()), content_ours);
        // When unmatched content was synthesized into a patch (no explicit patch blocks),
        // don't also send it as "unmatched" — the plugin would apply both and duplicate.
        let effective_unmatched_socket =
            effective_unmatched_for_patch_payload(unmatched, patches.len(), ipc_patches_json.len());
        if effective_unmatched_socket.is_empty()
            && !unmatched.trim().is_empty()
            && patches.is_empty()
            && !ipc_patches_json.is_empty()
        {
            eprintln!(
                "[write] synthesis consumed unmatched content — clearing from socket payload (prevent double-apply)"
            );
        }
        let mut socket_payload = serde_json::json!({
            "type": agent_doc_ipc_protocol::EditorIntent::ApplyCanonical.as_str(),
            "file": canonical.to_string_lossy(),
            "patches": ipc_patches_json,
            "node_patches": ipc_node_patches_json,
            "unmatched": effective_unmatched_socket,
            "baseline": baseline.unwrap_or(""),
            "reposition_boundary": true,
        });
        if let Some(target_baseline) = ipc_before_content.as_deref().or(baseline) {
            socket_payload["baseline_hash"] =
                serde_json::Value::String(agent_doc_hash::content_hash(target_baseline));
            socket_payload["baseline_normalized_hash"] =
                serde_json::Value::String(agent_doc_hash::content_hash(
                    &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                        target_baseline,
                    ),
                ));
        }
        socket_payload["patch_id"] = serde_json::Value::String(patch_id.clone());
        if let Some(cycle_id) = projected_cycle_id(file) {
            socket_payload["cycle_id"] = serde_json::Value::String(cycle_id);
        }
        if let Some(yaml) = frontmatter_yaml {
            socket_payload["frontmatter"] = serde_json::Value::String(yaml.to_string());
        }
        if let Some(lines) = normalize_prefix_lines
            && !lines.is_empty()
        {
            socket_payload["normalize_prefix_lines"] = serde_json::Value::Array(
                lines
                    .iter()
                    .map(|l| serde_json::Value::String(l.clone()))
                    .collect(),
            );
            if ipc_patches_json.is_empty()
                && let Some(ours) = content_ours
                && full_content_ipc_scope_allows(
                    file,
                    FullContentIpcMode::ResponseFallback,
                    &patch_id,
                    ours,
                    ipc_before_content.as_deref(),
                    ipc_before_content.as_deref(),
                )
            {
                log_full_content_ipc_disabled(
                    file,
                    FullContentIpcMode::ResponseFallback,
                    &patch_id,
                    ours,
                    ipc_before_content.as_deref(),
                    ipc_before_content.as_deref(),
                );
            }
        }
        target_payload_to_editor(
            file,
            &mut socket_payload,
            "socket_patch",
            &editor_delivery_target.editor_id,
            editor_delivery_target.pid,
        );
        let socket_editor_id = Some(editor_delivery_target.editor_id.clone());
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_socket_attempt file={} hash={} patch_id={} patches={} ipc_patches={} unmatched_len={} effective_unmatched_len={} baseline_len={} normalize_targets={} unmatched_marker_count={}",
                file.display(),
                hash,
                patch_id,
                patches.len(),
                ipc_patches_json.len(),
                unmatched.trim().len(),
                effective_unmatched_socket.len(),
                baseline.map(str::len).unwrap_or(0),
                normalize_prefix_lines.map(|lines| lines.len()).unwrap_or(0),
                agent_doc_template::patchback::patchback_marker_count_outside_code(unmatched)
            ),
        );
        match agent_doc_ipc_io::send_message_to_pid(
            &project_root,
            editor_delivery_target.pid,
            &socket_payload,
        ) {
            Ok(Some(_ack)) => {
                eprintln!("[write] socket IPC patch delivered");
                clear_ipc_socket_ack_timeouts(&project_root, file, "socket_ack")?;
                // Poll for lazily-backed visible-write proof (published by the
                // editor after the Document API applies the patch).
                let visible_write = poll_visible_write_content_lazily_event_or_projection(
                    file,
                    &project_root,
                    &patch_id,
                    agent_doc_write_converge_io::visible_write_receipt_timeout(),
                    agent_doc_write_converge_io::visible_write_receipt_poll_interval(),
                )?;
                if let Some(visible_write_content) = visible_write {
                    let snap_content = visible_write_content.content;
                    let mut repair_decision = ipc_repair_decision_from_visible_write(
                        file,
                        Some(&patch_id),
                        baseline,
                        snap_content,
                        content_ours,
                        normalize_prefix_lines,
                    );
                    let pre_dedupe_snap = repair_decision.snapshot_content.clone();
                    let (effective_snap, dedupe_repair) = dedupe_ipc_snapshot_content(
                        file,
                        ipc_before_content.as_deref(),
                        &repair_decision.snapshot_content,
                        repair_decision.snap_source.label(),
                    )?;
                    if dedupe_repair {
                        repair_decision =
                            repair_decision.apply_ipc_dedupe(effective_snap, pre_dedupe_snap);
                    } else {
                        repair_decision.snapshot_content = effective_snap;
                    }
                    let expected_response =
                        agent_doc_template::response_materialization::response_materialization_probe(
                            patches, unmatched,
                        );
                    let visible_source = "socket_visible_write";
                    prefer_visible_content_over_stale_visible_write_snapshot(
                        file,
                        visible_source,
                        Some(&patch_id),
                        baseline,
                        content_ours,
                        &expected_response,
                        &mut repair_decision,
                    );
                    // Capture the live editor buffer before the guards replace it,
                    // so the #ipcfullprompt forensic detector sees the candidate.
                    let ipcfullprompt_candidate = repair_decision.snapshot_content.clone();
                    let drift_fired = guard_ipc_snapshot_adoption_against_live_prompt_drift(
                        file,
                        visible_source,
                        Some(&patch_id),
                        baseline,
                        content_ours,
                        &mut repair_decision,
                    );
                    let dup_fired = guard_ipc_snapshot_adoption_against_prompt_duplication(
                        file,
                        visible_source,
                        Some(&patch_id),
                        content_ours,
                        &mut repair_decision,
                    );
                    let missing_response_repair =
                        materialize_missing_response_for_socket_visible_write_drift(
                            file,
                            Some(&patch_id),
                            content_ours,
                            &expected_response,
                            drift_fired,
                            &mut repair_decision,
                        );
                    log_ipc_snapshot_adoption_allowed(
                        file,
                        visible_source,
                        Some(&patch_id),
                        baseline,
                        content_ours,
                        &repair_decision,
                        drift_fired || dup_fired || missing_response_repair,
                    );
                    log_ipcfullprompt_corruption_if_any(
                        file,
                        visible_source,
                        Some(&patch_id),
                        baseline,
                        &ipcfullprompt_candidate,
                    );

                    if !ipc_response_materialized_or_fallback(
                        file,
                        visible_source,
                        &expected_response,
                        &repair_decision.snapshot_content,
                    ) {
                        log_partial_response_materialization_for_retry(
                            file,
                            visible_source,
                            &expected_response,
                        )?;
                        return Ok(IpcResult {
                            success: false,
                            patch_id,
                            skipped_committed_cycle: false,
                        });
                    }

                    eprintln!(
                        "[write] snapshot from {} ({} bytes)",
                        repair_decision.snap_source.label(),
                        repair_decision.snapshot_content.len()
                    );
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "ipc_socket_visible_write file={} patch_id={} snap_source={} visible_len={} visible_hash={} disk_len={} disk_hash={}",
                            file.display(),
                            patch_id,
                            repair_decision.snap_source.label(),
                            repair_decision.snapshot_content.len(),
                            agent_doc_hash::content_hash(&repair_decision.snapshot_content),
                            ipc_before_content.as_deref().map(str::len).unwrap_or(0),
                            ipc_before_content
                                .as_deref()
                                .map(agent_doc_hash::content_hash)
                                .unwrap_or_else(|| "-".to_string())
                        ),
                    );
                    // #adoc-live-prompt-drift-operator-edit: if the operator kept
                    // editing past the ack capture, reconcile the snapshot forward to
                    // their newer live buffer BEFORE the visible-state repair and disk
                    // proof, so the closeout persists the operator's latest edits and
                    // the proof matches the live buffer instead of wedging on a stale
                    // source buffer.
                    reconcile_visible_write_snapshot_to_newer_operator_buffer(
                        file,
                        socket_editor_id.as_deref(),
                        &mut repair_decision,
                    );
                    agent_doc_write_converge_io::repair_ipc_decision_visible_state(
                        effects,
                        file,
                        &repair_decision,
                        Some(&patch_id),
                        |file, repaired_content, expected_bad_state| {
                            agent_doc_write_converge_io::try_ipc_full_content_response_fallback_from_source(
                                effects,
                                file,
                                repaired_content,
                                expected_bad_state,
                            )
                        },
                    )?;
                    if repair_decision.snap_source.is_visible_write_proven() {
                        // Fold the content-bearing editor ACK into the canonical relay
                        // before asking whether disk materialization is safe. A controller
                        // recycle may leave a durable editor owner with zero registered
                        // replicas; in that shape the Lazily receipt is the only complete
                        // content proof and disk must not be used to rediscover it.
                        fold_visible_write_into_canonical(
                            file,
                            &patch_id,
                            &repair_decision.snapshot_content,
                            "socket_visible_write",
                        )?;
                        let proof = visible_write_disk_proof(
                            file,
                            socket_editor_id.as_deref(),
                            &repair_decision.snapshot_content,
                        );
                        let disk_synced = write_visible_write_through_to_disk(
                            effects,
                            file,
                            &patch_id,
                            &repair_decision.snapshot_content,
                            proof,
                        )?;
                        if !disk_synced {
                            return Ok(IpcResult {
                                success: false,
                                patch_id,
                                skipped_committed_cycle: false,
                            });
                        }
                        mark_visible_write_live_buffer_synced_after_write(
                            file,
                            &patch_id,
                            socket_editor_id.as_deref(),
                            &repair_decision.snapshot_content,
                        );
                    }
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "ipc_socket_delivered file={} snap_source={} snap_len={}",
                            file.display(),
                            repair_decision.snap_source.label(),
                            repair_decision.snapshot_content.len()
                        ),
                    );
                    if let Some(before) = ipc_before_content.as_deref() {
                        log_exchange_write_diagnostic(
                            file,
                            "try_ipc_socket",
                            "socket_ipc",
                            Some(&patch_id),
                            baseline,
                            before,
                            &repair_decision.snapshot_content,
                            patches,
                            unmatched,
                        );
                    }
                    checkpoint_ipc_baseline_nonfatal(
                        file,
                        &repair_decision.snapshot_content,
                        "snapshot_saved_socket_ipc",
                        None,
                    );
                    return Ok(IpcResult {
                        success: true,
                        patch_id,
                        skipped_committed_cycle: false,
                    });
                }
                // The endpoint accepted and applied the command, but the Lazily
                // visible-write proof has not arrived. Retain this exact transition
                // for replay; creating another delivery carrier would fork intent.
                let receipt_detail =
                    missing_lazily_receipt_detail(file, "lazily_receipt_timeout=true");
                eprintln!(
                    "[write] socket delivered but Lazily visible-write receipt is pending — retaining transition for replay ({receipt_detail})"
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ipc_socket_visible_write_receipt_slow_no_degrade file={} patch_id={}",
                        file.display(),
                        patch_id
                    ),
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ipc_socket_visible_write_receipt_timeout file={} recovery=retain_same_transition_no_sidecar",
                        file.display()
                    ),
                );
                log_ipc_proof_failure(
                    file,
                    "socket_ipc",
                    Some(&patch_id),
                    "no_lazily_visible_write_receipt",
                    "retain_same_transition",
                    &receipt_detail,
                );
                return Ok(IpcResult {
                    success: false,
                    patch_id,
                    skipped_committed_cycle: false,
                });
            }
            Ok(None) => {
                eprintln!("[write] socket IPC sent but no receipt — retaining transition");
                return Ok(IpcResult {
                    success: false,
                    patch_id,
                    skipped_committed_cycle: false,
                });
            }
            Err(e) if is_already_applied_receipt_error_message(&e.to_string()) => {
                // The plugin detected the response body is already present
                // in the live buffer and chose not to re-apply it. Re-writing
                // through any second transport would create a duplicate
                // response. Treat the retained intent as already visible.
                // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
                // Phase 2.
                eprintln!(
                    "[write] editor endpoint reported already_applied: {} — retaining the same visible intent",
                    e
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_no_file_transport file={} patch_id={}",
                        file.display(),
                        patch_id
                    ),
                );
                let expected_response =
                    agent_doc_template::response_materialization::response_materialization_probe(
                        patches, unmatched,
                    );
                if persist_already_applied_socket_content_ours_snapshot(
                    effects,
                    AlreadyAppliedSocketSnapshotContext {
                        file,
                        patch_id: &patch_id,
                        editor_id: socket_editor_id.as_deref(),
                        baseline,
                        content_ours,
                        normalize_prefix_lines,
                        expected_response: &expected_response,
                    },
                )? == AlreadyAppliedSnapshotOutcome::Persisted
                {
                    return Ok(IpcResult {
                        success: true,
                        patch_id,
                        skipped_committed_cycle: false,
                    });
                }
                eprintln!(
                    "[write] socket already_applied lacked an authoritative editor receipt containing the response — retaining the response operation for CP retry"
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_defer_authoritative_retry file={} patch_id={} secondary_transport=false disk_write=false",
                        file.display(),
                        patch_id
                    ),
                );
                return Ok(IpcResult {
                    success: false,
                    patch_id,
                    skipped_committed_cycle: false,
                });
            }
            Err(e) => {
                eprintln!(
                    "[write] targeted socket IPC failed: {} — retaining transition",
                    e
                );
                if is_socket_receipt_timeout_error(e.to_string()) {
                    let degraded = record_ipc_socket_ack_timeout(
                        &project_root,
                        file,
                        Some(&patch_id),
                        "socket_ipc",
                    )?;
                    let _ = degraded;
                }
                return Ok(IpcResult {
                    success: false,
                    patch_id,
                    skipped_committed_cycle: false,
                });
            }
        }
    }

    Ok(IpcResult {
        success: false,
        patch_id,
        skipped_committed_cycle: false,
    })
}

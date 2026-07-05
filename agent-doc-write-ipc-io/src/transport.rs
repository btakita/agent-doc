//! Write IPC transport: socket-first delivery with durable file-IPC fallback.

use crate::{IpcResult, build_ipc_patches_json, patch_response_headings_already_in_head};
use agent_doc_document::write_normalization::strip_boundary_for_dedup;
use agent_doc_element_exchange::{
    exchange_has_live_user_edit, exchange_prompt_prefix_count, exchange_prompt_text_duplicated,
};
use agent_doc_flow::types::{FlowOutcome, FlowStage};
use agent_doc_flow_io::closeout::{cleanup_fallback_patch_files, cycle_already_committed};
use agent_doc_ipc_io::editor_target::target_payload_to_live_editor;
use agent_doc_ipc_protocol::{
    AlreadyAppliedSnapshotOutcome, FullContentIpcMode, IpcSnapshotSource,
    build_ipc_node_patches_json, effective_unmatched_for_patch_payload,
    is_already_applied_receipt_error_message, is_socket_receipt_timeout_error,
};
use agent_doc_template as template;
use agent_doc_template::stale_baseline::patch_touches_exchange;
use agent_doc_write_converge_io::{
    AlreadyAppliedSocketSnapshotContext, cleanup_legacy_ipc_degraded,
    clear_ipc_socket_ack_timeouts, dedupe_ipc_snapshot_content, full_content_ipc_scope_allows,
    guard_ipc_snapshot_adoption_against_live_prompt_drift,
    guard_ipc_snapshot_adoption_against_prompt_duplication, ipc_direct_disk_degraded,
    ipc_repair_decision_from_visible_write, log_full_content_ipc_disabled,
    log_ipc_dewedge_prefer_file_ipc, log_ipc_snapshot_adoption_allowed,
    log_ipcfullprompt_corruption_if_any, mark_visible_write_live_buffer_synced_after_write,
    materialize_missing_response_for_socket_visible_write_drift,
    persist_already_applied_socket_content_ours_snapshot,
    poll_visible_write_content_lazily_event_or_projection,
    prefer_visible_content_over_stale_visible_write_snapshot,
    reconcile_visible_write_snapshot_to_newer_operator_buffer, record_ipc_socket_ack_timeout,
    save_ipc_snapshot_and_crdt_nonfatal, stale_supervisor_write_short_circuit,
    visible_write_disk_proof, write_visible_write_through_to_disk,
};
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
        |file, source| {
            let _ =
                agent_doc_write_converge_io::schedule_stale_supervisor_pcp_recycle(file, source);
        },
    )
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
        |file, source| {
            let _ =
                agent_doc_write_converge_io::schedule_stale_supervisor_pcp_recycle(file, source);
        },
    );
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
    let cycle_id = agent_doc_cycle_state_io::load(file)
        .ok()
        .flatten()
        .map(|state| state.cycle_id)
        .unwrap_or_else(|| "-".to_string());
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
    let ipc_before_content = std::fs::read_to_string(file).ok();

    // `#turnsaferecycle` Goal 3 — shared stale-supervisor short-circuit. Before any
    // proof-retry work, if the hosting supervisor is running a stale binary, skip the
    // doomed IPC write, schedule the recycle, and defer uniformly (returns a
    // non-success result so the caller retains the response for the post-recycle
    // retry — never a disk write). Fresh supervisor → `None`, proceed normally.
    if stale_supervisor_write_short_circuit(file, "try_ipc").is_some() {
        return Ok(IpcResult {
            success: false,
            patch_id,
            skipped_committed_cycle: false,
        });
    }

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
        let response_in_head = patch_response_headings_already_in_head(file, patches);
        if !response_in_head {
            eprintln!(
                "[write] mid-turn cycle rotation detected for {}: cycle {} marked committed \
                 but the incoming response heading(s) are absent from HEAD — starting a fresh \
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
            let snapshot_content = agent_doc_snapshot_io::load(file)?;
            let file_content_for_state = std::fs::read_to_string(file).ok();
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
                    "late_fallback_patch_rejected file={} cycle_id={} patch_id={} reason=already_committed",
                    file.display(),
                    cycle_id,
                    patch_id
                ),
            );
            cleanup_fallback_patch_files(file);
            return Ok(IpcResult {
                success: false,
                patch_id,
                skipped_committed_cycle: true,
            });
        }
    }

    // Clean up any legacy degraded marker from older versions
    cleanup_legacy_ipc_degraded(&project_root);

    // `#ipc-degraded-prefers-file-ipc`: when the socket listener is latched
    // degraded, do NOT jump straight to a raw disk write. Skip only the (wedged)
    // socket attempt and let the write fall through to the file-IPC patch queue
    // below — the plugin's file watcher still applies it via the Document API,
    // so a degraded session never manufactures an IDEA "File Cache Conflict".
    // If file IPC also cannot prove delivery, callers fail closed and retry
    // instead of writing the document directly.
    let socket_degraded = ipc_direct_disk_degraded(&project_root, file)?;
    if socket_degraded {
        eprintln!(
            "[write] IPC socket degraded for {} — preferring file-IPC patch queue (no automatic disk fallback)",
            file.display()
        );
        log_ipc_dewedge_prefer_file_ipc(file, "try_ipc");
    }

    // Try socket IPC first (lower latency, no inotify) unless the socket is
    // latched degraded — in that case the file-IPC patch queue below is the
    // reliable plugin path.
    if !socket_degraded && agent_doc_ipc_io::is_listener_active(&project_root) {
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
            "type": "patch",
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
        if let Ok(Some(ref cs)) = agent_doc_cycle_state_io::load(file) {
            socket_payload["cycle_id"] = serde_json::Value::String(cs.cycle_id.clone());
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
        let socket_editor_id =
            target_payload_to_live_editor(file, &mut socket_payload, "socket_patch");
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
        // Pre-write fallback patch file before socket send. If socket delivery
        // succeeds but sidecar ack times out, the file watcher can recover the
        // response from this file. patch_id dedup prevents double-apply when
        // both socket and file watcher fire. Overwrites any stale content.
        let fallback_patch_file = {
            let patches_dir = project_root.join(".agent-doc/patches");
            if patches_dir.exists() {
                let path = patches_dir.join(format!("{}.json", hash));
                match serde_json::to_string_pretty(&socket_payload) {
                    Ok(json) => {
                        if let Err(e) = std::fs::write(&path, &json) {
                            eprintln!(
                                "[write] WARNING: failed to write fallback patch file: {}",
                                e
                            );
                            None
                        } else {
                            eprintln!("[write] fallback patch file pre-written for recovery");
                            Some(path)
                        }
                    }
                    Err(e) => {
                        eprintln!("[write] WARNING: failed to serialize fallback patch: {}", e);
                        None
                    }
                }
            } else {
                None
            }
        };
        match agent_doc_ipc_io::send_message(&project_root, &socket_payload) {
            Ok(Some(_ack)) => {
                eprintln!("[write] socket IPC patch delivered");
                clear_ipc_socket_ack_timeouts(&project_root, file, "socket_ack")?;
                // Poll for lazily-backed visible-write proof (published by the
                // editor after the Document API applies the patch).
                let visible_write = poll_visible_write_content_lazily_event_or_projection(
                    file,
                    &project_root,
                    &patch_id,
                    std::time::Duration::from_millis(200),
                    std::time::Duration::from_millis(25),
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
                    if let Some(ref path) = fallback_patch_file {
                        let _ = std::fs::remove_file(path);
                    }
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
                    save_ipc_snapshot_and_crdt_nonfatal(
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
                // `#ipc-degrade-false-vote`: the socket returned a transport
                // response (`Ok(Some(_ack))`), so the plugin received the patch
                // and is applying it through the Document API — only the lazily
                // visible-write receipt was slow. A slow receipt is NOT a listener
                // timeout: it must not vote toward the de-wedge degrade threshold
                // and must not latch this session to disk-only.
                // Recover the snapshot through the file-IPC patch queue below
                // (still the plugin path) so a confirmed-but-slow delivery never
                // manufactures a raw foreign disk write — the source of IDEA
                // "File Cache Conflict". Genuine transport failures still vote in
                // the `Err(timeout)` arm.
                eprintln!(
                    "[write] socket delivered but lazily visible-write receipt was slow — recovering snapshot via file-IPC fallback (no degrade vote)"
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ipc_socket_visible_write_receipt_slow_no_degrade file={} patch_id={}",
                        file.display(),
                        patch_id
                    ),
                );
                if fallback_patch_file.is_some() {
                    eprintln!("[write] fallback patch file left for file watcher recovery");
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ipc_socket_visible_write_receipt_timeout file={} — retrying through file_ipc_without_disk_write",
                        file.display()
                    ),
                );
                log_ipc_proof_failure(
                    file,
                    "socket_ipc",
                    Some(&patch_id),
                    "no_lazily_visible_write_receipt",
                    "file_ipc_retry_without_disk_write",
                    "lazily_receipt_timeout=true",
                );
                if let Some(ref cycle_id) = cycle_already_committed(file) {
                    eprintln!(
                        "[write] socket IPC fallback: cycle {} already committed — skipping file IPC",
                        cycle_id
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
                            "ipc_socket_visible_write_receipt_timeout_skip_file_fallback file={} cycle_id={} reason=already_committed",
                            file.display(),
                            cycle_id
                        ),
                    );
                    cleanup_fallback_patch_files(file);
                    return Ok(IpcResult {
                        success: false,
                        patch_id,
                        skipped_committed_cycle: true,
                    });
                }
            }
            Ok(None) => {
                eprintln!("[write] socket IPC sent but no receipt — falling back to file IPC");
            }
            Err(e) if is_already_applied_receipt_error_message(&e.to_string()) => {
                // The plugin detected the response body is already present
                // in the live buffer and chose not to re-apply it. Re-writing
                // through the file-IPC fallback would create a duplicate
                // response. Treat as success and skip the fallback.
                // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
                // Phase 2.
                eprintln!(
                    "[write] socket IPC reported already_applied: {} — skipping file IPC fallback (response already in live buffer)",
                    e
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_skip_file_fallback file={} patch_id={}",
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
                    cleanup_fallback_patch_files(file);
                    return Ok(IpcResult {
                        success: true,
                        patch_id,
                        skipped_committed_cycle: false,
                    });
                }
                eprintln!(
                    "[write] socket already_applied could not prove the response on disk — falling back to file IPC"
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_fallback_to_file_ipc file={} patch_id={}",
                        file.display(),
                        patch_id
                    ),
                );
            }
            Err(e) => {
                eprintln!(
                    "[write] socket IPC failed: {} — falling back to file IPC",
                    e
                );
                if is_socket_receipt_timeout_error(e.to_string()) {
                    let degraded = record_ipc_socket_ack_timeout(
                        &project_root,
                        file,
                        Some(&patch_id),
                        "socket_ipc",
                    )?;
                    if degraded {
                        // `#ipc-degraded-prefers-file-ipc`: the socket just
                        // latched degraded, but the plugin's file watcher is a
                        // separate transport that is very likely still alive.
                        // Fall through to the file-IPC patch queue below instead
                        // of skipping straight to a raw disk write — the plugin
                        // applies the queued patch via the Document API, so this
                        // degraded write never manufactures a File Cache Conflict.
                        // File-IPC timeout now fails closed so the editor remains authoritative.
                        eprintln!(
                            "[write] IPC socket degraded for {} after repeated socket receipt timeouts — falling back to file-IPC patch queue (no automatic disk fallback)",
                            file.display()
                        );
                        log_ipc_dewedge_prefer_file_ipc(file, "socket_ipc_timeout");
                    }
                }
            }
        }
    }

    let patches_dir = project_root.join(".agent-doc/patches");

    // Only attempt file-based IPC if the patches directory exists (plugin has started)
    if !patches_dir.exists() {
        return Ok(IpcResult {
            success: false,
            patch_id,
            skipped_committed_cycle: false,
        });
    }

    if let Some(ref cycle_id) = cycle_already_committed(file) {
        eprintln!(
            "[write] file IPC fallback: cycle {} already committed — skipping patch write",
            cycle_id
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
                "file_ipc_fallback_skip file={} cycle_id={} reason=already_committed",
                file.display(),
                cycle_id
            ),
        );
        cleanup_fallback_patch_files(file);
        return Ok(IpcResult {
            success: false,
            patch_id,
            skipped_committed_cycle: true,
        });
    }

    let patch_file = patches_dir.join(format!("{}.json", hash));

    // Build patches using shared helper (same logic as socket path). Seed the
    // boundary from patch_id so a later file/fallback rebuild reuses the same
    // boundary (#finalize-visible-buffer-ipc-timeout-race).
    let ipc_patches = build_ipc_patches_json(
        file,
        patches,
        unmatched,
        normalize_prefix_lines,
        Some(&patch_id),
    )?;
    let ipc_node_patches =
        build_ipc_node_patches_json(baseline.or(ipc_before_content.as_deref()), content_ours);

    // Same dedup guard as socket path: don't send unmatched when it was synthesized into a patch.
    let effective_unmatched_file =
        effective_unmatched_for_patch_payload(unmatched, patches.len(), ipc_patches.len());

    let mut ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": ipc_patches,
        "node_patches": ipc_node_patches,
        "unmatched": effective_unmatched_file,
        "baseline": baseline.unwrap_or(""),
        "reposition_boundary": true,
    });
    if let Some(target_baseline) = ipc_before_content.as_deref().or(baseline) {
        ipc_payload["baseline_hash"] =
            serde_json::Value::String(agent_doc_hash::content_hash(target_baseline));
        ipc_payload["baseline_normalized_hash"] =
            serde_json::Value::String(agent_doc_hash::content_hash(
                &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                    target_baseline,
                ),
            ));
    }
    ipc_payload["patch_id"] = serde_json::Value::String(patch_id.clone());
    if let Ok(Some(ref cs)) = agent_doc_cycle_state_io::load(file) {
        ipc_payload["cycle_id"] = serde_json::Value::String(cs.cycle_id.clone());
    }

    if let Some(yaml) = frontmatter_yaml {
        ipc_payload["frontmatter"] = serde_json::Value::String(yaml.to_string());
    }
    if let Some(lines) = normalize_prefix_lines
        && !lines.is_empty()
    {
        ipc_payload["normalize_prefix_lines"] = serde_json::Value::Array(
            lines
                .iter()
                .map(|l| serde_json::Value::String(l.clone()))
                .collect(),
        );
        if ipc_patches.is_empty()
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
    target_payload_to_live_editor(file, &mut ipc_payload, "file_patch");

    // Log IPC write details for debugging cross-contamination
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_write_attempt file={} hash={} patches={} ipc_patches={} unmatched_len={}",
            file.display(),
            hash,
            patches.len(),
            ipc_patches.len(),
            unmatched.trim().len()
        ),
    );

    // Warn when unmatched content exists but no IPC patches were synthesized —
    // this means content will be silently dropped by the plugin
    if ipc_patches.is_empty() && !unmatched.trim().is_empty() {
        eprintln!(
            "[write] WARNING: {} bytes of unmatched content with no IPC patches — content will be dropped. \
             Does the target file have template components (<!-- agent:exchange -->)?",
            unmatched.trim().len()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_unmatched_content_dropped file={} unmatched_len={}",
                file.display(),
                unmatched.trim().len()
            ),
        );
    }

    // Defense-in-depth dedupe gate for the file-IPC fallback when delivering
    // a response patch. When the plugin already applied the response via a
    // prior socket retry whose ack-write was slow, applying the same response
    // patch through file IPC would land a duplicate `### Re:` heading on top
    // of the live buffer.
    //
    // The socket-IPC path catches this via
    // `agent_doc_ipc_protocol::is_already_applied_receipt_error_message` when the
    // plugin sends `{"type":"receipt","status":"applied","reason":"already_applied"}`.
    // Until every plugin emits that ack (`#ipcpluginalready`), the file-IPC
    // fallback hash-compares response-patch outcomes against the current file:
    // if applying the response patches to the current file is a structural
    // no-op (boundary markers excluded), skip the write so the duplicate
    // cannot land.
    //
    // Scope: only response-bearing patches (contain at least one `### Re:`
    // heading). Pure prompt/component patches fall through to the existing
    // path, which has its own no-ack guard for unacknowledged live-edit IPC.
    //
    // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
    // Phase 2 (remaining) / `[#ipcfilehashskip]`.
    if !patches.is_empty()
        && patches
            .iter()
            .any(|patch| patch.content.contains("### Re:"))
        && let Ok(current) = std::fs::read_to_string(file)
        && let Ok(after_apply) = agent_doc_template_io::apply_patches(&current, patches, "", file)
        && strip_boundary_for_dedup(&after_apply) == strip_boundary_for_dedup(&current)
    {
        eprintln!(
            "[write] file IPC fallback: patches already present in live buffer — skipping file IPC write (defense-in-depth dedupe)"
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "file_ipc_fallback_skip_already_applied file={} patch_id={} patches={}",
                file.display(),
                patch_id,
                patches.len()
            ),
        );
        cleanup_fallback_patch_files(file);
        return Ok(IpcResult {
            success: true,
            patch_id,
            skipped_committed_cycle: false,
        });
    }

    let success = write_ipc_and_poll(
        effects,
        &patch_file,
        &ipc_payload,
        file,
        ipc_patches.len(),
        IpcPollOptions {
            content_ours,
            normalize_prefix_lines,
            project_root: &project_root,
            guard_committed_cycle: true,
        },
    )?;
    Ok(IpcResult {
        success,
        patch_id,
        skipped_committed_cycle: false,
    })
}

pub(crate) struct IpcPollOptions<'a> {
    content_ours: Option<&'a str>,
    normalize_prefix_lines: Option<&'a [String]>,
    project_root: &'a Path,
    guard_committed_cycle: bool,
}

/// Write an IPC patch file and poll for plugin ACK (file deletion).
///
/// Returns `Ok(true)` if consumed, `Ok(false)` on timeout.
pub(crate) fn write_ipc_and_poll(
    effects: &dyn agent_doc_write_converge_io::EditorConvergenceEffects,
    patch_file: &Path,
    payload: &serde_json::Value,
    doc_file: &Path,
    patch_count: usize,
    options: IpcPollOptions<'_>,
) -> Result<bool> {
    let before_content = std::fs::read_to_string(doc_file).ok();
    let delivered = agent_doc_write_converge_io::write_file_ipc_and_poll_delivery(
        effects,
        patch_file,
        payload,
        doc_file,
        patch_count,
        agent_doc_write_converge_io::FileIpcDeliveryOptions {
            guard_committed_cycle: options.guard_committed_cycle,
        },
    )?;
    if !delivered {
        return Ok(false);
    }

    {
        // Plugin consumed the patch file. The authoritative post-apply proof is
        // the lazily visible-write receipt/projection, not deletion of the file
        // and not a compatibility sidecar.
        let patch_id = payload
            .get("patch_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (mut current_on_disk, mut repair_decision, visible_write_proven) = if !patch_id
            .is_empty()
        {
            match poll_visible_write_content_lazily_event_or_projection(
                doc_file,
                options.project_root,
                patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )? {
                Some(visible_write_content) => {
                    let baseline = payload
                        .get("baseline")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty());
                    let decision = ipc_repair_decision_from_visible_write(
                        doc_file,
                        Some(patch_id),
                        baseline,
                        visible_write_content.content,
                        options.content_ours,
                        options.normalize_prefix_lines,
                    );
                    if decision.snap_source == IpcSnapshotSource::LazilyVisibleWriteEvent {
                        eprintln!(
                            "[write] snapshot from lazily visible-write receipt ({} bytes)",
                            decision.snapshot_content.len()
                        );
                    }
                    let visible_write_proven = decision.visible_write_proven();
                    let snapshot_content = decision.snapshot_content.clone();
                    (snapshot_content, decision, visible_write_proven)
                }
                None => {
                    eprintln!(
                        "[write] file IPC consumed but lazily visible-write receipt was not available after 500ms"
                    );
                    log_ipc_proof_failure(
                        doc_file,
                        "file_ipc",
                        Some(patch_id),
                        "no_lazily_visible_write_receipt",
                        "retry_without_disk_write",
                        "lazily_receipt_timeout=true",
                    );
                    return Ok(false);
                }
            }
        } else {
            eprintln!(
                "[write] file IPC consumed without patch_id; lazily receipt proof unavailable"
            );
            log_ipc_proof_failure(
                doc_file,
                "file_ipc",
                None,
                "missing_patch_id",
                "retry_without_disk_write",
                "lazily_receipt_unavailable=true",
            );
            return Ok(false);
        };
        let baseline_content = payload
            .get("baseline")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !baseline_content.is_empty() && current_on_disk == baseline_content {
            // File on disk hasn't changed — plugin likely failed to apply the patch.
            // Don't save snapshot with content that was never applied.
            eprintln!(
                "[write] IPC patch consumed but file unchanged on disk — plugin may have failed to apply; retry required."
            );
            return Ok(false);
        }

        if let Some(full_content) = payload
            .get("fullContent")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            && current_on_disk != full_content
        {
            eprintln!(
                "[write] IPC full-content patch consumed but final content does not match payload — retry required."
            );
            agent_doc_ops_log_io::log_op(
                doc_file,
                &format!(
                    "full_content_ipc_post_apply_mismatch file={} expected_len={} actual_len={}",
                    doc_file.display(),
                    full_content.len(),
                    current_on_disk.len()
                ),
            );
            return Ok(false);
        }

        // Verify patch content is present in the file (catches partial application).
        // Check that at least one non-empty patch's content appears in the result.
        let patch_list = payload.get("patches").and_then(|v| v.as_array());
        if let Some(patches) = patch_list {
            let has_content_patch = patches.iter().any(|p| {
                let content = p.get("content").and_then(|c| c.as_str()).unwrap_or("");
                !content.trim().is_empty()
            });
            if has_content_patch {
                let any_present = patches.iter().any(|p| {
                    let content = p.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    if content.trim().is_empty() {
                        return true;
                    }
                    // Check first meaningful line of content appears in file
                    content
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .is_none_or(|first_line| current_on_disk.contains(first_line.trim()))
                });
                if !any_present {
                    eprintln!(
                        "[write] IPC patch consumed but response content not found in file — plugin may have partially failed. Retry required without direct disk write."
                    );
                    return Ok(false);
                }
            }
        }
        let expected_response =
            agent_doc_template_io::response_materialization_probe_from_ipc_payload(payload);
        if prefer_visible_content_over_stale_visible_write_snapshot(
            doc_file,
            "file_ipc",
            Some(patch_id),
            payload
                .get("baseline")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty()),
            options.content_ours,
            &expected_response,
            &mut repair_decision,
        ) {
            current_on_disk = repair_decision.snapshot_content.clone();
        }
        if !ipc_response_materialized_or_fallback(
            doc_file,
            "file_ipc",
            &expected_response,
            &current_on_disk,
        ) {
            log_partial_response_materialization_for_retry(
                doc_file,
                "file_ipc",
                &expected_response,
            )?;
            return Ok(false);
        }
        if agent_doc_element_exchange_io::file_ipc_consumed_without_live_exchange_visible_write_with_log(
            doc_file,
            "file_ipc",
            Some(patch_id),
            payload
                .get("baseline")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty()),
            before_content.as_deref(),
            &current_on_disk,
            visible_write_proven,
            agent_doc_ops_log_io::log_op,
            log_ipc_proof_failure,
        ) {
            return Ok(false);
        }

        // Plugin applied the patch — update snapshot from the lazily visible-write
        // receipt/projection, which is the editor-visible post-write state.
        // Bug 2A fix: snapshot save failure after IPC success is non-fatal.
        let pre_dedupe_content = repair_decision.snapshot_content.clone();
        let (snap_content, dedupe_repair) = dedupe_ipc_snapshot_content(
            doc_file,
            before_content.as_deref(),
            &repair_decision.snapshot_content,
            repair_decision.snap_source.label(),
        )?;
        if dedupe_repair {
            repair_decision = repair_decision.apply_ipc_dedupe(snap_content, pre_dedupe_content);
        } else {
            repair_decision.snapshot_content = snap_content;
        }
        if agent_doc_element_exchange_io::file_ipc_consumed_without_live_exchange_visible_write_with_log(
            doc_file,
            "file_ipc",
            Some(patch_id),
            payload
                .get("baseline")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty()),
            before_content.as_deref(),
            &repair_decision.snapshot_content,
            visible_write_proven,
            agent_doc_ops_log_io::log_op,
            log_ipc_proof_failure,
        ) {
            return Ok(false);
        }
        let file_baseline = payload
            .get("baseline")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty());
        // Capture the live editor buffer before the guards replace it, so the
        // #ipcfullprompt forensic detector sees the candidate.
        let ipcfullprompt_candidate = repair_decision.snapshot_content.clone();
        let drift_fired = guard_ipc_snapshot_adoption_against_live_prompt_drift(
            doc_file,
            "file_ipc",
            Some(patch_id),
            file_baseline,
            options.content_ours,
            &mut repair_decision,
        );
        let dup_fired = guard_ipc_snapshot_adoption_against_prompt_duplication(
            doc_file,
            "file_ipc",
            Some(patch_id),
            options.content_ours,
            &mut repair_decision,
        );
        log_ipc_snapshot_adoption_allowed(
            doc_file,
            "file_ipc",
            Some(patch_id),
            file_baseline,
            options.content_ours,
            &repair_decision,
            drift_fired || dup_fired,
        );
        log_ipcfullprompt_corruption_if_any(
            doc_file,
            "file_ipc",
            Some(patch_id),
            file_baseline,
            &ipcfullprompt_candidate,
        );
        agent_doc_write_converge_io::repair_ipc_decision_visible_state(
            effects,
            doc_file,
            &repair_decision,
            Some(patch_id),
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
            let editor_id = payload.get("editor_id").and_then(|value| value.as_str());
            let proof =
                visible_write_disk_proof(doc_file, editor_id, &repair_decision.snapshot_content);
            let disk_synced = write_visible_write_through_to_disk(
                effects,
                doc_file,
                patch_id,
                &repair_decision.snapshot_content,
                proof,
            )?;
            if !disk_synced {
                return Ok(false);
            }
            mark_visible_write_live_buffer_synced_after_write(
                doc_file,
                patch_id,
                editor_id,
                &repair_decision.snapshot_content,
            );
        }
        agent_doc_ops_log_io::log_op(
            doc_file,
            &format!(
                "ipc_file_delivered file={} snap_len={}",
                doc_file.display(),
                repair_decision.snapshot_content.len()
            ),
        );
        if let Some(before) = before_content.as_deref() {
            let patch_id = payload.get("patch_id").and_then(|value| value.as_str());
            let baseline = payload
                .get("baseline")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty());
            let payload_patches: Vec<template::PatchBlock> = payload
                .get("patches")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            let name = item
                                .get("component")
                                .or_else(|| item.get("name"))
                                .and_then(|value| value.as_str())?;
                            let content = item.get("content").and_then(|value| value.as_str())?;
                            Some(template::PatchBlock::new(name, content))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let unmatched = payload
                .get("unmatched")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            log_exchange_write_diagnostic(
                doc_file,
                "write_ipc_and_poll",
                "file_ipc",
                patch_id,
                baseline,
                before,
                &repair_decision.snapshot_content,
                &payload_patches,
                unmatched,
            );
        }
        save_ipc_snapshot_and_crdt_nonfatal(
            doc_file,
            &repair_decision.snapshot_content,
            "snapshot_saved_file_ipc",
            Some("[write] IPC patch consumed by plugin — snapshot updated"),
        );
        Ok(true)
    }
}

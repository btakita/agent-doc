//! Write convergence sidecar adapters.
//!
//! This crate owns file-backed write-convergence decisions that sit between
//! pure realtime/write policy and durable sidecars. It keeps those decision
//! graphs out of the orchestration command crate.

use agent_doc_document::write_normalization::{
    AGENT_RESPONSE_COMPONENT, blank_components_except,
    convergence_recovered_editor_wins_for_payload,
    convergence_recovered_editor_wins_outside_response, strip_boundary_for_dedup,
};
use agent_doc_document_realtime::write_policy::{
    AckMismatchRecovery, FullContentSourceProof, OperatorReconcileStep, WholeBufferAuthority,
    WholeBufferAuthorityFacts, WholeBufferDelivery, WholeBufferDeliveryAction,
    classify_socket_receipt_mismatch_recovery, decide_whole_buffer_delivery,
    dropped_prompt_lines_after_content_ours, first_response_heading,
    ipc_snapshot_would_absorb_live_prompt_drift_after_preflight, live_prompt_drift_recovery_target,
    new_agent_response_headings, normalize_visible_recovery_compare, operator_reconcile_step,
    response_already_in_current, response_converged_in_visible_target,
    response_target_disjoint_from_user_edit, should_refuse_disk_fallback,
    snapshot_contains_dropped_prompt,
};
use agent_doc_element_exchange::{
    duplicate_prompt_line_count, normalization_prefix_observation_counts,
    normalize_exchange_prefixes_for_targets, user_prompt_count_growth,
    verify_sidecar_normalization,
};
use agent_doc_element_exchange_io::DuplicatePromptRepairOptions;
use agent_doc_ipc_io::editor_target::{
    live_editor_delivery_has_operator_authority, target_payload_to_live_editor,
};
use agent_doc_ipc_protocol::{
    AlreadyAppliedSnapshotOutcome, EditorBadStateFingerprint, FullContentIpcMode,
    FullContentRepairRedelivery, IpcDiskRepairReason, IpcRepairDecision, IpcSnapshotSource,
    is_socket_receipt_timeout_error, is_socket_status_error,
};
use agent_doc_queue::queue_prompt_drift::{
    dropped_queue_prompt_lines_after_content_ours, merge_visible_queue_additions_into_content_ours,
    preserve_content_ours_over_live_queue_deletions,
};
use agent_doc_turn::response_replay::{
    materialize_response_in_current_exchange, response_materialized_in_content,
};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn log_flow_event(file: &Path, event: agent_doc_flow::types::FlowEvent) {
    let message =
        agent_doc_flow::types::flow_event_log_message(&file.display().to_string(), &event);
    agent_doc_ops_log_io::log_op(file, &message);
}

fn current_text_via_recovery_authority(
    file: &Path,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    #[cfg(any(test, feature = "test-support"))]
    if test_local_crdt_relay_enabled(file) {
        return Ok(Some(agent_doc_crdt_relay_io::current_text_for_file(file)?));
    }
    agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
        file, source,
    )
}

#[cfg(any(test, feature = "test-support"))]
fn test_local_crdt_relay_enabled(file: &Path) -> bool {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file)
        .or_else(|| file.parent().map(Path::to_path_buf))
    else {
        return false;
    };
    project_root
        .join(".agent-doc/test-local-crdt-relay")
        .is_file()
}

pub fn save_document_snapshot_and_crdt(file: &Path, snapshot_content: &str) -> Result<()> {
    agent_doc_snapshot_io::save(file, snapshot_content, agent_doc_ops_log_io::log_op)?;
    let crdt_doc = agent_doc_merge::crdt::CrdtDoc::from_text(snapshot_content);
    agent_doc_merge_io::save_document_crdt(file, &crdt_doc.encode_state(), snapshot_content)?;
    Ok(())
}

pub fn save_ipc_snapshot_and_crdt_nonfatal(
    file: &Path,
    snapshot_content: &str,
    saved_log_event: &str,
    success_message: Option<&str>,
) -> bool {
    if let Err(e) =
        agent_doc_snapshot_io::save(file, snapshot_content, agent_doc_ops_log_io::log_op)
    {
        eprintln!(
            "[write] WARNING: IPC write succeeded but snapshot save failed: {}. \
             Commit will auto-recover via divergence detection.",
            e
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "snapshot_save_failed_after_ipc file={} error={}",
                file.display(),
                e
            ),
        );
        return false;
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{saved_log_event} file={} snap_len={}",
            file.display(),
            snapshot_content.len()
        ),
    );
    let crdt_doc = agent_doc_merge::crdt::CrdtDoc::from_text(snapshot_content);
    if let Err(e) =
        agent_doc_merge_io::save_document_crdt(file, &crdt_doc.encode_state(), snapshot_content)
    {
        eprintln!("[write] WARNING: CRDT state save failed: {}", e);
    }
    if let Some(message) = success_message {
        eprintln!("{message}");
    }
    true
}

pub fn try_semantic_merge_convergence(
    base: &str,
    candidate: &str,
    content_ours: &str,
) -> Option<agent_doc_merge::document_cell_merge::DocumentCellMerge> {
    if agent_doc_markdown_ast::overlay::components(base).is_empty()
        || agent_doc_markdown_ast::overlay::components(candidate).is_empty()
        || agent_doc_markdown_ast::overlay::components(content_ours).is_empty()
    {
        return None;
    }

    let active =
        agent_doc_merge::document_cell_merge::ActiveNodes::new().active_component("exchange");
    let sm = agent_doc_merge::document_cell_merge::document_cell_merge_scoped(
        base,
        candidate,
        content_ours,
        &active,
    );

    if sm.merged_doc.is_empty() {
        return None;
    }
    if agent_doc_element::element::structural_corruption_reason(&sm.merged_doc).is_some() {
        return None;
    }
    if !dropped_prompt_lines_after_content_ours(base, candidate, &sm.merged_doc).is_empty() {
        return None;
    }
    if !dropped_queue_prompt_lines_after_content_ours(base, candidate, &sm.merged_doc).is_empty() {
        return None;
    }
    if !preserves_visible_non_component_edits(base, candidate, &sm.merged_doc) {
        return None;
    }
    for heading in new_agent_response_headings(base, candidate) {
        if !sm.merged_doc.contains(&heading) {
            return None;
        }
    }

    Some(sm)
}

fn preserves_visible_non_component_edits(base: &str, candidate: &str, _merged: &str) -> bool {
    let (Some(base_non_components), Some(candidate_non_components)) = (
        blank_components_except(base, &[]),
        blank_components_except(candidate, &[]),
    ) else {
        return false;
    };
    base_non_components == candidate_non_components
}

pub fn log_ipc_snapshot_adoption_allowed(
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

struct StaleVisibleWriteContext<'a> {
    file: &'a Path,
    source: &'a str,
    patch_id: Option<&'a str>,
    baseline: Option<&'a str>,
    content_ours: Option<&'a str>,
    expected_response: &'a str,
}

fn visible_content_supersedes_visible_write_snapshot(
    context: &StaleVisibleWriteContext<'_>,
    snapshot_content: &str,
    visible_content: &str,
) -> bool {
    if strip_boundary_for_dedup(snapshot_content) == strip_boundary_for_dedup(visible_content) {
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
            "{source}_visible_write_snapshot_stale_visible_adopted file={} patch_id={} visible_len={} visible_hash={} snapshot_len={} snapshot_hash={} response_present=true prompt_drift={}",
            context.file.display(),
            context.patch_id.unwrap_or("-"),
            visible_content.len(),
            agent_doc_hash::content_hash(visible_content),
            snapshot_content.len(),
            agent_doc_hash::content_hash(snapshot_content),
            prompt_drift,
            source = context.source
        ),
    );
    true
}

pub fn prefer_visible_content_over_stale_visible_write_snapshot(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    expected_response: &str,
    decision: &mut IpcRepairDecision,
) -> bool {
    if decision.snap_source != IpcSnapshotSource::LazilyVisibleWriteEvent {
        return false;
    }
    if decision.disk_repair_reason.is_some() {
        return false;
    }
    let Ok(visible_content) = std::fs::read_to_string(file) else {
        return false;
    };
    let context = StaleVisibleWriteContext {
        file,
        source,
        patch_id,
        baseline,
        content_ours,
        expected_response,
    };
    if !visible_content_supersedes_visible_write_snapshot(
        &context,
        &decision.snapshot_content,
        &visible_content,
    ) {
        return false;
    }
    *decision = IpcRepairDecision::file_read(visible_content);
    true
}

pub struct AlreadyAppliedSocketSnapshotContext<'a> {
    pub file: &'a Path,
    pub patch_id: &'a str,
    pub editor_id: Option<&'a str>,
    pub baseline: Option<&'a str>,
    pub content_ours: Option<&'a str>,
    pub normalize_prefix_lines: Option<&'a [String]>,
    pub expected_response: &'a str,
}

pub fn persist_already_applied_socket_content_ours_snapshot(
    effects: &dyn EditorConvergenceEffects,
    context: AlreadyAppliedSocketSnapshotContext<'_>,
) -> Result<AlreadyAppliedSnapshotOutcome> {
    let AlreadyAppliedSocketSnapshotContext {
        file,
        patch_id,
        editor_id,
        baseline,
        content_ours,
        normalize_prefix_lines,
        expected_response,
    } = context;
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

    let visible_write_content = if !patch_id.is_empty() {
        file.canonicalize().ok().and_then(|canonical| {
            let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
            poll_visible_write_content_lazily_event_or_projection(
                file,
                &project_root,
                patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )
            .ok()
            .flatten()
            .map(|content| content.content)
        })
    } else {
        None
    };
    let current_source = if visible_write_content.is_some() {
        IpcSnapshotSource::LazilyVisibleWriteEvent
    } else {
        IpcSnapshotSource::FileRead
    };
    let current = visible_write_content.or_else(|| std::fs::read_to_string(file).ok());
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
                log_ipc_proof_failure_with_recycle(
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
                if let Err(defer) = effects.guard_visible_write_idle_and_current(
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
                effects.atomic_write(file, &repaired_current)?;
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
                log_ipc_proof_failure_with_recycle(
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
                IpcSnapshotSource::LazilyVisibleWriteEvent => {
                    IpcRepairDecision::lazily_visible_write(current.to_string())
                }
                IpcSnapshotSource::LegacySidecarProjection
                | IpcSnapshotSource::FileRead
                | IpcSnapshotSource::ContentOurs => {
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
            IpcSnapshotSource::LazilyVisibleWriteEvent => {
                IpcRepairDecision::lazily_visible_write(current.to_string())
            }
            IpcSnapshotSource::LegacySidecarProjection
            | IpcSnapshotSource::FileRead
            | IpcSnapshotSource::ContentOurs => IpcRepairDecision::file_read(current.to_string()),
        };
    }

    prefer_visible_content_over_stale_visible_write_snapshot(
        file,
        "already_applied",
        Some(patch_id),
        baseline,
        Some(ours),
        expected_response,
        &mut repair_decision,
    );

    if expected_response.trim().is_empty() {
        log_ipc_proof_failure_with_recycle(
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
        log_ipc_proof_failure_with_recycle(
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
        log_ipc_proof_failure_with_recycle(
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

    repair_ipc_decision_visible_state(
        effects,
        file,
        &repair_decision,
        Some(patch_id),
        |file, repaired_content, expected_bad_state| {
            try_ipc_full_content_response_fallback_from_source(
                effects,
                file,
                repaired_content,
                expected_bad_state,
            )
        },
    )?;
    if repair_decision.snap_source.is_visible_write_proven() {
        let proof = visible_write_disk_proof(file, editor_id, &repair_decision.snapshot_content);
        let disk_synced = write_visible_write_through_to_disk(
            effects,
            file,
            patch_id,
            &repair_decision.snapshot_content,
            proof,
        )?;
        if !disk_synced {
            return Ok(AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
        }
        mark_visible_write_live_buffer_synced_after_write(
            file,
            patch_id,
            editor_id,
            &repair_decision.snapshot_content,
        );
    }
    save_document_snapshot_and_crdt(file, &repair_decision.snapshot_content)?;
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

pub fn dedupe_consecutive_response_blocks(content: &str, file: &Path) -> String {
    agent_doc_element_exchange_io::dedupe_consecutive_response_blocks_with_log(
        content,
        file,
        agent_doc_ops_log_io::log_op,
    )
}

fn log_duplicate_prompt_residue_guard(file: &Path) {
    log_flow_event(
        file,
        agent_doc_template::structure_guard::template_structure_guard_event(
            agent_doc_template::structure_guard::TemplateStructureGuardReason::DuplicatePromptResidue,
            agent_doc_flow::types::FlowOutcome::FailedClosed,
        ),
    );
}

pub fn dedupe_ipc_snapshot_content(
    file: &Path,
    before: Option<&str>,
    content: &str,
    source: &str,
) -> Result<(String, bool)> {
    let singleton_repair =
        agent_doc_document::singleton_repair::repair_duplicate_singleton_components(
            before, content,
        );
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

pub fn guard_ipc_snapshot_adoption_against_live_prompt_drift(
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
    if let Some(reason) = agent_doc_element::element::structural_corruption_reason(ours) {
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

    let candidate = decision.snapshot_content.clone();
    let (queue_reconciled_ours, ignored_queue_deletions) =
        preserve_content_ours_over_live_queue_deletions(base, &candidate, ours);
    let prior_source = decision.snap_source.label();
    log_flow_event(
        file,
        agent_doc_flow::types::FlowEvent::new(
            agent_doc_flow::types::FlowName::DocumentMutation,
            agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
            agent_doc_flow::types::FlowOutcome::Blocked,
        )
        .with_reason("live_prompt_drift_after_preflight"),
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
    log_ipc_proof_failure_with_recycle(
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

    let response_headings = new_agent_response_headings(base, &queue_reconciled_ours);
    let candidate_response_headings = new_agent_response_headings(base, &candidate);
    let visible_write_response_present = decision.snap_source.is_visible_write_proven()
        && ((!response_headings.is_empty()
            && response_converged_in_visible_target(base, &queue_reconciled_ours, &candidate))
            || (!candidate_response_headings.is_empty()
                && response_converged_in_visible_target(base, &candidate, &candidate)));
    if visible_write_response_present {
        if matches!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergence)
        ) {
            let visible_candidate_has_non_component_edits =
                !preserves_visible_non_component_edits(base, &candidate, &candidate);
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "live_prompt_drift_visible_write_prefix_repair_evaluated file={} source={} patch_id={} visible_candidate_has_non_component_edits={} candidate_len={} candidate_hash={}",
                    file.display(),
                    source,
                    patch_id.unwrap_or("-"),
                    visible_candidate_has_non_component_edits,
                    candidate.len(),
                    agent_doc_hash::content_hash(&candidate),
                ),
            );
            if visible_candidate_has_non_component_edits {
                decision.disk_repair_reason = None;
                decision.editor_bad_state = None;
                decision.normalize_prefix_lines.clear();
                decision.redeliver_editor = false;
            }
            return true;
        }
        if decision.snap_source.is_visible_write_proven() {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "live_prompt_drift_visible_write_authority_preserved file={} source={} patch_id={} candidate_len={} candidate_hash={} agent_target_len={} agent_target_hash={} reason=visible_write_contains_response",
                    file.display(),
                    source,
                    patch_id.unwrap_or("-"),
                    candidate.len(),
                    agent_doc_hash::content_hash(&candidate),
                    queue_reconciled_ours.len(),
                    agent_doc_hash::content_hash(&queue_reconciled_ours),
                ),
            );
            decision.disk_repair_reason = None;
            decision.editor_bad_state = None;
            decision.normalize_prefix_lines.clear();
            decision.redeliver_editor = false;
            return true;
        }
        let dropped_visible_prompts =
            dropped_prompt_lines_after_content_ours(base, &candidate, &queue_reconciled_ours);
        let dropped_visible_queue =
            dropped_queue_prompt_lines_after_content_ours(base, &candidate, &queue_reconciled_ours);
        if dropped_visible_prompts.is_empty()
            && !dropped_visible_queue.is_empty()
            && preserves_visible_non_component_edits(base, &candidate, &candidate)
            && let Some(union) = merge_visible_queue_additions_into_content_ours(
                base,
                &candidate,
                &queue_reconciled_ours,
                &dropped_visible_queue,
            )
            .or_else(|| {
                agent_doc_merge_io::merge_contents(base, &queue_reconciled_ours, &candidate)
                    .ok()
                    .filter(|union| !union.contains("<<<<<<<"))
                    .filter(|union| {
                        dropped_queue_prompt_lines_after_content_ours(base, &candidate, union)
                            .is_empty()
                    })
            })
            && response_converged_in_visible_target(base, &queue_reconciled_ours, &union)
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "live_prompt_drift_visible_write_component_reconciled file={} source={} patch_id={} candidate_len={} candidate_hash={} agent_target_len={} agent_target_hash={} union_len={} union_hash={} queue_prompts={} reason=visible_write_queue_component_reconcile",
                    file.display(),
                    source,
                    patch_id.unwrap_or("-"),
                    candidate.len(),
                    agent_doc_hash::content_hash(&candidate),
                    queue_reconciled_ours.len(),
                    agent_doc_hash::content_hash(&queue_reconciled_ours),
                    union.len(),
                    agent_doc_hash::content_hash(&union),
                    dropped_visible_queue.len(),
                ),
            );
            decision.replace_snapshot_with_content_ours_for_live_prompt_drift(&union, false);
            return true;
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "live_prompt_drift_agent_target_not_snapshot_authority file={} source={} patch_id={} live_candidate_contains_response=true visible_repair_required=false candidate_len={} candidate_hash={} agent_target_len={} agent_target_hash={}",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                candidate.len(),
                agent_doc_hash::content_hash(&candidate),
                queue_reconciled_ours.len(),
                agent_doc_hash::content_hash(&queue_reconciled_ours),
            ),
        );
        decision.replace_snapshot_with_content_ours_for_live_prompt_drift(
            &queue_reconciled_ours,
            false,
        );
        return true;
    }

    if !decision.snap_source.is_visible_write_proven()
        && let Some(sm) = try_semantic_merge_convergence(base, &candidate, &queue_reconciled_ours)
    {
        let merged_doc = sm.merged_doc.clone();
        let outcome_count = sm.outcomes.len();
        let ack_count = sm.requires_ack.len();
        let merged_response_present = !response_headings.is_empty()
            && response_converged_in_visible_target(base, &queue_reconciled_ours, &merged_doc);
        let visible_repair_required = !visible_write_response_present;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "live_prompt_drift_semantic_merged file={} source={} patch_id={} base_len={} base_hash={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={} merged_len={} merged_hash={} outcomes={} acks={} visible_write_response_present={} merged_response_present={} visible_repair_required={} reason=node_keyed_semantic_merge",
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
                visible_write_response_present,
                merged_response_present,
                visible_repair_required,
            ),
        );
        if ack_count > 0 {
            let reasons: Vec<String> = sm
                .requires_ack
                .iter()
                .map(|ack| format!("{}:{}:{}", ack.component, ack.id, ack.reason.token()))
                .collect();
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_cell_merge_ack_pending file={} source={} patch_id={} ack_count={} reasons={}",
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
                    "[write] warning: failed to record document_cell_merge acks for carry-forward: {e}"
                );
            }
        }
        decision.replace_snapshot_with_content_ours_for_live_prompt_drift(
            &merged_doc,
            visible_repair_required,
        );
        return true;
    }

    if !decision.snap_source.is_visible_write_proven()
        && response_target_disjoint_from_user_edit(
            base,
            &queue_reconciled_ours,
            &candidate,
            |base, ours, theirs| agent_doc_merge_io::merge_contents(base, ours, theirs).ok(),
        )
        && let Ok(union) =
            agent_doc_merge_io::merge_contents(base, &queue_reconciled_ours, &candidate)
        && !union.contains("<<<<<<<")
    {
        let union_response_present = !response_headings.is_empty()
            && response_converged_in_visible_target(base, &queue_reconciled_ours, &union);
        let visible_repair_required = !visible_write_response_present && !union_response_present;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "live_prompt_drift_forward_merged file={} source={} patch_id={} candidate_len={} candidate_hash={} union_len={} union_hash={} visible_write_response_present={} union_response_present={} visible_repair_required={} reason=independent_concurrent_edit",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                candidate.len(),
                agent_doc_hash::content_hash(&candidate),
                union.len(),
                agent_doc_hash::content_hash(&union),
                visible_write_response_present,
                union_response_present,
                visible_repair_required,
            ),
        );
        decision.replace_snapshot_with_content_ours_for_live_prompt_drift(
            &union,
            visible_repair_required,
        );
        return true;
    }

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
    let live_candidate_contains_response = (!response_headings.is_empty()
        && response_converged_in_visible_target(base, &queue_reconciled_ours, &candidate))
        || (!candidate_response_headings.is_empty()
            && response_converged_in_visible_target(base, &candidate, &candidate));
    let visible_repair_required =
        !(decision.snap_source.is_visible_write_proven() && live_candidate_contains_response);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "live_prompt_drift_agent_target_not_snapshot_authority file={} source={} patch_id={} live_candidate_contains_response={} visible_repair_required={} candidate_len={} candidate_hash={} agent_target_len={} agent_target_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            live_candidate_contains_response,
            visible_repair_required,
            candidate.len(),
            agent_doc_hash::content_hash(&candidate),
            queue_reconciled_ours.len(),
            agent_doc_hash::content_hash(&queue_reconciled_ours),
        ),
    );
    decision.replace_snapshot_with_content_ours_for_live_prompt_drift(
        &queue_reconciled_ours,
        visible_repair_required,
    );
    true
}

pub fn guard_ipc_snapshot_adoption_against_prompt_duplication(
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
    if let Some(reason) = agent_doc_element::element::structural_corruption_reason(ours) {
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

    let prior_source = decision.snap_source.label();
    let bad_state = decision.snapshot_content.clone();
    log_flow_event(
        file,
        agent_doc_flow::types::FlowEvent::new(
            agent_doc_flow::types::FlowName::DocumentMutation,
            agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
            agent_doc_flow::types::FlowOutcome::Blocked,
        )
        .with_reason("prompt_duplication_in_visible_write"),
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_blocked file={} source={} patch_id={} snap_source={} reason=prompt_duplication_in_visible_write duplicate_prompt_count={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
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
    log_ipc_proof_failure_with_recycle(
        file,
        source,
        patch_id,
        "prompt_duplication_in_visible_write",
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

/// #ipcfullprompt-recur2 — default-on forensic capture for live editor
/// full-prompt corruption candidates.
pub fn log_ipcfullprompt_corruption_if_any(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    candidate: &str,
) {
    let mut findings =
        agent_doc_document_realtime::ipc_corruption::detect_duplicated_scaffold(candidate);
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

pub fn materialize_missing_response_for_socket_visible_write_drift(
    file: &Path,
    patch_id: Option<&str>,
    content_ours: Option<&str>,
    expected_response: &str,
    drift_fired: bool,
    decision: &mut IpcRepairDecision,
) -> bool {
    if !drift_fired || decision.snap_source != IpcSnapshotSource::LazilyVisibleWriteEvent {
        return false;
    }
    if matches!(
        decision.disk_repair_reason,
        Some(IpcDiskRepairReason::LivePromptDrift)
    ) {
        return false;
    }
    let response =
        agent_doc_template::response_materialization::response_materialization_probe_from_response(
            expected_response,
        );
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
    if decision.editor_bad_state.is_none() {
        decision.editor_bad_state = Some(EditorBadStateFingerprint::new(pre_materialize));
    }
    decision.redeliver_editor = true;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_visible_write_drift_missing_response_materialized file={} patch_id={} repaired_len={} repaired_hash={} response_sha256={}",
            file.display(),
            patch_id.unwrap_or("-"),
            decision.snapshot_content.len(),
            agent_doc_hash::content_hash(&decision.snapshot_content),
            agent_doc_hash::content_hash(&response),
        ),
    );
    true
}

fn record_visible_write_commit_candidate_receipt(
    file: &Path,
    patch_id: &str,
    candidate_content: &str,
    source: &str,
) {
    if let Err(err) =
        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
            file,
            patch_id,
            candidate_content,
            source,
        )
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_visible_write_commit_candidate_proof_failed file={} patch_id={} error={}",
                file.display(),
                patch_id,
                err.to_string().replace('\n', " ")
            ),
        );
    }
}

fn visible_write_content_hash(content: &str) -> String {
    agent_doc_hash::content_hash(
        &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(content),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisibleWriteContentAuthority {
    LazilyEvent,
    Projection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisibleWriteContent {
    pub content: String,
    pub authority: VisibleWriteContentAuthority,
}

fn visible_write_content_from_lazily_event(
    file: &Path,
    patch_id: &str,
) -> Result<
    Option<(
        String,
        agent_doc_state_backbone::VisibleWriteCommitCandidateProjection,
    )>,
> {
    let Some(proof) =
        agent_doc_controller_io::project_controller::visible_write_commit_candidate_for_patch_file(
            file, patch_id,
        )
    else {
        return Ok(None);
    };
    match current_text_via_recovery_authority(file, "visible_write_lazily_event_content") {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }))
            if visible_write_content_hash(&text)
                .eq_ignore_ascii_case(&proof.commit_candidate_hash) =>
        {
            return Ok(Some((text, proof)));
        }
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_lazily_event_cpc_current_hash_mismatch file={} patch_id={} candidate_hash={} current_len={} current_hash={}",
                    file.display(),
                    patch_id,
                    proof.commit_candidate_hash,
                    text.len(),
                    visible_write_content_hash(&text)
                ),
            );
        }
        Ok(_) => {}
        Err(err) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "visible_write_lazily_event_cpc_current_unavailable file={} patch_id={} error={}",
                file.display(),
                patch_id,
                err
            ),
        ),
    }
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if let Ok(content) = std::fs::read_to_string(&canonical)
        && visible_write_content_hash(&content).eq_ignore_ascii_case(&proof.commit_candidate_hash)
    {
        return Ok(Some((content, proof)));
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_lazily_event_content_missing file={} patch_id={} candidate_hash={} source={}",
            file.display(),
            patch_id,
            proof.commit_candidate_hash,
            proof.source
        ),
    );
    Ok(None)
}

pub fn poll_visible_write_content_lazily_event(
    file: &Path,
    patch_id: &str,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
) -> Result<Option<String>> {
    let start = std::time::Instant::now();
    loop {
        if let Some((content, proof)) = visible_write_content_from_lazily_event(file, patch_id)? {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_lazily_event_observed file={} patch_id={} model_revision={} candidate_hash={} source={} len={}",
                    file.display(),
                    patch_id,
                    proof.model_revision,
                    proof.commit_candidate_hash,
                    proof.source,
                    content.len()
                ),
            );
            return Ok(Some(content));
        }
        if start.elapsed() >= timeout {
            return Ok(None);
        }
        std::thread::sleep(poll_interval);
    }
}

pub fn poll_visible_write_text_lazily_event_or_projection(
    file: &Path,
    project_root: &Path,
    patch_id: &str,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
) -> Result<Option<String>> {
    Ok(poll_visible_write_content_lazily_event_or_projection(
        file,
        project_root,
        patch_id,
        timeout,
        poll_interval,
    )?
    .map(|value| value.content))
}

pub fn poll_visible_write_content_lazily_event_or_projection(
    file: &Path,
    _project_root: &Path,
    patch_id: &str,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
) -> Result<Option<VisibleWriteContent>> {
    if let Some(content) =
        poll_visible_write_content_lazily_event(file, patch_id, timeout, poll_interval)?
    {
        return Ok(Some(VisibleWriteContent {
            content,
            authority: VisibleWriteContentAuthority::LazilyEvent,
        }));
    }
    Ok(None)
}

/// Effects still owned by the orchestration command crate while the write
/// authority queue is being extracted.
pub trait EditorConvergenceEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;

    fn apply_canonical_replace_if_attached(
        &self,
        file: &Path,
        expected_current: &str,
        content: &str,
        source: &str,
    ) -> Result<Option<agent_doc_crdt_relay_io::CpcRelayWrite>> {
        let _ = (file, expected_current, content, source);
        Ok(None)
    }

    fn guard_visible_write_idle_and_current(
        &self,
        file: &Path,
        source: &str,
        expected_current: &str,
    ) -> Result<()>;

    fn atomic_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<()>;

    fn cycle_already_committed(&self, file: &Path) -> Option<String>;

    fn log_file_ipc_already_committed(&self, file: &Path, cycle_id: &str);

    fn cleanup_fallback_patch_files(&self, file: &Path);

    fn file_ipc_patch_rejected(&self, file: &Path, patch_id: &str) -> Option<String>;

    fn log_file_ipc_proof_failure(
        &self,
        file: &Path,
        patch_id: Option<&str>,
        invariant: &str,
        recovery: &str,
        detail: &str,
    );
}

#[cfg(test)]
const VISIBLE_WRITE_TYPING_SETTLE_MS: u64 = 75;
#[cfg(not(test))]
const VISIBLE_WRITE_TYPING_SETTLE_MS: u64 = 500;
#[cfg(test)]
const VISIBLE_WRITE_TYPING_TIMEOUT_MS: u64 = 1_000;
#[cfg(not(test))]
const VISIBLE_WRITE_TYPING_TIMEOUT_MS: u64 = 2_000;

fn live_buffer_file_keys(file: &Path) -> Vec<String> {
    let mut keys = Vec::new();
    if let Ok(canonical) = file.canonicalize() {
        keys.push(canonical.to_string_lossy().to_string());
    }
    let raw = file.to_string_lossy().to_string();
    if !keys.iter().any(|key| key == &raw) {
        keys.push(raw);
    }
    keys
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisibleWriteDiskProof {
    pub authority: WholeBufferAuthority,
    pub source_buffer_matches: bool,
}

impl VisibleWriteDiskProof {
    fn unproven() -> Self {
        Self {
            authority: WholeBufferAuthority::None,
            source_buffer_matches: false,
        }
    }
}

pub fn visible_write_disk_proof(
    file: &Path,
    editor_id: Option<&str>,
    content: &str,
) -> VisibleWriteDiskProof {
    let Some(editor_id) = editor_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return VisibleWriteDiskProof::unproven();
    };
    let content_hash = agent_doc_hash::content_hash(content);

    if let Some(typing_key) = live_buffer_file_keys(file).into_iter().find(|file_key| {
        agent_doc_debounce::is_typing_via_file(file_key, VISIBLE_WRITE_TYPING_SETTLE_MS)
    }) {
        let settled = agent_doc_debounce::await_idle_via_file(
            &typing_key,
            VISIBLE_WRITE_TYPING_SETTLE_MS,
            VISIBLE_WRITE_TYPING_TIMEOUT_MS,
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "visible_write_disk_proof_typing_settle file={} settled={} settle_ms={} timeout_ms={} key={}",
                file.display(),
                settled,
                VISIBLE_WRITE_TYPING_SETTLE_MS,
                VISIBLE_WRITE_TYPING_TIMEOUT_MS,
                typing_key
            ),
        );
        if !settled {
            return VisibleWriteDiskProof::unproven();
        }
    }

    match current_text_via_recovery_authority(file, "visible_write_disk_proof") {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => {
            let current_hash = agent_doc_hash::content_hash(&text);
            let source_buffer_matches = current_hash.eq_ignore_ascii_case(&content_hash);
            if !source_buffer_matches {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "visible_write_cpc_proof_hash_mismatch file={} editor_id={} expected_len={} expected_hash={} current_len={} current_hash={}",
                        file.display(),
                        editor_id,
                        content.len(),
                        content_hash,
                        text.len(),
                        current_hash
                    ),
                );
            }
            VisibleWriteDiskProof {
                authority: WholeBufferAuthority::OperatorTextAuthority,
                source_buffer_matches,
            }
        }
        Ok(_) => VisibleWriteDiskProof::unproven(),
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_cpc_proof_unavailable file={} editor_id={} error={}",
                    file.display(),
                    editor_id,
                    err
                ),
            );
            VisibleWriteDiskProof::unproven()
        }
    }
}

/// CPC relay current text, or `None` when no editor-attached CRDT current is
/// available.
fn newest_operator_authoritative_buffer(file: &Path, editor_id: &str) -> Option<String> {
    match current_text_via_recovery_authority(file, "visible_write_reconcile_operator_buffer") {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => Some(text),
        Ok(_) => None,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "visible_write_reconcile_cpc_current_unavailable file={} editor_id={} error={}",
                    file.display(),
                    editor_id,
                    err
                ),
            );
            None
        }
    }
}

/// Settle the editor (wait out active typing) so the next live-buffer read is
/// quiescent. Bounded by the visible-write settle/timeout budget; a no-op when
/// the editor is not typing.
fn settle_editor_typing(file: &Path) {
    for key in live_buffer_file_keys(file) {
        if agent_doc_debounce::is_typing_via_file(&key, VISIBLE_WRITE_TYPING_SETTLE_MS) {
            agent_doc_debounce::await_idle_via_file(
                &key,
                VISIBLE_WRITE_TYPING_SETTLE_MS,
                VISIBLE_WRITE_TYPING_TIMEOUT_MS,
            );
        }
    }
}

/// Max rounds of the bounded reconcile-before-accept loop. Each round settles the
/// editor and re-samples the operator buffer; the loop ends when the buffer is
/// stable across two reads (a fixpoint) or this bound is hit.
const VISIBLE_WRITE_RECONCILE_MAX_ROUNDS: usize = 4;

/// `#adoc-live-prompt-drift-operator-edit` (Phase 2): the bounded
/// reconcile-before-accept loop. When the operator kept editing past the ack
/// capture (so the visible-write snapshot is stale relative to the live buffer),
/// settle the editor and re-sample its buffer until it reaches a fixpoint
/// (unchanged across two reads) or the round bound is hit, then adopt that
/// settled operator-authoritative buffer as the snapshot, provided it still
/// presents this cycle's response. This owns only the IO/settling; the decision
/// each round is owned by the realtime model.
pub fn reconcile_visible_write_snapshot_to_newer_operator_buffer(
    file: &Path,
    editor_id: Option<&str>,
    decision: &mut IpcRepairDecision,
) -> bool {
    let Some(editor_id) = editor_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return false;
    };
    let mut prev: Option<String> = None;
    for round in 0..VISIBLE_WRITE_RECONCILE_MAX_ROUNDS {
        settle_editor_typing(file);
        let Some(curr) = newest_operator_authoritative_buffer(file, editor_id) else {
            return false;
        };
        match operator_reconcile_step(&decision.snapshot_content, prev.as_deref(), &curr) {
            OperatorReconcileStep::Accept(content) => {
                if content == decision.snapshot_content {
                    return false;
                }
                if agent_doc_element::element::structural_corruption_reason(&content).is_some() {
                    return false;
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "visible_write_snapshot_reconciled_forward file={} editor_id={} reason=operator_buffer_ahead rounds={} stale_len={} stale_hash={} newer_len={} newer_hash={}",
                        file.display(),
                        editor_id,
                        round + 1,
                        decision.snapshot_content.len(),
                        agent_doc_hash::content_hash(&decision.snapshot_content),
                        content.len(),
                        agent_doc_hash::content_hash(&content),
                    ),
                );
                decision.snapshot_content = content;
                return true;
            }
            OperatorReconcileStep::Continue => {
                prev = Some(curr);
            }
            OperatorReconcileStep::FailClosed => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "visible_write_snapshot_reconcile_fail_closed file={} editor_id={} reason=settled_buffer_dropped_response rounds={}",
                        file.display(),
                        editor_id,
                        round + 1,
                    ),
                );
                return false;
            }
        }
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_snapshot_reconcile_timeout file={} editor_id={} reason=operator_still_editing rounds={}",
            file.display(),
            editor_id,
            VISIBLE_WRITE_RECONCILE_MAX_ROUNDS,
        ),
    );
    false
}

pub fn mark_visible_write_live_buffer_synced(
    file: &Path,
    patch_id: &str,
    _editor_id: Option<&str>,
    content: &str,
) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_live_buffer_sync_skipped file={} patch_id={} reason=sidecar_removed len={} hash={}",
            file.display(),
            patch_id,
            content.len(),
            agent_doc_hash::content_hash(content)
        ),
    );
}

pub fn write_visible_write_through_to_disk(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    patch_id: &str,
    content: &str,
    proof: VisibleWriteDiskProof,
) -> Result<bool> {
    let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
        delivery: WholeBufferDelivery::VisibleWriteDiskWriteThrough,
        authority: proof.authority,
        source_buffer_matches: proof.source_buffer_matches,
        scope_rejection: None,
        enabled: true,
    });
    if decision.action != WholeBufferDeliveryAction::Apply {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "visible_write_disk_write_through_blocked file={} patch_id={} authority={} source_buffer_matches={} action={} reason={} len={} hash={}",
                file.display(),
                patch_id,
                proof.authority.as_str(),
                proof.source_buffer_matches,
                decision.action.as_str(),
                decision.reason,
                content.len(),
                agent_doc_hash::content_hash(content)
            ),
        );
        let stale_operator_source = proof.authority == WholeBufferAuthority::OperatorTextAuthority
            && decision.reason == "stale_source_buffer";
        return Ok(!stale_operator_source);
    }

    let before = std::fs::read_to_string(file).ok();
    if before.as_deref() == Some(content) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "visible_write_disk_write_through_skipped file={} patch_id={} authority={} reason=already_current len={} hash={}",
                file.display(),
                patch_id,
                proof.authority.as_str(),
                content.len(),
                agent_doc_hash::content_hash(content)
            ),
        );
        return Ok(true);
    }

    effects.atomic_write(file, content).with_context(|| {
        format!(
            "failed to write proven visible-write content through to disk for {}",
            file.display()
        )
    })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_disk_write_through file={} patch_id={} authority={} before_len={} before_hash={} visible_len={} visible_hash={}",
            file.display(),
            patch_id,
            proof.authority.as_str(),
            before.as_deref().map(str::len).unwrap_or(0),
            before
                .as_deref()
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            content.len(),
            agent_doc_hash::content_hash(content)
        ),
    );
    Ok(true)
}

pub fn mark_visible_write_live_buffer_synced_after_write(
    file: &Path,
    patch_id: &str,
    editor_id: Option<&str>,
    content: &str,
) {
    let proof = visible_write_disk_proof(file, editor_id, content);
    if proof.authority == WholeBufferAuthority::OperatorTextAuthority && proof.source_buffer_matches
    {
        mark_visible_write_live_buffer_synced(file, patch_id, editor_id, content);
        return;
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "visible_write_live_buffer_sync_skipped file={} patch_id={} reason=post_write_source_unproven authority={} source_buffer_matches={}",
            file.display(),
            patch_id,
            proof.authority.as_str(),
            proof.source_buffer_matches
        ),
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileIpcDeliveryOptions {
    pub guard_committed_cycle: bool,
}

impl FileIpcDeliveryOptions {
    pub fn guard_committed_cycle() -> Self {
        Self {
            guard_committed_cycle: true,
        }
    }
}

const FILE_IPC_TIMEOUT_MS_ENV: &str = "AGENT_DOC_FILE_IPC_TIMEOUT_MS";

fn file_ipc_delivery_timeout() -> std::time::Duration {
    std::env::var(FILE_IPC_TIMEOUT_MS_ENV)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(std::time::Duration::from_millis)
        .unwrap_or_else(|| std::time::Duration::from_secs(2))
}

/// Write a file-IPC patch and prove the plugin consumed it.
///
/// This owns the durable delivery loop: atomic patch write, committed-cycle
/// fence, lazily rejection receipt handling, and no-receipt timeout proof logging. The
/// caller remains responsible for post-consumption snapshot/response validation.
pub fn write_file_ipc_and_poll_delivery(
    effects: &dyn EditorConvergenceEffects,
    patch_file: &Path,
    payload: &serde_json::Value,
    doc_file: &Path,
    patch_count: usize,
    options: FileIpcDeliveryOptions,
) -> Result<bool> {
    let patch_id_for_diagnostics = payload.get("patch_id").and_then(|value| value.as_str());

    effects.atomic_write(patch_file, &serde_json::to_string_pretty(payload)?)?;

    eprintln!(
        "[write] IPC patch written to {} ({} components)",
        patch_file.display(),
        patch_count
    );

    let timeout = file_ipc_delivery_timeout();
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        if options.guard_committed_cycle
            && let Some(ref cycle_id) = effects.cycle_already_committed(doc_file)
        {
            eprintln!(
                "[write] IPC poll skipped: cycle {} already committed for {}",
                cycle_id,
                doc_file.display()
            );
            effects.log_file_ipc_already_committed(doc_file, cycle_id);
            agent_doc_ops_log_io::log_op(
                doc_file,
                &format!(
                    "file_ipc_poll_skip file={} cycle_id={} reason=already_committed",
                    doc_file.display(),
                    cycle_id
                ),
            );
            effects.cleanup_fallback_patch_files(doc_file);
            return Ok(false);
        }

        if let Some(patch_id) = patch_id_for_diagnostics
            && let Some(reason) = effects.file_ipc_patch_rejected(doc_file, patch_id)
        {
            let _ = std::fs::remove_file(patch_file);
            eprintln!(
                "[write] IPC lazily rejection receipt: plugin rejected patch {} — refusing direct document write",
                patch_file.display()
            );
            effects.log_file_ipc_proof_failure(
                doc_file,
                patch_id_for_diagnostics,
                "editor_patch_rejected",
                "retry_without_disk_write",
                &format!(
                    "receipt_reason={} patch_file={}",
                    reason,
                    patch_file.display()
                ),
            );
            return Ok(false);
        }

        if !patch_file.exists() {
            return Ok(true);
        }

        std::thread::sleep(poll_interval);
    }

    eprintln!(
        "[write] IPC timeout ({}s) — leaving patch for editor retry; refusing direct document write",
        timeout.as_secs()
    );
    effects.log_file_ipc_proof_failure(
        doc_file,
        patch_id_for_diagnostics,
        "no_ack",
        "retry_without_disk_write",
        &format!(
            "timeout_secs={} patch_file={}",
            timeout.as_secs(),
            patch_file.display()
        ),
    );
    Ok(false)
}

#[allow(dead_code)]
pub fn try_ipc_full_content(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    content: &str,
) -> Result<bool> {
    try_ipc_full_content_with_mode(
        effects,
        file,
        content,
        FullContentIpcMode::ResponseFallback,
        None,
    )
}

pub fn try_ipc_full_content_response_fallback_from_source(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    content: &str,
    source_content: &str,
) -> Result<bool> {
    try_ipc_full_content_with_mode(
        effects,
        file,
        content,
        FullContentIpcMode::ResponseFallback,
        Some(source_content),
    )
}

#[allow(dead_code)]
pub fn try_ipc_full_content_operator_mutation(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    content: &str,
) -> Result<bool> {
    try_ipc_full_content_with_mode(
        effects,
        file,
        content,
        FullContentIpcMode::OperatorMutation,
        None,
    )
}

#[allow(dead_code)]
pub fn try_ipc_full_content_operator_mutation_from_source(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    content: &str,
    source_content: &str,
) -> Result<bool> {
    try_ipc_full_content_with_mode(
        effects,
        file,
        content,
        FullContentIpcMode::OperatorMutation,
        Some(source_content),
    )
}

pub fn log_full_content_ipc_disabled(
    file: &Path,
    mode: FullContentIpcMode,
    patch_id: &str,
    target_content: &str,
    source_content: Option<&str>,
    current_content: Option<&str>,
) {
    let source = mode.source_label();
    eprintln!(
        "[write] full-content IPC disabled for {}: falling back to guarded disk path",
        file.display()
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "full_content_ipc_disabled file={} source={} patch_id={} reason=disabled_by_default target_len={} target_hash={} source_len={} source_hash={} current_len={} current_hash={}",
            file.display(),
            source,
            patch_id,
            target_content.len(),
            agent_doc_hash::content_hash(target_content),
            source_content.map(str::len).unwrap_or(0),
            source_content
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            current_content.map(str::len).unwrap_or(0),
            current_content
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string())
        ),
    );
}

struct FullContentIpcAuthorityRejectionLog<'a> {
    file: &'a Path,
    mode: FullContentIpcMode,
    patch_id: &'a str,
    target_content: &'a str,
    source_content: Option<&'a str>,
    current_content: Option<&'a str>,
    authority: WholeBufferAuthority,
    reason: &'a str,
}

fn log_full_content_ipc_authority_rejected(facts: FullContentIpcAuthorityRejectionLog<'_>) {
    let source = facts.mode.source_label();
    eprintln!(
        "[write] full-content IPC skipped for {}: whole-buffer delivery rejected ({})",
        facts.file.display(),
        facts.reason
    );
    agent_doc_ops_log_io::log_op(
        facts.file,
        &format!(
            "full_content_ipc_authority_rejected file={} source={} patch_id={} authority={} reason={} target_len={} target_hash={} source_len={} source_hash={} current_len={} current_hash={}",
            facts.file.display(),
            source,
            facts.patch_id,
            facts.authority.as_str(),
            facts.reason,
            facts.target_content.len(),
            agent_doc_hash::content_hash(facts.target_content),
            facts.source_content.map(str::len).unwrap_or(0),
            facts
                .source_content
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            facts.current_content.map(str::len).unwrap_or(0),
            facts
                .current_content
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string())
        ),
    );
}

pub fn full_content_ipc_scope_allows(
    file: &Path,
    mode: FullContentIpcMode,
    patch_id: &str,
    target_content: &str,
    source_content: Option<&str>,
    current_content: Option<&str>,
) -> bool {
    let reason = agent_doc_document_realtime::write_policy::full_content_scope_rejection_reason(&[
        Some(target_content),
        source_content,
        current_content,
    ]);
    let Some(reason) = reason else {
        return true;
    };

    let reason = reason.as_str();
    let source = mode.source_label();
    eprintln!(
        "[write] full-content IPC skipped for {}: {} is not eligible for whole-document editor replacement",
        file.display(),
        reason
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "full_content_ipc_scope_rejected file={} source={} patch_id={} scope={} target_len={} target_hash={} source_len={} source_hash={} current_len={} current_hash={}",
            file.display(),
            source,
            patch_id,
            reason,
            target_content.len(),
            agent_doc_hash::content_hash(target_content),
            source_content.map(str::len).unwrap_or(0),
            source_content
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            current_content.map(str::len).unwrap_or(0),
            current_content
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string())
        ),
    );
    false
}

pub fn try_ipc_full_content_with_mode(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    content: &str,
    mode: FullContentIpcMode,
    source_content: Option<&str>,
) -> Result<bool> {
    let _canonical = file.canonicalize()?;
    let before_content = std::fs::read_to_string(file).ok();
    let effective_source_content = match (mode, source_content) {
        (FullContentIpcMode::ResponseFallback, None) => Some(content),
        _ => source_content,
    };
    let patch_id = uuid::Uuid::new_v4().to_string();

    if mode == FullContentIpcMode::ResponseFallback
        && let Some(ref cycle_id) = effects.cycle_already_committed(file)
    {
        eprintln!(
            "[write] full-content IPC skipped: cycle {} already committed for {}",
            cycle_id,
            file.display()
        );
        effects.log_file_ipc_already_committed(file, cycle_id);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "late_fallback_patch_rejected file={} cycle_id={} patch_id=full_content reason=already_committed transport=full_content",
                file.display(),
                cycle_id
            ),
        );
        effects.cleanup_fallback_patch_files(file);
        return Ok(false);
    }

    let scope_rejection =
        agent_doc_document_realtime::write_policy::full_content_scope_rejection_reason(&[
            Some(content),
            effective_source_content,
            before_content.as_deref(),
        ]);
    let authority = if live_editor_delivery_has_operator_authority(file) {
        WholeBufferAuthority::OperatorTextAuthority
    } else {
        WholeBufferAuthority::None
    };
    let source_buffer_matches = effective_source_content
        .zip(before_content.as_deref())
        .is_some_and(|(source, current)| source == current);
    let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
        delivery: WholeBufferDelivery::FullContentEditorIpc,
        authority,
        source_buffer_matches,
        scope_rejection,
        enabled: false,
    });
    match decision.action {
        WholeBufferDeliveryAction::Reject if scope_rejection.is_some() => {
            full_content_ipc_scope_allows(
                file,
                mode,
                &patch_id,
                content,
                effective_source_content,
                before_content.as_deref(),
            );
            return Ok(false);
        }
        WholeBufferDeliveryAction::Reject => {
            log_full_content_ipc_authority_rejected(FullContentIpcAuthorityRejectionLog {
                file,
                mode,
                patch_id: &patch_id,
                target_content: content,
                source_content: effective_source_content,
                current_content: before_content.as_deref(),
                authority,
                reason: decision.reason,
            });
            return Ok(false);
        }
        WholeBufferDeliveryAction::ObserveOnly => {
            log_full_content_ipc_disabled(
                file,
                mode,
                &patch_id,
                content,
                effective_source_content,
                before_content.as_deref(),
            );
        }
        WholeBufferDeliveryAction::Apply => {}
    }
    Ok(false)
}

fn write_converge_cycle_id_for_payload(file: &Path) -> Option<String> {
    agent_doc_cycle_state_io::load_closeout_projection(file)
        .ok()
        .flatten()
        .and_then(|projection| projection.cycle_id)
        .or_else(|| {
            agent_doc_cycle_state_io::load_with_closeout_projection(file)
                .ok()
                .flatten()
                .map(|cycle| cycle.cycle_id)
        })
}

/// `#exch-intermix`: auto-recover the `live_prompt_drift_after_preflight`
/// closeout wedge by rebasing the missing agent response onto the realtime
/// document. Returns the recovered file content on success (the caller must
/// refresh its `file_content` and snapshot), or `None` when no recovery applies.
pub fn try_auto_recover_live_prompt_drift(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    snapshot: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let Some(cycle) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if !cycle.ipc_snapshot_adoption_blocked {
        return Ok(None);
    }

    let dropped_missing_from_snapshot = cycle
        .dropped_exchange_prompts
        .iter()
        .chain(cycle.dropped_queue_prompts.iter())
        .any(|prompt| !snapshot_contains_dropped_prompt(snapshot, prompt));
    if dropped_missing_from_snapshot {
        return Ok(None);
    }
    let Some(recovery_target) = live_prompt_drift_recovery_target(
        snapshot,
        file_content,
        normalize_visible_recovery_compare,
    ) else {
        return Ok(None);
    };

    let ipc_project_root = file
        .canonicalize()
        .ok()
        .map(|c| agent_doc_project_root_io::resolve_ipc_project_root(&c));
    let ipc_listener_active = ipc_project_root
        .as_deref()
        .map(agent_doc_ipc_io::is_listener_active)
        .unwrap_or(false);

    if let Some(project_root) = ipc_project_root.as_deref()
        && ipc_listener_active
    {
        match try_editor_converge_live_prompt_drift(
            file,
            project_root,
            &recovery_target,
            file_content,
        ) {
            Ok(Some(recovered)) => {
                log_live_prompt_drift_auto_recovered(
                    file,
                    &recovery_target,
                    file_content,
                    true,
                    "editor_ipc",
                );
                log_flow_event(
                    file,
                    agent_doc_flow::types::FlowEvent::new(
                        agent_doc_flow::types::FlowName::DocumentMutation,
                        agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
                        agent_doc_flow::types::FlowOutcome::Completed,
                    )
                    .with_reason("live_prompt_drift_auto_recovered"),
                );
                eprintln!(
                    "[commit] auto-recovered live_prompt_drift wedge for {} via editor IPC convergence ({} bytes)",
                    file.display(),
                    recovery_target.len()
                );
                return Ok(Some(recovered));
            }
            Ok(None) => {}
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_error file={} error={}",
                        file.display(),
                        err
                    ),
                );
            }
        }
    }

    if ipc_listener_active {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "[jbstalecache] auto_recovery_disk_write_blocked file={} target_len={} reason=editor_ipc_unconfirmed",
                file.display(),
                recovery_target.len()
            ),
        );
        return Ok(None);
    }

    effects
        .atomic_write(file, &recovery_target)
        .with_context(|| {
            format!(
                "live_prompt_drift auto-recover write for {}",
                file.display()
            )
        })?;
    save_document_snapshot_and_crdt(file, &recovery_target)?;
    log_live_prompt_drift_auto_recovered(
        file,
        &recovery_target,
        file_content,
        ipc_listener_active,
        "disk_fallback",
    );
    log_flow_event(
        file,
        agent_doc_flow::types::FlowEvent::new(
            agent_doc_flow::types::FlowName::DocumentMutation,
            agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
            agent_doc_flow::types::FlowOutcome::Completed,
        )
        .with_reason("live_prompt_drift_auto_recovered"),
    );
    eprintln!(
        "[commit] auto-recovered live_prompt_drift wedge for {} — merged the missing response into the realtime document ({} bytes) so operator-visible edits stay authoritative",
        file.display(),
        recovery_target.len()
    );
    Ok(Some(recovery_target))
}

pub fn log_live_prompt_drift_auto_recovered(
    file: &Path,
    target: &str,
    file_content: &str,
    ipc_listener_active: bool,
    transport: &str,
) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "live_prompt_drift_auto_recovered file={} target_len={} file_len={} target_hash={} ipc_listener_active={} transport={}",
            file.display(),
            target.len(),
            file_content.len(),
            agent_doc_hash::content_hash(target),
            ipc_listener_active,
            transport
        ),
    );
}

pub fn try_editor_converge_live_prompt_drift(
    file: &Path,
    project_root: &Path,
    target: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let patches = live_prompt_drift_response_patches(file_content, target)?;
    let frontmatter = None;
    if patches.is_empty() && frontmatter.is_none() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "[jbstalecache] editor_convergence_skipped file={} skip=no_component_or_frontmatter_delta",
                file.display()
            ),
        );
        return Ok(None);
    }

    let canonical = file.canonicalize()?;
    let patch_id = uuid::Uuid::new_v4().to_string();
    let mut payload = serde_json::json!({
        "type": "patch",
        "file": canonical.to_string_lossy(),
        "patches": patches,
        "node_patches": [],
        "unmatched": "",
        "baseline": file_content,
        "reposition_boundary": false,
        "patch_id": patch_id,
    });
    if let Some(frontmatter) = frontmatter {
        payload["frontmatter"] = serde_json::Value::String(frontmatter);
    }
    if let Some(cycle_id) = write_converge_cycle_id_for_payload(file) {
        payload["cycle_id"] = serde_json::Value::String(cycle_id);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "[jbstalecache] editor_convergence_attempt file={} patch_id={} patches={} frontmatter={} target_hash={}",
            file.display(),
            payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("-"),
            payload
                .get("patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload.get("frontmatter").is_some(),
            agent_doc_hash::content_hash(target)
        ),
    );

    match agent_doc_ipc_io::send_message(project_root, &payload) {
        Ok(Some(_ack)) => {
            let patch_id = payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("-");
            let visible_write = poll_visible_write_text_lazily_event_or_projection(
                file,
                project_root,
                patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )?;
            let Some(recovered) = visible_write else {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_no_visible_write_receipt file={} patch_id={} action=block_external_disk_write",
                        file.display(),
                        patch_id
                    ),
                );
                return Ok(None);
            };
            if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                &recovered,
            ) == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                target,
            ) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_succeeded file={} patch_id={} recovered_len={} transport=editor_ipc",
                        file.display(),
                        patch_id,
                        recovered.len()
                    ),
                );
                record_visible_write_commit_candidate_receipt(
                    file,
                    patch_id,
                    target,
                    "editor_convergence",
                );
                Ok(Some(recovered))
            } else if convergence_recovered_editor_wins_outside_response(&recovered, target) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_succeeded file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc resolution=editor_wins_outside_response #qpcwcmerge",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                record_visible_write_commit_candidate_receipt(
                    file,
                    patch_id,
                    &recovered,
                    "editor_convergence",
                );
                Ok(Some(recovered))
            } else {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_ack_mismatch file={} patch_id={} recovered_len={} target_len={} action=block_external_disk_write",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                Ok(None)
            }
        }
        Ok(None) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "[jbstalecache] editor_convergence_no_ack file={} action=block_external_disk_write",
                    file.display()
                ),
            );
            Ok(None)
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "[jbstalecache] editor_convergence_send_failed file={} error={} action=block_external_disk_write",
                    file.display(),
                    err
                ),
            );
            Ok(None)
        }
    }
}

pub fn live_prompt_drift_response_patches(
    file_content: &str,
    snapshot: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut patches =
        agent_doc_document::component_patches::component_replace_patches(file_content, snapshot)?;
    patches.retain(|patch| {
        patch.get("component").and_then(|value| value.as_str()) == Some(AGENT_RESPONSE_COMPONENT)
    });
    Ok(patches)
}

/// The plugin-owner lease still claims an attached editor, but the CRDT recovery
/// authority proves zero live replicas with delivery converged. That is a stale
/// lease: the editor process exited (or the controller recycled and the editor
/// has not re-registered its replica) without releasing the plugin-owner marker,
/// so `editor_attached()` / `live_editor_attached()` keep reporting a live editor
/// that no longer has a convergeable buffer. Attempting editor-IPC convergence in
/// this state ACKs/rejects against a phantom endpoint and then refuses the disk
/// write forever (`no_visible_write_receipt` — the reported JB `Compact Exchange`
/// exit-1). Mirror the relay's own disk-authority resolution
/// (`crdt_cpc_write_disk_authority_stale_lease`, `#stale-lease-cpc-authority`):
/// with no live replica there is no editor buffer to protect, so disk is the
/// authority and the guarded disk write is safe (the controller's disk-change
/// watcher reconciles the stale relay canonical when an editor re-attaches).
///
/// Requires `delivery_converged` so a transient attach/sync gap (a real editor
/// mid-registration, briefly `live_editors == 0`) is not mistaken for a stale
/// lease. On any recovery-authority error or non-`Current` state this returns
/// `false` (conservative: keep the editor-authority refusal).
fn stale_owner_zero_live_replica(file: &Path, source: &str) -> bool {
    matches!(
        current_text_via_recovery_authority(file, source),
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
            live_editors: 0,
            delivery_converged: true,
            ..
        }))
    )
}

fn refuse_unproven_editor_delivery(
    file: &Path,
    source: &str,
    reason: &str,
    patch_id: Option<&str>,
) -> Result<bool> {
    let sidecar_live = live_editor_attached(file);
    let owner_holds =
        !agent_doc_plugin_owner::disk_write_permitted_for_file(&file.to_string_lossy());
    let editor_endpoint = if should_refuse_disk_fallback(
        sidecar_live,
        owner_holds,
        editor_ipc_listener_active(file),
    ) && !stale_owner_zero_live_replica(file, "refuse_unproven_editor_delivery_stale_lease_probe")
    {
        "live"
    } else {
        "absent"
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_writeback file={} transport=blocked reason={reason} editor_endpoint={} action=editor_convergence_required",
            file.display(),
            editor_endpoint
        ),
    );
    let detail = format!("editor_endpoint={editor_endpoint}");
    if let Err(err) = agent_doc_cycle_state_io::record_editor_convergence_required(
        file,
        source,
        reason,
        patch_id,
        Some(&detail),
    ) {
        eprintln!(
            "[write] WARNING: failed to record editor-convergence blocked closeout for {}: {err}",
            file.display()
        );
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} while editor convergence is unproven (reason={reason}, editor_endpoint={editor_endpoint})",
        file.display()
    );
}

fn live_editor_attached(file: &Path) -> bool {
    let indicator_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    agent_doc_plugin_owner::live_plugin_owner_consumer_id(&indicator_path).is_some()
}

fn editor_ipc_listener_active(file: &Path) -> bool {
    file.canonicalize()
        .ok()
        .map(|c| agent_doc_project_root_io::resolve_ipc_project_root(&c))
        .map(|root| agent_doc_ipc_io::is_listener_active(&root))
        .unwrap_or(false)
}

fn try_detached_disk_write(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
    reason: &str,
) -> Result<bool> {
    let editor_attached = live_editor_attached(file);
    let owner_holds =
        !agent_doc_plugin_owner::disk_write_permitted_for_file(&file.to_string_lossy());
    if should_refuse_disk_fallback(
        editor_attached,
        owner_holds,
        editor_ipc_listener_active(file),
    ) && !stale_owner_zero_live_replica(file, "detached_disk_write_stale_lease_probe")
    {
        return Ok(false);
    }

    effects
        .atomic_write_if_current(file, target, current, source)
        .with_context(|| {
            format!(
                "{source}: failed detached disk write for {}",
                file.display()
            )
        })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_writeback file={} transport=disk_detached reason={} len={} hash={}",
            file.display(),
            reason,
            target.len(),
            agent_doc_hash::content_hash(target)
        ),
    );
    Ok(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckMismatchRefreshOutcome {
    NoRecovery,
    RevertedToCurrent,
    ReplayedTarget,
}

fn refresh_editor_after_ack_mismatch(
    file: &Path,
    project_root: &Path,
    canonical: &Path,
    target: &str,
    recovered: &str,
    current_content: &str,
    source: &str,
) -> AckMismatchRefreshOutcome {
    let stale_hash = agent_doc_hash::content_hash(recovered);
    let Some(recovery) = classify_socket_receipt_mismatch_recovery(
        target,
        recovered,
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers,
    ) else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_visible_write_mismatch_editor_refresh file={} transport=blocked reason=untrusted_visible_write_contains_user_drift action=leave_editor_owned_visible_write stale_len={} stale_hash={}",
                file.display(),
                recovered.len(),
                &stale_hash[..stale_hash.len().min(12)]
            ),
        );
        return AckMismatchRefreshOutcome::NoRecovery;
    };
    let (refresh_content, action, success_outcome) = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => (
            current_content,
            "revert_untrusted_visible_write",
            AckMismatchRefreshOutcome::RevertedToCurrent,
        ),
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => (
            target,
            "replay_missing_agent_response",
            AckMismatchRefreshOutcome::ReplayedTarget,
        ),
    };
    let target_hash = agent_doc_hash::content_hash(refresh_content);
    let failure_action = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => {
            "left_untrusted_visible_write_editor_owned"
        }
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => {
            "left_missing_agent_response_editor_owned"
        }
    };
    let failure_reason = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => "safe_stale_prompt_refresh_failed",
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => {
            "safe_missing_agent_response_refresh_failed"
        }
    };
    match agent_doc_ipc_io::send_refresh_content(
        project_root,
        &canonical.to_string_lossy(),
        refresh_content,
        &stale_hash,
        recovered.len(),
    ) {
        Ok(true) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=editor_ipc action={} stale_len={} stale_hash={} target_len={} target_hash={}",
                    file.display(),
                    action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)],
                    refresh_content.len(),
                    &target_hash[..target_hash.len().min(12)]
                ),
            );
            success_outcome
        }
        Ok(false) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason={} no_ack=true action={} stale_len={} stale_hash={}",
                    file.display(),
                    failure_reason,
                    failure_action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)]
                ),
            );
            AckMismatchRefreshOutcome::NoRecovery
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason={} send_failed=true error={} action={} stale_len={} stale_hash={}",
                    file.display(),
                    failure_reason,
                    err,
                    failure_action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)]
                ),
            );
            AckMismatchRefreshOutcome::NoRecovery
        }
    }
}

pub fn live_buffer_delivery_missing_operator_text_authority_after_refresh(
    file: &Path,
    content: &str,
    source: &str,
) -> Option<agent_doc_debounce::LiveBufferSnapshot> {
    let canonical_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let indicator_path = canonical_file.to_string_lossy().to_string();
    let missing = agent_doc_debounce::live_buffer_delivery_missing_operator_text_authority(
        &indicator_path,
        content,
    )?;
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical_file);
    if !agent_doc_ipc_io::is_listener_active(&project_root) {
        return match agent_doc_ipc_io::send_publish_live_buffer_file_signal(
            &project_root,
            &indicator_path,
        ) {
            Ok(true) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=file_signal action=publish_live_buffer",
                        file.display()
                    ),
                );
                wait_for_operator_text_authority_refresh(&indicator_path, content, missing)
            }
            Ok(false) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=blocked outcome=publish_live_buffer_file_signal_unavailable action=editor_reload_required",
                        file.display()
                    ),
                );
                Some(missing)
            }
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=blocked outcome=publish_live_buffer_file_signal_failed error={} action=editor_reload_required",
                        file.display(),
                        err
                    ),
                );
                Some(missing)
            }
        };
    }

    match agent_doc_ipc_io::send_publish_live_buffer(&project_root, &indicator_path) {
        Ok(true) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=editor_ipc action=publish_live_buffer",
                    file.display()
                ),
            );
            wait_for_operator_text_authority_refresh(&indicator_path, content, missing)
        }
        Ok(false) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=blocked reason=publish_live_buffer_failed action=editor_reload_required",
                    file.display()
                ),
            );
            Some(missing)
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=blocked reason=publish_live_buffer_failed error={} action=editor_reload_required",
                    file.display(),
                    err
                ),
            );
            Some(missing)
        }
    }
}

fn wait_for_operator_text_authority_refresh(
    indicator_path: &str,
    content: &str,
    mut latest_missing: agent_doc_debounce::LiveBufferSnapshot,
) -> Option<agent_doc_debounce::LiveBufferSnapshot> {
    for _ in 0..20 {
        let still_missing = agent_doc_debounce::live_buffer_delivery_missing_operator_text_authority(
            indicator_path,
            content,
        )?;
        latest_missing = still_missing;
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Some(latest_missing)
}

fn content_matches_recent_committed_blob(file: &Path, content: &str, limit: usize) -> bool {
    let lines = match agent_doc_git_io::revision::recent_commit_lines(file, None, limit) {
        agent_doc_git_io::revision::RecentCommitLog::Lines(lines) => lines,
        _ => return false,
    };
    for line in lines {
        let Some(sha) = line.split_whitespace().next() else {
            continue;
        };
        if let Ok(Some(blob)) = agent_doc_git_io::revision::show_rev(file, sha)
            && blob == content
        {
            return true;
        }
    }
    false
}

pub fn ipc_repair_decision_from_visible_write(
    file: &Path,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    snap_content: String,
    _content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
) -> IpcRepairDecision {
    if let Some(lines) = normalize_prefix_lines
        && !lines.is_empty()
        && !verify_sidecar_normalization(&snap_content, lines)
    {
        let bad_state = snap_content;
        let normalized = normalize_exchange_prefixes_for_targets(&bad_state, lines);
        let repaired = agent_doc_element_exchange_io::repair_duplicate_prompt_artifacts_with_log(
            &normalized,
            file,
            DuplicatePromptRepairOptions::new("normalization_sidecar_retry")
                .with_before(baseline)
                .preserving(baseline)
                .without_residue_guard(),
            agent_doc_ops_log_io::log_op,
            |_| {},
        )
        .map(|(repaired, _)| repaired)
        .unwrap_or(normalized);
        let (required_prefix_count, observed_prefix_count) =
            normalization_prefix_observation_counts(&bad_state, lines);
        let duplicate_prompt_count = duplicate_prompt_line_count(&bad_state);
        eprintln!(
            "[write] visible-write normalization diverged — retrying from lazily event ({} bytes)",
            repaired.len()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "visible_write_normalization_fallback file={} patch_id={} snap_source=lazily_visible_write_event reason=prefix_divergence bad_len={} bad_hash={} fallback_len={} fallback_hash={} required_prefix_count={} observed_prefix_count={} duplicate_prompt_count={}",
                file.display(),
                patch_id.unwrap_or("-"),
                bad_state.len(),
                agent_doc_hash::content_hash(&bad_state),
                repaired.len(),
                agent_doc_hash::content_hash(&repaired),
                required_prefix_count,
                observed_prefix_count,
                duplicate_prompt_count
            ),
        );
        return IpcRepairDecision::lazily_visible_write_prefix_repair(repaired, bad_state, lines);
    }

    IpcRepairDecision::lazily_visible_write(snap_content)
}

pub fn redelivery_missing_operator_text_authority(
    file: &Path,
    expected_bad_state: &str,
    label: &str,
    source_patch_id: Option<&str>,
) -> bool {
    let Some(live) = live_buffer_delivery_missing_operator_text_authority_after_refresh(
        file,
        expected_bad_state,
        label,
    ) else {
        return false;
    };
    let editor_id = live.editor_id.as_deref().unwrap_or("unknown");
    // The capability gate exists to avoid clobbering a live buffer that may hold
    // unsaved operator edits. If the stale buffer byte-matches a recent
    // committed blob, it is recoverable, so refresh the editor from disk instead
    // of blocking the write.
    if content_matches_recent_committed_blob(file, expected_bad_state, 15) {
        let stale_hash = agent_doc_hash::content_hash(expected_bad_state);
        if let Ok(canonical) = file.canonicalize() {
            let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
            let disk = std::fs::read_to_string(&canonical).unwrap_or_default();
            let refresh_target = if disk.is_empty() {
                expected_bad_state
            } else {
                &disk
            };
            let _ = agent_doc_ipc_io::send_refresh_content(
                &project_root,
                &canonical.to_string_lossy(),
                refresh_target,
                &stale_hash,
                expected_bad_state.len(),
            );
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{label}_editor_authority_autoheal file={} action=refresh_editor_from_disk reason=stale_behind_committed_blob editor_id={} stale_len={} stale_hash={}",
                file.display(),
                editor_id,
                expected_bad_state.len(),
                &stale_hash[..stale_hash.len().min(12)],
            ),
        );
        return false;
    }
    eprintln!(
        "[write] {label} editor repair skipped: live editor buffer {editor_id} lacks required capability {}",
        agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{label}_editor_redelivery_skipped file={} patch_id={} skip=editor_capability_missing capability={} editor_id={} live_len={} live_hash={}",
            file.display(),
            source_patch_id.unwrap_or("-"),
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            editor_id,
            live.len,
            live.hash
        ),
    );
    true
}

pub fn verify_normalization_repair_observed(
    file: &Path,
    project_root: &Path,
    patch_id: &str,
    repaired_content: &str,
    transport: &str,
) -> bool {
    let observed = match poll_visible_write_content_lazily_event_or_projection(
        file,
        project_root,
        patch_id,
        std::time::Duration::from_millis(200),
        std::time::Duration::from_millis(25),
    ) {
        Ok(Some(content)) => content.content,
        Ok(None) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "sidecar_normalization_fallback_narrow_repair_lazily_receipt_missing file={} patch_id={} transport={}",
                    file.display(),
                    patch_id,
                    transport,
                ),
            );
            return false;
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "sidecar_normalization_fallback_narrow_repair_lazily_receipt_read_failed file={} patch_id={} transport={} error={}",
                    file.display(),
                    patch_id,
                    transport,
                    e
                ),
            );
            return false;
        }
    };

    let observed_matches =
        strip_boundary_for_dedup(&observed) == strip_boundary_for_dedup(repaired_content);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_narrow_repair_observed file={} patch_id={} transport={} observed_len={} observed_hash={} expected_len={} expected_hash={} matched={}",
            file.display(),
            patch_id,
            transport,
            observed.len(),
            agent_doc_hash::content_hash(&observed),
            repaired_content.len(),
            agent_doc_hash::content_hash(repaired_content),
            observed_matches
        ),
    );
    observed_matches
}

pub fn try_ipc_normalization_repair_patch(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    normalize_prefix_lines: &[String],
    source_patch_id: Option<&str>,
) -> Result<bool> {
    if !agent_doc_document_realtime::write_policy::normalization_repair_candidate_matches(
        expected_bad_state,
        repaired_content,
        normalize_prefix_lines,
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_narrow_repair_ineligible file={} patch_id={} skip=normalization_only_patch_not_equivalent normalize_targets={}",
                file.display(),
                source_patch_id.unwrap_or("-"),
                normalize_prefix_lines.len()
            ),
        );
        return Ok(false);
    }

    let current_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} before normalization repair",
            file.display()
        )
    })?;
    if current_content != expected_bad_state {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_narrow_repair_skipped file={} patch_id={} skip=stale_bad_state expected_len={} expected_hash={} current_len={} current_hash={}",
                file.display(),
                source_patch_id.unwrap_or("-"),
                expected_bad_state.len(),
                agent_doc_hash::content_hash(expected_bad_state),
                current_content.len(),
                agent_doc_hash::content_hash(&current_content)
            ),
        );
        return Ok(false);
    }

    if redelivery_missing_operator_text_authority(
        file,
        expected_bad_state,
        "sidecar_normalization_fallback_narrow_repair",
        source_patch_id,
    ) {
        return Ok(false);
    }

    let canonical = file.canonicalize()?;
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    let patch_id = uuid::Uuid::new_v4().to_string();
    let canonical_path = canonical.to_string_lossy();
    let proof = FullContentSourceProof::from_content(expected_bad_state);
    let payload = agent_doc_ipc_protocol::normalization_repair_patch_message(
        canonical_path.as_ref(),
        &patch_id,
        normalize_prefix_lines,
        &proof.expected_content_hash,
        proof.expected_content_len,
        true,
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_narrow_repair_attempt file={} patch_id={} source_patch_id={} normalize_targets={} expected_bad_len={} expected_bad_hash={} repaired_len={} repaired_hash={}",
            file.display(),
            patch_id,
            source_patch_id.unwrap_or("-"),
            normalize_prefix_lines.len(),
            expected_bad_state.len(),
            agent_doc_hash::content_hash(expected_bad_state),
            repaired_content.len(),
            agent_doc_hash::content_hash(repaired_content)
        ),
    );

    if agent_doc_ipc_io::is_listener_active(&project_root) {
        match agent_doc_ipc_io::send_message(&project_root, &payload) {
            Ok(Some(_)) => {
                if verify_normalization_repair_observed(
                    file,
                    &project_root,
                    &patch_id,
                    repaired_content,
                    "socket",
                ) {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "sidecar_normalization_fallback_narrow_repaired_editor file={} patch_id={} transport=socket",
                            file.display(),
                            patch_id
                        ),
                    );
                    return Ok(true);
                }
                return Ok(false);
            }
            Ok(None) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "sidecar_normalization_fallback_narrow_repair_not_consumed file={} patch_id={} transport=socket",
                        file.display(),
                        patch_id
                    ),
                );
            }
            Err(e) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "sidecar_normalization_fallback_narrow_repair_failed file={} patch_id={} transport=socket error={}",
                        file.display(),
                        patch_id,
                        e
                    ),
                );
            }
        }
    }

    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        return Ok(false);
    }

    let hash = agent_doc_fs::document_state_hash(file)?;
    let patch_file = patches_dir.join(format!("{hash}.json"));
    let payload = agent_doc_ipc_protocol::normalization_repair_patch_message(
        canonical_path.as_ref(),
        &patch_id,
        normalize_prefix_lines,
        &proof.expected_content_hash,
        proof.expected_content_len,
        false,
    );
    atomic_write(&patch_file, &serde_json::to_string_pretty(&payload)?)?;

    let timeout = file_ipc_delivery_timeout();
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if !patch_file.exists() {
            if verify_normalization_repair_observed(
                file,
                &project_root,
                &patch_id,
                repaired_content,
                "file",
            ) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "sidecar_normalization_fallback_narrow_repaired_editor file={} patch_id={} transport=file",
                        file.display(),
                        patch_id
                    ),
                );
                return Ok(true);
            }
            return Ok(false);
        }
        std::thread::sleep(poll_interval);
    }
    let _ = std::fs::remove_file(&patch_file);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_narrow_repair_not_consumed file={} patch_id={} transport=file",
            file.display(),
            patch_id
        ),
    );
    Ok(false)
}

pub fn redeliver_normalization_fallback_to_editor(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    normalize_prefix_lines: &[String],
    source_patch_id: Option<&str>,
    full_content_fallback: impl FnOnce(&Path, &str, &str, Option<&str>) -> bool,
) -> bool {
    if redelivery_missing_operator_text_authority(
        file,
        expected_bad_state,
        "sidecar_normalization_fallback_narrow_repair",
        source_patch_id,
    ) {
        return false;
    }

    match try_ipc_normalization_repair_patch(
        file,
        repaired_content,
        expected_bad_state,
        normalize_prefix_lines,
        source_patch_id,
    ) {
        Ok(true) => return true,
        Ok(false) => {}
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "sidecar_normalization_fallback_narrow_repair_failed file={} patch_id={} error={}",
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    e
                ),
            );
        }
    }

    full_content_fallback(file, repaired_content, expected_bad_state, source_patch_id)
}

pub fn redeliver_full_content_repair_to_editor(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    kind: FullContentRepairRedelivery,
    source_patch_id: Option<&str>,
    try_full_content_response_fallback: &mut dyn FnMut(&Path, &str, &str) -> Result<bool>,
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
        && redelivery_missing_operator_text_authority(
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

    match current_text_via_recovery_authority(file, "editor_redelivery_guard") {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }))
            if text != expected_bad_state =>
        {
            eprintln!(
                "[write] {} editor repair skipped: CPC document model has advanced past the bad state",
                kind.label()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{}_editor_redelivery_skipped file={} patch_id={} skip=cpc_model_diverges expected_len={} expected_hash={} cpc_len={} cpc_hash={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    expected_bad_state.len(),
                    agent_doc_hash::content_hash(expected_bad_state),
                    text.len(),
                    agent_doc_hash::content_hash(&text)
                ),
            );
            return false;
        }
        Ok(_) => {}
        Err(err) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{}_editor_redelivery_cpc_guard_unavailable file={} patch_id={} error={}",
                kind.label(),
                file.display(),
                source_patch_id.unwrap_or("-"),
                err
            ),
        ),
    }

    match try_full_content_response_fallback(file, repaired_content, expected_bad_state) {
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

fn log_ipc_proof_failure_with_recycle(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    invariant: &str,
    recovery: &str,
    detail: &str,
) {
    eprintln!(
        "[write] IPC proof insufficient for {}: source={} patch_id={} invariant={} recovery={}{}{}",
        file.display(),
        source,
        patch_id.unwrap_or("-"),
        invariant,
        recovery,
        if detail.is_empty() { "" } else { " " },
        detail
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_proof_insufficient file={} source={} patch_id={} invariant={} recovery={}{}{}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            invariant,
            recovery,
            if detail.is_empty() { "" } else { " " },
            detail
        ),
    );
}

pub fn repair_ipc_decision_visible_state(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    decision: &IpcRepairDecision,
    patch_id: Option<&str>,
    mut try_full_content_response_fallback: impl FnMut(&Path, &str, &str) -> Result<bool>,
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
                    .map(|state| state.content())
                    .unwrap_or(&decision.snapshot_content)
            )
        ),
    );

    if decision.redeliver_editor
        && let Some(expected_bad_state) = decision.editor_bad_state.as_ref()
        && match reason {
            IpcDiskRepairReason::PrefixDivergence => redeliver_normalization_fallback_to_editor(
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
                        &mut try_full_content_response_fallback,
                    )
                },
            ),
            IpcDiskRepairReason::IpcDedupe
            | IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe
            | IpcDiskRepairReason::LivePromptDrift => redeliver_full_content_repair_to_editor(
                file,
                &decision.snapshot_content,
                expected_bad_state.content(),
                reason.redelivery_kind(),
                patch_id,
                &mut try_full_content_response_fallback,
            ),
        }
    {
        return Ok(());
    }

    if matches!(reason, IpcDiskRepairReason::LivePromptDrift) {
        let ipc_project_root = file
            .canonicalize()
            .ok()
            .map(|canonical| agent_doc_project_root_io::resolve_ipc_project_root(&canonical));
        let listener_active = ipc_project_root
            .as_deref()
            .map(agent_doc_ipc_io::is_listener_active)
            .unwrap_or(false);

        let log_reconciled = |transport: &str| {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "ipc_visible_repair_incycle_editor_converged file={} patch_id={} repair_reason={} redeliver_editor={} bad_len={} bad_hash={} repaired_len={} repaired_hash={} transport={}",
                    file.display(),
                    patch_id.unwrap_or("-"),
                    reason.label(),
                    decision.redeliver_editor,
                    bad_len,
                    bad_hash,
                    decision.snapshot_content.len(),
                    agent_doc_hash::content_hash(&decision.snapshot_content),
                    transport,
                ),
            );
        };

        if let Ok(file_content) = std::fs::read_to_string(file) {
            if let Some(project_root) = ipc_project_root.as_deref()
                && listener_active
                && let Ok(Some(_recovered)) = try_editor_converge_live_prompt_drift(
                    file,
                    project_root,
                    &decision.snapshot_content,
                    &file_content,
                )
            {
                log_reconciled("editor_ipc");
                return Ok(());
            }

            let bad_state_matches = decision
                .editor_bad_state
                .as_ref()
                .is_some_and(|state| state.content() == file_content);
            if !listener_active && bad_state_matches {
                effects
                    .atomic_write(file, &decision.snapshot_content)
                    .with_context(|| {
                        format!(
                            "live_prompt_drift visible repair write for {}",
                            file.display()
                        )
                    })?;
                save_document_snapshot_and_crdt(file, &decision.snapshot_content)?;
                log_reconciled("disk_fallback");
                log_flow_event(
                    file,
                    agent_doc_flow::types::FlowEvent::new(
                        agent_doc_flow::types::FlowName::DocumentMutation,
                        agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
                        agent_doc_flow::types::FlowOutcome::Completed,
                    )
                    .with_reason("live_prompt_drift_visible_repair_reconciled"),
                );
                eprintln!(
                    "[commit] reconciled live_prompt_drift visible repair for {} via guarded disk fallback ({} bytes)",
                    file.display(),
                    decision.snapshot_content.len()
                );
                return Ok(());
            }

            if let Ok(Some(_recovered)) = try_auto_recover_live_prompt_drift(
                effects,
                file,
                &decision.snapshot_content,
                &file_content,
            ) {
                log_reconciled(if listener_active {
                    "editor_ipc"
                } else {
                    "disk_fallback"
                });
                return Ok(());
            }
        }
    }

    let detail = format!(
        "redeliver_editor={} bad_len={} bad_hash={} repaired_len={} repaired_hash={}",
        decision.redeliver_editor,
        bad_len,
        bad_hash,
        decision.snapshot_content.len(),
        agent_doc_hash::content_hash(&decision.snapshot_content)
    );
    log_ipc_proof_failure_with_recycle(
        file,
        "ipc_visible_repair",
        patch_id,
        reason.label(),
        "retry_without_disk_write",
        &detail,
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

pub fn try_editor_converge(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
) -> Result<bool> {
    let canonical_file = file
        .canonicalize()
        .with_context(|| format!("{source}: failed to resolve {}", file.display()))?;
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical_file);
    cleanup_legacy_ipc_degraded(&project_root);
    if current_content == target {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=already_current",
                file.display()
            ),
        );
        return Ok(true);
    }
    // `Some` means a LIVE CRDT replica accepted the canonical write (a real editor
    // buffer to converge). `None` means the relay resolved to disk authority — either
    // no editor is attached, or the plugin-owner lease is stale (attached lease but
    // zero live replicas, `#stale-lease-cpc-authority`). We use this below to skip a
    // controller round-trip on the healthy live-editor path.
    let cpc_relay_write =
        effects.apply_canonical_replace_if_attached(file, current_content, target, source)?;
    if let Some(snapshot) = live_buffer_delivery_missing_operator_text_authority_after_refresh(
        &canonical_file,
        current_content,
        source,
    ) {
        let editor_id = snapshot.editor_id.as_deref().unwrap_or("unknown");
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked reason=editor_capability_missing capability={} editor_id={} live_len={} live_hash={} action=editor_reload_required",
                file.display(),
                agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
                editor_id,
                snapshot.len,
                snapshot.hash
            ),
        );
        anyhow::bail!(
            "{source}: refused editor convergence for {} because live editor buffer {} lacks required capability {}",
            file.display(),
            editor_id,
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY
        );
    }
    // `#stale-lease-cpc-authority` (write-converge mirror): the plugin-owner lease
    // still claims an attached editor, but the CRDT recovery authority just proved
    // zero live replicas with delivery converged — the editor exited (or the
    // controller recycled and the editor has not re-registered its replica) without
    // releasing the lease. There is no live editor buffer to converge, so attempting
    // editor-IPC convergence would ACK/reject against a phantom endpoint and then
    // refuse the disk write forever (`no_visible_write_receipt` — the reported JB
    // `Compact Exchange` exit-1). Resolve to the guarded disk write now, exactly as
    // the relay's own disk-authority resolution intends; the controller disk-change
    // watcher reconciles the stale relay canonical when an editor re-attaches.
    if cpc_relay_write.is_none()
        && live_editor_attached(file)
        && stale_owner_zero_live_replica(&canonical_file, source)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=disk_stale_lease reason=zero_live_replica_disk_authority action=demote_phantom_editor",
                file.display()
            ),
        );
        if try_detached_disk_write(
            effects,
            file,
            current_content,
            target,
            source,
            "stale_lease_zero_live_replica",
        )? {
            return Ok(true);
        }
    }
    match ipc_direct_disk_degraded(&project_root, file) {
        Ok(true) => {
            log_ipc_dewedge_prefer_file_ipc(file, source);
            let canonical = file.canonicalize()?;
            let patch_id = uuid::Uuid::new_v4().to_string();
            let Some(mut payload) =
                editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
            else {
                if try_detached_disk_write(
                    effects,
                    file,
                    current_content,
                    target,
                    source,
                    "listener_degraded_no_component_delta",
                )? {
                    return Ok(true);
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} transport=blocked degraded_cause=no_component_delta action=refuse_external_disk_write",
                        file.display()
                    ),
                );
                anyhow::bail!(
                    "{source}: refused direct disk write for {} while editor IPC listener is degraded (cause=no_component_delta)",
                    file.display()
                );
            };
            target_payload_to_live_editor(file, &mut payload, "file_ipc_convergence");
            if try_editor_converge_file_ipc(
                effects,
                FileIpcConvergenceRequest {
                    file,
                    project_root: &project_root,
                    payload: &payload,
                    patch_id: &patch_id,
                    source,
                    reason: "listener_degraded",
                },
            )? {
                return Ok(true);
            }
            if try_detached_disk_write(
                effects,
                file,
                current_content,
                target,
                source,
                "listener_degraded_editor_detached",
            )? {
                return Ok(true);
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_writeback file={} transport=blocked degraded_cause=listener_degraded action=refuse_external_disk_write",
                    file.display()
                ),
            );
            anyhow::bail!(
                "{source}: refused direct disk write for {} while editor IPC listener is degraded",
                file.display()
            );
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!(
                "[write] WARNING: {source} converge degradation check failed (non-fatal): {e}"
            );
        }
    }
    if !agent_doc_ipc_io::is_listener_active(&project_root) {
        if try_detached_disk_write(
            effects,
            file,
            current_content,
            target,
            source,
            "no_listener",
        )? {
            return Ok(true);
        }
        return refuse_unproven_editor_delivery(file, source, "no_listener", None);
    }

    let canonical = canonical_file;
    let patch_id = uuid::Uuid::new_v4().to_string();
    let Some(mut payload) =
        editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
    else {
        if try_detached_disk_write(
            effects,
            file,
            current_content,
            target,
            source,
            "no_component_delta",
        )? {
            return Ok(true);
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked reason=no_component_delta action=refuse_external_disk_write",
                file.display()
            ),
        );
        anyhow::bail!(
            "{source}: refused direct disk write for {} while editor IPC listener is active (reason=no_component_delta)",
            file.display()
        );
    };
    target_payload_to_live_editor(file, &mut payload, "editor_convergence");

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_editor_convergence_attempt file={} patch_id={} patches={} node_patches={} frontmatter={}",
            file.display(),
            patch_id,
            payload
                .get("patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload
                .get("node_patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload.get("frontmatter").is_some(),
        ),
    );

    match agent_doc_ipc_io::send_message(&project_root, &payload) {
        Ok(Some(_ack)) => {
            let visible_write = poll_visible_write_text_lazily_event_or_projection(
                file,
                &project_root,
                &patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )?;
            let Some(recovered) = visible_write else {
                return refuse_unproven_editor_delivery(
                    file,
                    source,
                    "no_visible_write_receipt",
                    Some(&patch_id),
                );
            };
            if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                &recovered,
            ) == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                target,
            ) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} recovered_len={} transport=editor_ipc",
                        file.display(),
                        patch_id,
                        recovered.len()
                    ),
                );
                if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                    eprintln!(
                        "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                    );
                }
                record_visible_write_commit_candidate_receipt(
                    file,
                    &patch_id,
                    target,
                    "editor_convergence",
                );
                Ok(true)
            } else if convergence_recovered_editor_wins_for_payload(&recovered, target, &payload) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc resolution=editor_wins_outside_touched_components",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                    eprintln!(
                        "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                    );
                }
                record_visible_write_commit_candidate_receipt(
                    file,
                    &patch_id,
                    &recovered,
                    "editor_convergence",
                );
                Ok(true)
            } else {
                let recovery = refresh_editor_after_ack_mismatch(
                    file,
                    &project_root,
                    &canonical,
                    target,
                    &recovered,
                    current_content,
                    source,
                );
                if recovery == AckMismatchRefreshOutcome::ReplayedTarget {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "{source}_writeback file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc recovery=ack_mismatch_replayed_target",
                            file.display(),
                            patch_id,
                            recovered.len(),
                            target.len()
                        ),
                    );
                    if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                        eprintln!(
                            "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                        );
                    }
                    record_visible_write_commit_candidate_receipt(
                        file,
                        &patch_id,
                        target,
                        "editor_convergence",
                    );
                    return Ok(true);
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} transport=blocked reason=ack_mismatch recovered_len={} target_len={} action=editor_convergence_required",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                refuse_unproven_editor_delivery(file, source, "ack_mismatch", Some(&patch_id))
            }
        }
        Ok(None) => {
            if try_detached_disk_write(effects, file, current_content, target, source, "no_ack")? {
                return Ok(true);
            }
            refuse_unproven_editor_delivery(file, source, "no_ack", Some(&patch_id))
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_writeback file={} reason=send_failed error={} note=converge_send_error",
                    file.display(),
                    err
                ),
            );
            if is_socket_status_error(err.to_string())
                && try_editor_converge_file_ipc(
                    effects,
                    FileIpcConvergenceRequest {
                        file,
                        project_root: &project_root,
                        payload: &payload,
                        patch_id: &patch_id,
                        source,
                        reason: "socket_status_error",
                    },
                )?
            {
                return Ok(true);
            }
            if is_socket_receipt_timeout_error(err.to_string()) {
                match record_ipc_socket_ack_timeout(&project_root, file, Some(&patch_id), source) {
                    Ok(true) => {
                        eprintln!(
                            "[write] IPC listener degraded for {} after repeated {source} receipt timeouts",
                            file.display()
                        );
                        log_write_wedge_requests_supervisor_recycle(file, source);
                    }
                    Ok(false) => {}
                    Err(e) => eprintln!(
                        "[write] WARNING: {source} converge ack-timeout record failed (non-fatal): {e}"
                    ),
                }
            }
            if try_detached_disk_write(
                effects,
                file,
                current_content,
                target,
                source,
                "send_failed",
            )? {
                return Ok(true);
            }
            refuse_unproven_editor_delivery(file, source, "send_failed", Some(&patch_id))
        }
    }
}

struct FileIpcConvergenceRequest<'a> {
    file: &'a Path,
    project_root: &'a Path,
    payload: &'a serde_json::Value,
    patch_id: &'a str,
    source: &'a str,
    reason: &'a str,
}

fn try_editor_converge_file_ipc(
    effects: &dyn EditorConvergenceEffects,
    request: FileIpcConvergenceRequest<'_>,
) -> Result<bool> {
    let FileIpcConvergenceRequest {
        file,
        project_root,
        payload,
        patch_id,
        source,
        reason,
    } = request;
    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked degraded_cause={reason}_no_file_ipc action=refuse_external_disk_write",
                file.display()
            ),
        );
        return Ok(false);
    }
    let patch_file = patches_dir.join(format!("{patch_id}.json"));
    let patch_count = payload
        .get("patches")
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or(0)
        + payload
            .get("node_patches")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .unwrap_or(0)
        + usize::from(payload.get("frontmatter").is_some());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_file_ipc_convergence_attempt file={} patch_id={} degraded_cause={} patches={}",
            file.display(),
            patch_id,
            reason,
            patch_count
        ),
    );
    if write_file_ipc_and_poll_delivery(
        effects,
        &patch_file,
        payload,
        file,
        patch_count,
        FileIpcDeliveryOptions::guard_committed_cycle(),
    )? {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} patch_id={} transport=file_ipc degraded_cause={}",
                file.display(),
                patch_id,
                reason
            ),
        );
        return Ok(true);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_writeback file={} patch_id={} transport=blocked degraded_cause={reason}_file_ipc_unproven action=refuse_external_disk_write",
            file.display(),
            patch_id
        ),
    );
    Ok(false)
}

pub fn editor_convergence_payload(
    canonical_file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
    patch_id: &str,
) -> Result<Option<serde_json::Value>> {
    let mut patches =
        agent_doc_document::component_patches::component_replace_patches(current_content, target)?;
    let frontmatter = live_prompt_drift_convergence_frontmatter(current_content, target);
    let node_patches = queue_consume_node_patches(current_content, target, source);

    if !node_patches.is_empty() {
        let node_patched_components = node_patches
            .iter()
            .filter_map(|patch| patch.get("component").and_then(|value| value.as_str()))
            .map(str::to_string)
            .collect::<HashSet<_>>();
        patches.retain(|patch| {
            patch
                .get("component")
                .and_then(|value| value.as_str())
                .is_none_or(|component| !node_patched_components.contains(component))
        });
    }

    if patches.is_empty() && node_patches.is_empty() && frontmatter.is_none() {
        return Ok(None);
    }

    let normalized_baseline =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
            current_content,
        );
    let mut payload = serde_json::json!({
        "type": "patch",
        "file": canonical_file.to_string_lossy(),
        "patches": patches,
        "node_patches": node_patches,
        "unmatched": "",
        "baseline": current_content,
        "baseline_hash": agent_doc_hash::content_hash(current_content),
        "baseline_normalized_hash": agent_doc_hash::content_hash(&normalized_baseline),
        "reposition_boundary": false,
        "patch_id": patch_id,
    });
    if let Some(frontmatter) = frontmatter {
        payload["frontmatter"] = serde_json::Value::String(frontmatter);
    }
    Ok(Some(payload))
}

fn queue_consume_node_patches(
    current_content: &str,
    target: &str,
    source: &str,
) -> Vec<serde_json::Value> {
    if source != "queue_consume" {
        return Vec::new();
    }
    agent_doc_ipc_protocol::build_ipc_node_patches_json(Some(current_content), Some(target))
        .into_iter()
        .filter(|patch| patch.get("component").and_then(|value| value.as_str()) == Some("queue"))
        .collect()
}

pub fn converge_document_or_disk(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    target: &str,
    current: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(effects, file, target, current, source)? {
        return Ok(());
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} because editor convergence did not complete",
        file.display()
    )
}

pub fn converge_or_disk_write(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(effects, file, target, current, source)? {
        return Ok(());
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} because editor convergence did not complete",
        file.display()
    )
}

pub fn live_prompt_drift_convergence_frontmatter(
    file_content: &str,
    snapshot: &str,
) -> Option<String> {
    let file_frontmatter = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(file_content);
    let snapshot_frontmatter = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(snapshot)?;
    if file_frontmatter == Some(snapshot_frontmatter) {
        None
    } else {
        Some(snapshot_frontmatter.to_string())
    }
}

pub fn cleanup_legacy_ipc_degraded(project_root: &Path) {
    let marker = project_root.join(".agent-doc/ipc-degraded");
    if marker.is_file()
        && let Err(e) = std::fs::remove_file(&marker)
    {
        eprintln!(
            "[write] WARNING: failed to remove legacy IPC degraded marker {}: {}",
            marker.display(),
            e
        );
    }
}

pub const IPC_DEWEDGE_TIMEOUT_THRESHOLD: u64 = 2;

pub fn ipc_dewedge_marker_path(project_root: &Path, file: &Path) -> Result<PathBuf> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    Ok(project_root
        .join(".agent-doc/ipc-degraded")
        .join(format!("{hash}.json")))
}

pub fn ipc_dewedge_marker_for_current_session(
    project_root: &Path,
    file: &Path,
) -> Result<Option<serde_json::Value>> {
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if !marker.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marker)
        .with_context(|| format!("failed to read IPC degraded marker {}", marker.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse IPC degraded marker {}", marker.display()))?;
    let marker_session = value
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let session_id =
        agent_doc_frontmatter_io::session::read_session_id(file).unwrap_or_else(|| "-".to_string());
    if marker_session != session_id {
        return Ok(None);
    }
    Ok(Some(value))
}

pub fn ipc_direct_disk_degraded(project_root: &Path, file: &Path) -> Result<bool> {
    let degraded = ipc_dewedge_marker_for_current_session(project_root, file)?
        .and_then(|value| value.get("degraded").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if !degraded {
        return Ok(false);
    }
    // `#ipc-degrade-self-heal`: the degrade latch is a circuit breaker, not a
    // permanent session verdict. It may clear only when the plugin proves it can
    // accept and receipt a lightweight message.
    match agent_doc_ipc_io::probe_listener_receipt(project_root, ipc_dewedge_probe_timeout()) {
        Ok(true) => {
            remove_ipc_dewedge_marker(project_root, file, "listener_receipt_recovered")?;
            return Ok(false);
        }
        Ok(false) => {}
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "ipc_socket_degraded_self_heal_probe_failed file={} reason={}",
                    file.display(),
                    err.to_string().replace(char::is_whitespace, "_")
                ),
            );
        }
    }
    Ok(true)
}

fn ipc_dewedge_probe_timeout() -> std::time::Duration {
    if cfg!(test) {
        std::time::Duration::from_millis(250)
    } else {
        std::time::Duration::from_millis(750)
    }
}

pub fn log_ipc_dewedge_direct_disk_skip(file: &Path, transport: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_listener_degraded_direct_disk file={} transport={} reason=repeated_ack_timeout",
            file.display(),
            transport
        ),
    );
}

/// `#ipc-degraded-prefers-file-ipc`: a latched-degraded socket means only the
/// plugin's *socket* listener is wedged. The file-IPC patch queue uses a
/// separate plugin file watcher that is very likely still alive, so a degraded
/// write routes through it (the plugin applies via the Document API) instead of
/// a raw disk write that manufactures an IDEA "File Cache Conflict". If file IPC
/// also fails to prove delivery, the write fails closed for retry.
pub fn log_ipc_dewedge_prefer_file_ipc(file: &Path, transport: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_degraded_prefer_file_ipc file={} transport={} reason=repeated_ack_timeout disk_write=disabled",
            file.display(),
            transport
        ),
    );
}

pub fn record_ipc_socket_ack_timeout(
    project_root: &Path,
    file: &Path,
    patch_id: Option<&str>,
    transport: &str,
) -> Result<bool> {
    cleanup_legacy_ipc_degraded(project_root);
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create IPC degraded marker directory {}",
                parent.display()
            )
        })?;
    }
    let prior = ipc_dewedge_marker_for_current_session(project_root, file)?;
    let prior_timeouts = prior
        .as_ref()
        .and_then(|value| value.get("consecutive_timeouts").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    // `#midturn-wedge-recycle`: preserve the once-per-episode recycle guard across
    // marker rewrites. If a mid-turn recycle was already attempted for this wedge
    // episode, further accruing timeouts must NOT reset it — re-recycling a binary
    // that a recycle already failed to un-wedge would spin. The flag is cleared only
    // when the whole marker is removed (a proven-live receipt self-heal).
    let prior_recycle_attempted = prior
        .as_ref()
        .and_then(|value| value.get("recycle_attempted").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    let consecutive_timeouts = prior_timeouts.saturating_add(1);
    let degraded = agent_doc_supervisor::lifecycle::write_wedged_from_ipc_failures(
        consecutive_timeouts,
        true,
        IPC_DEWEDGE_TIMEOUT_THRESHOLD,
    );
    let value = serde_json::json!({
        "session_id": agent_doc_frontmatter_io::session::read_session_id(file)
            .unwrap_or_else(|| "-".to_string()),
        "consecutive_timeouts": consecutive_timeouts,
        "degraded": degraded,
        "recycle_attempted": prior_recycle_attempted,
        "last_patch_id": patch_id.unwrap_or("-"),
        "last_transport": transport,
    });
    atomic_write(&marker, &serde_json::to_string_pretty(&value)?)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_ack_timeout_recorded file={} transport={} patch_id={} consecutive_timeouts={} degraded={}",
            file.display(),
            transport,
            patch_id.unwrap_or("-"),
            consecutive_timeouts,
            degraded
        ),
    );
    Ok(degraded)
}

pub fn remove_ipc_dewedge_marker(project_root: &Path, file: &Path, reason: &str) -> Result<()> {
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if marker.exists() {
        std::fs::remove_file(&marker).with_context(|| {
            format!("failed to remove IPC degraded marker {}", marker.display())
        })?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_socket_ack_timeouts_cleared file={} reason={}",
                file.display(),
                reason
            ),
        );
    }
    Ok(())
}

pub fn clear_ipc_socket_ack_timeouts(project_root: &Path, file: &Path, reason: &str) -> Result<()> {
    let Some(value) = ipc_dewedge_marker_for_current_session(project_root, file)? else {
        return Ok(());
    };
    // A routine successful write clears accrued timeout votes, but it must NOT
    // clear a *degraded* latch on its own. The degraded latch is cleared only by
    // a proven-live listener re-probe (`#ipc-degrade-self-heal`).
    if value
        .get("degraded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok(());
    }
    remove_ipc_dewedge_marker(project_root, file, reason)
}

/// `#supselfheal` Phase 2 — read the persisted editor-IPC wedge fact for `file`
/// so the route-owned supervisor idle watch can feed `write_wedged` into
/// `supervisor_recycle_action`. Best effort: a missing/unreadable marker is
/// "not wedged".
pub fn editor_ipc_write_wedged(project_root: &Path, file: &Path) -> bool {
    ipc_dewedge_marker_for_current_session(project_root, file)
        .ok()
        .flatten()
        .and_then(|value| value.get("degraded").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// `#midturn-wedge-recycle` — a latched editor-IPC wedge that has NOT yet had a
/// mid-turn recycle attempted for this episode. This is the signal the idle watch
/// feeds into `supervisor_recycle_action` as `write_wedged`: it goes false as soon
/// as [`mark_ipc_wedge_recycle_attempted`] latches the guard, so a single wedge
/// episode drives at most one auto-recycle and cannot spin. Best effort: a
/// missing/unreadable marker is "no recycle needed".
pub fn editor_ipc_write_wedge_needs_recycle(project_root: &Path, file: &Path) -> bool {
    let Some(value) = ipc_dewedge_marker_for_current_session(project_root, file)
        .ok()
        .flatten()
    else {
        return false;
    };
    let degraded = value
        .get("degraded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let recycle_attempted = value
        .get("recycle_attempted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    degraded && !recycle_attempted
}

/// `#midturn-wedge-recycle` — latch the once-per-episode recycle guard on the
/// dewedge marker so the fresh supervisor (post-`execve`) does not immediately
/// re-read the still-latched wedge and recycle-loop. Called right before the
/// recycle `execve`. A no-op (Ok) when there is no current-session marker.
pub fn mark_ipc_wedge_recycle_attempted(project_root: &Path, file: &Path) -> Result<()> {
    let Some(mut value) = ipc_dewedge_marker_for_current_session(project_root, file)? else {
        return Ok(());
    };
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "recycle_attempted".to_string(),
            serde_json::Value::Bool(true),
        );
    }
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    atomic_write(&marker, &serde_json::to_string_pretty(&value)?)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_wedge_recycle_attempted_latched file={} reason=midturn_wedge_recycle",
            file.display()
        ),
    );
    Ok(())
}

/// `#supselfheal` Phase 2 — log that a wedged editor-IPC write is now requesting
/// a supervisor recycle through the policy owner.
pub fn log_write_wedge_requests_supervisor_recycle(file: &Path, source: &str) {
    let request_status = if !agent_doc_supervisor_io::config::supervisor_auto_recycle_enabled(file)
    {
        "request_skipped reason=auto_recycle_disabled".to_string()
    } else if let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) {
        match agent_doc_supervisor_io::recycle_request::request_recycle_for_doc(
            file,
            "repeated_ack_timeout_active_listener",
        ) {
            Ok(()) => format!("requested project_root={}", project_root.display()),
            Err(err) => format!(
                "request_failed project_root={} error={}",
                project_root.display(),
                format!("{err:#}").replace('\n', "\\n")
            ),
        }
    } else {
        "request_skipped reason=no_project_root".to_string()
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "write_wedged_supervisor_recycle_requested file={} source={} action=request_recycle_through_owner request_status={} reason=repeated_ack_timeout_active_listener",
            file.display(),
            source,
            request_status
        ),
    );
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod convergence_fixture_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn write_converge_payload_cycle_id_prefers_latest_projection_over_stale_sidecar() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test-session\n---\n\ncontent";
        fs::write(&doc, content).unwrap();

        let first =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        let sidecar_path = agent_doc_fs::cycle_state_path_for(&doc).unwrap().unwrap();
        let stale_first_sidecar = fs::read(&sidecar_path).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(2));
        let second =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        assert_ne!(first.cycle_id, second.cycle_id);
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        fs::write(&sidecar_path, stale_first_sidecar).unwrap();

        assert_eq!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .cycle_id,
            first.cycle_id
        );
        assert_eq!(
            write_converge_cycle_id_for_payload(&doc).as_deref(),
            Some(second.cycle_id.as_str())
        );
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

        let root_clone = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root_clone, |_msg| {
                Some(r#"{"type":"receipt","status":"applied","id":"x"}"#.to_string())
            });
        });
        std::thread::sleep(std::time::Duration::from_millis(150));

        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a recovered live listener must self-heal the degrade latch"
        );
        let marker = ipc_dewedge_marker_path(dir.path(), &doc).unwrap();
        assert!(
            !marker.exists(),
            "self-heal must remove the degraded marker"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(dir.path()));
        drop(server);
    }

    #[test]
    fn editor_ipc_write_wedged_reads_latched_degraded_marker() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        let file = project_root.join("plan.md");
        fs::write(&file, "# plan\n").unwrap();
        assert!(!editor_ipc_write_wedged(project_root, &file));
        for _ in 0..IPC_DEWEDGE_TIMEOUT_THRESHOLD {
            record_ipc_socket_ack_timeout(project_root, &file, Some("p1"), "finalize").unwrap();
        }
        assert!(
            editor_ipc_write_wedged(project_root, &file),
            "a latched degraded marker should read as a write wedge"
        );
    }

    #[test]
    fn wedge_needs_recycle_latches_once_per_episode() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        let file = project_root.join("plan.md");
        fs::write(&file, "# plan\n").unwrap();

        // No marker yet → nothing to recycle.
        assert!(!editor_ipc_write_wedge_needs_recycle(project_root, &file));

        // Latch the degraded wedge.
        for _ in 0..IPC_DEWEDGE_TIMEOUT_THRESHOLD {
            record_ipc_socket_ack_timeout(project_root, &file, Some("p1"), "finalize").unwrap();
        }
        assert!(
            editor_ipc_write_wedge_needs_recycle(project_root, &file),
            "a fresh latched wedge should request a mid-turn recycle"
        );

        // After the recycle is attempted, the guard flips false so the fresh
        // supervisor cannot re-read the still-latched wedge and recycle-loop.
        mark_ipc_wedge_recycle_attempted(project_root, &file).unwrap();
        assert!(
            !editor_ipc_write_wedge_needs_recycle(project_root, &file),
            "an already-attempted recycle must not request another one"
        );
        assert!(
            editor_ipc_write_wedged(project_root, &file),
            "the underlying degrade latch is unchanged; only the recycle guard flipped"
        );

        // Further accruing timeouts must NOT reset the guard (no spin).
        record_ipc_socket_ack_timeout(project_root, &file, Some("p2"), "finalize").unwrap();
        assert!(
            !editor_ipc_write_wedge_needs_recycle(project_root, &file),
            "accruing more timeouts after a recycle attempt must not re-arm the recycle"
        );

        // A full self-heal removal starts a clean episode.
        remove_ipc_dewedge_marker(project_root, &file, "test_self_heal").unwrap();
        for _ in 0..IPC_DEWEDGE_TIMEOUT_THRESHOLD {
            record_ipc_socket_ack_timeout(project_root, &file, Some("p3"), "finalize").unwrap();
        }
        assert!(
            editor_ipc_write_wedge_needs_recycle(project_root, &file),
            "a fresh wedge episode after marker removal should request a recycle again"
        );
    }
}

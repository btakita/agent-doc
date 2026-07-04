//! Repair sidecar I/O.

pub mod pending;

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub use pending::{clear_pending, save_pending};

pub trait RepairIoEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;

    fn mark_committed_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState>;

    fn mark_abandoned_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState>;

    fn apply_closeout_recovery_mutation(
        &self,
        file: &Path,
        mutation: agent_doc_flow_io::closeout::CloseoutRecoveryMutation<'_>,
    ) -> Result<()>;
}

#[derive(Debug, Serialize)]
struct BlockedRepairPayloadRecord<'a> {
    captured_at: u64,
    file: String,
    reason: &'a str,
    payload_sha256: String,
    response_body: &'a str,
}

/// Persist a blocked repair replay payload under `.agent-doc/repair-blocked`.
pub fn save_blocked_repair_payload(file: &Path, response: &str, reason: &str) -> Result<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("resolve project root for blocked repair payload")?;
    let dir = root.join(".agent-doc/repair-blocked");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create blocked repair dir {}", dir.display()))?;
    let filename = format!(
        "{}-{}.json",
        agent_doc_hash::content_hash(canonical.to_string_lossy().as_ref()),
        now_millis()
    );
    let path = dir.join(filename);
    let record = BlockedRepairPayloadRecord {
        captured_at: now_secs(),
        file: canonical.display().to_string(),
        reason,
        payload_sha256: agent_doc_hash::content_hash(response),
        response_body: response,
    };
    let json = serde_json::to_string_pretty(&record)?;
    std::fs::write(&path, json)
        .with_context(|| format!("write blocked repair payload {}", path.display()))?;
    Ok(path)
}

pub fn historical_committed_capture_replay(
    file: &Path,
    doc_content: &str,
) -> Result<Option<agent_doc_capture_io::CaptureRecord>> {
    let Some(capture) = agent_doc_capture_io::latest_committed(file)? else {
        return Ok(None);
    };
    if agent_doc_turn::response_replay::response_already_applied(
        doc_content,
        &capture.response_body,
    ) {
        return Ok(None);
    }
    let Some(response_heading) =
        agent_doc_turn::response_replay::first_response_heading_line(&capture.response_body)
    else {
        return Ok(None);
    };
    if !agent_doc_turn::response_replay::has_matching_orphan_prompt_for_committed_capture(
        doc_content,
        response_heading,
    ) {
        return Ok(None);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "repair_replay_committed_capture file={} capture_id={}",
            file.display(),
            capture.capture_id
        ),
    );
    Ok(Some(capture))
}

pub fn visible_response_patch_from_document(
    file: &Path,
    doc_content: &str,
) -> Result<Option<String>> {
    let Some(snapshot_doc) = agent_doc_snapshot_io::load(file)? else {
        return Ok(None);
    };
    let template_mode = agent_doc_frontmatter::frontmatter::parse(doc_content)
        .map(|(fm, _)| fm.resolve_mode().is_template())
        .unwrap_or(false);
    Ok(
        agent_doc_turn::response_replay::extract_visible_response_patch_between(
            &snapshot_doc,
            doc_content,
            template_mode,
        ),
    )
}

pub fn head_already_matches_current_doc(file: &Path, doc_content: &str) -> Result<bool> {
    Ok(agent_doc_git_io::revision::show_head(file)?
        .as_deref()
        .is_some_and(|head| {
            agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(head)
                == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                    doc_content,
                )
        }))
}

fn discard_pending_capture_for_manual_repair(
    effects: &impl RepairIoEffects,
    file: &Path,
    current_doc: &str,
) -> Result<()> {
    effects.apply_closeout_recovery_mutation(
        file,
        agent_doc_flow_io::closeout::CloseoutRecoveryMutation::RetireStaleCapture {
            content: Some(current_doc),
            clear_pending_response: true,
            delete_pre_response: true,
            mark_cycle_committed_event: Some("repair_respect_manual_exchange_tail_removal"),
            reason: agent_doc_turn::closeout_recovery::CloseoutRecoveryMutationReason::RespectManualTailRemoval,
        },
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "repair_discard_stale_capture_after_manual_tail_removal file={}",
            file.display()
        ),
    );
    eprintln!(
        "[repair] respected manual removal of escaped conversation tail in {}",
        file.display()
    );
    Ok(())
}

pub fn retire_stale_capture_if_drifted(
    effects: &impl RepairIoEffects,
    file: &Path,
    doc_content: &str,
    capture: &agent_doc_capture_io::CaptureRecord,
) -> Result<bool> {
    let captured_response_body_missing = !agent_doc_turn::response_replay::response_already_applied(
        doc_content,
        &capture.response_body,
    )
        && !agent_doc_turn::response_replay::response_already_applied_after_prefix_strip(
            doc_content,
            &capture.response_body,
        );
    let captured_response_heading_answered =
        agent_doc_turn::response_replay::first_response_heading_line(&capture.response_body)
            .is_some_and(|heading| {
                agent_doc_turn::response_replay::live_exchange_answers_heading(doc_content, heading)
            });
    let decision = agent_doc_workflow::capture::decide_stale_capture_retirement(
        agent_doc_workflow::capture::StaleCaptureRetirementEvidence {
            state: capture.state,
            replay_baseline_drifted: agent_doc_capture_io::replay_baseline_drifted(file, capture)?,
            captured_response_body_missing,
            captured_response_heading_answered,
        },
    );

    match decision {
        agent_doc_workflow::capture::StaleCaptureRetirementDecision::Keep => Ok(false),
        agent_doc_workflow::capture::StaleCaptureRetirementDecision::RetireWedgedWriteApplied => {
            effects.apply_closeout_recovery_mutation(
                file,
                agent_doc_flow_io::closeout::CloseoutRecoveryMutation::RetireStaleCapture {
                    content: Some(doc_content),
                    clear_pending_response: true,
                    delete_pre_response: true,
                    mark_cycle_committed_event: Some("repair_retire_wedged_write_applied_capture"),
                    reason: agent_doc_turn::closeout_recovery::CloseoutRecoveryMutationReason::RetireWedgedWriteAppliedCapture,
                },
            )?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "repair_retire_wedged_write_applied_capture file={} capture_id={} cycle_id={}",
                    file.display(),
                    capture.capture_id,
                    capture.cycle_id
                ),
            );
            eprintln!(
                "[repair] retired wedged write-applied capture for {} (response missing from document + baseline drifted); rebuilt snapshot/CRDT from current and preserved the captured body for forensics",
                file.display()
            );
            Ok(true)
        }
        agent_doc_workflow::capture::StaleCaptureRetirementDecision::RetireSupersededCapturedOnlyOrphan => {
            effects.apply_closeout_recovery_mutation(
                file,
                agent_doc_flow_io::closeout::CloseoutRecoveryMutation::RetireStaleCapture {
                    content: None,
                    clear_pending_response: true,
                    delete_pre_response: true,
                    mark_cycle_committed_event: None,
                    reason: agent_doc_turn::closeout_recovery::CloseoutRecoveryMutationReason::RetireSupersededCapturedOnlyOrphan,
                },
            )?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "repair_retire_superseded_captured_only_orphan file={} capture_id={} cycle_id={}",
                    file.display(),
                    capture.capture_id,
                    capture.cycle_id
                ),
            );
            eprintln!(
                "[repair] retired superseded Captured-only orphan for {} (captured response's heading already answered in the live exchange + baseline drifted); preserved the captured body for forensics",
                file.display()
            );
            Ok(true)
        }
    }
}

pub fn respect_manual_exchange_tail_removal_if_safe(
    effects: &impl RepairIoEffects,
    file: &Path,
    doc_content: &str,
    capture: &agent_doc_capture_io::CaptureRecord,
) -> Result<bool> {
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    if !fm.resolve_mode().is_template() {
        return Ok(false);
    }

    let Some(snapshot_content) = agent_doc_snapshot_io::load(file)? else {
        return Ok(false);
    };
    if capture.snapshot_hash != Some(agent_doc_hash::content_hash(&snapshot_content)) {
        return Ok(false);
    }

    let Some(stripped_snapshot) =
        agent_doc_template::strip_conversation_tail_outside_exchange(&snapshot_content)?
    else {
        return Ok(false);
    };
    if !agent_doc_turn::closeout_recovery::content_matches_ignoring_trailing_newlines(
        &stripped_snapshot,
        doc_content,
    ) {
        return Ok(false);
    }

    discard_pending_capture_for_manual_repair(effects, file, doc_content)?;
    Ok(true)
}

pub fn cancel_preflight_cycle(
    effects: &impl RepairIoEffects,
    file: &Path,
) -> Result<agent_doc_turn::repair::CancelOutcome> {
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(agent_doc_turn::repair::CancelOutcome::NoOpenCycle);
    };
    if !state.is_open() {
        return Ok(agent_doc_turn::repair::CancelOutcome::NoOpenCycle);
    }
    if !matches!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted) {
        return Ok(agent_doc_turn::repair::CancelOutcome::Protected);
    }
    if agent_doc_capture_io::load_by_id(file, &state.cycle_id)?.is_some() {
        return Ok(agent_doc_turn::repair::CancelOutcome::Protected);
    }
    let snapshot_content = agent_doc_snapshot_io::load(file)?;
    let file_content = std::fs::read_to_string(file).ok();
    effects.mark_abandoned_frontmatter(
        file,
        "cancel_preflight_cycle_abandoned",
        snapshot_content.as_deref(),
        file_content.as_deref(),
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "cancel_preflight_cycle_abandoned file={} cycle_id={}",
            file.display(),
            state.cycle_id
        ),
    );
    eprintln!(
        "[cancel] abandoned empty preflight_started cycle {} for {} on explicit run cancel; next dispatch starts fresh",
        state.cycle_id,
        file.display()
    );
    Ok(agent_doc_turn::repair::CancelOutcome::Abandoned)
}

pub fn repair_stale_preflight_started_cycle(
    effects: &impl RepairIoEffects,
    file: &Path,
) -> Result<agent_doc_turn::repair::RepairOutcome> {
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(agent_doc_turn::repair::RepairOutcome::Noop);
    };
    if state.phase != agent_doc_turn::CyclePhase::PreflightStarted {
        return Ok(agent_doc_turn::repair::RepairOutcome::Noop);
    }

    let file_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for stale preflight repair",
            file.display()
        )
    })?;
    let snapshot_content = agent_doc_snapshot_io::load(file)?;
    let current_file_hash = agent_doc_hash::content_hash(&file_content);
    let current_snapshot_hash = snapshot_content
        .as_deref()
        .map(agent_doc_hash::content_hash);
    let current_normalized_file_hash =
        agent_doc_document::transient_markers::replay_content_hash(&file_content);
    let current_normalized_snapshot_hash = snapshot_content
        .as_deref()
        .map(agent_doc_document::transient_markers::replay_content_hash);

    let raw_hashes_match = state.file_hash.as_deref() == Some(current_file_hash.as_str())
        && state.snapshot_hash == current_snapshot_hash;
    let normalized_hashes_match = state.normalized_file_hash.as_deref()
        == Some(current_normalized_file_hash.as_str())
        && state.normalized_snapshot_hash == current_normalized_snapshot_hash;

    if raw_hashes_match || normalized_hashes_match {
        if !head_already_matches_current_doc(file, &file_content)?
            && let Some(marker) = agent_doc_session_check_io::detect_bypassed_response_write(file)?
        {
            agent_doc_flow_io::closeout::log_closeout_guard_event(
                file,
                agent_doc_flow::types::FlowStage::TerminalGuard,
                agent_doc_flow::types::FlowOutcome::FailedClosed,
                agent_doc_turn::closeout_guard::CloseoutGuardReason::ResponsePatchbackUncommitted,
            );
            anyhow::bail!(
                "{} for {}: stale preflight_started cycle `{}` has visible response patchback drift ({marker}) that is not committed in HEAD. Run `agent-doc write --commit {}` or `agent-doc finalize {}` through the normal closeout path; recovery will not report an already-committed cycle while this response is still only in the working tree.",
                agent_doc_turn::repair::RESPONSE_PATCHBACK_UNCOMMITTED_ERROR,
                file.display(),
                state.cycle_id,
                file.display(),
                file.display(),
            );
        }
        effects.mark_committed_frontmatter(
            file,
            "repair_preflight_stale_lock",
            snapshot_content.as_deref(),
            Some(&file_content),
        )?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "repair_preflight_stale_lock file={} cycle_id={}",
                file.display(),
                state.cycle_id
            ),
        );
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::TerminalGuard,
            agent_doc_flow::types::FlowOutcome::Completed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::StalePreflightLockRepaired,
        );
        eprintln!(
            "[repair] repaired stale preflight_started cycle {} for {}",
            state.cycle_id,
            file.display()
        );
        return Ok(agent_doc_turn::repair::RepairOutcome::StalePreflightLockRepaired);
    }

    if let Some(reason) = repair_committed_historical_snapshot_drift(file)? {
        let repaired_snapshot = agent_doc_snapshot_io::load(file)?;
        effects.mark_committed_frontmatter(
            file,
            "repair_preflight_committed_historical",
            repaired_snapshot.as_deref(),
            Some(&file_content),
        )?;
        agent_doc_capture_io::mark_committed(file)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "repair_preflight_committed_historical file={} cycle_id={} reason={}",
                file.display(),
                state.cycle_id,
                reason
            ),
        );
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::TerminalGuard,
            agent_doc_flow::types::FlowOutcome::Completed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::StalePreflightLockRepaired,
        );
        eprintln!(
            "[repair] closed stale preflight_started cycle {} for {} after repairing committed historical {} drift",
            state.cycle_id,
            file.display(),
            reason
        );
        return Ok(agent_doc_turn::repair::RepairOutcome::StalePreflightLockRepaired);
    }

    if let Some(marker) = agent_doc_session_check_io::detect_bypassed_response_write(file)? {
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::TerminalGuard,
            agent_doc_flow::types::FlowOutcome::FailedClosed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::OpenCycle,
        );
        anyhow::bail!(
            "{} for {}: found visible response patchback ({marker}) but no pending/capture artifact exists and HEAD cannot prove the patchback was already committed",
            agent_doc_turn::repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR,
            file.display(),
        );
    }

    let cycle_capture_exists = agent_doc_capture_io::load_by_id(file, &state.cycle_id)?.is_some();
    let age_secs = agent_doc_turn::closeout_recovery::stale_preflight_cycle_age_secs(
        state.started_at,
        state.updated_at,
        now_secs(),
    );
    if !cycle_capture_exists
        && let Some(change) =
            agent_doc_session_check_io::first_unstarted_prompt_bearing_change(file)?
        && !agent_doc_turn::closeout_recovery::prompt_change_is_orchestration_handoff_marker(
            &change.text,
        )
    {
        let preview = change
            .text
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or(change.text.as_str())
            .trim();
        if age_secs >= agent_doc_turn::repair::STALE_EMPTY_PREFLIGHT_TTL_SECS {
            effects.mark_abandoned_frontmatter(
                file,
                "repair_preflight_stale_prompt_cycle_abandoned",
                snapshot_content.as_deref(),
                Some(&file_content),
            )?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "repair_preflight_stale_prompt_cycle_abandoned file={} cycle_id={} age_secs={} prompt_preview={}",
                    file.display(),
                    state.cycle_id,
                    age_secs,
                    preview
                ),
            );
            agent_doc_flow_io::closeout::log_closeout_guard_event(
                file,
                agent_doc_flow::types::FlowStage::TerminalGuard,
                agent_doc_flow::types::FlowOutcome::FailedClosed,
                agent_doc_turn::closeout_guard::CloseoutGuardReason::StalePreflightCycleAbandoned,
            );
            eprintln!(
                "[repair] abandoned stale empty preflight_started cycle {} for {} after {}s; unresolved prompt remains visible for the next preflight",
                state.cycle_id,
                file.display(),
                age_secs
            );
            return Ok(agent_doc_turn::repair::RepairOutcome::StalePreflightCycleAbandoned);
        }
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::TerminalGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::OpenCycle,
        );
        anyhow::bail!(
            "{} for {}: previous cycle `{}` is still `preflight_started`, the live document has unresolved prompt_target: {preview}, and no response exists to replay. The cycle is only {}s old; wait until it is stale or restart the harness pane and rerun `agent-doc {}` (or use `agent-doc start {}` from a fresh pane) so the prompt is handled by a new response cycle.",
            agent_doc_turn::repair::EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR,
            file.display(),
            state.cycle_id,
            age_secs,
            file.display(),
            file.display(),
        );
    }

    if age_secs >= agent_doc_turn::repair::STALE_EMPTY_PREFLIGHT_TTL_SECS && !cycle_capture_exists {
        effects.mark_committed_frontmatter(
            file,
            "repair_preflight_stale_empty_cycle",
            snapshot_content.as_deref(),
            Some(&file_content),
        )?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "repair_preflight_stale_empty_cycle file={} cycle_id={} age_secs={}",
                file.display(),
                state.cycle_id,
                age_secs
            ),
        );
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::TerminalGuard,
            agent_doc_flow::types::FlowOutcome::Completed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::StalePreflightLockRepaired,
        );
        eprintln!(
            "[repair] closed stale empty preflight_started cycle {} for {} after {}s without a capture",
            state.cycle_id,
            file.display(),
            age_secs
        );
        return Ok(agent_doc_turn::repair::RepairOutcome::StalePreflightLockRepaired);
    }

    Ok(agent_doc_turn::repair::RepairOutcome::Noop)
}

pub fn repair_committed_historical_snapshot_drift(file: &Path) -> Result<Option<&'static str>> {
    let Some(snapshot_doc) = agent_doc_snapshot_io::load(file)? else {
        return Ok(None);
    };
    let current_doc = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current_doc == snapshot_doc {
        return Ok(None);
    }

    let Some(head_doc) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(None);
    };
    let historical_mutation =
        agent_doc_document_realtime::write_policy::classify_committed_historical_agent_doc_mutation(
            &snapshot_doc,
            &head_doc,
        );
    // #nm1x: intersect the drift against the current turn scope so independent
    // out-of-scope edits (e.g. a queue item added beside the running one) do not
    // block the historical snapshot repair.
    let turn_scope = agent_doc_turn_scope_io::load(file);
    let non_exchange_component_drift = agent_doc_git::has_blocking_non_exchange_component_drift(
        &snapshot_doc,
        &head_doc,
        turn_scope.as_ref(),
    );
    let historical_response_marker =
        agent_doc_turn::document_drift::detect_bypassed_response_write_between(
            &snapshot_doc,
            &head_doc,
        );
    let historical_prompt_prefix_artifact = snapshot_doc != head_doc
        && !non_exchange_component_drift
        && agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
            &snapshot_doc,
        ) == agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
            &head_doc,
        );
    let Some(reason) = (match historical_mutation {
        Some("exchange") => Some("exchange"),
        None if !non_exchange_component_drift && historical_response_marker.is_some() => {
            Some("exchange")
        }
        None if historical_prompt_prefix_artifact => Some("exchange"),
        _ => None,
    }) else {
        return Ok(None);
    };

    if agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
        &current_doc,
    ) == agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
        &head_doc,
    ) {
        agent_doc_snapshot_io::save(file, &current_doc, agent_doc_ops_log_io::log_op)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "snapshot_repair file={} reason={} basis=head",
                file.display(),
                reason
            ),
        );
        return Ok(Some(reason));
    }

    if agent_doc_turn::document_drift::detect_bypassed_response_write_between(
        &head_doc,
        &current_doc,
    )
    .is_none()
    {
        if agent_doc_write_converge_io::guard_no_stale_snapshot_reset_drift(
            file,
            Some(&head_doc),
            &current_doc,
            "historical snapshot repair",
        )? {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "snapshot_repair file={} reason={} basis=visible_rebase_guard",
                    file.display(),
                    reason
                ),
            );
            return Ok(Some(reason));
        }
        let basis = if agent_doc_git::is_safe_user_only_follow_up_after_committed_head(
            &head_doc,
            &current_doc,
        ) {
            "head_follow_up"
        } else {
            "head_local_drift"
        };
        agent_doc_snapshot_io::save(file, &head_doc, agent_doc_ops_log_io::log_op)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "snapshot_repair file={} reason={} basis={}",
                file.display(),
                reason,
                basis
            ),
        );
        return Ok(Some(reason));
    }

    Ok(None)
}

pub fn recover_missing_commit_boundary(
    effects: &impl RepairIoEffects,
    file: &Path,
    event: &str,
) -> Result<Option<&'static str>> {
    let state = agent_doc_cycle_state_io::load(file)?;
    let has_open_commit_boundary = state.as_ref().is_some_and(|state| {
        matches!(
            state.phase,
            agent_doc_turn::CyclePhase::ResponseCaptured | agent_doc_turn::CyclePhase::WriteApplied
        )
    });
    let has_missing_commit_event = if has_open_commit_boundary {
        false
    } else {
        agent_doc_ops_log_io::detect_write_completed_commit_missing(file)?.is_some()
    };
    if !has_open_commit_boundary && !has_missing_commit_event {
        return Ok(None);
    }

    let current_doc = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for commit-boundary recovery",
            file.display()
        )
    })?;
    let head_doc = agent_doc_git_io::revision::show_head(file)?;
    let reason = match agent_doc_snapshot_io::verify_snapshot_committed(file)? {
        agent_doc_snapshot_io::SnapshotCommitStatus::Committed => head_doc
            .as_deref()
            .filter(|head| {
                agent_doc_turn::document_drift::detect_bypassed_response_write_between(
                    head,
                    &current_doc,
                )
                .is_none()
            })
            .map(|_| "already-committed HEAD"),
        _ => repair_committed_historical_snapshot_drift(file)?
            .map(|_| "committed historical exchange snapshot drift"),
    };
    let Some(reason) = reason else {
        return Ok(None);
    };

    let repaired_snapshot = agent_doc_snapshot_io::load(file)?;
    effects.mark_committed_frontmatter(
        file,
        event,
        repaired_snapshot.as_deref(),
        Some(&current_doc),
    )?;
    agent_doc_capture_io::mark_committed(file)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "repair_commit_boundary_recovered file={} event={} reason={}",
            file.display(),
            event,
            reason
        ),
    );
    agent_doc_flow_io::closeout::log_closeout_guard_event(
        file,
        agent_doc_flow::types::FlowStage::TerminalGuard,
        agent_doc_flow::types::FlowOutcome::Completed,
        agent_doc_turn::closeout_guard::CloseoutGuardReason::CommitBoundaryRecovered,
    );
    Ok(Some(reason))
}

pub fn repair_completed_backlog_items(
    effects: &impl RepairIoEffects,
    file: &Path,
) -> Result<agent_doc_turn::repair::RepairOutcome> {
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} for completed backlog reap repair",
            file.display()
        )
    })?;
    let components = agent_doc_element::element::parse(&content).with_context(|| {
        format!(
            "failed to parse {} for completed backlog reap repair",
            file.display()
        )
    })?;
    let Some(backlog) = components
        .iter()
        .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
    else {
        return Ok(agent_doc_turn::repair::RepairOutcome::Noop);
    };

    let doc_id = agent_doc_hash::document_id_for_path(file);
    let (canonical_body, _) = agent_doc_element_backlog::backlog::backfill(
        backlog.content(&content),
        &doc_id,
        &HashSet::new(),
    );
    let (new_body, removed) = agent_doc_element_backlog::backlog::reap_with_items(&canonical_body)?;
    if removed.is_empty() {
        return Ok(agent_doc_turn::repair::RepairOutcome::Noop);
    }

    let mut repaired = backlog.replace_content(&content, &new_body);
    if let Some(archived) =
        agent_doc_element_backlog_io::done_archive::archive_pending_done(file, &repaired, &removed)?
    {
        repaired = archived;
    }
    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_top_backlog_status_content(&repaired)?
    {
        repaired = reconciled;
    }

    effects.atomic_write(file, &repaired)?;

    let repaired_snapshot = if let Some(snap_content) = agent_doc_snapshot_io::load(file)? {
        let snap_components =
            agent_doc_element::element::parse(&snap_content).with_context(|| {
                format!(
                    "failed to parse snapshot for completed backlog reap repair {}",
                    file.display()
                )
            })?;
        let snap_backlog = snap_components
            .iter()
            .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
            .with_context(|| {
                format!(
                    "completed backlog reap repair requires the snapshot backlog component in {}",
                    file.display()
                )
            })?;

        let mut new_snapshot = snap_backlog.replace_content(&snap_content, &new_body);
        if let Some(archived) = agent_doc_element_backlog_io::done_archive::archive_pending_done(
            file,
            &new_snapshot,
            &removed,
        )? {
            new_snapshot = archived;
        }
        if let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_top_backlog_status_content(
                &new_snapshot,
            )?
        {
            new_snapshot = reconciled;
        }
        agent_doc_snapshot_io::save(file, &new_snapshot, agent_doc_ops_log_io::log_op)?;
        Some(new_snapshot)
    } else {
        None
    };

    if repaired_snapshot.as_deref() == Some(repaired.as_str()) {
        let _ = effects.mark_committed_frontmatter(
            file,
            "repair_completed_backlog_reap",
            Some(&repaired),
            Some(&repaired),
        );
    }

    let refs = removed
        .iter()
        .map(|item| format!("#{}", item.id))
        .collect::<Vec<_>>()
        .join(", ");
    let removed_ids: Vec<String> = removed.iter().map(|item| item.id.clone()).collect();
    let _ = agent_doc_cycle_state_io::record_reaped_pending_ids(file, &removed_ids);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "repair_completed_backlog_reap file={} count={} ids={}",
            file.display(),
            removed.len(),
            refs
        ),
    );
    eprintln!(
        "[repair] reaped stale completed backlog item(s) in {}: {}",
        file.display(),
        refs
    );

    Ok(agent_doc_turn::repair::RepairOutcome::CompletedBacklogReaped)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir_without_agent_doc_ancestor() -> tempfile::TempDir {
        for base in [
            Path::new("/var/tmp"),
            Path::new("/dev/shm"),
            Path::new("/tmp"),
        ] {
            if !base.is_dir() || agent_doc_project_root_io::project_root_containing(base).is_some()
            {
                continue;
            }
            if let Ok(dir) = tempfile::Builder::new()
                .prefix("agent-doc-repair-io-")
                .tempdir_in(base)
                && agent_doc_project_root_io::project_root_containing(dir.path()).is_none()
            {
                return dir;
            }
        }
        panic!("no writable temp base without a .agent-doc ancestor");
    }

    #[test]
    fn saves_blocked_repair_payload_under_project_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let path = save_blocked_repair_payload(&doc, "response body", "agent markers").unwrap();

        assert!(path.starts_with(root.join(".agent-doc/repair-blocked")));
        let json = std::fs::read_to_string(path).unwrap();
        assert!(json.contains("\"reason\": \"agent markers\""));
        assert!(json.contains("\"response_body\": \"response body\""));
        assert!(json.contains("\"payload_sha256\""));
    }

    #[test]
    fn falls_back_to_file_parent_without_project_root() {
        let dir = tempdir_without_agent_doc_ancestor();
        let doc = dir.path().join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let path = save_blocked_repair_payload(&doc, "response body", "blocked").unwrap();

        assert!(path.starts_with(dir.path().join(".agent-doc/repair-blocked")));
    }
}

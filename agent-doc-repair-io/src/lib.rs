//! Repair sidecar I/O.

pub mod pending;

use agent_doc_turn::op_log::OpsLogEvent;
use agent_doc_turn::{
    closeout_recovery::visible_response_recovery_is_adoptable, repair::RepairOutcome,
    response_replay,
};
use agent_doc_workflow::capture::capture_state_is_repairable;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const STRUCTURAL_CAPTURE_HISTORY_LIMIT: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructuralHistoryRecovery {
    content: String,
    checkpoint_sequence: u64,
    checkpoint_capture_id: String,
    checkpoint_response_sha256: String,
    observed_response_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LossyEditorProjectionRecovery {
    content: String,
    checkpoint_sequence: u64,
    checkpoint_capture_id: String,
    checkpoint_response_sha256: String,
}

fn replace_frontmatter_from_current(rebuilt: &str, current: &str) -> Option<String> {
    let rebuilt_yaml = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(rebuilt)?;
    let current_yaml = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(current)?;
    if rebuilt_yaml == current_yaml {
        return Some(rebuilt.to_string());
    }
    let rebuilt_suffix = rebuilt.get("---\n".len() + rebuilt_yaml.len()..)?;
    Some(format!("---\n{current_yaml}{rebuilt_suffix}"))
}

fn current_projection_is_explained_by_recovery(current: &str, recovered: &str) -> bool {
    let normalize = |content: &str| {
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(content)
    };
    let current = normalize(current);
    let recovered = normalize(recovered);
    let allowed_lines = recovered
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let exact_lines = allowed_lines.iter().copied().collect::<HashSet<_>>();
    current
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .all(|line| {
            exact_lines.contains(line)
                || (line.len() >= 16
                    && allowed_lines
                        .iter()
                        .any(|allowed| allowed.len() > line.len() && allowed.contains(line)))
        })
}

fn select_lossy_editor_projection_recovery(
    current: &str,
    operator_projection: &str,
    history: &[agent_doc_cycle_state_io::CapturedResponseCheckpoint],
) -> Option<LossyEditorProjectionRecovery> {
    for checkpoint in history {
        let Some(captured_baseline) = checkpoint.baseline_content.as_deref() else {
            continue;
        };
        let captured_baseline_hash = agent_doc_capture_io::replay_file_hash(captured_baseline);
        if checkpoint.file_hash.as_deref() != Some(captured_baseline_hash.as_str())
            || !corrupted_materialization_is_explained_by_checkpoint(
                current,
                captured_baseline,
                &checkpoint.response_body,
            )
            || !agent_doc_turn::response_replay::response_materialized_in_content(
                &checkpoint.response_body,
                current,
            )
            || agent_doc_turn::response_replay::response_materialized_in_content(
                &checkpoint.response_body,
                operator_projection,
            )
        {
            continue;
        }
        let Some(rebuilt) =
            agent_doc_turn::response_replay::materialize_response_in_current_exchange(
                operator_projection,
                &checkpoint.response_body,
            )
        else {
            continue;
        };
        let Some(content) = replace_frontmatter_from_current(&rebuilt, current) else {
            continue;
        };
        if content.len() <= current.len()
            || content == current
            || agent_doc_element::element::structural_corruption_reason(&content).is_some()
            || !agent_doc_turn::response_replay::response_materialized_in_content(
                &checkpoint.response_body,
                &content,
            )
            || !current_projection_is_explained_by_recovery(current, &content)
        {
            continue;
        }
        return Some(LossyEditorProjectionRecovery {
            content,
            checkpoint_sequence: checkpoint.sequence,
            checkpoint_capture_id: checkpoint.capture_id.clone(),
            checkpoint_response_sha256: checkpoint.response_sha256.clone(),
        });
    }
    None
}

fn select_structural_history_recovery(
    current: &str,
    active: &agent_doc_cycle_state_io::ProjectedCapturedResponse,
    history: &[agent_doc_cycle_state_io::CapturedResponseCheckpoint],
) -> Option<StructuralHistoryRecovery> {
    agent_doc_element::element::structural_corruption_reason(current)?;
    let current_replay_hash = agent_doc_capture_io::replay_file_hash(current);
    let active_index = history.iter().position(|checkpoint| {
        checkpoint.cycle_id == active.cycle_id
            && checkpoint.capture_id == active.capture_id
            && checkpoint.response_sha256 == active.response_sha256
            && checkpoint.file_hash.as_deref() == Some(current_replay_hash.as_str())
            && checkpoint
                .baseline_content
                .as_deref()
                .is_some_and(|baseline| {
                    agent_doc_capture_io::replay_file_hash(baseline) == current_replay_hash
                })
    })?;

    for checkpoint in history.iter().skip(active_index + 1) {
        if checkpoint.cycle_id != active.cycle_id {
            continue;
        }
        let Some(baseline) = checkpoint.baseline_content.as_deref() else {
            continue;
        };
        if checkpoint.file_hash.as_deref()
            != Some(agent_doc_capture_io::replay_file_hash(baseline).as_str())
            || agent_doc_element::element::structural_corruption_reason(baseline).is_some()
        {
            continue;
        }
        let Some(partial) = agent_doc_capture_io::reconcile_partial_response_lines(
            baseline,
            current,
            &checkpoint.response_body,
        ) else {
            continue;
        };
        if !corrupted_materialization_is_explained_by_checkpoint(
            current,
            baseline,
            &checkpoint.response_body,
        ) {
            continue;
        }
        let Some(content) =
            agent_doc_turn::response_replay::materialize_response_in_current_exchange(
                baseline,
                &checkpoint.response_body,
            )
        else {
            continue;
        };
        if agent_doc_element::element::structural_corruption_reason(&content).is_some()
            || !agent_doc_turn::response_replay::response_materialized_in_content(
                &checkpoint.response_body,
                &content,
            )
        {
            continue;
        }
        return Some(StructuralHistoryRecovery {
            content,
            checkpoint_sequence: checkpoint.sequence,
            checkpoint_capture_id: checkpoint.capture_id.clone(),
            checkpoint_response_sha256: checkpoint.response_sha256.clone(),
            observed_response_lines: partial.removed_nonblank_lines,
        });
    }
    None
}

fn corrupted_materialization_is_explained_by_checkpoint(
    current: &str,
    baseline: &str,
    response: &str,
) -> bool {
    let normalize = |content: &str| {
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(content)
    };
    let current = normalize(current);
    let baseline = normalize(baseline);
    let response = normalize(response);
    let common_prefix = current
        .as_bytes()
        .iter()
        .zip(baseline.as_bytes())
        .take_while(|(left, right)| left == right)
        .count();
    let required_prefix = 512usize.max(baseline.len() / 10).min(baseline.len());
    if common_prefix < required_prefix {
        return false;
    }

    let allowed_lines = baseline
        .lines()
        .chain(response.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let exact_lines = allowed_lines.iter().copied().collect::<HashSet<_>>();
    current
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .all(|line| {
            exact_lines.contains(line)
                || (line.len() >= 16
                    && allowed_lines
                        .iter()
                        .any(|allowed| allowed.len() > line.len() && allowed.contains(line)))
        })
}

pub use pending::load_active_pending_response;
pub use pending::{clear_pending, save_pending};

pub trait RepairIoEffects {
    fn atomic_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<String>;

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

pub trait RepairStrictReplayWriteEffects {
    fn run_strict_write_replay(
        &self,
        file: &Path,
        response: &str,
        is_template: bool,
        is_stream: bool,
        force_disk: bool,
        queue_completion_ids: &[String],
    ) -> Result<()>;
}

pub trait RepairFallbackWriteEffects {
    fn apply_template_from_string(
        &self,
        file: &Path,
        response: &str,
        force_disk: bool,
    ) -> Result<()>;

    fn apply_append_from_string(&self, file: &Path, response: &str) -> Result<()>;
}

pub trait RepairRecoveredQueueHeadEffects {
    fn strike_recovered_free_text_queue_head(&self, file: &Path, expected_head: &str)
    -> Result<()>;
}

pub trait RepairReplayWriteEffects:
    RepairStrictReplayWriteEffects + RepairFallbackWriteEffects + RepairRecoveredQueueHeadEffects
{
}

impl<T> RepairReplayWriteEffects for T where
    T: RepairStrictReplayWriteEffects
        + RepairFallbackWriteEffects
        + RepairRecoveredQueueHeadEffects
{
}

pub trait RepairTemplateWriteEffects {
    fn atomic_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<String>;

    fn repair_response_prompt_order_for_file(
        &self,
        content: &str,
        known_response: Option<&str>,
        file: &Path,
        fallback_snapshot: Option<&str>,
    ) -> Result<Option<String>>;

    fn normalize_template_structure_or_fail_preserving(
        &self,
        content: &str,
        file: &Path,
        prompt_input: Option<&str>,
    ) -> Result<String>;
}

pub struct RepairCoordinatorEffects<
    'a,
    R: RepairIoEffects + RepairTemplateWriteEffects,
    W: RepairReplayWriteEffects,
> {
    pub repair_io_effects: &'a R,
    pub replay_write_effects: &'a W,
    pub complete_required_closeout: fn(&Path) -> Result<bool>,
    pub inspect_session: fn(&Path) -> Result<agent_doc_session_check_io::SessionCheckStatus>,
    pub recover_missing_committed_head_response: fn(&Path) -> Result<bool>,
    pub recover_dedupe_only_drift: fn(&Path) -> Result<bool>,
}

impl<'a, R, W> Copy for RepairCoordinatorEffects<'a, R, W>
where
    R: RepairIoEffects + RepairTemplateWriteEffects,
    W: RepairReplayWriteEffects,
{
}

impl<'a, R, W> Clone for RepairCoordinatorEffects<'a, R, W>
where
    R: RepairIoEffects + RepairTemplateWriteEffects,
    W: RepairReplayWriteEffects,
{
    fn clone(&self) -> Self {
        *self
    }
}

pub fn recover_empty_response_for_strict_closeout<
    R: RepairIoEffects + RepairTemplateWriteEffects,
    W: RepairReplayWriteEffects,
>(
    effects: RepairCoordinatorEffects<'_, R, W>,
    file: &Path,
    strict_closeout: bool,
    has_pending_mutation: bool,
    force_disk_override: Option<bool>,
) -> Result<bool> {
    if strict_closeout {
        let outcome =
            run_with_queue_completion_ids_and_force_disk(effects, file, &[], force_disk_override)?;
        if force_disk_override != Some(true)
            && (effects.recover_missing_committed_head_response)(file)?
        {
            return Ok(true);
        }
        if outcome.repaired() {
            if outcome == RepairOutcome::AlreadyApplied {
                (effects.complete_required_closeout)(file)?;
            }
            eprintln!(
                "[write] empty response stdin; recovered existing agent-doc response state with {:?}",
                outcome
            );
            return Ok(true);
        }
        if (effects.recover_dedupe_only_drift)(file)? {
            return Ok(true);
        }
    }
    if has_pending_mutation {
        eprintln!(
            "[write] empty response stdin; committing pending mutations without a response body"
        );
        return Ok(true);
    }
    Ok(false)
}

/// Check for a pending response and apply it if found.
pub fn run<R: RepairIoEffects + RepairTemplateWriteEffects, W: RepairReplayWriteEffects>(
    effects: RepairCoordinatorEffects<'_, R, W>,
    file: &Path,
) -> Result<RepairOutcome> {
    run_with_queue_completion_ids(effects, file, &[])
}

fn log_slow_repair_phase(file: &Path, phase: &str, started: &mut std::time::Instant) {
    let elapsed = started.elapsed();
    if elapsed >= std::time::Duration::from_millis(250) {
        eprintln!(
            "[perf] repair.{} file={} elapsed_ms={}",
            phase,
            file.display(),
            elapsed.as_millis()
        );
    }
    *started = std::time::Instant::now();
}

pub fn run_with_queue_completion_ids<
    R: RepairIoEffects + RepairTemplateWriteEffects,
    W: RepairReplayWriteEffects,
>(
    effects: RepairCoordinatorEffects<'_, R, W>,
    file: &Path,
    queue_completion_ids: &[String],
) -> Result<RepairOutcome> {
    run_with_queue_completion_ids_and_force_disk(effects, file, queue_completion_ids, None)
}

pub fn run_with_queue_completion_ids_and_force_disk<
    R: RepairIoEffects + RepairTemplateWriteEffects,
    W: RepairReplayWriteEffects,
>(
    effects: RepairCoordinatorEffects<'_, R, W>,
    file: &Path,
    queue_completion_ids: &[String],
    force_disk_override: Option<bool>,
) -> Result<RepairOutcome> {
    let mut phase_started = std::time::Instant::now();
    // Canonicalize first to handle CWD drift (e.g., when CWD is in a submodule).
    let canonical = file
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("file not found: {}", file.display()))?;
    agent_doc_preflight_io::warnings::report_live_plugin_generation_refresh(&canonical);
    log_slow_repair_phase(&canonical, "plugin_generation_refresh", &mut phase_started);

    let pending_response = pending::load_active_pending_response(&canonical)?;
    log_slow_repair_phase(&canonical, "pending_intent_projection", &mut phase_started);
    let has_pending_response = pending_response.is_some();
    let loaded_capture = agent_doc_capture_io::load_active(&canonical)?;
    log_slow_repair_phase(&canonical, "capture_load", &mut phase_started);
    let projected_capture_is_repairable = loaded_capture
        .as_ref()
        .is_none_or(|capture| capture_state_is_repairable(capture.state));
    let mut capture = loaded_capture
        .clone()
        .filter(|capture| capture_state_is_repairable(capture.state));
    log_slow_repair_phase(&canonical, "capture_projection", &mut phase_started);
    let current_document = repair_current_document_content(
        &canonical,
        "repair_current_document",
        force_disk_override,
    )?;
    log_slow_repair_phase(&canonical, "current_document", &mut phase_started);
    let current_authority = current_document.authority;
    let mut doc_content = current_document.content;
    if force_disk_override != Some(true)
        && let Some(restored) =
            restore_committed_head_after_authority_regression(&canonical, &doc_content)?
    {
        doc_content = restored;
    }
    let cycle_state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    let projected_capture = if projected_capture_is_repairable
        && let Some(state) = cycle_state.as_ref()
        && matches!(
            state.phase,
            agent_doc_turn::CyclePhase::ResponseCaptured | agent_doc_turn::CyclePhase::WriteApplied
        )
        && let Some(capture_id) = state.capture_id.as_deref()
    {
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
    } else {
        None
    };
    log_slow_repair_phase(&canonical, "cycle_projection", &mut phase_started);
    if force_disk_override != Some(true)
        && let Some(active) = projected_capture.as_ref()
    {
        let history = agent_doc_cycle_state_io::load_recent_captured_response_checkpoints(
            &canonical,
            STRUCTURAL_CAPTURE_HISTORY_LIMIT,
        )?;
        if let Some(recovery) = select_structural_history_recovery(&doc_content, active, &history) {
            let prior_content = doc_content;
            agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
                &canonical,
                &recovery.content,
                &prior_content,
                "repair_structural_response_history",
            )?;
            agent_doc_snapshot_io::checkpoint_document_baseline(
                &canonical,
                &recovery.content,
                agent_doc_ops_log_io::log_op,
            )?;
            let file_hash = agent_doc_capture_io::replay_file_hash(&recovery.content);
            let snapshot_hash = agent_doc_hash::content_hash(&recovery.content);
            agent_doc_cycle_state_io::append_response_captured_body(
                &canonical,
                agent_doc_cycle_state_io::CapturedResponseFactInput {
                    cycle_id: &active.cycle_id,
                    capture_id: &active.capture_id,
                    response_sha256: &active.response_sha256,
                    response_body: &active.response_body,
                    intent_body: active.intent_body.as_deref(),
                    mutation_plan_json: active.mutation_plan_json.as_deref(),
                    file_hash: Some(&file_hash),
                    snapshot_hash: Some(&snapshot_hash),
                    baseline_content: Some(&recovery.content),
                },
            )?;
            if let Some(record) = capture.as_mut() {
                let old_file_hash = record.file_hash.clone();
                record.file_hash = Some(file_hash.clone());
                record.snapshot_hash = Some(snapshot_hash.clone());
                record.baseline_content = Some(recovery.content.clone());
                let _ = agent_doc_capture_io::project_structural_recovery_baseline(
                    &canonical,
                    record,
                    old_file_hash.as_deref(),
                    &recovery.content,
                    &snapshot_hash,
                )?;
            }
            agent_doc_ops_log_io::log_op(
                &canonical,
                &format!(
                    "repair_structural_response_history_reconciled file={} checkpoint_sequence={} checkpoint_capture_id={} checkpoint_response_sha256={} observed_response_lines={} prior_hash={} recovered_hash={} authority=lazily_history",
                    canonical.display(),
                    recovery.checkpoint_sequence,
                    recovery.checkpoint_capture_id,
                    recovery.checkpoint_response_sha256,
                    recovery.observed_response_lines,
                    agent_doc_hash::content_hash(&prior_content),
                    agent_doc_hash::content_hash(&recovery.content),
                ),
            );
            doc_content = recovery.content;
        }
    }
    if force_disk_override != Some(true)
        && let Some(merge_baseline) = agent_doc_snapshot_io::load_document_baseline(&canonical)?
        && let Some(operator_projection) =
            agent_doc_op_capture_io::last_editor_text_for_base(&canonical, &merge_baseline)?
        && operator_projection != doc_content
    {
        let history = agent_doc_cycle_state_io::load_recent_captured_response_checkpoints(
            &canonical,
            STRUCTURAL_CAPTURE_HISTORY_LIMIT,
        )?;
        if let Some(recovery) =
            select_lossy_editor_projection_recovery(&doc_content, &operator_projection, &history)
        {
            let prior_content = doc_content;
            agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
                &canonical,
                &recovery.content,
                &prior_content,
                "repair_lossy_editor_projection",
            )?;
            agent_doc_snapshot_io::checkpoint_document_baseline(
                &canonical,
                &recovery.content,
                agent_doc_ops_log_io::log_op,
            )?;
            agent_doc_ops_log_io::log_op(
                &canonical,
                &format!(
                    "repair_lossy_editor_projection_reconciled file={} checkpoint_sequence={} checkpoint_capture_id={} checkpoint_response_sha256={} prior_hash={} recovered_hash={} authority=editor_ops_plus_response_history",
                    canonical.display(),
                    recovery.checkpoint_sequence,
                    recovery.checkpoint_capture_id,
                    recovery.checkpoint_response_sha256,
                    agent_doc_hash::content_hash(&prior_content),
                    agent_doc_hash::content_hash(&recovery.content),
                ),
            );
            doc_content = recovery.content;
        }
    }
    let has_active_capture_evidence = capture.is_some() || projected_capture.is_some();
    log_slow_repair_phase(
        &canonical,
        "structural_and_lossy_projection_history",
        &mut phase_started,
    );
    let historical_capture = if !has_pending_response && !has_active_capture_evidence {
        historical_committed_capture_replay(&canonical, &doc_content)?
    } else {
        None
    };
    let live_editor_authority = force_disk_override != Some(true)
        && agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(file);
    let active_codex_session = agent_doc_codex_hook_io::load_active_session_for_current_file(file)
        .ok()
        .flatten()
        .is_some();
    let visible_response_recovery = if !has_pending_response
        && !has_active_capture_evidence
        && historical_capture.is_none()
        && visible_response_recovery_is_adoptable(
            cycle_state.as_ref().map(|state| state.phase),
            live_editor_authority || active_codex_session,
        )
        && agent_doc_git_io::status::is_in_git_repo(file)
        && !head_already_matches_current_doc(file, &doc_content)?
    {
        visible_response_patch_from_document(file, &doc_content)?
    } else {
        None
    };
    log_slow_repair_phase(&canonical, "recovery_evidence", &mut phase_started);
    if !has_pending_response
        && !has_active_capture_evidence
        && historical_capture.is_none()
        && visible_response_recovery.is_none()
    {
        let outcome = repair_stale_preflight_started_cycle(effects.repair_io_effects, file)?;
        if outcome != RepairOutcome::Noop {
            let refreshed_content = repair_current_document_content(
                &canonical,
                "repair_after_stale_preflight",
                force_disk_override,
            )?
            .content;
            let response_prefix_repaired_doc = repair_response_body_prompt_prefixes_if_needed(
                effects.repair_io_effects,
                file,
                &refreshed_content,
            )?;
            if response_prefix_repaired_doc != refreshed_content {
                return Ok(RepairOutcome::TemplateNormalized);
            }
            return Ok(outcome);
        }
        if recover_missing_commit_boundary(
            effects.repair_io_effects,
            file,
            "repair_commit_boundary_recovered",
        )?
        .is_some()
        {
            return Ok(RepairOutcome::CommitBoundaryRecovered);
        }
        let scaffold_repaired_doc = repair_duplicate_exchange_scaffold_if_needed(
            effects.repair_io_effects,
            file,
            &doc_content,
        )?;
        if scaffold_repaired_doc != doc_content {
            return Ok(RepairOutcome::TemplateNormalized);
        }
        let response_prefix_repaired_doc = repair_response_body_prompt_prefixes_if_needed(
            effects.repair_io_effects,
            file,
            &doc_content,
        )?;
        if response_prefix_repaired_doc != doc_content {
            return Ok(RepairOutcome::TemplateNormalized);
        }
        let has_live_prompt =
            agent_doc_session_check_io::realtime_steering_since_turn_baseline(file)?.is_present();
        if !has_live_prompt {
            let repaired_doc =
                repair_template_doc_if_needed(effects.repair_io_effects, file, &doc_content, None)?;
            if repaired_doc != doc_content {
                return Ok(RepairOutcome::TemplateNormalized);
            }
        }
        return repair_completed_backlog_items(effects.repair_io_effects, file);
    }

    let response = projected_capture
        .as_ref()
        .map(|capture| {
            capture
                .intent_body
                .clone()
                .unwrap_or_else(|| capture.response_body.clone())
        })
        .or_else(|| {
            capture.as_ref().map(|record| {
                record
                    .intent_body
                    .clone()
                    .unwrap_or_else(|| record.response_body.clone())
            })
        })
        .or_else(|| historical_capture.as_ref().map(|r| r.response_body.clone()))
        .or_else(|| visible_response_recovery.clone())
        .or(pending_response.clone())
        .unwrap_or_default();

    if response.trim().is_empty() {
        // An empty retained intent has no document mutation to replay.
        let _ = agent_doc_capture_io::mark_discarded(&canonical);
        return Ok(RepairOutcome::Noop);
    }

    if let Some(reconciliation) = reconcile_partial_captured_response(
        &canonical,
        &doc_content,
        &response,
        capture.as_ref(),
        historical_capture.as_ref(),
    )? {
        agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
            &canonical,
            &reconciliation.content,
            &doc_content,
            "repair_partial_captured_response",
        )?;
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &canonical,
            &reconciliation.content,
            agent_doc_ops_log_io::log_op,
        )?;
        agent_doc_ops_log_io::log_op(
            &canonical,
            &format!(
                "repair_partial_captured_response_reconciled file={} removed_nonblank_lines={} recovery=replay_complete_capture",
                canonical.display(),
                reconciliation.removed_nonblank_lines
            ),
        );
        doc_content = reconciliation.content;
    }

    // Dedup guard: check if the response content is already present in the document.
    // This prevents double-apply when retained intent outlived a successful
    // IPC write (e.g., IPC timeout path exits with code 75 without calling clear_pending,
    // but the plugin already applied the content via the IPC patch file).
    let response_already_present =
        response_replay::response_already_applied(&doc_content, &response)
            || response_replay::response_already_applied_after_prefix_strip(
                &doc_content,
                &response,
            );
    if response_already_present {
        if let Some(ref capture) = capture {
            agent_doc_capture_io::validate_replay_with_current_content(
                &canonical,
                capture,
                &doc_content,
            )?;
        }
        eprintln!(
            "[repair] Response already present in document; skipping apply and retiring retained intent"
        );
        let repaired_doc = repair_template_doc_if_needed(
            effects.repair_io_effects,
            file,
            &doc_content,
            Some(&response),
        )?;
        let state_is_open = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
            .map(|state| state.is_open())
            .unwrap_or(true);
        let snapshot_missing_response = agent_doc_snapshot_io::load_document_baseline(file)?
            .as_deref()
            .map(|snapshot_doc| {
                !response_replay::response_already_applied(snapshot_doc, &response)
                    && !response_replay::response_already_applied_after_prefix_strip(
                        snapshot_doc,
                        &response,
                    )
            })
            .unwrap_or(true);
        if (state_is_open || visible_response_recovery.is_some()) && snapshot_missing_response {
            agent_doc_snapshot_io::checkpoint_document_baseline(
                file,
                &repaired_doc,
                agent_doc_ops_log_io::log_op,
            )?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "repair_adopt_existing_response file={} reason=snapshot_missing_response",
                    file.display()
                ),
            );
            eprintln!(
                "[repair] advanced snapshot to the already-present response for {}",
                file.display()
            );
        }
        if state_is_open {
            let head_matches_repaired_doc =
                agent_doc_git_io::revision::show_head(file)?.as_deref() == Some(&repaired_doc);
            let state_result = if head_matches_repaired_doc {
                effects.repair_io_effects.mark_committed_frontmatter(
                    file,
                    "commit_already_current",
                    Some(&repaired_doc),
                    Some(&repaired_doc),
                )
            } else {
                agent_doc_cycle_state_io::mark_write_applied(
                    file,
                    "repair_already_applied",
                    Some(&repaired_doc),
                    Some(&repaired_doc),
                )
            };
            if let Err(e) = state_result {
                eprintln!("[repair] cycle-state update failed: {} (non-fatal)", e);
            }
        }
        pending::clear_pending(&canonical)?;
        return Ok(RepairOutcome::AlreadyApplied);
    }

    if let Some(ref capture) = capture {
        if respect_manual_exchange_tail_removal_if_safe(
            effects.repair_io_effects,
            &canonical,
            &doc_content,
            capture,
        )? {
            return Ok(RepairOutcome::ManualTailRemovalRespected);
        }
        if rebase_capture_to_authoritative_current_if_safe(
            &canonical,
            &doc_content,
            capture,
            current_authority,
        )? {
            eprintln!(
                "[repair] adopted the current {} cut for retained response replay in {} (capture_id={})",
                current_authority.as_str(),
                canonical.display(),
                capture.capture_id,
            );
        } else {
            if retire_stale_capture_if_drifted(
                effects.repair_io_effects,
                &canonical,
                &doc_content,
                capture,
            )? {
                return Ok(RepairOutcome::StaleCaptureRetired);
            }
            agent_doc_capture_io::validate_replay_with_current_content(
                &canonical,
                capture,
                &doc_content,
            )?;
        }
    }

    replay_orphaned_response(
        effects.replay_write_effects,
        OrphanedResponseReplay {
            canonical: &canonical,
            file,
            doc_content: &doc_content,
            response: &response,
            queue_completion_ids,
            historical_capture_present: historical_capture.is_some(),
            force_disk_override,
        },
    )
}

struct RepairCurrentDocument {
    content: String,
    authority: agent_doc_document_realtime::DocAuthority,
}

fn repair_current_document_content(
    file: &Path,
    source: &str,
    force_disk_override: Option<bool>,
) -> Result<RepairCurrentDocument> {
    let current = if force_disk_override == Some(true) {
        agent_doc_document_realtime_io::resolve_disk_current_document(file, source)
            .with_context(|| format!("{source}: failed to read {}", file.display()))?
    } else {
        agent_doc_document_realtime_io::try_resolve_current_document_with_source(file, source)
            .with_context(|| {
                format!(
                    "{source}: failed to resolve current document {}",
                    file.display()
                )
            })?
    };
    Ok(RepairCurrentDocument {
        authority: current.authority(),
        content: current.into_content(),
    })
}

fn reconcile_partial_captured_response(
    file: &Path,
    current: &str,
    response: &str,
    active: Option<&agent_doc_capture_io::CaptureRecord>,
    historical: Option<&HistoricalCommittedCapture>,
) -> Result<Option<agent_doc_capture_io::PartialResponseReconciliation>> {
    let evidence = if let Some(capture) = active {
        Some((
            capture.cycle_id.as_str(),
            capture.capture_id.as_str(),
            capture.response_sha256.as_str(),
            capture.response_body.as_str(),
            capture.intent_body.as_deref(),
            capture.mutation_plan_json.as_deref(),
            capture.file_hash.as_deref(),
            capture.snapshot_hash.as_deref(),
            capture.baseline_content.as_deref(),
        ))
    } else {
        historical.map(|capture| {
            (
                capture.cycle_id.as_str(),
                capture.capture_id.as_str(),
                capture.response_sha256.as_str(),
                capture.response_body.as_str(),
                None,
                None,
                capture.file_hash.as_deref(),
                None,
                capture.baseline_content.as_deref(),
            )
        })
    };
    let Some((
        cycle_id,
        capture_id,
        response_sha256,
        response_body,
        intent_body,
        mutation_plan_json,
        file_hash,
        snapshot_hash,
        durable_baseline,
    )) = evidence
    else {
        return Ok(None);
    };

    let baseline_matches = |content: &str| {
        file_hash
            .is_some_and(|expected| expected == agent_doc_capture_io::replay_file_hash(content))
    };
    let baseline = if let Some(content) = durable_baseline
        && baseline_matches(content)
    {
        Some(content.to_string())
    } else if let Some(head) = agent_doc_git_io::revision::show_head(file)?
        && baseline_matches(&head)
    {
        // HEAD is only an independently hashed historical anchor. The target
        // is still published through editor authority; this is never a Git
        // restore or a disk-only overwrite.
        Some(head)
    } else {
        None
    };
    let Some(baseline) = baseline else {
        return Ok(None);
    };
    let Some(reconciliation) =
        agent_doc_capture_io::reconcile_partial_response_lines(&baseline, current, response)
    else {
        return Ok(None);
    };

    let _ = agent_doc_capture_io::fortify_baseline_content(file, capture_id, &baseline)?;
    agent_doc_cycle_state_io::append_response_captured_body(
        file,
        agent_doc_cycle_state_io::CapturedResponseFactInput {
            cycle_id,
            capture_id,
            response_sha256,
            response_body,
            intent_body,
            mutation_plan_json,
            file_hash,
            snapshot_hash,
            baseline_content: Some(&baseline),
        },
    )?;
    Ok(Some(reconciliation))
}

pub fn repair<R: RepairIoEffects + RepairTemplateWriteEffects, W: RepairReplayWriteEffects>(
    effects: RepairCoordinatorEffects<'_, R, W>,
    file: &Path,
) -> Result<RepairOutcome> {
    repair_with_force_disk_override(effects, file, None)
}

/// Resume a captured closeout while preserving the normal editor/CRDT authority
/// path. This is the supervisor-owned finalize recovery entrypoint: it may replay
/// the durable capture, dedupe an already-materialized response, and finish the
/// commit boundary, but it never elects disk as a fallback authority.
pub fn repair_preserving_live_authority<
    R: RepairIoEffects + RepairTemplateWriteEffects,
    W: RepairReplayWriteEffects,
>(
    effects: RepairCoordinatorEffects<'_, R, W>,
    file: &Path,
) -> Result<RepairOutcome> {
    repair_with_force_disk_override(effects, file, Some(false))
}

fn repair_with_force_disk_override<
    R: RepairIoEffects + RepairTemplateWriteEffects,
    W: RepairReplayWriteEffects,
>(
    effects: RepairCoordinatorEffects<'_, R, W>,
    file: &Path,
    force_disk_override: Option<bool>,
) -> Result<RepairOutcome> {
    let outcome =
        run_with_queue_completion_ids_and_force_disk(effects, file, &[], force_disk_override)?;
    if outcome.repaired()
        && outcome != RepairOutcome::StalePreflightCycleAbandoned
        && agent_doc_git_io::status::is_in_git_repo(file)
    {
        (effects.complete_required_closeout)(file)?;
    } else if !outcome.repaired()
        && let agent_doc_session_check_io::SessionCheckStatus::Interrupted(message) =
            (effects.inspect_session)(file)?
    {
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::SessionCheck,
            agent_doc_flow::types::FlowOutcome::FailedClosed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::SessionCheckInterrupted,
        );
        anyhow::bail!(message);
    }
    Ok(outcome)
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

pub fn fail_closed_on_blocked_template_replay(
    file: &Path,
    response: &str,
    reason: &str,
) -> Result<()> {
    match save_blocked_repair_payload(file, response, reason) {
        Ok(path) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "repair_blocked_replay path={} reason={}",
                    path.display(),
                    reason
                ),
            );
            anyhow::bail!(
                "refused to replay pending response for {} because {}; blocked payload captured at {}",
                file.display(),
                reason,
                path.display()
            );
        }
        Err(err) => {
            anyhow::bail!(
                "refused to replay pending response for {} because {}; additionally failed to save blocked payload: {}",
                file.display(),
                reason,
                err
            );
        }
    }
}

pub fn ensure_repair_materialized_response(
    file: &Path,
    final_doc: &str,
    response: &str,
) -> Result<()> {
    if agent_doc_turn::response_replay::response_materialized_in_content(response, final_doc) {
        return Ok(());
    }
    anyhow::bail!(
        "orphaned response replay did not materialize captured response in {}; refusing to clear capture",
        file.display()
    )
}

pub fn repair_replay_force_disk(file: &Path) -> bool {
    repair_replay_force_disk_with_override(file, None)
}

fn repair_replay_force_disk_with_override(_file: &Path, force_disk_override: Option<bool>) -> bool {
    // Repair replays retained semantic intent through the normal CRDT/CAS write
    // path. Editor absence inferred by a short-lived client is never sufficient
    // authority for a second direct document mutation; only the operator's
    // explicit force-disk escape hatch may request that behavior.
    force_disk_override == Some(true)
}

fn replay_orphaned_response_through_strict_write(
    effects: &impl RepairReplayWriteEffects,
    file: &Path,
    response: &str,
    is_template: bool,
    is_stream: bool,
    queue_completion_ids: &[String],
    force_disk_override: Option<bool>,
) -> Result<()> {
    let force_disk = repair_replay_force_disk_with_override(file, force_disk_override);
    let mode = if is_stream {
        "crdt"
    } else if is_template {
        "template"
    } else {
        "append"
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "repair_replay_via_strict_write file={} mode={} force_disk={} response_hash={}",
            file.display(),
            mode,
            force_disk,
            agent_doc_hash::content_hash(response)
        ),
    );
    effects.run_strict_write_replay(
        file,
        response,
        is_template,
        is_stream,
        force_disk,
        queue_completion_ids,
    )
}

pub struct OrphanedResponseReplay<'a> {
    pub canonical: &'a Path,
    pub file: &'a Path,
    pub doc_content: &'a str,
    pub response: &'a str,
    pub queue_completion_ids: &'a [String],
    pub historical_capture_present: bool,
    pub force_disk_override: Option<bool>,
}

pub fn replay_orphaned_response(
    effects: &impl RepairReplayWriteEffects,
    request: OrphanedResponseReplay<'_>,
) -> Result<agent_doc_turn::repair::RepairOutcome> {
    let OrphanedResponseReplay {
        canonical,
        file,
        doc_content,
        response,
        queue_completion_ids,
        historical_capture_present,
        force_disk_override,
    } = request;

    eprintln!(
        "[repair] Found orphaned response for {} ({} bytes). Applying...",
        file.display(),
        response.len()
    );

    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    let document_is_template = fm.resolve_mode().is_template();
    let use_template_write = document_is_template || response.contains("<!-- patch:");
    let response_to_write = if use_template_write {
        match agent_doc_template::replay_guard::classify_replay_payload(response) {
            agent_doc_template::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
                fail_closed_on_blocked_template_replay(file, response, &reason)?;
                response.to_string()
            }
            agent_doc_template::replay_guard::ReplayPayloadClassification::Replayable(response) => {
                response.into_owned()
            }
            agent_doc_template::replay_guard::ReplayPayloadClassification::Empty => {
                response.to_string()
            }
        }
    } else {
        response.to_string()
    };
    let response_to_write = if document_is_template && !response_to_write.contains("<!-- patch:") {
        format!(
            "<!-- patch:exchange -->\n{}\n<!-- /patch:exchange -->\n",
            response_to_write.trim_end()
        )
    } else {
        response_to_write
    };
    let expected_recovered_queue_head = if recovered_queue_head_is_free_text(doc_content) {
        agent_doc_queue::queue_heads::active_queue_head_text(doc_content)?
    } else {
        None
    };

    if use_template_write {
        if agent_doc_git_io::status::is_in_git_repo(file) {
            replay_orphaned_response_through_strict_write(
                effects,
                file,
                &response_to_write,
                true,
                false,
                queue_completion_ids,
                force_disk_override,
            )?;
        } else {
            effects.apply_template_from_string(
                file,
                &response_to_write,
                repair_replay_force_disk_with_override(file, force_disk_override),
            )?;
        }
    } else if agent_doc_git_io::status::is_in_git_repo(file) {
        replay_orphaned_response_through_strict_write(
            effects,
            file,
            &response_to_write,
            false,
            false,
            queue_completion_ids,
            force_disk_override,
        )?;
    } else {
        effects.apply_append_from_string(file, &response_to_write)?;
    }

    materialize_replayed_response(
        effects,
        canonical,
        file,
        &response_to_write,
        historical_capture_present,
        expected_recovered_queue_head.as_deref(),
    )?;
    Ok(agent_doc_turn::repair::RepairOutcome::ReplayedResponse)
}

fn materialize_replayed_response(
    effects: &impl RepairReplayWriteEffects,
    canonical: &Path,
    file: &Path,
    response_to_write: &str,
    historical_capture_present: bool,
    expected_recovered_queue_head: Option<&str>,
) -> Result<()> {
    let final_doc_after_write =
        agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "repair_replayed_response_after_write",
        )?;
    ensure_repair_materialized_response(file, &final_doc_after_write, response_to_write)?;

    pending::clear_pending(canonical)?;

    if recovered_queue_head_is_free_text(&final_doc_after_write)
        && let Some(expected_head) = expected_recovered_queue_head
        && let Err(err) = effects.strike_recovered_free_text_queue_head(file, expected_head)
    {
        eprintln!("[repair] queue-head strike after replay failed: {err} (non-fatal)");
    }

    eprintln!(
        "[repair] Response repaired and written to {}",
        file.display()
    );
    let final_doc = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "repair_replayed_response_final",
    )?;
    if let Err(e) = agent_doc_cycle_state_io::mark_write_applied(
        file,
        "repair_applied",
        Some(&final_doc),
        Some(&final_doc),
    ) {
        eprintln!("[repair] cycle-state update failed: {} (non-fatal)", e);
    }
    if !historical_capture_present && let Err(e) = agent_doc_capture_io::mark_replayed(canonical) {
        eprintln!("[repair] capture-state update failed: {} (non-fatal)", e);
    }
    Ok(())
}

fn recovered_queue_head_is_free_text(content: &str) -> bool {
    let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(content) else {
        return false;
    };
    fm.queue_active == Some(true)
        && agent_doc_queue::queue_response::queue_head_is_free_text_prompt(content).unwrap_or(false)
}

pub fn repair_template_doc_if_needed(
    effects: &impl RepairTemplateWriteEffects,
    file: &Path,
    doc_content: &str,
    known_response: Option<&str>,
) -> Result<String> {
    let mut dup_opener_input = doc_content.to_string();
    let mut duplicate_opener_changed = false;
    while let Some(merged) =
        agent_doc_template::repair_duplicate_exchange_opener(&dup_opener_input)?
    {
        dup_opener_input = merged;
        duplicate_opener_changed = true;
    }
    let duplicate_scaffold_repaired =
        agent_doc_template::repair_duplicate_exchange_close_scaffold(&dup_opener_input)?
            .unwrap_or_else(|| dup_opener_input.clone());
    let duplicate_scaffold_changed = duplicate_scaffold_repaired != dup_opener_input;
    let duplicate_close_repaired =
        agent_doc_template::repair_duplicate_exchange_close_tail(&duplicate_scaffold_repaired)?
            .unwrap_or_else(|| duplicate_scaffold_repaired.clone());
    let duplicate_close_changed = duplicate_close_repaired != duplicate_scaffold_repaired;
    let tail_repaired =
        agent_doc_template::repair_conversation_tail_outside_exchange(&duplicate_close_repaired)?
            .unwrap_or_else(|| duplicate_close_repaired.clone());
    let tail_changed = tail_repaired != duplicate_close_repaired;
    let inline_boundary_repaired = repair_inline_boundary_fragmentation(&tail_repaired)?;
    let boundary_input = inline_boundary_repaired
        .clone()
        .unwrap_or_else(|| tail_repaired.clone());
    let boundary_repaired = repair_answered_stale_boundary_if_safe(file, &boundary_input)?;
    let boundary_changed = inline_boundary_repaired.is_some() || boundary_repaired.is_some();
    let mut repaired = boundary_repaired.unwrap_or(boundary_input);
    let order_repaired =
        effects.repair_response_prompt_order_for_file(&repaired, known_response, file, None)?;
    let order_changed = order_repaired.is_some();
    if let Some(ordered) = order_repaired {
        repaired = ordered;
    }

    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&repaired)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;

    let prompt_input = repaired.clone();
    if fm.resolve_mode().is_template()
        && let Some(snapshot_content) = agent_doc_snapshot_io::load_document_baseline(file)?
    {
        repaired = agent_doc_template_io::normalize_user_prompts_in_exchange_safe(
            &repaired,
            &repaired,
            &snapshot_content,
            file,
        );
        if let Some(stripped) =
            agent_doc_element_exchange::strip_prompt_prefix_from_response_body_first_lines(
                &repaired,
            )
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "repair_response_body_prompt_prefix_stripped file={}",
                    file.display()
                ),
            );
            repaired = stripped;
        }
        repaired = effects.normalize_template_structure_or_fail_preserving(
            &repaired,
            file,
            Some(&prompt_input),
        )?;
    }
    let prompt_changed = repaired != prompt_input;

    let template_changes = agent_doc_workflow::capture::RepairTemplateChanges {
        duplicate_opener: duplicate_opener_changed,
        duplicate_close: duplicate_close_changed,
        duplicate_scaffold: duplicate_scaffold_changed,
        conversation_tail: tail_changed,
        completed_turn_boundary: boundary_changed,
        response_prompt_order: order_changed,
        prompt_prefixes: prompt_changed,
    };

    if template_changes.should_persist() {
        let save_repaired_snapshot = match agent_doc_snapshot_io::load_document_baseline(file)? {
            Some(snapshot_content) => {
                !agent_doc_turn::closeout_recovery::repair_leaves_unanswered_prompt_diff(
                    &snapshot_content,
                    &repaired,
                    known_response,
                )
            }
            None => true,
        };
        repaired = effects.atomic_write_if_current(
            file,
            &repaired,
            doc_content,
            "repair_template_normalization",
        )?;
        if save_repaired_snapshot {
            agent_doc_snapshot_io::checkpoint_document_baseline(
                file,
                &repaired,
                agent_doc_ops_log_io::log_op,
            )?;
        }
        for change in template_changes.changed_kinds() {
            match change {
                agent_doc_workflow::capture::RepairTemplateChangeKind::DuplicateOpener => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!("repair_duplicate_exchange_opener file={}", file.display()),
                    );
                    eprintln!(
                        "[repair] merged duplicate exchange opener(s) in {}",
                        file.display()
                    );
                }
                agent_doc_workflow::capture::RepairTemplateChangeKind::DuplicateClose => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!("repair_duplicate_exchange_close file={}", file.display()),
                    );
                    eprintln!(
                        "[repair] removed duplicate exchange close and restored escaped content in {}",
                        file.display()
                    );
                }
                agent_doc_workflow::capture::RepairTemplateChangeKind::DuplicateScaffold => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!("repair_duplicate_exchange_scaffold file={}", file.display()),
                    );
                    eprintln!(
                        "[repair] removed duplicate template scaffold after exchange close in {}",
                        file.display()
                    );
                }
                agent_doc_workflow::capture::RepairTemplateChangeKind::ConversationTail => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!("repair_exchange_tail file={}", file.display()),
                    );
                    eprintln!(
                        "[repair] repaired escaped conversation tail in {}",
                        file.display()
                    );
                }
                agent_doc_workflow::capture::RepairTemplateChangeKind::CompletedTurnBoundary => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!("repair_completed_turn_boundary file={}", file.display()),
                    );
                    eprintln!(
                        "[repair] moved stale boundary to the end of the completed exchange turn in {}",
                        file.display()
                    );
                }
                agent_doc_workflow::capture::RepairTemplateChangeKind::ResponsePromptOrder => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!("repair_response_prompt_order file={}", file.display()),
                    );
                    eprintln!(
                        "[repair] repaired response/prompt ordering in {}",
                        file.display()
                    );
                }
                agent_doc_workflow::capture::RepairTemplateChangeKind::PromptPrefixes => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!("repair_prompt_prefixes file={}", file.display()),
                    );
                    eprintln!(
                        "[repair] repaired transcript prompt prefixes in {}",
                        file.display()
                    );
                }
            }
        }
    }

    Ok(repaired)
}

pub fn repair_response_body_prompt_prefixes_if_needed(
    effects: &impl RepairTemplateWriteEffects,
    file: &Path,
    doc_content: &str,
) -> Result<String> {
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    if !fm.resolve_mode().is_template() {
        return Ok(doc_content.to_string());
    }

    let Some(repaired) =
        agent_doc_element_exchange::strip_prompt_prefix_from_response_body_first_lines(doc_content)
    else {
        return Ok(doc_content.to_string());
    };

    let save_repaired_snapshot = match agent_doc_snapshot_io::load_document_baseline(file)? {
        Some(snapshot_content) => {
            !agent_doc_turn::closeout_recovery::repair_leaves_unanswered_prompt_diff(
                &snapshot_content,
                &repaired,
                None,
            )
        }
        None => true,
    };
    let repaired = effects.atomic_write_if_current(
        file,
        &repaired,
        doc_content,
        "repair_response_body_prompt_prefixes",
    )?;
    if save_repaired_snapshot {
        agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &repaired,
            agent_doc_ops_log_io::log_op,
        )?;
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "repair_response_body_prompt_prefix_stripped file={}",
            file.display()
        ),
    );
    eprintln!(
        "[repair] stripped leaked response-body prompt prefixes in {}",
        file.display()
    );
    Ok(repaired)
}

pub fn repair_duplicate_exchange_scaffold_if_needed(
    effects: &impl RepairTemplateWriteEffects,
    file: &Path,
    doc_content: &str,
) -> Result<String> {
    let repaired = agent_doc_template::repair_duplicate_exchange_close_scaffold(doc_content)?
        .unwrap_or_else(|| doc_content.to_string());
    if repaired == doc_content {
        return Ok(repaired);
    }

    let save_repaired_snapshot = match agent_doc_snapshot_io::load_document_baseline(file)? {
        Some(snapshot_content) => {
            !agent_doc_turn::closeout_recovery::repair_leaves_unanswered_prompt_diff(
                &snapshot_content,
                &repaired,
                None,
            )
        }
        None => true,
    };
    let repaired = effects.atomic_write_if_current(
        file,
        &repaired,
        doc_content,
        "repair_duplicate_exchange_scaffold",
    )?;
    if save_repaired_snapshot {
        agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &repaired,
            agent_doc_ops_log_io::log_op,
        )?;
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!("repair_duplicate_exchange_scaffold file={}", file.display()),
    );
    eprintln!(
        "[repair] removed duplicate template scaffold after exchange close in {}",
        file.display()
    );
    Ok(repaired)
}

fn repair_answered_stale_boundary_if_safe(
    file: &Path,
    doc_content: &str,
) -> Result<Option<String>> {
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    if !fm.resolve_mode().is_template()
        || agent_doc_snapshot_io::load_document_baseline(file)?.is_none()
    {
        return Ok(None);
    }

    let components = agent_doc_element::element::parse(doc_content).with_context(|| {
        format!(
            "failed to parse {} for completed-turn boundary repair",
            file.display()
        )
    })?;
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let boundary_markers = boundary_markers_outside_code(doc_content, exchange);
    let Some((boundary_id, _)) = boundary_markers.first() else {
        return Ok(None);
    };

    if boundary_markers.len() > 1 || boundary_markers.iter().any(|(_, standalone)| !standalone) {
        let repaired = agent_doc_template::reposition_boundary_to_end_preserve_head_with_id(
            doc_content,
            Some(boundary_id.as_str()),
        );
        return Ok((repaired != doc_content).then_some(repaired));
    }

    let exchange_body = exchange.content(doc_content);
    let marker = agent_doc_element_boundary::boundary::format_marker(boundary_id);
    let Some(marker_idx) = exchange_body.find(&marker) else {
        return Ok(None);
    };
    let tail_after_boundary = &exchange_body[marker_idx + marker.len()..];
    if tail_after_boundary.trim().is_empty()
        || !agent_doc_diff::prompt_change_is_already_answered(tail_after_boundary)
        || agent_doc_session_check_io::realtime_steering_since_turn_baseline(file)?.is_present()
    {
        return Ok(None);
    }

    let repaired = agent_doc_template::reposition_boundary_to_end_preserve_head_with_id(
        doc_content,
        Some(boundary_id.as_str()),
    );
    if repaired == doc_content {
        return Ok(None);
    }
    Ok(Some(repaired))
}

/// Recover the historical partial-patchback shape where the insertion boundary
/// was duplicated inline on a newer prompt while fragments of the in-flight
/// response landed after it. This runs only from the explicit repair path.
///
/// The inline marker supplies an unambiguous transaction cut: content between
/// that prompt and the next complete response heading is an incomplete write.
/// Move the complete prompt after that response, discard the incomplete
/// fragment, collapse a nearby truncated copy of the prompt, and restore one
/// standalone boundary at exchange end.
fn repair_inline_boundary_fragmentation(doc_content: &str) -> Result<Option<String>> {
    let components = agent_doc_element::element::parse(doc_content)?;
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let exchange_body = exchange.content(doc_content);
    let lines = exchange_body.split_inclusive('\n').collect::<Vec<_>>();
    let boundary_markers = boundary_markers_outside_code(doc_content, exchange);
    let Some((boundary_id, _)) = boundary_markers.iter().find(|(_, standalone)| !standalone) else {
        return Ok(None);
    };
    let marker = agent_doc_element_boundary::boundary::format_marker(boundary_id);
    let Some(inline_idx) = lines
        .iter()
        .position(|line| line.contains(&marker) && line.trim() != marker)
    else {
        return Ok(None);
    };
    let prompt_line = agent_doc_element_boundary::boundary::remove_all(lines[inline_idx]);
    let prompt = prompt_line.trim();
    if prompt.is_empty() || prompt.starts_with("### Re:") || prompt.starts_with("<!--") {
        return Ok(None);
    }

    let Some(response_start) = lines
        .iter()
        .enumerate()
        .skip(inline_idx + 1)
        .find_map(|(idx, line)| line.trim_start().starts_with("### Re:").then_some(idx))
    else {
        return Ok(None);
    };
    let Some(next_response_start) = lines
        .iter()
        .enumerate()
        .skip(response_start + 1)
        .find_map(|(idx, line)| line.trim_start().starts_with("### Re:").then_some(idx))
    else {
        return Ok(None);
    };

    let truncated_prompt_idx = (response_start + 1..next_response_start)
        .rev()
        .find(|&idx| prompt_lines_are_prefix_variants(prompt, lines[idx].trim()));
    let response_end = truncated_prompt_idx.unwrap_or(next_response_start);

    let mut rebuilt_exchange = String::with_capacity(exchange_body.len());
    rebuilt_exchange.push_str(&lines[..inline_idx].concat());
    rebuilt_exchange.push_str(&lines[response_start..response_end].concat());
    if !rebuilt_exchange.ends_with('\n') {
        rebuilt_exchange.push('\n');
    }
    rebuilt_exchange.push_str(prompt);
    rebuilt_exchange.push('\n');
    rebuilt_exchange.push_str(&lines[next_response_start..].concat());

    let rebuilt_doc = exchange.replace_content(doc_content, &rebuilt_exchange);
    let repaired = agent_doc_template::reposition_boundary_to_end_preserve_head_with_id(
        &rebuilt_doc,
        Some(boundary_id),
    );
    Ok((repaired != doc_content).then_some(repaired))
}

fn boundary_markers_outside_code(
    doc_content: &str,
    exchange: &agent_doc_element::element::Component,
) -> Vec<(String, bool)> {
    const PREFIX: &str = "<!-- agent:boundary:";
    const SUFFIX: &str = " -->";

    let code_ranges = agent_doc_element::element::find_code_ranges(doc_content);
    let exchange_body = exchange.content(doc_content);
    let mut markers = Vec::new();
    let mut line_offset = 0;
    for line in exchange_body.split_inclusive('\n') {
        let mut cursor = 0;
        while let Some(relative_start) = line[cursor..].find(PREFIX) {
            let start = cursor + relative_start;
            let absolute = exchange.open_end + line_offset + start;
            if code_ranges
                .iter()
                .any(|&(code_start, code_end)| absolute >= code_start && absolute < code_end)
            {
                cursor = start + PREFIX.len();
                continue;
            }
            let id_start = start + PREFIX.len();
            let Some(relative_end) = line[id_start..].find(SUFFIX) else {
                break;
            };
            let end = id_start + relative_end + SUFFIX.len();
            let marker = &line[start..end];
            markers.push((
                line[id_start..id_start + relative_end].trim().to_string(),
                line.trim() == marker,
            ));
            cursor = end;
        }
        line_offset += line.len();
    }
    markers
}

fn prompt_lines_are_prefix_variants(complete: &str, candidate: &str) -> bool {
    if candidate.is_empty() || candidate.starts_with("###") || candidate.starts_with("<!--") {
        return false;
    }
    let complete = complete.trim_start_matches('❯').trim();
    let candidate = candidate.trim_start_matches('❯').trim();
    let common = complete
        .chars()
        .zip(candidate.chars())
        .take_while(|(left, right)| left == right)
        .count();
    let shorter = complete.chars().count().min(candidate.chars().count());
    common >= 16 && common * 4 >= shorter * 3
}

pub fn historical_committed_capture_replay(
    file: &Path,
    doc_content: &str,
) -> Result<Option<HistoricalCommittedCapture>> {
    if let Some(capture) = projected_committed_capture_response(file)? {
        return historical_committed_capture_replay_candidate(file, doc_content, capture);
    }

    let Some(capture) = agent_doc_capture_io::latest_committed(file)? else {
        return Ok(None);
    };
    historical_committed_capture_replay_candidate(
        file,
        doc_content,
        HistoricalCommittedCapture {
            cycle_id: capture.cycle_id,
            capture_id: capture.capture_id,
            response_sha256: capture.response_sha256,
            response_body: capture.response_body,
            file_hash: capture.file_hash,
            baseline_content: capture.baseline_content,
        },
    )
}

#[derive(Debug, Clone)]
pub struct HistoricalCommittedCapture {
    cycle_id: String,
    capture_id: String,
    response_sha256: String,
    response_body: String,
    file_hash: Option<String>,
    baseline_content: Option<String>,
}

/// A terminal response already committed in `HEAD` must not be removable by a
/// late editor/CRDT projection of the capture baseline. This is deliberately
/// narrower than generic snapshot repair: the committed capture must be
/// materialized in `HEAD`, absent from the current authority, and the current
/// authority must equal that capture's exact pre-write baseline after
/// transient-marker normalization. Those proofs make restoring `HEAD` an exact
/// rollback of the stale projection rather than an overwrite of operator work.
fn restore_committed_head_after_authority_regression(
    file: &Path,
    current_doc: &str,
) -> Result<Option<String>> {
    let capture = if let Some(capture) = agent_doc_capture_io::latest_committed(file)? {
        HistoricalCommittedCapture {
            cycle_id: capture.cycle_id,
            capture_id: capture.capture_id,
            response_sha256: capture.response_sha256,
            response_body: capture.response_body,
            file_hash: capture.file_hash,
            baseline_content: capture.baseline_content,
        }
    } else if let Some(projected) = projected_committed_capture_response(file)? {
        projected
    } else {
        return Ok(None);
    };
    if capture.response_body.trim().is_empty()
        || agent_doc_turn::response_replay::response_materialized_in_content(
            &capture.response_body,
            current_doc,
        )
    {
        return Ok(None);
    }
    let Some(head_doc) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(None);
    };
    if !agent_doc_turn::response_replay::response_materialized_in_content(
        &capture.response_body,
        &head_doc,
    ) {
        return Ok(None);
    }
    let Some(baseline) = capture.baseline_content.as_deref() else {
        return Ok(None);
    };
    if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(current_doc)
        != agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(baseline)
    {
        return Ok(None);
    }

    agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
        file,
        &head_doc,
        current_doc,
        "repair_restore_committed_head_after_authority_regression",
    )?;
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &head_doc,
        agent_doc_ops_log_io::log_op,
    )?;
    agent_doc_document_realtime_io::clear_all_deferred_document_write_intents(
        file,
        "repair_restore_committed_head_after_authority_regression",
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "committed_head_restored_after_authority_regression file={} capture_id={} response_sha256={} stale_projection=exact_capture_baseline",
            file.display(),
            capture.capture_id,
            capture.response_sha256,
        ),
    );
    Ok(Some(head_doc))
}

fn historical_committed_capture_replay_candidate(
    file: &Path,
    doc_content: &str,
    capture: HistoricalCommittedCapture,
) -> Result<Option<HistoricalCommittedCapture>> {
    if agent_doc_turn::response_replay::response_materialized_in_content(
        &capture.response_body,
        doc_content,
    ) {
        return Ok(None);
    }
    let Some(response_heading) =
        agent_doc_turn::response_replay::first_response_heading_line(&capture.response_body)
    else {
        return Ok(None);
    };
    let has_matching_prompt =
        agent_doc_turn::response_replay::has_matching_orphan_prompt_for_committed_capture(
            doc_content,
            response_heading,
        );
    let has_partial_response_proof =
        historical_capture_has_partial_response_proof(file, doc_content, &capture)?;
    if !has_matching_prompt && !has_partial_response_proof {
        return Ok(None);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "repair_replay_committed_capture file={} capture_id={} response_sha256={} selector={}",
            file.display(),
            capture.capture_id,
            capture.response_sha256,
            if has_partial_response_proof {
                "partial_response_proof"
            } else {
                "matching_orphan_prompt"
            }
        ),
    );
    Ok(Some(capture))
}

fn historical_capture_has_partial_response_proof(
    file: &Path,
    current: &str,
    capture: &HistoricalCommittedCapture,
) -> Result<bool> {
    let baseline_matches = |content: &str| {
        capture
            .file_hash
            .as_deref()
            .is_some_and(|expected| expected == agent_doc_capture_io::replay_file_hash(content))
    };
    let baseline = if let Some(content) = capture.baseline_content.as_deref()
        && baseline_matches(content)
    {
        Some(content.to_string())
    } else if let Some(head) = agent_doc_git_io::revision::show_head(file)?
        && baseline_matches(&head)
    {
        Some(head)
    } else {
        None
    };
    Ok(baseline.is_some_and(|baseline| {
        agent_doc_capture_io::reconcile_partial_response_lines(
            &baseline,
            current,
            &capture.response_body,
        )
        .is_some()
    }))
}

pub fn visible_response_patch_from_document(
    file: &Path,
    doc_content: &str,
) -> Result<Option<String>> {
    let Some(snapshot_doc) = agent_doc_snapshot_io::load_document_baseline(file)? else {
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
            projection:
                agent_doc_flow_io::closeout::RetiredCaptureProjection::RefreshFromContent(
                    current_doc,
                ),
            clear_pending_response: true,
            clear_undo_content: true,
            mark_cycle_committed_event: Some("repair_respect_manual_exchange_tail_removal"),
            mark_cycle_abandoned_event: None,
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

pub fn rebase_capture_to_authoritative_current_if_safe(
    file: &Path,
    doc_content: &str,
    capture: &agent_doc_capture_io::CaptureRecord,
    authority: agent_doc_document_realtime::DocAuthority,
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
    let decision = agent_doc_workflow::capture::decide_authoritative_replay_rebase(
        agent_doc_workflow::capture::AuthoritativeReplayRebaseEvidence {
            capture_repairable: agent_doc_workflow::capture::capture_state_is_repairable(
                capture.state,
            ),
            replay_baseline_drifted:
                agent_doc_capture_io::replay_baseline_drifted_with_current_content(
                    file,
                    capture,
                    doc_content,
                )?,
            // `RepairCurrentDocument` can only come from the typed editor/disk
            // authority resolver. Both variants are authoritative for this cut.
            authoritative_current: matches!(
                authority,
                agent_doc_document_realtime::DocAuthority::EditorBuffer
                    | agent_doc_document_realtime::DocAuthority::Disk
            ),
            matching_open_cycle: agent_doc_capture_io::capture_matches_open_cycle(file, capture)?,
            captured_response_body_missing,
            captured_response_heading_answered,
            current_monotonically_extends_baseline:
                agent_doc_capture_io::authoritative_current_monotonically_extends_capture_baseline(
                    file,
                    capture,
                    doc_content,
                )?,
        },
    );
    if decision
        != agent_doc_workflow::capture::AuthoritativeReplayRebaseDecision::RebaseToAuthoritativeCurrent
    {
        return Ok(false);
    }
    agent_doc_capture_io::rebase_replay_baseline_to_authoritative_current(
        file,
        capture,
        doc_content,
    )
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
            replay_baseline_drifted:
                agent_doc_capture_io::replay_baseline_drifted_with_current_content(
                    file,
                    capture,
                    doc_content,
                )?,
            captured_response_body_missing,
            captured_response_heading_answered,
            retained_document_write: agent_doc_document_realtime_io::pending_document_write(file)
                .is_some(),
        },
    );

    match decision {
        agent_doc_workflow::capture::StaleCaptureRetirementDecision::Keep => Ok(false),
        agent_doc_workflow::capture::StaleCaptureRetirementDecision::RetireSupersededCapturedOnlyOrphan => {
            effects.apply_closeout_recovery_mutation(
                file,
                agent_doc_flow_io::closeout::CloseoutRecoveryMutation::RetireStaleCapture {
                    projection:
                        agent_doc_flow_io::closeout::RetiredCaptureProjection::ResolveContentOnDemand,
                    clear_pending_response: true,
                    clear_undo_content: true,
                    mark_cycle_committed_event: None,
                    mark_cycle_abandoned_event: Some("repair_retire_superseded_captured_only_orphan"),
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
                "[repair] retired superseded Captured-only orphan for {} (captured response's heading already answered in the live exchange); preserved the captured body for forensics",
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

    let Some(snapshot_content) = agent_doc_snapshot_io::load_document_baseline(file)? else {
        return Ok(false);
    };
    let snapshot_hash = agent_doc_hash::content_hash(&snapshot_content);
    let snapshot_hash_matches = capture.snapshot_hash.as_deref() == Some(snapshot_hash.as_str());
    let captured_baseline_matches =
        capture.baseline_content.as_deref() == Some(snapshot_content.as_str());
    if !snapshot_hash_matches && !captured_baseline_matches {
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
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(agent_doc_turn::repair::CancelOutcome::NoOpenCycle);
    };
    if !state.is_open() {
        return Ok(agent_doc_turn::repair::CancelOutcome::NoOpenCycle);
    }
    if !matches!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted) {
        return Ok(agent_doc_turn::repair::CancelOutcome::Protected);
    }
    if cycle_has_captured_response_projection(file, &state)? {
        return Ok(agent_doc_turn::repair::CancelOutcome::Protected);
    }
    let snapshot_content = agent_doc_snapshot_io::load_document_baseline(file)?;
    let file_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "cancel_preflight_cycle",
    )
    .ok();
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
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(agent_doc_turn::repair::RepairOutcome::Noop);
    };
    if state.phase != agent_doc_turn::CyclePhase::PreflightStarted {
        return Ok(agent_doc_turn::repair::RepairOutcome::Noop);
    }

    let file_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "stale_preflight_repair",
    )?;
    let snapshot_content = agent_doc_snapshot_io::load_document_baseline(file)?;
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
        let repaired_snapshot = agent_doc_snapshot_io::load_document_baseline(file)?;
        effects.mark_committed_frontmatter(
            file,
            "repair_preflight_committed_historical",
            repaired_snapshot.as_deref(),
            Some(&file_content),
        )?;
        agent_doc_capture_io::mark_committed_with_current_content(file, &file_content)?;
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

    let cycle_capture_exists = cycle_has_captured_response_projection(file, &state)?;
    let age_secs = agent_doc_turn::closeout_recovery::stale_preflight_cycle_age_secs(
        state.started_at,
        state.updated_at,
        now_secs(),
    );
    if !cycle_capture_exists && {
        let steering = agent_doc_session_check_io::realtime_steering_since_turn_baseline(file)?;
        steering.is_present()
            && !agent_doc_turn::closeout_recovery::prompt_change_is_orchestration_handoff_marker(
                steering.preview().unwrap_or_default(),
            )
    } {
        let steering = agent_doc_session_check_io::realtime_steering_since_turn_baseline(file)?;
        let preview = steering.preview().unwrap_or_default();
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

fn cycle_has_captured_response_projection(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
) -> Result<bool> {
    if let Some(projection) = agent_doc_cycle_state_io::load_closeout_projection(file)?
        && projection.matches_cycle(&state.cycle_id)
        && (projection.captured_response.is_some()
            || (projection.capture_id.is_some() && projection.response_sha256.is_some()))
    {
        return Ok(true);
    }

    Ok(false)
}

pub fn repair_committed_historical_snapshot_drift(file: &Path) -> Result<Option<&'static str>> {
    let Some(snapshot_doc) = agent_doc_snapshot_io::load_document_baseline(file)? else {
        return Ok(None);
    };
    let current_doc = match agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "committed_historical_snapshot_drift",
    ) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current_doc == snapshot_doc {
        return Ok(None);
    }

    let Some(head_doc) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(None);
    };
    let committed_capture_materialized =
        committed_capture_response_materialized_in_head(file, &head_doc);
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
        agent_doc_document_realtime::baseline_comparison::detect_bypassed_response_write_between(
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
        _ if committed_capture_materialized => Some("committed_capture"),
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &head_doc,
            agent_doc_ops_log_io::log_op,
        )?;
        // `#external-commit-crdt-staleness`: reconciling the durable baseline to
        // HEAD is sufficient for a detached document, but a warm live canonical
        // replica can still hold the pre-commit text
        // and out-vote HEAD on the next merge (the `#compact-overlay-crdt-staleness`
        // mechanism, generalized to any external commit). Converge the live CRDT
        // canonical to HEAD too. Only safe in this `basis=head` branch: here the
        // whole document is at HEAD (`current_doc` normalizes equal), so no operator
        // content beyond HEAD exists to drop — the follow-up/local-drift branch
        // below deliberately does NOT converge (it would clobber a live operator
        // follow-up). Authority-gated + best-effort: headless returns `Ok(None)`
        // (the durable baseline already owns it) and a convergence error is
        // logged, never failing the snapshot repair that already succeeded.
        converge_crdt_canonical_to_head(file, &head_doc);
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

    if agent_doc_document_realtime::baseline_comparison::detect_bypassed_response_write_between(
        &head_doc,
        &current_doc,
    )
    .is_none()
    {
        let basis = if agent_doc_git::is_safe_user_only_follow_up_after_committed_head(
            &head_doc,
            &current_doc,
        ) {
            "head_follow_up"
        } else {
            "head_local_drift"
        };
        agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &head_doc,
            agent_doc_ops_log_io::log_op,
        )?;
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

/// Converge the live CRDT canonical replica to `head_doc` after a snapshot→HEAD
/// repair, so a warm (or phantom-frozen) replica cannot replay pre-commit text and
/// revert an externally committed mutation (`#external-commit-crdt-staleness`).
///
/// Authority-gated by `adopt_authoritative_text_for_file`: headless returns
/// `Ok(None)` and does nothing (the cold-load baseline-wins path already discards a
/// stale disk `.yrs`). Best-effort: a convergence error is logged, never
/// propagated — the snapshot repair that already landed is the primary guarantee.
fn converge_crdt_canonical_to_head(file: &Path, head_doc: &str) {
    match agent_doc_crdt_relay_io::adopt_authoritative_text_for_file(file, head_doc) {
        Ok(_) => {}
        Err(e) => {
            eprintln!(
                "[repair] warning: could not converge CRDT canonical to HEAD after snapshot repair for {} (cold-load baseline-wins still owns staleness): {e}",
                file.display()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "snapshot_repair_crdt_converge_warning file={} err={}",
                    file.display(),
                    e
                ),
            );
        }
    }
}

fn committed_capture_response_materialized_in_head(file: &Path, head_doc: &str) -> bool {
    match projected_committed_capture_response_body(file) {
        Ok(Some(response_body)) => {
            return !response_body.trim().is_empty()
                && agent_doc_turn::response_replay::response_materialized_in_content(
                    &response_body,
                    head_doc,
                );
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!(
                "[repair] committed-capture projection warning for {}: {}",
                file.display(),
                e
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "snapshot_repair_committed_capture_projection_warning file={} err={}",
                    file.display(),
                    e
                ),
            );
        }
    }

    let capture = match agent_doc_capture_io::latest_committed(file) {
        Ok(Some(capture)) => capture,
        Ok(None) => return false,
        Err(e) => {
            eprintln!(
                "[repair] committed-capture snapshot repair warning for {}: {}",
                file.display(),
                e
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "snapshot_repair_committed_capture_warning file={} err={}",
                    file.display(),
                    e
                ),
            );
            return false;
        }
    };
    !capture.response_body.trim().is_empty()
        && agent_doc_turn::response_replay::response_materialized_in_content(
            &capture.response_body,
            head_doc,
        )
}

fn projected_committed_capture_response_body(file: &Path) -> Result<Option<String>> {
    Ok(projected_committed_capture_response(file)?.map(|capture| capture.response_body))
}

fn projected_committed_capture_response(file: &Path) -> Result<Option<HistoricalCommittedCapture>> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if state.phase != agent_doc_turn::CyclePhase::Committed {
        return Ok(None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(None);
    };
    let Some(projected) =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
    else {
        return Ok(None);
    };
    if state.cycle_id != projected.cycle_id
        || state.response_sha256.as_deref() != Some(projected.response_sha256.as_str())
    {
        return Ok(None);
    }
    Ok(Some(HistoricalCommittedCapture {
        cycle_id: projected.cycle_id,
        capture_id: projected.capture_id,
        response_sha256: projected.response_sha256,
        response_body: projected.response_body,
        file_hash: projected.file_hash,
        baseline_content: projected.baseline_content,
    }))
}

pub fn recover_missing_commit_boundary(
    effects: &impl RepairIoEffects,
    file: &Path,
    event: &str,
) -> Result<Option<&'static str>> {
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    let closeout_projection = agent_doc_cycle_state_io::load_closeout_projection(file)?;
    let has_open_commit_boundary = state.as_ref().is_some_and(|state| {
        matches!(
            state.phase,
            agent_doc_turn::CyclePhase::ResponseCaptured | agent_doc_turn::CyclePhase::WriteApplied
        )
    }) || (state.is_none()
        && closeout_projection
            .as_ref()
            .and_then(|projection| projection.phase)
            .is_some_and(|phase| {
                matches!(
                    phase,
                    agent_doc_turn::CyclePhase::ResponseCaptured
                        | agent_doc_turn::CyclePhase::WriteApplied
                )
            }));
    let has_missing_commit_event = if has_open_commit_boundary {
        false
    } else {
        agent_doc_ops_log_io::latest_unclosed_write_completed_commit_missing(file)?.is_some()
    };
    if !has_open_commit_boundary && !has_missing_commit_event {
        return Ok(None);
    }

    let current_doc = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "commit_boundary_recovery",
    )?;
    let head_doc = agent_doc_git_io::revision::show_head(file)?;
    if has_open_commit_boundary
        && let (Some(state), Some(head)) = (state.as_ref(), head_doc.as_deref())
        && captured_response_materialized_in_head(file, state, head)? == Some(false)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "repair_commit_boundary_not_recovered file={} cycle={} reason=captured_response_missing_from_head",
                file.display(),
                state.cycle_id
            ),
        );
        return Ok(None);
    }
    let reason = match agent_doc_snapshot_io::verify_snapshot_committed(file)? {
        agent_doc_snapshot_io::SnapshotCommitStatus::Committed => head_doc
            .as_deref()
            .filter(|head| {
                agent_doc_document_realtime::baseline_comparison::detect_bypassed_response_write_between(
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

    let repaired_snapshot = agent_doc_snapshot_io::load_document_baseline(file)?;
    effects.mark_committed_frontmatter(
        file,
        event,
        repaired_snapshot.as_deref(),
        Some(&current_doc),
    )?;
    agent_doc_capture_io::mark_committed_with_current_content(file, &current_doc)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{} file={} event={} reason={}",
            OpsLogEvent::RepairCommitBoundaryRecovered,
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

fn captured_response_materialized_in_head(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
    head: &str,
) -> Result<Option<bool>> {
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(None);
    };
    let response_body = if let Some(projected) =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
        && projected.cycle_id == state.cycle_id
        && state.response_sha256.as_deref() == Some(projected.response_sha256.as_str())
    {
        Some(projected.response_body)
    } else {
        None
    };
    Ok(response_body.map(|response| {
        agent_doc_turn::response_replay::response_materialized_in_content(&response, head)
    }))
}

pub fn repair_completed_backlog_items(
    effects: &impl RepairIoEffects,
    file: &Path,
) -> Result<agent_doc_turn::repair::RepairOutcome> {
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "completed_backlog_reap_repair",
    )?;
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

    repaired = effects.atomic_write_if_current(
        file,
        &repaired,
        &content,
        "repair_completed_backlog_reap",
    )?;

    let repaired_snapshot = if let Some(snap_content) =
        agent_doc_snapshot_io::load_document_baseline(file)?
    {
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &new_snapshot,
            agent_doc_ops_log_io::log_op,
        )?;
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
    use std::cell::Cell;

    fn structural_history_fixture(
        novel_line: Option<&str>,
    ) -> (
        String,
        agent_doc_cycle_state_io::ProjectedCapturedResponse,
        Vec<agent_doc_cycle_state_io::CapturedResponseCheckpoint>,
        String,
    ) {
        let padding = "stable operator-authored context ".repeat(24);
        let baseline = format!(
            "---\nformat: exchange\n---\n{padding}\n<!-- agent:exchange -->\nPlease handle the checkpoint.\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do [#task]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n- [ ] [#task] Preserve the document.\n<!-- /agent:backlog -->\n\n<!-- agent:review -->\n<!-- /agent:review -->\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n\n<!-- agent:done -->\n<!-- /agent:done -->\n"
        );
        let previous_response = "### Re: checkpoint - model\n\n- recovered response line alpha\n- recovered response line beta\n";
        let novel = novel_line
            .map(|line| format!("{line}\n"))
            .unwrap_or_default();
        let current = format!(
            "---\nformat: exchange\n---\n{padding}\n<!-- agent:exchange -->\nPlease handle the checkpoint.\n- recovered response line alpha\n- recovered response line beta\n{novel}- [ ] [#task] Preserve the document.\n<!-- /agent:backlog -->\n\n<!-- agent:review -->\n<!-- /agent:review -->\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n\n<!-- agent:done -->\n<!-- /agent:done -->\n\n<!-- agent:review -->\n<!-- /agent:review -->\n"
        );
        let active_response = "### Re: latest - model\n\nReady to repair.\n";
        let active = agent_doc_cycle_state_io::ProjectedCapturedResponse {
            cycle_id: "cycle-1".to_string(),
            capture_id: "capture-1".to_string(),
            response_sha256: "latest-sha".to_string(),
            response_body: active_response.to_string(),
            intent_body: None,
            mutation_plan_json: None,
            file_hash: Some(agent_doc_capture_io::replay_file_hash(&current)),
            snapshot_hash: None,
            baseline_content: Some(current.clone()),
        };
        let history = vec![
            agent_doc_cycle_state_io::CapturedResponseCheckpoint {
                sequence: 20,
                cycle_id: active.cycle_id.clone(),
                capture_id: active.capture_id.clone(),
                response_sha256: active.response_sha256.clone(),
                response_body: active.response_body.clone(),
                file_hash: active.file_hash.clone(),
                snapshot_hash: None,
                baseline_content: active.baseline_content.clone(),
            },
            agent_doc_cycle_state_io::CapturedResponseCheckpoint {
                sequence: 19,
                cycle_id: active.cycle_id.clone(),
                capture_id: active.capture_id.clone(),
                response_sha256: "previous-sha".to_string(),
                response_body: previous_response.to_string(),
                file_hash: Some(agent_doc_capture_io::replay_file_hash(&baseline)),
                snapshot_hash: None,
                baseline_content: Some(baseline),
            },
        ];
        (current, active, history, previous_response.to_string())
    }

    #[test]
    fn structural_history_recovery_reconstructs_valid_prior_response() {
        let (current, active, history, previous_response) = structural_history_fixture(None);
        let recovered = select_structural_history_recovery(&current, &active, &history)
            .expect("corrupted whole-document materialization should be recoverable");
        assert_eq!(recovered.checkpoint_sequence, 19);
        assert!(
            agent_doc_element::element::structural_corruption_reason(&recovered.content).is_none()
        );
        assert!(
            agent_doc_turn::response_replay::response_materialized_in_content(
                &previous_response,
                &recovered.content,
            )
        );
    }

    #[test]
    fn structural_history_recovery_rejects_novel_operator_text() {
        let (current, active, history, _) =
            structural_history_fixture(Some("operator note must survive"));
        assert!(select_structural_history_recovery(&current, &active, &history).is_none());
    }

    fn lossy_editor_projection_fixture() -> (
        String,
        String,
        Vec<agent_doc_cycle_state_io::CapturedResponseCheckpoint>,
        String,
    ) {
        let full_prompt = "- do [#fresh-project-supervisor-log]: agent-doc start should auto-create a configurable path to the supervisor-stderr.log. The default should be PROJECT_ROOT/.agent-doc/logs/supervisor-stderr.log. Auto-create directory structure. The agent-doc start command should not redirect stderr to the file.";
        let truncated_prompt = "- do [#fresh-project-supervisor-log]: agent-doc start should auto-create a configurable path";
        let padding = "stable operator-authored context ".repeat(24);
        let document = |resume: &str, exchange: &str, queue: &str| {
            format!(
                "---\nagent_doc_session: test\nresume: {resume}\nagent_doc_format: template\n---\n{padding}\n<!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n{queue}\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n- [ ] [#fresh-project-supervisor-log] Preserve supervisor logging.\n<!-- /agent:backlog -->\n\n<!-- agent:review -->\n<!-- /agent:review -->\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n\n<!-- agent:done -->\n<!-- /agent:done -->\n"
            )
        };
        let operator_projection = document(
            "old-resume",
            "Earlier exchange content must survive.\n\n",
            full_prompt,
        );
        let captured_baseline = document("new-resume", "", truncated_prompt);
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: Response\n\n",
            "The convergence fix is installed. Please restart IDEA.\n",
            "<!-- /patch:exchange -->\n",
        );
        let current = agent_doc_turn::response_replay::materialize_response_in_current_exchange(
            &captured_baseline,
            response,
        )
        .expect("captured response should materialize");
        let history = vec![agent_doc_cycle_state_io::CapturedResponseCheckpoint {
            sequence: 42,
            cycle_id: "cycle-old".to_string(),
            capture_id: "capture-old".to_string(),
            response_sha256: agent_doc_hash::content_hash(response),
            response_body: response.to_string(),
            file_hash: Some(agent_doc_capture_io::replay_file_hash(&captured_baseline)),
            snapshot_hash: None,
            baseline_content: Some(captured_baseline),
        }];
        (
            current,
            operator_projection,
            history,
            full_prompt.to_string(),
        )
    }

    #[test]
    fn lossy_editor_projection_replays_ops_then_retained_response() {
        let (current, operator_projection, history, full_prompt) =
            lossy_editor_projection_fixture();
        let recovered =
            select_lossy_editor_projection_recovery(&current, &operator_projection, &history)
                .expect("lossy editor projection should be recoverable");
        assert_eq!(recovered.checkpoint_sequence, 42);
        assert!(recovered.content.contains(&full_prompt));
        assert!(
            recovered
                .content
                .contains("Earlier exchange content must survive.")
        );
        assert!(recovered.content.contains("resume: new-resume"));
        assert!(!recovered.content.contains("resume: old-resume"));
        assert_eq!(
            recovered
                .content
                .matches("The convergence fix is installed.")
                .count(),
            1
        );
        assert!(
            agent_doc_element::element::structural_corruption_reason(&recovered.content).is_none()
        );
    }

    #[test]
    fn lossy_editor_projection_rejects_post_capture_operator_text() {
        let (current, operator_projection, history, _) = lossy_editor_projection_fixture();
        let current = current.replace(
            "<!-- agent:queue -->",
            "<!-- agent:queue -->\noperator note must survive",
        );
        assert!(
            select_lossy_editor_projection_recovery(&current, &operator_projection, &history)
                .is_none()
        );
    }

    #[test]
    fn historical_capture_does_not_replay_semantically_materialized_response() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n\n",
            "Line one.\nLine two.\n",
            "<!-- no-pending-capture -->\n",
            "<!-- /patch:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ topic\n",
            "### Re: topic — gpt-5\n\n",
            "Line one.\nLine two.\n",
            "<!-- /agent:exchange -->\n",
        );
        let capture = HistoricalCommittedCapture {
            cycle_id: "cycle-test".to_string(),
            capture_id: "capture-test".to_string(),
            response_sha256: agent_doc_hash::content_hash(response),
            response_body: response.to_string(),
            file_hash: None,
            baseline_content: None,
        };

        assert!(
            historical_committed_capture_replay_candidate(
                Path::new("unused.md"),
                current,
                capture,
            )
            .unwrap()
            .is_none()
        );
    }

    #[derive(Default)]
    struct TestRepairIoEffects {
        committed_calls: Cell<usize>,
        abandoned_calls: Cell<usize>,
    }

    impl RepairIoEffects for TestRepairIoEffects {
        fn atomic_write_if_current(
            &self,
            file: &Path,
            content: &str,
            expected_current: &str,
            _source: &str,
        ) -> Result<String> {
            anyhow::ensure!(
                agent_doc_document_realtime_io::resolve_disk_current_document_content(
                    file,
                    "repair_test_atomic_write_if_current",
                )? == expected_current,
                "test repair write baseline changed"
            );
            std::fs::write(file, content)?;
            Ok(content.to_string())
        }

        fn mark_committed_frontmatter(
            &self,
            file: &Path,
            event: &str,
            snapshot_content: Option<&str>,
            file_content: Option<&str>,
        ) -> Result<agent_doc_cycle_state_io::CycleState> {
            self.committed_calls.set(self.committed_calls.get() + 1);
            agent_doc_cycle_state_io::mark_committed(file, event, snapshot_content, file_content)
        }

        fn mark_abandoned_frontmatter(
            &self,
            file: &Path,
            event: &str,
            snapshot_content: Option<&str>,
            file_content: Option<&str>,
        ) -> Result<agent_doc_cycle_state_io::CycleState> {
            self.abandoned_calls.set(self.abandoned_calls.get() + 1);
            agent_doc_cycle_state_io::mark_abandoned(file, event, snapshot_content, file_content)
        }

        fn apply_closeout_recovery_mutation(
            &self,
            _file: &Path,
            _mutation: agent_doc_flow_io::closeout::CloseoutRecoveryMutation<'_>,
        ) -> Result<()> {
            Ok(())
        }
    }

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
    fn retained_repair_replay_never_infers_force_disk() {
        let file = Path::new("/tmp/agent-doc-retained-repair.md");

        assert!(
            !repair_replay_force_disk(file),
            "an inferred editor absence must still replay through CRDT/CAS"
        );
        assert!(!repair_replay_force_disk_with_override(file, Some(false)));
        assert!(
            repair_replay_force_disk_with_override(file, Some(true)),
            "only the explicit operator escape hatch may force disk"
        );
    }

    #[test]
    fn repair_converges_crdt_and_lands_snapshot_on_external_commit() {
        // `#external-commit-crdt-staleness`: an external commit lands a new
        // historical `### Re:` turn in HEAD that the snapshot does not reflect. The
        // repair must reconcile the snapshot to HEAD (basis=head) AND run the
        // CRDT-canonical convergence wiring without error. Headless (no attached
        // editor) the convergence is an authority-gated no-op — the warm-path
        // convergence itself is covered by crdt-relay-io's
        // `adopt_authoritative_text_converges_a_stale_canonical_for_the_commit_read`.
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        std::fs::write(root.join("README.md"), "# test\n").unwrap();
        git(&["add", "README.md"]);
        git(&["commit", "-m", "initial", "--no-verify"]);

        let doc = root.join("session.md");
        let old = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- /agent:exchange -->\n";
        std::fs::write(&doc, old).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            old,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        git(&["add", "session.md"]);
        git(&["commit", "-m", "add doc", "--no-verify"]);

        // External commit lands a new historical turn in HEAD (and on disk).
        let externally_committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: external\n\
            committed elsewhere\n\
            <!-- /agent:exchange -->\n";
        std::fs::write(&doc, externally_committed).unwrap();
        git(&["add", "session.md"]);
        git(&["commit", "-m", "external commit", "--no-verify"]);
        // Snapshot still lags at the pre-commit content.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            old,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let reason = repair_committed_historical_snapshot_drift(&doc)
            .expect("repair must not error while converging the CRDT canonical");
        assert_eq!(
            reason,
            Some("exchange"),
            "external commit is a committed historical exchange mutation"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("### Re: external\n"),
            "snapshot must reconcile to the externally committed HEAD:\n{snap}"
        );
    }

    #[test]
    fn stale_preflight_repair_uses_terminal_ledger_projection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("task.md");
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(content),
            Some(content),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::Committed
        );

        let effects = TestRepairIoEffects::default();
        let outcome = repair_stale_preflight_started_cycle(&effects, &doc).unwrap();
        assert_eq!(outcome, agent_doc_turn::repair::RepairOutcome::Noop);
        assert_eq!(
            effects.committed_calls.get(),
            0,
            "a committed ledger phase must not trigger stale-preflight repair"
        );
        assert_eq!(effects.abandoned_calls.get(), 0);
    }

    #[test]
    fn captured_response_projection_protects_cycle_when_capture_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("task.md");
        let content = "---\nagent_doc_session: test\n---\n\n## User\n\nDo the thing\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let capture =
            agent_doc_capture_io::capture_response(&doc, "### Re: do - opus-4-8\n\nDone.\n")
                .unwrap();
        assert!(!capture.capture_id.is_empty());
        let stale_state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            stale_state.phase,
            agent_doc_turn::CyclePhase::ResponseCaptured
        );
        assert!(
            cycle_has_captured_response_projection(&doc, &stale_state).unwrap(),
            "captured-response projection must prove capture when the cold capture file is missing"
        );

        let effects = TestRepairIoEffects::default();
        let outcome = cancel_preflight_cycle(&effects, &doc).unwrap();
        assert_eq!(outcome, agent_doc_turn::repair::CancelOutcome::Protected);
        assert_eq!(effects.abandoned_calls.get(), 0);
    }

    #[test]
    fn committed_capture_materialization_prefers_projection_when_capture_sidecar_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("task.md");
        let content = "---\nagent_doc_session: test\n---\n\n## User\n\nDo the thing\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let response = "### Re: do - opus-4-8\n\nDone.\n";
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        assert!(!capture.capture_id.is_empty());

        let head_doc = format!("{content}\n{response}");
        assert!(
            committed_capture_response_materialized_in_head(&doc, &head_doc),
            "committed captured-response projection must prove materialization without capture JSON"
        );
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

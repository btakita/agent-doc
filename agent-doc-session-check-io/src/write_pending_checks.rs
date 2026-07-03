//! Write-time pending capture/done closeout guards.

use anyhow::{Context, Result};
use std::path::Path;

use crate::{
    promised_backlog_item_inventory_shortfall, promised_plan_reference_shortfall,
    unresolved_backlog_capture_targets, unresolved_promised_backlog_item_ids,
};

#[derive(Clone, Debug, Default)]
pub struct PendingWriteFlags {
    pub has_pending_add: bool,
    pub has_pending_done: bool,
    pub pending_done_ids: Vec<String>,
    pub pending_kept_open_ids: Vec<String>,
    pub strict_closeout: bool,
    pub force_disk: bool,
    pub rerun_command_base: Option<String>,
}

fn log_closeout_guard(
    file: &Path,
    stage: agent_doc_flow::types::FlowStage,
    outcome: agent_doc_flow::types::FlowOutcome,
    reason: agent_doc_turn::closeout_guard::CloseoutGuardReason,
) {
    agent_doc_flow_io::log_flow_event(
        file,
        agent_doc_turn::closeout_guard::closeout_guard_event(stage, outcome, reason),
        agent_doc_ops_log_io::log_op,
    );
}

pub fn precommit_pending_capture_check(file: &Path) -> Result<()> {
    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(());
    };
    if state.had_pending_mutations && state.required_backlog_targets.is_empty() {
        return Ok(());
    }

    let Some(capture) = agent_doc_capture_io::load_active(file)? else {
        return Ok(());
    };
    if capture
        .response_body
        .contains("<!-- no-pending-capture -->")
    {
        return Ok(());
    }

    let response_text =
        agent_doc_turn::closeout_signal::response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(());
    }
    let missing_targets = unresolved_backlog_capture_targets(file, &state);
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreCommitGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCaptureTargetMissing,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: active prompt required backlog capture in {} \
             but those tracked-work surfaces did not change this cycle\n\
             [finalize] hint: update those backlog targets before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            missing_targets.join(", ")
        );
    }
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            promised_backlog_item_inventory_shortfall(&state, &response_text)
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreCommitGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCaptureInventoryShortfall,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: active #agent-doc-bug contract described at least {} distinct issue(s), \
             but the response only enumerated {} explicit backlog item(s) for target(s) {}\n\
             [finalize] hint: enumerate each transferred bug as a tracked backlog item in the response \
             (for example `- [ ] [#id] ...`) before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            expected_count,
            promised_count,
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            promised_plan_reference_shortfall(file, &state, &response_text)
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreCommitGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCapturePlanShortfall,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: active #agent-doc-bug contract required at least {} explicit plan reference(s), \
             but the response only cited {} existing plan path(s)\n\
             [finalize] hint: create each plan file and cite it in the response \
             (for example `Plan: tasks/agent-doc/plan-foo.md`) before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            expected_count,
            promised_count
        );
    }
    let missing_ids = unresolved_promised_backlog_item_ids(file, &state, &response_text);
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreCommitGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCapturePromisedIdsMissing,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: response promised new tracked item(s) {} \
             for explicit backlog target(s) {}, but those ids are still missing after this cycle\n\
             [finalize] hint: transfer every listed item into the explicit target backlog, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            missing_ids.join(", "),
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if state.requires_backlog_capture
        && state.required_backlog_targets.is_empty()
        && !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreCommitGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCaptureRequired,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: active prompt requested backlog capture \
             but no backlog mutations were recorded this cycle\n\
             [finalize] hint: re-run finalize with --pending-add flags, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures"
        );
    }

    if state.had_pending_mutations {
        return Ok(());
    }

    let mode = crate::resolve_pending_capture_guard_mode(file)?;
    if mode != agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let signal = agent_doc_turn::heuristics::detect_uncaptured_recommendations(&response_text);
    let skip = match signal.estimated_count {
        0 => true,
        1 => signal.confidence < 0.7,
        _ => signal.confidence < 0.5,
    };
    if skip {
        return Ok(());
    }

    log_closeout_guard(
        file,
        agent_doc_flow::types::FlowStage::PreCommitGuard,
        agent_doc_flow::types::FlowOutcome::Blocked,
        agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCaptureRecommendations,
    );
    anyhow::bail!(
        "[finalize] pre-commit gate: response contains ~{} recommendation-like items \
         but no --pending-add flags were used this cycle\n\
         [finalize] hint: re-run finalize with --pending-add flags, \
         add <!-- no-pending-capture --> to suppress, \
         or set pending_capture_guard = \"warn\" to downgrade",
        signal.estimated_count
    );
}

pub fn prewrite_pending_capture_check(
    file: &Path,
    response_body: &str,
    flags: &PendingWriteFlags,
) -> Result<()> {
    if !flags.strict_closeout {
        return Ok(());
    }

    let state = agent_doc_cycle_state_io::load(file)?;
    let has_explicit_targets = state
        .as_ref()
        .is_some_and(|state| !state.required_backlog_targets.is_empty());
    if !has_explicit_targets
        && (state
            .as_ref()
            .is_some_and(|state| state.had_pending_mutations)
            || flags.has_pending_add
            || flags.has_pending_done)
    {
        return Ok(());
    }
    if response_body.contains("<!-- no-pending-capture -->") {
        return Ok(());
    }

    let response_text = agent_doc_turn::closeout_signal::response_text_for_guards(response_body);
    if response_text.trim().is_empty() {
        return Ok(());
    }
    let missing_targets = state
        .as_ref()
        .map(|state| unresolved_backlog_capture_targets(file, state))
        .unwrap_or_default();
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreWriteGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCaptureTargetMissing,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: active prompt required backlog capture in {} \
             but those tracked-work surfaces did not change this cycle\n\
             [finalize] hint: update those backlog targets before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            missing_targets.join(", ")
        );
    }
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) = state
            .as_ref()
            .and_then(|state| promised_backlog_item_inventory_shortfall(state, &response_text))
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreWriteGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCaptureInventoryShortfall,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: active #agent-doc-bug contract described at least {} distinct issue(s), \
             but the response only enumerated {} explicit backlog item(s) for target(s) {}\n\
             [finalize] hint: enumerate each transferred bug as a tracked backlog item in the response \
             (for example `- [ ] [#id] ...`) before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            expected_count,
            promised_count,
            state
                .as_ref()
                .map(|state| {
                    state
                        .required_backlog_targets
                        .iter()
                        .map(|target| target.path.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        );
    }
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) = state
            .as_ref()
            .and_then(|state| promised_plan_reference_shortfall(file, state, &response_text))
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreWriteGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCapturePlanShortfall,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: active #agent-doc-bug contract required at least {} explicit plan reference(s), \
             but the response only cited {} existing plan path(s)\n\
             [finalize] hint: create each plan file and cite it in the response \
             (for example `Plan: tasks/agent-doc/plan-foo.md`) before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            expected_count,
            promised_count
        );
    }
    let missing_ids = state
        .as_ref()
        .map(|state| unresolved_promised_backlog_item_ids(file, state, &response_text))
        .unwrap_or_default();
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreWriteGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCapturePromisedIdsMissing,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: response promised new tracked item(s) {} \
             for explicit backlog target(s) {}, but those ids are still missing after this cycle\n\
             [finalize] hint: transfer every listed item into the explicit target backlog, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            missing_ids.join(", "),
            state
                .as_ref()
                .map(|state| {
                    state
                        .required_backlog_targets
                        .iter()
                        .map(|target| target.path.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        );
    }
    if state.as_ref().is_some_and(|state| {
        state.requires_backlog_capture && state.required_backlog_targets.is_empty()
    }) && !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
    {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreWriteGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCaptureRequired,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: active prompt requested backlog capture \
             but no backlog mutations were recorded this cycle\n\
             [finalize] hint: re-run finalize with --pending-add flags, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures"
        );
    }

    if state
        .as_ref()
        .is_some_and(|state| state.had_pending_mutations)
        || flags.has_pending_add
        || flags.has_pending_done
    {
        return Ok(());
    }

    let mode = crate::resolve_pending_capture_guard_mode(file)?;
    if mode != agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let signal = agent_doc_turn::heuristics::detect_uncaptured_recommendations(&response_text);
    let skip = match signal.estimated_count {
        0 => true,
        1 => signal.confidence < 0.7,
        _ => signal.confidence < 0.5,
    };
    if skip {
        return Ok(());
    }

    log_closeout_guard(
        file,
        agent_doc_flow::types::FlowStage::PreWriteGuard,
        agent_doc_flow::types::FlowOutcome::Blocked,
        agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingCaptureRecommendations,
    );
    anyhow::bail!(
        "[finalize] pre-write gate: response contains ~{} recommendation-like items \
         but no --pending-add flags were used this cycle\n\
         [finalize] hint: re-run finalize with --pending-add flags, \
         add <!-- no-pending-capture --> to suppress, \
         or set pending_capture_guard = \"warn\" to downgrade",
        signal.estimated_count
    );
}

#[derive(Clone, Copy, Default)]
pub struct PendingDoneCheckOptions {
    pub force_disk: bool,
    pub backlog_effects: Option<&'static dyn agent_doc_element_backlog_io::BacklogCommandEffects>,
}

fn malformed_tracked_item_refs_completed_by_response(
    file: &Path,
    response_text: &str,
) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    Ok(
        agent_doc_element_backlog::backlog::malformed_tracked_item_refs(&content)
            .into_iter()
            .filter(|item| {
                agent_doc_turn::closeout_signal::response_clearly_completes_pending_id(
                    response_text,
                    &item.item.id,
                )
            })
            .map(|item| item.reference())
            .collect(),
    )
}

#[cfg(test)]
pub fn precommit_pending_done_check(file: &Path) -> Result<()> {
    precommit_pending_done_check_with_options(file, PendingDoneCheckOptions::default())
}

pub fn precommit_pending_done_check_with_options(
    file: &Path,
    options: PendingDoneCheckOptions,
) -> Result<()> {
    let mode = crate::resolve_pending_done_guard_mode(file)?;
    if mode != agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let Some(state) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(());
    };

    let Some(capture) = agent_doc_capture_io::load_active(file)? else {
        return Ok(());
    };
    if agent_doc_turn::closeout_signal::pending_done_suppressed(&capture.response_body) {
        return Ok(());
    }

    let response_text =
        agent_doc_turn::closeout_signal::response_text_for_guards(&capture.response_body);
    let malformed = malformed_tracked_item_refs_completed_by_response(file, &response_text)?;
    if !malformed.is_empty() {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreCommitGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingDoneMalformedTrackedItem,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: {}",
            agent_doc_element_backlog::backlog::malformed_tracked_item_interruption_message(
                &malformed
            )
        );
    }
    let content = std::fs::read_to_string(file)?;
    let open_tracked_work_ids =
        agent_doc_document::tracked_work_projection::open_tracked_work_ids(&content);
    let missing = agent_doc_turn::closeout_signal::tracked_work_completion_missing_done_ids(
        &response_text,
        &state.pending_done_ids,
        &state.pending_kept_open_ids,
        &open_tracked_work_ids,
    );
    if missing.is_empty() {
        return Ok(());
    }

    if crate::resolve_auto_done(file)? {
        let Some(backlog_effects) = options.backlog_effects else {
            anyhow::bail!(
                "auto_done is enabled for {} but backlog command effects are not installed",
                file.display()
            );
        };
        agent_doc_element_backlog_io::with_backlog_command_effects(backlog_effects, || {
            agent_doc_element_backlog_io::backlog_cmd::with_force_disk_pending_writes(
                options.force_disk,
                || {
                    for id in &missing {
                        auto_apply_pending_done_id(file, id)?;
                    }
                    Ok(())
                },
            )
        })?;
        agent_doc_cycle_state_io::record_pending_done_ids(file, &missing)?;
        agent_doc_cycle_state_io::mark_pending_mutations(file)?;
        eprintln!(
            "[finalize] auto_done: recorded {}",
            missing
                .iter()
                .map(|id| format!("--done {}", id))
                .collect::<Vec<_>>()
                .join(" ")
        );
        return Ok(());
    }

    let ids = missing
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let hint = missing
        .iter()
        .map(|id| format!("--done {}", id))
        .collect::<Vec<_>>()
        .join(" ");

    log_closeout_guard(
        file,
        agent_doc_flow::types::FlowStage::PreCommitGuard,
        agent_doc_flow::types::FlowOutcome::Blocked,
        agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingDoneMissing,
    );
    anyhow::bail!(
        "[finalize] pre-commit gate: response appears to complete existing pending {} \
         but no matching `--done` was recorded this cycle\n\
         [finalize] hint: re-run finalize with {}, \
         add <!-- no-pending-done-guard --> to suppress, \
         or set pending_done_guard = \"warn\" to downgrade",
        ids,
        hint
    );
}

pub fn prewrite_pending_done_check(
    file: &Path,
    response_body: &str,
    flags: &PendingWriteFlags,
) -> Result<()> {
    if !flags.strict_closeout {
        return Ok(());
    }

    let mode = crate::resolve_pending_done_guard_mode(file)?;
    if mode != agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let state = agent_doc_cycle_state_io::load(file)?;
    let mut recorded_done_ids = state
        .as_ref()
        .map(|state| state.pending_done_ids.clone())
        .unwrap_or_default();
    recorded_done_ids.extend(flags.pending_done_ids.clone());
    let mut kept_open_ids = state
        .as_ref()
        .map(|state| state.pending_kept_open_ids.clone())
        .unwrap_or_default();
    kept_open_ids.extend(flags.pending_kept_open_ids.clone());
    if agent_doc_turn::closeout_signal::pending_done_suppressed(response_body) {
        return Ok(());
    }

    let response_text = agent_doc_turn::closeout_signal::response_text_for_guards(response_body);
    let malformed = malformed_tracked_item_refs_completed_by_response(file, &response_text)?;
    if !malformed.is_empty() {
        log_closeout_guard(
            file,
            agent_doc_flow::types::FlowStage::PreWriteGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingDoneMalformedTrackedItem,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: {}",
            agent_doc_element_backlog::backlog::malformed_tracked_item_interruption_message(
                &malformed
            )
        );
    }
    let content = std::fs::read_to_string(file)?;
    let open_tracked_work_ids =
        agent_doc_document::tracked_work_projection::open_tracked_work_ids(&content);
    let missing = agent_doc_turn::closeout_signal::tracked_work_completion_missing_done_ids(
        &response_text,
        &recorded_done_ids,
        &kept_open_ids,
        &open_tracked_work_ids,
    );
    if missing.is_empty() {
        return Ok(());
    }

    let ids = missing
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let hint = missing
        .iter()
        .map(|id| format!("--done {}", id))
        .collect::<Vec<_>>()
        .join(" ");
    let recovery = flags
        .rerun_command_base
        .as_ref()
        .map(|base| {
            format!(
                "\n[finalize] recovery: re-run the same response with {} {}",
                base, hint
            )
        })
        .unwrap_or_default();

    log_closeout_guard(
        file,
        agent_doc_flow::types::FlowStage::PreWriteGuard,
        agent_doc_flow::types::FlowOutcome::Blocked,
        agent_doc_turn::closeout_guard::CloseoutGuardReason::PendingDoneMissing,
    );
    anyhow::bail!(
        "[finalize] pre-write gate: response appears to complete existing pending {} \
         but no matching `--done` was recorded this cycle\n\
         [finalize] hint: re-run finalize with {}, \
         add <!-- no-pending-done-guard --> to suppress, \
         or set pending_done_guard = \"warn\" to downgrade{}",
        ids,
        hint,
        recovery
    );
}

pub fn auto_apply_pending_done_if_enabled(
    file: &Path,
    response_body: &str,
    flags: &PendingWriteFlags,
    current_content: &mut String,
    backlog_effects: &'static dyn agent_doc_element_backlog_io::BacklogCommandEffects,
) -> Result<()> {
    if !flags.strict_closeout || !crate::resolve_auto_done(file)? {
        return Ok(());
    }
    if agent_doc_turn::closeout_signal::pending_done_suppressed(response_body) {
        return Ok(());
    }

    let state = agent_doc_cycle_state_io::load(file)?;
    let mut recorded_done_ids = state
        .as_ref()
        .map(|state| state.pending_done_ids.clone())
        .unwrap_or_default();
    recorded_done_ids.extend(flags.pending_done_ids.clone());
    let mut kept_open_ids = state
        .as_ref()
        .map(|state| state.pending_kept_open_ids.clone())
        .unwrap_or_default();
    kept_open_ids.extend(flags.pending_kept_open_ids.clone());

    let content = std::fs::read_to_string(file)?;
    let open_tracked_work_ids =
        agent_doc_document::tracked_work_projection::open_tracked_work_ids(&content);
    let missing = agent_doc_turn::closeout_signal::tracked_work_completion_decision(
        agent_doc_turn::closeout_signal::TrackedWorkCompletionEvidence {
            response_body,
            recorded_done_ids: &recorded_done_ids,
            kept_open_ids: &kept_open_ids,
            open_tracked_work_ids: &open_tracked_work_ids,
        },
    );
    let agent_doc_turn::closeout_signal::TrackedWorkCompletionDecision::MissingDone {
        missing_ids: missing,
    } = missing
    else {
        return Ok(());
    };

    agent_doc_element_backlog_io::with_backlog_command_effects(backlog_effects, || {
        agent_doc_element_backlog_io::backlog_cmd::with_force_disk_pending_writes(
            flags.force_disk,
            || {
                for id in &missing {
                    auto_apply_pending_done_id(file, id)?;
                }
                Ok(())
            },
        )
    })?;
    agent_doc_cycle_state_io::record_pending_done_ids(file, &missing)?;
    agent_doc_cycle_state_io::mark_pending_mutations(file)?;
    *current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {} after auto_done", file.display()))?;
    eprintln!(
        "[finalize] auto_done: recorded {}",
        missing
            .iter()
            .map(|id| format!("--done {}", id))
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok(())
}

pub fn auto_apply_pending_done_id(file: &Path, id: &str) -> Result<()> {
    if let Some(component) =
        agent_doc_element_backlog_io::backlog_cmd::open_item_component_name(file, id)?
        && agent_doc_element::element::is_backlog_component(&component)
    {
        agent_doc_element_backlog_io::backlog_cmd::gate(file, id)?;
    }
    enforce_review_done_guard(file, id)?;
    agent_doc_element_backlog_io::backlog_cmd::done(file, id)
}

pub fn enforce_review_done_guard(file: &Path, id: &str) -> Result<()> {
    let mode = crate::resolve_review_done_guard_mode(file)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(());
    }
    let Some(component_name) =
        agent_doc_element_backlog_io::backlog_cmd::open_item_component_name(file, id)?
    else {
        return Ok(());
    };
    if agent_doc_element::element::is_review_component(&component_name) {
        return Ok(());
    }

    let normalized = agent_doc_element_backlog::backlog::normalize_pending_id(id);
    let message = format!(
        "review_done_guard: --done #{} resolved from agent:{} instead of agent:review; run --backlog-gate {} first or set review_done_guard = \"off\"",
        normalized, component_name, normalized
    );
    match mode {
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Warn => {
            eprintln!("[write] warning: {}", message);
            Ok(())
        }
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Strict => {
            log_closeout_guard(
                file,
                agent_doc_flow::types::FlowStage::PreWriteGuard,
                agent_doc_flow::types::FlowOutcome::Blocked,
                agent_doc_turn::closeout_guard::CloseoutGuardReason::ReviewDoneSourceNotReviewed,
            );
            anyhow::bail!("{}", message)
        }
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off => Ok(()),
    }
}

pub fn run_closeout_pending_maintenance(
    file: &Path,
    commit_required: bool,
    force_disk: bool,
    run_pending_maintenance: impl FnOnce(&Path, bool) -> Result<()>,
) -> Result<()> {
    if !commit_required {
        return Ok(());
    }
    if !closeout_pending_maintenance_required(file)? {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "closeout_pending_maintenance_skipped file={} basis=no_tracked_work_closeout",
                file.display()
            ),
        );
        return Ok(());
    }
    run_pending_maintenance(file, force_disk)
}

fn closeout_pending_maintenance_required(file: &Path) -> Result<bool> {
    if let Some(state) = agent_doc_cycle_state_io::load(file)?
        && (state.had_pending_mutations
            || state.pending_added_this_cycle
            || !state.pending_done_ids.is_empty()
            || !state.reaped_pending_ids.is_empty()
            || !state.pending_gated_ids.is_empty()
            || !state.pending_added_ids.is_empty())
    {
        return Ok(true);
    }

    let Ok(content) = std::fs::read_to_string(file) else {
        return Ok(false);
    };
    let Ok(components) = agent_doc_element::element::parse(&content) else {
        return Ok(false);
    };

    Ok(components
        .iter()
        .filter(|component| agent_doc_element::element::is_tracked_work_component(&component.name))
        .any(|component| {
            let (_, items, _) =
                agent_doc_element_backlog::backlog::parse_items(component.content(&content));
            items.iter().any(|item| item.is_done())
        }))
}

#[cfg(test)]
mod precommit_pending_capture_tests {
    use std::fs;
    use std::path::Path;

    struct TestBacklogCommandEffects;

    static TEST_BACKLOG_COMMAND_EFFECTS: TestBacklogCommandEffects = TestBacklogCommandEffects;

    impl agent_doc_element_backlog_io::BacklogCommandEffects for TestBacklogCommandEffects {
        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current_content: &str,
            target_content: &str,
            _reason: &str,
        ) -> anyhow::Result<()> {
            fs::write(file, target_content)?;
            Ok(())
        }

        fn record_document_write_provenance(&self, _file: &Path, _content: &str) {}
    }

    fn setup_precommit(
        root: &std::path::Path,
        frontmatter: &str,
        response: &str,
        had_pending_mutations: bool,
    ) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let content = format!("{frontmatter}## Exchange\n\nHello\n");
        fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::save(&doc, &content, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        if had_pending_mutations {
            agent_doc_cycle_state_io::mark_pending_mutations(&doc).unwrap();
        }
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(&content),
            Some(&content),
        )
        .unwrap();
        doc
    }

    fn setup_precommit_with_pending(
        root: &std::path::Path,
        frontmatter: &str,
        response: &str,
        pending_body: &str,
        pending_done_ids: &[&str],
    ) -> std::path::PathBuf {
        setup_precommit_with_tracked_work(
            root,
            frontmatter,
            response,
            pending_body,
            None,
            pending_done_ids,
        )
    }

    fn setup_precommit_with_tracked_work(
        root: &std::path::Path,
        frontmatter: &str,
        response: &str,
        pending_body: &str,
        icebox_body: Option<&str>,
        pending_done_ids: &[&str],
    ) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let mut content = format!(
            "{frontmatter}<!-- agent:exchange -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n{pending_body}<!-- /agent:pending -->\n"
        );
        if let Some(icebox_body) = icebox_body {
            content.push_str("\n<!-- agent:icebox -->\n");
            content.push_str(icebox_body);
            if !icebox_body.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("<!-- /agent:icebox -->\n");
        }
        fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::save(&doc, &content, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        if !pending_done_ids.is_empty() {
            agent_doc_cycle_state_io::record_pending_done_ids(
                &doc,
                &pending_done_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(&content),
            Some(&content),
        )
        .unwrap();
        doc
    }

    fn write_backlog_doc(path: &Path, backlog_body: &str) {
        let content = format!(
            "---\nagent_doc_session: target\n---\n\n<!-- agent:backlog -->\n{backlog_body}<!-- /agent:backlog -->\n"
        );
        fs::write(path, content).unwrap();
    }

    fn backlog_component_hash(path: &Path) -> String {
        let content = fs::read_to_string(path).unwrap();
        let components = agent_doc_element::element::parse(&content).unwrap();
        let component = components
            .iter()
            .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
            .unwrap();
        agent_doc_hash::content_hash(component.content(&content))
    }

    #[test]
    fn precommit_blocks_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("[finalize] pre-commit gate"));
        assert!(err.to_string().contains("--pending-add"));
    }

    #[test]
    fn precommit_passes_with_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            true,
        );

        super::precommit_pending_capture_check(&doc)
            .expect("should pass when pending mutations were recorded");
    }

    #[test]
    fn prewrite_pending_capture_accepts_pending_done_resolution() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: #done1 — gpt-5\n\nImplemented and verified.\n",
            false,
        );
        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();

        super::prewrite_pending_capture_check(
            &doc,
            "### Re: #done1 — gpt-5\n\nImplemented and verified.\n",
            &super::PendingWriteFlags {
                has_pending_done: true,
                pending_done_ids: vec!["done1".to_string()],
                strict_closeout: true,
                ..Default::default()
            },
        )
        .expect("pending-done should satisfy do-id backlog capture");
    }

    #[test]
    fn precommit_pending_capture_accepts_recorded_pending_done_mutation() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: #done1 — gpt-5\n\nImplemented and verified.\n",
            false,
        );
        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();
        agent_doc_cycle_state_io::record_pending_done_ids(&doc, &["done1".to_string()]).unwrap();
        agent_doc_cycle_state_io::mark_pending_mutations(&doc).unwrap();

        super::precommit_pending_capture_check(&doc)
            .expect("recorded pending-done mutation should satisfy capture guard");
    }

    #[test]
    fn precommit_inactive_in_warn_mode() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        super::precommit_pending_capture_check(&doc)
            .expect("should pass in warn mode — only post-commit session-check fires");
    }

    #[test]
    fn precommit_inactive_in_default_mode() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        super::precommit_pending_capture_check(&doc).expect("should pass in default (warn) mode");
    }

    #[test]
    fn precommit_respects_suppression_marker() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n<!-- no-pending-capture -->\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        super::precommit_pending_capture_check(&doc)
            .expect("should pass when suppression marker present");
    }

    #[test]
    fn precommit_blocks_single_unresolved_bug_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: tmux pane closure — opus-4-6\n\nBecause that session was still hitting the older tmux route/sync cleanup bug that #4qgx was meant to close.\n",
            false,
        );

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("[finalize] pre-commit gate"));
        assert!(err.to_string().contains("--pending-add"));
    }

    #[test]
    fn precommit_blocks_backlog_required_review_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: code review — opus-4-6\n\n1. High: Queue closeout can drift.\n2. Medium: Snapshot repair is too permissive.\n",
            false,
        );
        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("requested backlog capture"));
        assert!(err.to_string().contains("--pending-add"));
    }

    #[test]
    fn precommit_allows_backlog_required_review_with_explicit_no_followups() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: code review — opus-4-6\n\nNo new backlog item came out of this change.\n",
            false,
        );
        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();

        super::precommit_pending_capture_check(&doc)
            .expect("explicit no-follow-up proof should satisfy backlog-required closeout");
    }

    #[test]
    fn precommit_blocks_when_explicit_backlog_target_is_unchanged() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nTransferred issue notes.\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");

        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();
        agent_doc_cycle_state_io::record_backlog_target_requirements(
            &doc,
            &[agent_doc_cycle_state_io::BacklogTargetRequirement {
                path: std::fs::canonicalize(&target)
                    .unwrap()
                    .display()
                    .to_string(),
                component: Some("backlog".to_string()),
                baseline_hash: Some(backlog_component_hash(&target)),
                baseline_item_ids: vec!["old1".to_string()],
            }],
        )
        .unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("required backlog capture in"));
        assert!(err.to_string().contains(target.to_string_lossy().as_ref()));
    }

    #[test]
    fn precommit_still_checks_explicit_backlog_target_after_current_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned item:\n- [ ] [#new1] New transferred item\n",
            true,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");

        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();
        agent_doc_cycle_state_io::record_backlog_target_requirements(
            &doc,
            &[agent_doc_cycle_state_io::BacklogTargetRequirement {
                path: std::fs::canonicalize(&target)
                    .unwrap()
                    .display()
                    .to_string(),
                component: Some("backlog".to_string()),
                baseline_hash: Some(backlog_component_hash(&target)),
                baseline_item_ids: vec!["old1".to_string()],
            }],
        )
        .unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("required backlog capture in"));
        assert!(err.to_string().contains(target.to_string_lossy().as_ref()));
    }

    #[test]
    fn prewrite_still_checks_explicit_backlog_target_after_pending_add_flag() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — gpt-5\n\nPlanned item:\n- [ ] [#new1] New transferred item\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");

        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();
        agent_doc_cycle_state_io::record_backlog_target_requirements(
            &doc,
            &[agent_doc_cycle_state_io::BacklogTargetRequirement {
                path: std::fs::canonicalize(&target)
                    .unwrap()
                    .display()
                    .to_string(),
                component: Some("backlog".to_string()),
                baseline_hash: Some(backlog_component_hash(&target)),
                baseline_item_ids: vec!["old1".to_string()],
            }],
        )
        .unwrap();

        let err = super::prewrite_pending_capture_check(
            &doc,
            "### Re: #agent-doc-bug — gpt-5\n\nPlanned item:\n- [ ] [#new1] New transferred item\n",
            &super::PendingWriteFlags {
                has_pending_add: true,
                strict_closeout: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("required backlog capture in"));
        assert!(err.to_string().contains(target.to_string_lossy().as_ref()));
    }

    #[test]
    fn precommit_allows_when_explicit_backlog_target_changed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nTransferred issue notes.\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");
        let requirement = agent_doc_cycle_state_io::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some(backlog_component_hash(&target)),
            baseline_item_ids: vec!["old1".to_string()],
        };
        write_backlog_doc(
            &target,
            "- [ ] [#new1] New transferred item\n- [ ] [#old1] Existing item\n",
        );

        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();
        agent_doc_cycle_state_io::record_backlog_target_requirements(&doc, &[requirement]).unwrap();

        super::precommit_pending_capture_check(&doc)
            .expect("changed explicit backlog target should satisfy closeout");
    }

    #[test]
    fn precommit_blocks_when_bug_transfer_inventory_is_smaller_than_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned agent-doc backlog items:\n- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#lvak] Routed-cycle ack follow-up\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(
            &target,
            "- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#lvak] Routed-cycle ack follow-up\n- [ ] [#old1] Existing item\n",
        );
        let requirement = agent_doc_cycle_state_io::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some("baseline".to_string()),
            baseline_item_ids: vec!["old1".to_string()],
        };

        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();
        agent_doc_cycle_state_io::record_backlog_target_requirements(&doc, &[requirement]).unwrap();
        agent_doc_cycle_state_io::record_required_explicit_backlog_item_count(&doc, 4).unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("described at least 4 distinct issue(s)")
        );
        assert!(
            err.to_string()
                .contains("only enumerated 2 explicit backlog item(s)")
        );
    }

    #[test]
    fn precommit_blocks_when_response_promises_multiple_new_target_items_but_only_some_exist() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned agent-doc backlog items:\n- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#mcrc] Uncommitted repair follow-up\n- [ ] [#lvls] Preserve list-shape constraint\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");
        let requirement = agent_doc_cycle_state_io::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some(backlog_component_hash(&target)),
            baseline_item_ids: vec!["old1".to_string()],
        };
        write_backlog_doc(
            &target,
            "- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#old1] Existing item\n",
        );

        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true).unwrap();
        agent_doc_cycle_state_io::record_backlog_target_requirements(&doc, &[requirement]).unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("promised new tracked item(s)"));
        assert!(err.to_string().contains("#mcrc"));
        assert!(err.to_string().contains("#lvls"));
    }

    #[test]
    fn precommit_blocks_when_bug_plan_reference_inventory_is_smaller_than_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nFiled two bugs.\nPlan: `tasks/agent-doc/plan-session-check-prefix-duplication.md`\n",
            false,
        );
        let plan = tmp
            .path()
            .join("tasks/agent-doc/plan-session-check-prefix-duplication.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# Plan\n").unwrap();

        agent_doc_cycle_state_io::record_required_plan_reference_count(&doc, 2).unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("required at least 2 explicit plan reference(s)")
        );
        assert!(
            err.to_string()
                .contains("only cited 1 existing plan path(s)")
        );
    }

    #[test]
    fn precommit_allows_when_bug_plan_reference_inventory_matches_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nFiled two bugs.\n1. **#scpd** Plan: `tasks/agent-doc/plan-session-check-prefix-duplication.md`\n2. **#nbla** Plan: `tasks/agent-doc/plan-chat-level-agent-doc-bug-contract.md`\n",
            false,
        );
        let first_plan = tmp
            .path()
            .join("tasks/agent-doc/plan-session-check-prefix-duplication.md");
        let second_plan = tmp
            .path()
            .join("tasks/agent-doc/plan-chat-level-agent-doc-bug-contract.md");
        std::fs::create_dir_all(first_plan.parent().unwrap()).unwrap();
        std::fs::write(&first_plan, "# Plan\n").unwrap();
        std::fs::write(&second_plan, "# Plan\n").unwrap();

        agent_doc_cycle_state_io::record_required_plan_reference_count(&doc, 2).unwrap();

        super::precommit_pending_capture_check(&doc)
            .expect("matching plan references should satisfy closeout");
    }

    #[test]
    fn precommit_pending_done_blocks_by_default_for_session_docs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &[],
        );

        let err = super::precommit_pending_done_check(&doc).unwrap_err();
        assert!(err.to_string().contains("[finalize] pre-commit gate"));
        assert!(err.to_string().contains("#4qja"));
        assert!(err.to_string().contains("--done 4qja"));
    }

    #[test]
    fn precommit_pending_done_passes_when_id_was_recorded() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &["4qja"],
        );

        super::precommit_pending_done_check(&doc)
            .expect("should pass when matching pending-done was recorded");
    }

    #[test]
    fn precommit_pending_done_auto_done_marks_item_done() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\nauto_done: true\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &[],
        );

        super::precommit_pending_done_check_with_options(
            &doc,
            super::PendingDoneCheckOptions {
                force_disk: true,
                backlog_effects: Some(&TEST_BACKLOG_COMMAND_EFFECTS),
            },
        )
        .expect("auto_done should record and apply missing --done mutations");
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- [x] [#4qja] Stream orchestrate patchback"));
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(state.pending_done_ids.contains(&"4qja".to_string()));
        assert!(state.had_pending_mutations);
    }

    #[test]
    fn precommit_pending_done_passes_when_id_was_kept_open() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #fvtg rescope — gpt-5\n\nUpdated #fvtg to keep the rollout validation item open.\nVerification:\n- cargo test\n",
            "- [ ] [#fvtg] Rollout validation item\n",
            &[],
        );
        agent_doc_cycle_state_io::record_pending_kept_open_ids(&doc, &["fvtg".to_string()])
            .unwrap()
            .unwrap();

        super::precommit_pending_done_check(&doc)
            .expect("kept-open pending ids should not require --done");
    }

    #[test]
    fn prewrite_pending_done_uses_kept_open_flag_ids() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "placeholder response",
            "- [ ] [#fvtg] Rollout validation item\n",
            &[],
        );

        super::prewrite_pending_done_check(
            &doc,
            "### Re: #fvtg rescope — gpt-5\n\nUpdated #fvtg to keep the rollout validation item open.\nVerification:\n- cargo test\n",
            &super::PendingWriteFlags {
                pending_kept_open_ids: vec!["#FVTG".to_string()],
                strict_closeout: true,
                ..Default::default()
            },
        )
        .expect("pre-write kept-open ids should not require --done");
    }

    #[test]
    fn precommit_pending_done_blocks_for_icebox_only_item_without_recorded_done() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_tracked_work(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #ice01 parked follow-up — gpt-5\n\nImplemented the parked follow-up.\n",
            "- [ ] [#keep1] Keep backlog item\n",
            Some("- [ ] [#ice01] Parked follow-up\n"),
            &[],
        );

        let err = super::precommit_pending_done_check(&doc).unwrap_err();
        assert!(err.to_string().contains("#ice01"));
        assert!(err.to_string().contains("--done ice01"));
    }

    #[test]
    fn precommit_pending_done_warn_mode_skips_precommit_block() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\npending_done_guard: warn\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &[],
        );

        super::precommit_pending_done_check(&doc)
            .expect("warn mode should defer to post-commit session-check");
    }

    #[test]
    fn precommit_pending_done_respects_suppression_marker() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\n<!-- no-pending-done-guard -->\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &[],
        );

        super::precommit_pending_done_check(&doc)
            .expect("suppression marker should disable the pre-commit pending-done gate");
    }
}

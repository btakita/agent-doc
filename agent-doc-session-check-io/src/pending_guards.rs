use std::path::Path;

use agent_doc_run_context_io::CycleContext;
use agent_doc_workflow::session_check::GuardResult;
use anyhow::Result;

use crate::{
    promised_backlog_item_inventory_shortfall, promised_plan_reference_shortfall,
    resolve_pending_capture_guard_mode_with_context, resolve_pending_done_guard_mode_with_context,
    unresolved_backlog_capture_targets, unresolved_promised_backlog_item_ids,
};

pub fn check_pending_capture_guard(file: &Path, rc: &CycleContext) -> Result<GuardResult> {
    let mode = resolve_pending_capture_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() || state.had_pending_mutations {
        return Ok(GuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = agent_doc_capture_io::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != agent_doc_workflow::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }
    if capture
        .response_body
        .contains("<!-- no-pending-capture -->")
    {
        return Ok(GuardResult::None);
    }

    let response_text =
        agent_doc_turn::closeout_signal::response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(GuardResult::None);
    }
    let missing_targets = unresolved_backlog_capture_targets(file, &state);
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        return Ok(
            agent_doc_workflow::session_check::pending_capture_missing_targets_guard_result(
                &missing_targets,
            ),
        );
    }
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            promised_backlog_item_inventory_shortfall(&state, &response_text)
    {
        let targets = state
            .required_backlog_targets
            .iter()
            .map(|target| target.path.clone())
            .collect::<Vec<_>>();
        return Ok(
            agent_doc_workflow::session_check::pending_capture_inventory_shortfall_guard_result(
                expected_count,
                promised_count,
                &targets,
            ),
        );
    }
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            promised_plan_reference_shortfall(file, &state, &response_text)
    {
        return Ok(
            agent_doc_workflow::session_check::pending_capture_plan_reference_shortfall_guard_result(
                expected_count,
                promised_count,
            ),
        );
    }
    let missing_ids = unresolved_promised_backlog_item_ids(file, &state, &response_text);
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        let targets = state
            .required_backlog_targets
            .iter()
            .map(|target| target.path.clone())
            .collect::<Vec<_>>();
        return Ok(
            agent_doc_workflow::session_check::pending_capture_missing_promised_ids_guard_result(
                &missing_ids,
                &targets,
            ),
        );
    }
    if state.requires_backlog_capture
        && state.required_backlog_targets.is_empty()
        && !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
    {
        return Ok(
            agent_doc_workflow::session_check::pending_capture_required_no_mutations_guard_result(),
        );
    }

    let signal = agent_doc_turn::heuristics::detect_uncaptured_recommendations(&response_text);
    let skip = match signal.estimated_count {
        0 => true,
        1 => signal.confidence < 0.7,
        _ => signal.confidence < 0.5,
    };
    if skip {
        return Ok(GuardResult::None);
    }

    Ok(
        agent_doc_workflow::session_check::pending_capture_recommendations_guard_result(
            signal.estimated_count,
            mode,
        ),
    )
}

pub fn check_pending_done_guard(file: &Path, rc: &CycleContext) -> Result<GuardResult> {
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() {
        return Ok(GuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = agent_doc_capture_io::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != agent_doc_workflow::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }

    let doc = crate::resolve_current_document(file, "pending_done_guard")?;
    let open_tracked_work_ids =
        agent_doc_document::tracked_work_projection::open_tracked_work_ids(doc.content());
    let missing = match agent_doc_turn::closeout_signal::tracked_work_completion_decision(
        agent_doc_turn::closeout_signal::TrackedWorkCompletionEvidence {
            response_body: &capture.response_body,
            recorded_done_ids: &state.pending_done_ids,
            kept_open_ids: &state.pending_kept_open_ids,
            open_tracked_work_ids: &open_tracked_work_ids,
        },
    ) {
        agent_doc_turn::closeout_signal::TrackedWorkCompletionDecision::Pass => {
            return Ok(GuardResult::None);
        }
        agent_doc_turn::closeout_signal::TrackedWorkCompletionDecision::MissingDone {
            missing_ids,
        } => missing_ids,
    };

    let file_display = doc.key().display().to_string();
    Ok(agent_doc_workflow::session_check::pending_done_guard_result(&file_display, &missing, mode))
}

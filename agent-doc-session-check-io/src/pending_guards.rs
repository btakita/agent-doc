use std::collections::BTreeSet;
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
    let Some(capture) = crate::captured_response_guard_evidence(file, &state, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if !capture.capture_committed {
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
        && !agent_doc_turn::heuristics::response_explicitly_closes_named_followup(&response_text)
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

fn known_ids_for_coined_guard(file: &Path, content: &str) -> Result<BTreeSet<String>> {
    let components = agent_doc_element::element::parse(content)?;
    let mut known = BTreeSet::new();
    for component in components
        .iter()
        .filter(|component| component.name != "exchange")
    {
        known.extend(agent_doc_turn::coined_ids::extract_tags(
            component.content(content),
        ));
    }
    // `#coinedguardledgerasymmetry`: one predicate, shared with the `PreToolUse`
    // guard, so the same archive cannot answer "tracked" on one path and
    // "invented" on the other.
    known.extend(agent_doc_element_backlog_io::done_archive::archived_tracked_ids(
        file, content,
    )?);
    Ok(known)
}

/// `#coinedid` — the response invented `#id` tags that no tracked item records.
///
/// The known universe is every component EXCEPT `exchange`: after commit the
/// response itself lives in `exchange`, so scanning the whole document would let
/// a coined id vouch for itself and the guard would never fire.
pub fn check_coined_ids_guard(file: &Path, _rc: &CycleContext) -> Result<GuardResult> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() {
        return Ok(GuardResult::None);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::captured_response_guard_evidence(file, &state, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if !capture.capture_committed {
        return Ok(GuardResult::None);
    }
    let response_text =
        agent_doc_turn::closeout_signal::response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(GuardResult::None);
    }
    let Ok(content) = std::fs::read_to_string(file) else {
        return Ok(GuardResult::None);
    };
    let Ok(known) = known_ids_for_coined_guard(file, &content) else {
        return Ok(GuardResult::None);
    };
    let coined = agent_doc_turn::coined_ids::coined_ids(&response_text, &known);
    // Second pass, lazy on purpose (`#hookhashanchortags`). An anchor such as
    // `#ci-no-closeout-wait` names a documented invariant in AGENTS.md, a SKILL,
    // a runbook, or a spec; a response that cites the rule by name is doing the
    // right thing, and warning about it trains the reader to ignore this guard.
    // Reading those files costs hundreds of kilobytes, so it happens only when
    // something is already about to be reported. Same file set as the
    // `PreToolUse` guard, from the same place, so the two cannot drift.
    let coined = match agent_doc_fs::find_project_root(file) {
        Some(root) if !coined.is_empty() => {
            let anchors = agent_doc_fs::instruction_surface_anchors(&root);
            coined
                .into_iter()
                .filter(|tag| !anchors.contains(tag))
                .collect::<Vec<_>>()
        }
        _ => coined,
    };
    Ok(agent_doc_workflow::session_check::coined_ids_guard_result(
        &coined,
    ))
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
    let Some(capture) = crate::captured_response_guard_evidence(file, &state, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if !capture.capture_committed {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_done_archive_ids_are_known_after_inline_reap() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(dir.path())
                .status()
                .unwrap()
                .success()
        );
        // `external_done_archive_ids` resolves the archive relative to the
        // nearest `.agent-doc` project root (not a bare git root), so the temp
        // project needs the marker directory the session doc lives under.
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        let archive = dir.path().join("session.done.md");
        std::fs::write(&archive, "- [x] [#qeditrace] shipped editor route fix\n").unwrap();
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: #qeditrace\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:done archive=session.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&file, content).unwrap();

        let known = known_ids_for_coined_guard(&file, content).unwrap();
        assert!(known.contains("qeditrace"));
        assert!(agent_doc_turn::coined_ids::coined_ids("closed #qeditrace", &known).is_empty());
    }
}

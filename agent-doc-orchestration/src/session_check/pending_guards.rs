use super::*;

pub(crate) fn check_pending_capture_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_capture_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() || state.had_pending_mutations {
        return Ok(GuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
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
    let missing_targets = crate::write::unresolved_backlog_capture_targets(file, &state);
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: committed response came from a prompt that required backlog capture in {}, but those tracked-work surfaces did not change this cycle",
            missing_targets.join(", ")
        )));
    }
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            crate::write::promised_backlog_item_inventory_shortfall(&state, &response_text)
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: active #agent-doc-bug contract described at least {} distinct issue(s), but the committed response only enumerated {} explicit backlog item(s) for target(s) {}",
            expected_count,
            promised_count,
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            crate::write::promised_plan_reference_shortfall(file, &state, &response_text)
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: active #agent-doc-bug contract required at least {} explicit plan reference(s), but the committed response only cited {} existing plan path(s)",
            expected_count, promised_count,
        )));
    }
    let missing_ids =
        crate::write::unresolved_promised_backlog_item_ids(file, &state, &response_text);
    if !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: committed response promised new tracked item(s) {} for explicit backlog target(s) {}, but those ids are still missing after this cycle",
            missing_ids.join(", "),
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if state.requires_backlog_capture
        && state.required_backlog_targets.is_empty()
        && !agent_doc_turn::heuristics::response_explicitly_has_no_followups(&response_text)
    {
        return Ok(GuardResult::Error(
            "[session-check] error: committed response came from a prompt that required backlog capture, but this cycle recorded no backlog mutations and did not explicitly state that there were no actionable follow-up items"
                .to_string(),
        ));
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

    let warn_line = format!(
        "[session-check] warn: response contains ~{} recommendation-like items but no --pending-add flags were used this cycle",
        signal.estimated_count
    );
    let hint_line =
        "[session-check] hint: consider adding pending items for actionable follow-up work"
            .to_string();

    Ok(match mode {
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Warn => {
            GuardResult::Warn(vec![warn_line, hint_line])
        }
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Strict => {
            GuardResult::Error(format!(
                "{}\n[session-check] hint: re-run with --pending-add flags or set pending_capture_guard = \"warn\" to downgrade",
                warn_line.replacen("[session-check] warn:", "[session-check] error:", 1)
            ))
        }
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

pub(crate) fn resolve_pending_capture_guard_mode(
    file: &Path,
) -> Result<agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(
        agent_doc_frontmatter::project_config::resolve_pending_capture_guard_mode(
            &fm,
            &project_config,
        ),
    )
}

pub(crate) fn resolve_pending_capture_guard_mode_with_context(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode> {
    // Phase 6 (#lr-content-6): read frontmatter from the cached `FrontmatterSlot`
    // instead of re-reading + re-parsing the document. The slot already parsed
    // `DocContentCell` (set once per inspect cycle); these guard-mode fields are
    // SSH-resolution-independent so the resolved value is identical.
    let fm = rc.frontmatter();
    let project_config = rc.project_config();
    Ok(
        agent_doc_frontmatter::project_config::resolve_pending_capture_guard_mode(
            &fm,
            &project_config,
        ),
    )
}

pub(crate) fn resolve_pending_done_guard_mode(
    file: &Path,
) -> Result<agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(
        agent_doc_frontmatter::project_config::resolve_pending_done_guard_mode(
            &fm,
            &project_config,
        ),
    )
}

pub(crate) fn resolve_pending_done_guard_mode_with_context(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode> {
    // Phase 6 (#lr-content-6): frontmatter from the cached slot, project config
    // from the cached `ProjectConfigSlot`.
    let fm = rc.frontmatter();
    let project_config = rc.project_config();
    Ok(
        agent_doc_frontmatter::project_config::resolve_pending_done_guard_mode(
            &fm,
            &project_config,
        ),
    )
}

pub(crate) fn resolve_review_done_guard_mode(
    file: &Path,
) -> Result<agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(agent_doc_frontmatter::project_config::resolve_review_done_guard_mode(&fm, &project_config))
}

pub(crate) fn resolve_auto_done(file: &Path) -> Result<bool> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(agent_doc_frontmatter::project_config::resolve_auto_done(
        &fm,
        &project_config,
    ))
}

pub(crate) fn check_pending_done_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(GuardResult::None);
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(GuardResult::None);
    };
    if state.is_open() {
        return Ok(GuardResult::None);
    }

    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(GuardResult::None);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
        return Ok(GuardResult::None);
    };
    if capture.state != crate::capture::CaptureState::Committed {
        return Ok(GuardResult::None);
    }

    let content = std::fs::read_to_string(file)?;
    let open_tracked_work_ids =
        agent_doc_document::tracked_work_projection::open_tracked_work_ids(&content);
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
    let repair = format!(
        "agent-doc write {} {} --pending-only --commit",
        file.display(),
        hint
    );
    let warn_line = format!(
        "[session-check] warn: response appears to complete existing pending {} but no matching `--done` was recorded this cycle",
        ids
    );

    Ok(match mode {
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Warn => {
            GuardResult::Warn(vec![
                warn_line,
                format!(
                    "[session-check] hint: repair with `{}` or add `pending_done_guard: off` for this document when the item should stay open",
                    repair
                ),
            ])
        }
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Strict => {
            GuardResult::Error(format!(
                "{}\n[session-check] hint: repair with `{}` or set pending_done_guard = \"warn\" to downgrade",
                warn_line.replacen("[session-check] warn:", "[session-check] error:", 1),
                repair
            ))
        }
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

use super::*;

pub(crate) fn check_pending_capture_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_capture_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
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

    let response_text = response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(GuardResult::None);
    }
    let missing_targets = crate::write::unresolved_backlog_capture_targets(file, &state);
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        return Ok(GuardResult::Error(format!(
            "[session-check] error: committed response came from a prompt that required backlog capture in {}, but those tracked-work surfaces did not change this cycle",
            missing_targets.join(", ")
        )));
    }
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
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
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
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
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
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
        && !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
    {
        return Ok(GuardResult::Error(
            "[session-check] error: committed response came from a prompt that required backlog capture, but this cycle recorded no backlog mutations and did not explicitly state that there were no actionable follow-up items"
                .to_string(),
        ));
    }

    let signal = crate::heuristics::detect_uncaptured_recommendations(&response_text);
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
        crate::frontmatter::PendingCaptureGuardMode::Warn => {
            GuardResult::Warn(vec![warn_line, hint_line])
        }
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: re-run with --pending-add flags or set pending_capture_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] error:", 1)
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

pub fn resolve_pending_capture_guard_mode(
    file: &Path,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(mode) = fm.pending_capture_guard {
        return Ok(mode);
    }
    Ok(crate::project_config::load_project_for_doc(file)
        .guards
        .pending_capture
        .unwrap_or_default())
}

pub fn resolve_pending_capture_guard_mode_with_context(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    // Phase 6 (#lr-content-6): read frontmatter from the cached `FrontmatterSlot`
    // instead of re-reading + re-parsing the document. The slot already parsed
    // `DocContentCell` (set once per inspect cycle); these guard-mode fields are
    // SSH-resolution-independent so the resolved value is identical.
    let fm = rc.frontmatter();
    if let Some(mode) = fm.pending_capture_guard {
        return Ok(mode);
    }
    Ok(rc
        .project_config()
        .guards
        .pending_capture
        .unwrap_or_default())
}

pub fn resolve_pending_done_guard_mode(
    file: &Path,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(mode) = fm.pending_done_guard {
        return Ok(mode);
    }
    if let Some(mode) = crate::project_config::load_project_for_doc(file)
        .guards
        .pending_done
    {
        return Ok(mode);
    }
    if fm
        .session
        .as_deref()
        .is_some_and(|session| !session.trim().is_empty())
    {
        return Ok(crate::frontmatter::PendingCaptureGuardMode::Strict);
    }
    Ok(crate::frontmatter::PendingCaptureGuardMode::Warn)
}

pub fn resolve_pending_done_guard_mode_with_context(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    // Phase 6 (#lr-content-6): frontmatter from the cached slot, project config
    // from the cached `ProjectConfigSlot`.
    let fm = rc.frontmatter();
    if let Some(mode) = fm.pending_done_guard {
        return Ok(mode);
    }
    if let Some(mode) = rc.project_config().guards.pending_done {
        return Ok(mode);
    }
    if fm
        .session
        .as_deref()
        .is_some_and(|session| !session.trim().is_empty())
    {
        return Ok(crate::frontmatter::PendingCaptureGuardMode::Strict);
    }
    Ok(crate::frontmatter::PendingCaptureGuardMode::Warn)
}

pub fn resolve_review_done_guard_mode(
    file: &Path,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(mode) = fm.review_done_guard {
        return Ok(mode);
    }
    if let Some(mode) = crate::project_config::load_project_for_doc(file)
        .guards
        .review_done
    {
        return Ok(mode);
    }
    Ok(crate::frontmatter::PendingCaptureGuardMode::Off)
}

pub fn resolve_review_done_guard_mode_with_context(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<crate::frontmatter::PendingCaptureGuardMode> {
    // Phase 6 (#lr-content-6): cached frontmatter + project config slots.
    let fm = rc.frontmatter();
    if let Some(mode) = fm.review_done_guard {
        return Ok(mode);
    }
    if let Some(mode) = rc.project_config().guards.review_done {
        return Ok(mode);
    }
    Ok(crate::frontmatter::PendingCaptureGuardMode::Off)
}

pub fn resolve_auto_done(file: &Path) -> Result<bool> {
    let content = std::fs::read_to_string(file)?;
    let (fm, _) = crate::frontmatter::parse(&content)?;
    if let Some(enabled) = fm.auto_done {
        return Ok(enabled);
    }
    Ok(crate::project_config::load_project_for_doc(file)
        .guards
        .auto_done
        .unwrap_or(false))
}

pub fn resolve_auto_done_with_context(_file: &Path, rc: &crate::graph::RunContext) -> Result<bool> {
    // Phase 6 (#lr-content-6): cached frontmatter + project config slots.
    let fm = rc.frontmatter();
    if let Some(enabled) = fm.auto_done {
        return Ok(enabled);
    }
    Ok(rc.project_config().guards.auto_done.unwrap_or(false))
}

pub(crate) fn check_pending_done_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): resolve guard mode from the cached frontmatter slot.
    let mode = resolve_pending_done_guard_mode_with_context(file, rc)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
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
    if capture
        .response_body
        .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(GuardResult::None);
    }

    let response_text = response_text_for_guards(&capture.response_body);
    let missing = detect_missing_pending_done_ids(
        file,
        &response_text,
        &state.pending_done_ids,
        &state.pending_kept_open_ids,
    )?;
    if missing.is_empty() {
        return Ok(GuardResult::None);
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
        crate::frontmatter::PendingCaptureGuardMode::Warn => GuardResult::Warn(vec![
            warn_line,
            format!(
                "[session-check] hint: repair with `{}` or add `pending_done_guard: off` for this document when the item should stay open",
                repair
            ),
        ]),
        crate::frontmatter::PendingCaptureGuardMode::Strict => GuardResult::Error(format!(
            "{}\n[session-check] hint: repair with `{}` or set pending_done_guard = \"warn\" to downgrade",
            warn_line.replacen("[session-check] warn:", "[session-check] error:", 1),
            repair
        )),
        crate::frontmatter::PendingCaptureGuardMode::Off => GuardResult::None,
    })
}

pub fn detect_missing_pending_done_ids(
    file: &Path,
    response_text: &str,
    recorded_done_ids: &[String],
    kept_open_ids: &[String],
) -> Result<Vec<String>> {
    if response_text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let open_ids = open_tracked_work_ids(file)?;
    if open_ids.is_empty() {
        return Ok(Vec::new());
    }

    let recorded_done: std::collections::HashSet<String> = recorded_done_ids
        .iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let kept_open: std::collections::HashSet<String> = kept_open_ids
        .iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(id))
        .filter(|id| !id.is_empty())
        .collect();

    Ok(open_ids
        .into_iter()
        .filter(|id| !kept_open.contains(id))
        .filter(|id| response_clearly_completes_pending_id(response_text, id))
        .filter(|id| !recorded_done.contains(id))
        .collect())
}

pub fn response_text_for_guards(response: &str) -> String {
    let Ok((patches, unmatched)) = crate::template::parse_patches(response) else {
        return response.to_string();
    };

    let preferred: Vec<String> = patches
        .iter()
        .filter(|patch| matches!(patch.name.as_str(), "exchange" | "findings"))
        .map(|patch| patch.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();
    if !preferred.is_empty() {
        return preferred.join("\n\n");
    }

    if !unmatched.trim().is_empty() {
        return unmatched.trim().to_string();
    }

    let fallback: Vec<String> = patches
        .iter()
        .filter(|patch| {
            !is_backlog_component(&patch.name)
                && !agent_doc_element::element::is_review_component(&patch.name)
        })
        .map(|patch| patch.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();
    if !fallback.is_empty() {
        return fallback.join("\n\n");
    }

    response.to_string()
}

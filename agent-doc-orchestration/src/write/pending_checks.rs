//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub fn unresolved_backlog_capture_targets(
    file: &Path,
    state: &crate::cycle_state::CycleState,
) -> Vec<String> {
    let current = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());

    state
        .required_backlog_targets
        .iter()
        .filter(|target| {
            let target_path = Path::new(&target.path);
            let normalized_target =
                std::fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf());
            if normalized_target == current {
                return !state.had_pending_mutations;
            }

            let Ok(Some(content)) = std::fs::read_to_string(&normalized_target).map(Some) else {
                return true;
            };
            let Ok(components) = crate::component::parse(&content) else {
                return true;
            };
            let component = target
                .component
                .as_deref()
                .and_then(|name| components.iter().find(|component| component.name == name))
                .or_else(|| {
                    components
                        .iter()
                        .find(|component| crate::component::is_backlog_component(&component.name))
                })
                .or_else(|| {
                    components.iter().find(|component| {
                        crate::component::is_tracked_work_component(&component.name)
                    })
                });
            let current_hash = component
                .map(|component| crate::ops_log::content_hash(component.content(&content)));
            match (&target.baseline_hash, current_hash) {
                (Some(expected), Some(current)) => current == *expected,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => true,
            }
        })
        .map(|target| target.path.clone())
        .collect()
}

pub(crate) fn normalize_pending_id(id: &str) -> String {
    id.trim().trim_start_matches('#').to_ascii_lowercase()
}

pub(crate) fn tracked_work_ids_from_component_body(body: &str) -> HashSet<String> {
    let (_, items, _) = crate::pending::parse_items(body);
    items
        .into_iter()
        .filter(|item| !item.is_done())
        .map(|item| normalize_pending_id(&item.id))
        .filter(|id| !id.is_empty())
        .collect()
}

pub(crate) fn tracked_work_ids_for_target(
    content: &str,
    preferred_component: Option<&str>,
) -> Result<HashSet<String>> {
    let components = crate::component::parse(content)?;
    let component = preferred_component
        .and_then(|name| components.iter().find(|component| component.name == name))
        .or_else(|| {
            components
                .iter()
                .find(|component| crate::component::is_backlog_component(&component.name))
        })
        .or_else(|| {
            components
                .iter()
                .find(|component| crate::component::is_tracked_work_component(&component.name))
        });
    Ok(component
        .map(|component| tracked_work_ids_from_component_body(component.content(content)))
        .unwrap_or_default())
}

pub(crate) fn promised_backlog_item_ids_from_response(
    response_text: &str,
    state: &crate::cycle_state::CycleState,
) -> Vec<String> {
    let baseline_ids: HashSet<String> = state
        .required_backlog_targets
        .iter()
        .flat_map(|target| target.baseline_item_ids.iter())
        .map(|id| normalize_pending_id(id))
        .collect();
    let (_, items, _) = crate::pending::parse_items(response_text);
    let mut promised = Vec::new();
    for item in items.into_iter().filter(|item| !item.is_done()) {
        let id = normalize_pending_id(&item.id);
        if id.is_empty()
            || baseline_ids.contains(&id)
            || promised.iter().any(|existing| existing == &id)
        {
            continue;
        }
        promised.push(id);
    }
    promised
}

pub fn promised_backlog_item_inventory_shortfall(
    state: &crate::cycle_state::CycleState,
    response_text: &str,
) -> Option<(usize, usize)> {
    if state.required_backlog_targets.is_empty() || state.required_explicit_backlog_item_count == 0
    {
        return None;
    }

    let promised_count = promised_backlog_item_ids_from_response(response_text, state).len();
    if promised_count >= state.required_explicit_backlog_item_count {
        None
    } else {
        Some((state.required_explicit_backlog_item_count, promised_count))
    }
}

pub(crate) fn promised_plan_reference_paths(file: &Path, response_text: &str) -> Vec<String> {
    let mut promised = Vec::new();
    for raw_line in response_text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") || trimmed.starts_with('>') {
            continue;
        }
        if !trimmed.to_ascii_lowercase().contains("plan") {
            continue;
        }
        let Some(path) = crate::security::referenced_markdown_path(file, trimmed) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !file_name.contains("plan") {
            continue;
        }
        let normalized = std::fs::canonicalize(&path)
            .unwrap_or(path)
            .display()
            .to_string();
        if !promised.iter().any(|existing| existing == &normalized) {
            promised.push(normalized);
        }
    }
    promised
}

pub fn promised_plan_reference_shortfall(
    file: &Path,
    state: &crate::cycle_state::CycleState,
    response_text: &str,
) -> Option<(usize, usize)> {
    if state.required_plan_reference_count == 0 {
        return None;
    }

    let promised_count = promised_plan_reference_paths(file, response_text).len();
    if promised_count >= state.required_plan_reference_count {
        None
    } else {
        Some((state.required_plan_reference_count, promised_count))
    }
}

pub fn unresolved_promised_backlog_item_ids(
    file: &Path,
    state: &crate::cycle_state::CycleState,
    response_text: &str,
) -> Vec<String> {
    if state.required_backlog_targets.is_empty() {
        return Vec::new();
    }

    let promised_ids = promised_backlog_item_ids_from_response(response_text, state);
    if promised_ids.is_empty() {
        return Vec::new();
    }

    let current = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut current_target_ids = HashSet::new();
    for target in &state.required_backlog_targets {
        let target_path = Path::new(&target.path);
        let normalized_target =
            std::fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf());
        let content = if normalized_target == current {
            match std::fs::read_to_string(file) {
                Ok(content) => content,
                Err(_) => continue,
            }
        } else {
            match std::fs::read_to_string(&normalized_target) {
                Ok(content) => content,
                Err(_) => continue,
            }
        };
        let Ok(ids) = tracked_work_ids_for_target(&content, target.component.as_deref()) else {
            continue;
        };
        current_target_ids.extend(ids);
    }

    promised_ids
        .into_iter()
        .filter(|id| !current_target_ids.contains(id))
        .map(|id| format!("#{}", id))
        .collect()
}

pub(crate) fn precommit_pending_capture_check(file: &Path) -> Result<()> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(());
    };
    if state.had_pending_mutations && state.required_backlog_targets.is_empty() {
        return Ok(());
    }

    let Some(capture) = crate::capture::load_active(file)? else {
        return Ok(());
    };
    if capture
        .response_body
        .contains("<!-- no-pending-capture -->")
    {
        return Ok(());
    }

    let response_text = crate::session_check::response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(());
    }
    let missing_targets = unresolved_backlog_capture_targets(file, &state);
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureTargetMissing,
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
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            promised_backlog_item_inventory_shortfall(&state, &response_text)
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureInventoryShortfall,
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
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            promised_plan_reference_shortfall(file, &state, &response_text)
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCapturePlanShortfall,
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
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCapturePromisedIdsMissing,
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
        && !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureRequired,
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

    let mode = crate::session_check::resolve_pending_capture_guard_mode(file)?;
    if mode != crate::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let signal = crate::heuristics::detect_uncaptured_recommendations(&response_text);
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
        crate::flow::types::FlowStage::PreCommitGuard,
        crate::flow::types::FlowOutcome::Blocked,
        crate::flow::closeout::CloseoutGuardReason::PendingCaptureRecommendations,
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

pub(crate) fn prewrite_pending_capture_check(
    file: &Path,
    response_body: &str,
    flags: &WriteFlags,
) -> Result<()> {
    if !flags.strict_closeout {
        return Ok(());
    }

    let state = crate::cycle_state::load(file)?;
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

    let response_text = crate::session_check::response_text_for_guards(response_body);
    if response_text.trim().is_empty() {
        return Ok(());
    }
    let missing_targets = state
        .as_ref()
        .map(|state| unresolved_backlog_capture_targets(file, state))
        .unwrap_or_default();
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureTargetMissing,
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
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) = state
            .as_ref()
            .and_then(|state| promised_backlog_item_inventory_shortfall(state, &response_text))
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureInventoryShortfall,
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
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) = state
            .as_ref()
            .and_then(|state| promised_plan_reference_shortfall(file, state, &response_text))
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCapturePlanShortfall,
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
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCapturePromisedIdsMissing,
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
    }) && !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureRequired,
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

    let mode = crate::session_check::resolve_pending_capture_guard_mode(file)?;
    if mode != crate::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let signal = crate::heuristics::detect_uncaptured_recommendations(&response_text);
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
        crate::flow::types::FlowStage::PreWriteGuard,
        crate::flow::types::FlowOutcome::Blocked,
        crate::flow::closeout::CloseoutGuardReason::PendingCaptureRecommendations,
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

pub(crate) fn precommit_pending_done_check(file: &Path) -> Result<()> {
    let mode = crate::session_check::resolve_pending_done_guard_mode(file)?;
    if mode != crate::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(());
    };

    let Some(capture) = crate::capture::load_active(file)? else {
        return Ok(());
    };
    if capture
        .response_body
        .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(());
    }

    let response_text = crate::session_check::response_text_for_guards(&capture.response_body);
    let malformed = crate::session_check::malformed_tracked_item_refs(file, Some(&response_text))?;
    if !malformed.is_empty() {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingDoneMalformedTrackedItem,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: {}",
            crate::session_check::malformed_tracked_item_message(&malformed)
        );
    }
    let missing = crate::session_check::detect_missing_pending_done_ids(
        file,
        &response_text,
        &state.pending_done_ids,
        &state.pending_kept_open_ids,
    )?;
    if missing.is_empty() {
        return Ok(());
    }

    if crate::session_check::resolve_auto_done(file)? {
        for id in &missing {
            auto_apply_pending_done_id(file, id)?;
        }
        crate::cycle_state::record_pending_done_ids(file, &missing)?;
        crate::cycle_state::mark_pending_mutations(file)?;
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
        crate::flow::types::FlowStage::PreCommitGuard,
        crate::flow::types::FlowOutcome::Blocked,
        crate::flow::closeout::CloseoutGuardReason::PendingDoneMissing,
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

pub(crate) fn prewrite_pending_done_check(file: &Path, response_body: &str, flags: &WriteFlags) -> Result<()> {
    if !flags.strict_closeout {
        return Ok(());
    }

    let mode = crate::session_check::resolve_pending_done_guard_mode(file)?;
    if mode != crate::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let state = crate::cycle_state::load(file)?;
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
    if response_body.contains("<!-- no-pending-done-guard -->") {
        return Ok(());
    }

    let response_text = crate::session_check::response_text_for_guards(response_body);
    let malformed = crate::session_check::malformed_tracked_item_refs(file, Some(&response_text))?;
    if !malformed.is_empty() {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingDoneMalformedTrackedItem,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: {}",
            crate::session_check::malformed_tracked_item_message(&malformed)
        );
    }
    let missing = crate::session_check::detect_missing_pending_done_ids(
        file,
        &response_text,
        &recorded_done_ids,
        &kept_open_ids,
    )?;
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
        crate::flow::types::FlowStage::PreWriteGuard,
        crate::flow::types::FlowOutcome::Blocked,
        crate::flow::closeout::CloseoutGuardReason::PendingDoneMissing,
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

pub(crate) fn auto_apply_pending_done_if_enabled(
    file: &Path,
    response_body: &str,
    flags: &WriteFlags,
    current_content: &mut String,
) -> Result<()> {
    if !flags.strict_closeout || !crate::session_check::resolve_auto_done(file)? {
        return Ok(());
    }
    if response_body.contains("<!-- no-pending-done-guard -->") {
        return Ok(());
    }

    let state = crate::cycle_state::load(file)?;
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

    let response_text = crate::session_check::response_text_for_guards(response_body);
    let missing = crate::session_check::detect_missing_pending_done_ids(
        file,
        &response_text,
        &recorded_done_ids,
        &kept_open_ids,
    )?;
    if missing.is_empty() {
        return Ok(());
    }

    for id in &missing {
        auto_apply_pending_done_id(file, id)?;
    }
    crate::cycle_state::record_pending_done_ids(file, &missing)?;
    crate::cycle_state::mark_pending_mutations(file)?;
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

pub(crate) fn auto_apply_pending_done_id(file: &Path, id: &str) -> Result<()> {
    if let Some(component) = crate::pending_cmd::open_item_component_name(file, id)?
        && crate::component::is_backlog_component(&component)
    {
        crate::pending_cmd::gate(file, id)?;
    }
    enforce_review_done_guard(file, id)?;
    crate::pending_cmd::done(file, id)
}

pub(crate) fn run_closeout_pending_maintenance(file: &Path, commit_mode: CommitMode) -> Result<()> {
    if commit_mode != CommitMode::Required {
        return Ok(());
    }
    crate::preflight::run_pending_maintenance(file).map(|_| ())
}

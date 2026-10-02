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
    known.extend(agent_doc_element_backlog_io::done_archive::archived_tracked_ids(file, content)?);
    // `#coinedpresetid`: a registered preset name resolves to its frontmatter
    // body, so it is a reference and not an invented id. Frontmatter is not a
    // component, so the scan above cannot see it.
    known.extend(agent_doc_turn::coined_ids::registered_prompt_preset_ids(
        content,
    ));
    Ok(known)
}

/// GH 92: the coined ids the document's CURRENT `exchange` still carries.
///
/// The guard reads the committed capture, which a later node-safe repair
/// (`exchange remove` + `add-response`) or an operator edit does not rewrite.
/// Re-deriving from live content is what lets removing the citation clear the
/// warn. An unparseable document keeps every id: this filter may only drop a
/// report whose evidence is provably gone.
fn still_cited_in_live_exchange(content: &str, coined: Vec<String>) -> Vec<String> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return coined;
    };
    let live: BTreeSet<String> = components
        .iter()
        .filter(|component| component.name == "exchange")
        .flat_map(|component| agent_doc_turn::coined_ids::extract_tags(component.content(content)))
        .collect();
    coined
        .into_iter()
        .filter(|tag| live.contains(tag))
        .collect()
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
    // GH 92: re-derive from CURRENT content. The captured body is what the
    // cycle committed, but an operator or a node-safe repair (`exchange remove`
    // + `add-response`) may since have rewritten the citation away. A warn that
    // survives its own remedy trains the reader to ignore it, so an id the live
    // exchange no longer carries is not reported.
    let coined = still_cited_in_live_exchange(&content, coined);
    // Widening passes, lazy on purpose (`#hookhashanchortags`, GH 92): an
    // instruction or source anchor names a documented invariant, and an id
    // tracked in a sibling session document is a citation of the decision that
    // document owns. Both cost a project read, so they run only when something
    // is already about to be reported — through the same predicate the
    // `PreToolUse` guard reads, so the two cannot drift.
    let coined = match agent_doc_fs::find_project_root(file) {
        Some(root) => {
            agent_doc_element_backlog_io::cross_document::unresolved_in_project(&root, coined)
        }
        None => coined,
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

    /// `#coinedpresetid` — frontmatter is not a component, so the component scan
    /// cannot see a registered preset name; the guard must read it separately.
    #[test]
    fn registered_prompt_preset_names_are_known_ids() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "prompt_presets:\n",
            "  '#actionable-review': Add actionable review items into backlog + queue\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: something\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&file, content).unwrap();

        let known = known_ids_for_coined_guard(&file, content).unwrap();
        assert!(known.contains("actionable-review"), "got {known:?}");
        assert!(
            agent_doc_turn::coined_ids::coined_ids(
                "matched #actionable-review inside the quoted bullet",
                &known
            )
            .is_empty()
        );
        assert_eq!(
            agent_doc_turn::coined_ids::coined_ids("#actionable-review and #inventedhere", &known),
            vec!["inventedhere".to_string()],
            "an unregistered id must still coin"
        );
    }
}

#[cfg(test)]
mod coined_live_exchange_tests {
    use super::still_cited_in_live_exchange;

    /// GH 92: removing a citation from the live exchange clears its warn, while
    /// an id the exchange still carries keeps it.
    #[test]
    fn a_citation_rewritten_out_of_the_live_exchange_is_not_reported() {
        let content = "---\nagent_doc_session: s\n---\n\n<!-- agent:exchange -->\n### Re: x\nStill cites #keptid; the other was rewritten as prose.\n<!-- /agent:exchange -->\n";
        let coined = vec!["keptid".to_string(), "pushurl".to_string()];
        assert_eq!(
            still_cited_in_live_exchange(content, coined),
            vec!["keptid".to_string()]
        );
    }
}

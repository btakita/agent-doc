use super::*;

/// Where a reaped `do #id` directive's `### Re: ... #id` response heading
pub(crate) fn check_shadow_backlog_guard(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let content = rc.doc_content();
    let report = agent_doc_element_backlog::backlog::detect_shadow_open_items(&content)?;
    if !report.shadow_only.is_empty() {
        return Ok(GuardResult::Error(format!(
            "[session-check] INTERRUPTED: open backlog item(s) exist only outside live agent:backlog: {}. Re-run preflight/repair after restoring them to the live backlog or marking them complete",
            format_shadow_refs(&report.shadow_only)
        )));
    }
    if !report.duplicated_in_live_backlog.is_empty() {
        return Ok(GuardResult::Warn(vec![format!(
            "[session-check] warning: open backlog item(s) also appear outside live agent:backlog: {}",
            format_shadow_refs(&report.duplicated_in_live_backlog)
        )]));
    }
    Ok(GuardResult::None)
}

pub(crate) fn format_shadow_refs(
    items: &[agent_doc_element_backlog::backlog::ShadowPendingItem],
) -> String {
    items
        .iter()
        .map(agent_doc_element_backlog::backlog::ShadowPendingItem::reference)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn check_malformed_tracked_item_guard(
    _file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    let content = rc.doc_content();
    let components = rc.components();
    let refs = malformed_tracked_item_reference_strings(
        agent_doc_element_backlog::backlog::malformed_tracked_item_refs_in_components(
            &content,
            &components,
        ),
        None,
    );
    if refs.is_empty() {
        return Ok(GuardResult::None);
    }

    Ok(GuardResult::Error(
        agent_doc_element_backlog::backlog::malformed_tracked_item_interruption_message(&refs),
    ))
}

/// Session-check adapter over focused malformed tracked-work syntax policy.
/// Phase 6 (#lr-content-6) lets `inspect`'s guard read content and components
/// from cached graph slots; the response filter is closeout-specific turn
/// context and intentionally stays outside the backlog element crate.
fn malformed_tracked_item_reference_strings(
    refs: impl IntoIterator<Item = agent_doc_element_backlog::backlog::MalformedTrackedItemRef>,
    completed_by_response: Option<&str>,
) -> Vec<String> {
    refs.into_iter()
        .filter(|item| {
            completed_by_response
                .map(|response| {
                    agent_doc_turn::closeout_signal::response_clearly_completes_pending_id(
                        response,
                        &item.item.id,
                    )
                })
                .unwrap_or(true)
        })
        .map(|item| item.reference())
        .collect::<Vec<_>>()
}

pub(crate) fn check_backlog_replay_guard(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let current_content = rc.doc_content();

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let hash = crate::snapshot::doc_hash(&canonical).unwrap_or_default();
    let baseline_content = agent_doc_fs::find_project_root(&canonical)
        .map(|root| root.join(format!(".agent-doc/baselines/{}.md", hash)))
        .and_then(|p| std::fs::read_to_string(p).ok());

    let baseline = match baseline_content {
        Some(content) => content,
        None => match rc.head_content() {
            Some(content) => content.to_string(),
            None => return Ok(GuardResult::None),
        },
    };

    let resolved_ids = crate::cycle_state::resolved_pending_ids(file)?;

    let external_done_ids = crate::preflight::external_done_archive_ids(file, &current_content)?;
    let report =
        agent_doc_element_backlog::backlog::detect_dropped_from_history_with_extra_current_ids(
            &current_content,
            &baseline,
            &resolved_ids,
            &external_done_ids,
        )?;

    if !report.dropped.is_empty() {
        let refs = report
            .dropped
            .iter()
            .map(agent_doc_element_backlog::backlog::DroppedBacklogItem::reference)
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(GuardResult::Error(format!(
            "[session-check] INTERRUPTED: open backlog item(s) from recent history are completely absent from the document: {}. Restore them to the live backlog, move them to icebox, or mark them done",
            refs
        )));
    }

    Ok(GuardResult::None)
}

use super::*;

/// Where a reaped `do #id` directive's `### Re: ... #id` response heading
pub(crate) fn check_shadow_backlog_guard(_file: &Path, rc: &crate::graph::RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let content = rc.doc_content();
    let report = crate::pending::detect_shadow_open_items(&content)?;
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

pub(crate) fn format_shadow_refs(items: &[crate::pending::ShadowPendingItem]) -> String {
    items
        .iter()
        .map(crate::pending::ShadowPendingItem::reference)
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
    let refs = malformed_tracked_item_refs_in(&content, &components, None);
    if refs.is_empty() {
        return Ok(GuardResult::None);
    }

    Ok(GuardResult::Error(malformed_tracked_item_message(&refs)))
}

pub fn malformed_tracked_item_refs(
    file: &Path,
    completed_by_response: Option<&str>,
) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    let Ok(components) = crate::component::parse(&content) else {
        return Ok(Vec::new());
    };
    Ok(malformed_tracked_item_refs_in(
        &content,
        &components,
        completed_by_response,
    ))
}

/// Shared malformed-item detection over already-read content + parsed
/// components. Phase 6 (#lr-content-6) lets `inspect`'s guard read these from
/// the cached graph slots while external callers still pass a freshly read
/// document.
pub(crate) fn malformed_tracked_item_refs_in(
    content: &str,
    components: &[crate::component::Component],
    completed_by_response: Option<&str>,
) -> Vec<String> {
    components
        .iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let name = component.name.clone();
            crate::pending::detect_malformed_item_lines(component.content(content))
                .into_iter()
                .map(move |item| (name.clone(), item))
        })
        .filter(|(_, item)| {
            completed_by_response
                .map(|response| response_clearly_completes_pending_id(response, &item.id))
                .unwrap_or(true)
        })
        .map(|(name, item)| format!("{} {}", name, item.reference()))
        .collect::<Vec<_>>()
}

pub fn malformed_tracked_item_message(refs: &[String]) -> String {
    format!(
        "[session-check] INTERRUPTED: malformed tracked checklist item(s) in live backlog/icebox: {}. Repair the checklist prefix before closeout so pending guards can prove the item state",
        refs.join("; ")
    )
}

pub(crate) fn check_backlog_replay_guard(file: &Path, rc: &crate::graph::RunContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let current_content = rc.doc_content();

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let hash = crate::snapshot::doc_hash(&canonical).unwrap_or_default();
    let baseline_content = crate::snapshot::find_project_root(&canonical)
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
    let report = crate::pending::detect_dropped_from_history_with_extra_current_ids(
        &current_content,
        &baseline,
        &resolved_ids,
        &external_done_ids,
    )?;

    if !report.dropped.is_empty() {
        let refs = report
            .dropped
            .iter()
            .map(crate::pending::DroppedBacklogItem::reference)
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(GuardResult::Error(format!(
            "[session-check] INTERRUPTED: open backlog item(s) from recent history are completely absent from the document: {}. Restore them to the live backlog, move them to icebox, or mark them done",
            refs
        )));
    }

    Ok(GuardResult::None)
}

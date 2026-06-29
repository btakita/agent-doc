use anyhow::{Context, Result};

use agent_doc_element::element;
use agent_doc_element::element::{is_backlog_component, is_tracked_work_component};

/// `#staleshow` — stable sentinel line inserted into the `<!-- agent:status -->`
/// component whenever the live route-owned supervisor/controller serving the
/// document is mapping a STALE agent-doc binary.
pub const STALE_SUPERVISOR_STATUS_MARKER: &str = "🔴 (restart/recycle your supervisor)";

fn first_live_backlog_id(content: &str) -> Result<Option<String>> {
    let components = element::parse(content).context("failed to parse components")?;
    for comp in components {
        if !is_backlog_component(&comp.name) {
            continue;
        }
        let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(comp.content(content));
        if let Some(item) = items
            .into_iter()
            .find(|item| !item.id.is_empty() && !item.is_done())
        {
            return Ok(Some(item.id));
        }
    }
    Ok(None)
}

fn extract_status_top_backlog_id(status: &str) -> Option<String> {
    let marker = "Top backlog item:";
    let marker_idx = status.find(marker)?;
    let after_marker = &status[marker_idx + marker.len()..];
    let hash_idx = after_marker.find('#')?;
    let after_hash = &after_marker[hash_idx + 1..];
    let id = after_hash
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect::<String>();
    if id.is_empty() { None } else { Some(id) }
}

fn replace_top_backlog_sentence(line: &str, live_id: Option<&str>) -> String {
    let marker = "Top backlog item:";
    let Some(marker_idx) = line.find(marker) else {
        return line.to_string();
    };
    let sentence_end = line[marker_idx..]
        .find('.')
        .map(|idx| marker_idx + idx + 1)
        .unwrap_or(line.len());
    let before = line[..marker_idx].trim_end();
    let after = line[sentence_end..].trim_start();
    let replacement = live_id
        .map(|id| format!("Top backlog item: #{id}."))
        .unwrap_or_else(|| "No open backlog items.".to_string());

    match (before.is_empty(), after.is_empty()) {
        (true, true) => replacement,
        (false, true) => format!("{before} {replacement}"),
        (true, false) => format!("{replacement} {after}"),
        (false, false) => format!("{before} {replacement} {after}"),
    }
}

/// Reconcile a `Top backlog item: #id.` status sentence with the live backlog.
///
/// This is intentionally narrow: free-form status text is user-editable, so we
/// only touch the generated top-backlog sentence when it names a stale id.
pub fn reconcile_top_backlog_status_content(content: &str) -> Result<Option<String>> {
    let components = element::parse(content).context("failed to parse components")?;
    let Some(status) = components.iter().find(|c| c.name == "status") else {
        return Ok(None);
    };
    if !components
        .iter()
        .any(|c| is_tracked_work_component(&c.name))
    {
        return Ok(None);
    }

    let status_body = status.content(content);
    let Some(status_id) = extract_status_top_backlog_id(status_body) else {
        return Ok(None);
    };
    let live_id = first_live_backlog_id(content)?;
    if live_id.as_deref() == Some(status_id.as_str()) {
        return Ok(None);
    }

    let mut replaced_any = false;
    let mut new_status = String::new();
    for segment in status_body.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if line.contains("Top backlog item:") {
            new_status.push_str(&replace_top_backlog_sentence(line, live_id.as_deref()));
            replaced_any = true;
        } else {
            new_status.push_str(line);
        }
        new_status.push_str(newline);
    }
    if !replaced_any {
        return Ok(None);
    }
    Ok(Some(status.replace_content(content, &new_status)))
}

/// `#staleshow` — pure insert/remove of the [`STALE_SUPERVISOR_STATUS_MARKER`]
/// line in the status component body, idempotently, given whether the live
/// supervisor is currently running a stale binary.
pub fn apply_stale_supervisor_marker(status_body: &str, is_stale: bool) -> Option<String> {
    let has_marker = status_body
        .lines()
        .any(|line| line.trim() == STALE_SUPERVISOR_STATUS_MARKER);

    if is_stale {
        if has_marker {
            return None;
        }
        let trimmed = status_body.strip_prefix('\n').unwrap_or(status_body);
        let new_body = format!("\n{STALE_SUPERVISOR_STATUS_MARKER}\n{trimmed}");
        Some(new_body)
    } else {
        if !has_marker {
            return None;
        }
        let mut new_body = String::new();
        for segment in status_body.split_inclusive('\n') {
            let line = segment.strip_suffix('\n').unwrap_or(segment);
            if line.trim() == STALE_SUPERVISOR_STATUS_MARKER {
                continue;
            }
            new_body.push_str(segment);
        }
        Some(new_body)
    }
}

/// `#staleshow` — reconcile the stale-supervisor marker in the document's status
/// component against the live staleness signal.
pub fn reconcile_stale_supervisor_status_content(
    content: &str,
    is_stale: bool,
) -> Result<Option<String>> {
    let components = element::parse(content).context("failed to parse components")?;
    let Some(status) = components.iter().find(|c| c.name == "status") else {
        return Ok(None);
    };
    let status_body = status.content(content);
    match apply_stale_supervisor_marker(status_body, is_stale) {
        Some(new_body) => Ok(Some(status.replace_content(content, &new_body))),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciles_stale_top_backlog_status_to_live_head() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Logged follow-ups. Top backlog item: #old.\n",
            "<!-- /agent:status -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#new] New head\n",
            "<!-- /agent:backlog -->\n"
        );
        let out = reconcile_top_backlog_status_content(doc)
            .unwrap()
            .expect("stale status should be reconciled");
        assert!(out.contains("Logged follow-ups. Top backlog item: #new."));
        assert!(!out.contains("#old"));
    }

    #[test]
    fn clears_stale_top_backlog_status_when_backlog_is_empty() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Top backlog item: #old.\n",
            "<!-- /agent:status -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        let out = reconcile_top_backlog_status_content(doc)
            .unwrap()
            .expect("stale status should be cleared");
        assert!(out.contains("No open backlog items."));
        assert!(!out.contains("#old"));
    }

    #[test]
    fn preserves_status_when_top_backlog_is_current() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Top backlog item: #live.\n",
            "<!-- /agent:status -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#live] Live head\n",
            "<!-- /agent:backlog -->\n"
        );
        assert!(reconcile_top_backlog_status_content(doc).unwrap().is_none());
    }

    #[test]
    fn inserts_stale_supervisor_marker_when_stale() {
        let body = "\nSession ready. Top backlog item: #live.\n";
        let out = apply_stale_supervisor_marker(body, true)
            .expect("marker should be inserted when stale");
        assert!(out.contains(STALE_SUPERVISOR_STATUS_MARKER));
        assert_eq!(
            out.lines().find(|l| !l.trim().is_empty()),
            Some(STALE_SUPERVISOR_STATUS_MARKER)
        );
        assert!(out.contains("Session ready. Top backlog item: #live."));
    }

    #[test]
    fn does_not_duplicate_stale_supervisor_marker_on_repeat() {
        let body = "\nSession ready.\n";
        let once = apply_stale_supervisor_marker(body, true).expect("first insert");
        assert!(apply_stale_supervisor_marker(&once, true).is_none());
        assert_eq!(once.matches(STALE_SUPERVISOR_STATUS_MARKER).count(), 1);
    }

    #[test]
    fn removes_stale_supervisor_marker_when_fresh() {
        let body = format!("\n{STALE_SUPERVISOR_STATUS_MARKER}\nSession ready.\n");
        let out = apply_stale_supervisor_marker(&body, false)
            .expect("marker should be removed when fresh");
        assert!(!out.contains(STALE_SUPERVISOR_STATUS_MARKER));
        assert!(out.contains("Session ready."));
    }

    #[test]
    fn no_change_removing_marker_when_already_fresh() {
        let body = "\nSession ready.\n";
        assert!(apply_stale_supervisor_marker(body, false).is_none());
    }

    #[test]
    fn reconcile_inserts_and_removes_stale_supervisor_marker_in_document() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Session ready.\n",
            "<!-- /agent:status -->\n"
        );
        let stale = reconcile_stale_supervisor_status_content(doc, true)
            .unwrap()
            .expect("stale should insert marker");
        assert!(stale.contains(STALE_SUPERVISOR_STATUS_MARKER));
        assert!(stale.contains("Session ready."));
        assert!(
            reconcile_stale_supervisor_status_content(&stale, true)
                .unwrap()
                .is_none()
        );
        let fresh = reconcile_stale_supervisor_status_content(&stale, false)
            .unwrap()
            .expect("fresh should clear marker");
        assert!(!fresh.contains(STALE_SUPERVISOR_STATUS_MARKER));
        assert!(fresh.contains("Session ready."));
    }

    #[test]
    fn reconcile_is_noop_without_status_component() {
        let doc = "## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
        assert!(
            reconcile_stale_supervisor_status_content(doc, true)
                .unwrap()
                .is_none()
        );
    }
}

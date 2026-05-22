//! CLI-driven status component mutations for `agent-doc write --status`.
//!
//! Replaces the content of `<!-- agent:status -->` with the provided text.

use anyhow::{Context, Result};
use std::path::Path;

use crate::component;
use crate::component::{is_backlog_component, is_tracked_work_component};

fn find_status_component(file: &Path) -> Result<(String, component::Component)> {
    let content = std::fs::read_to_string(file).context("failed to read document")?;
    let components = component::parse(&content).context("failed to parse components")?;
    let comp = components
        .into_iter()
        .find(|c| c.name == "status")
        .context("document has no status component")?;
    Ok((content, comp))
}

/// Replace the status component content with the provided text.
pub fn set(file: &Path, text: &str) -> Result<()> {
    let (full_content, comp) = find_status_component(file)?;
    let new_content = format!("\n{}\n", text);
    let new_doc = comp.replace_content(&full_content, &new_content);
    std::fs::write(file, &new_doc)?;
    Ok(())
}

fn first_live_backlog_id(content: &str) -> Result<Option<String>> {
    let components = component::parse(content).context("failed to parse components")?;
    for comp in components {
        if !is_backlog_component(&comp.name) {
            continue;
        }
        let (_, items, _) = crate::pending::parse_items(comp.content(content));
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
pub(crate) fn reconcile_top_backlog_status_content(content: &str) -> Result<Option<String>> {
    let components = component::parse(content).context("failed to parse components")?;
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
}

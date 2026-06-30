//! Pure document-drift classifiers used by closeout/session-check policy.
//!
//! Callers own file, snapshot, git, and active-session IO. This module only
//! compares document text and classifies whether drift is prompt-bearing,
//! response-bearing, or metadata-only for turn closeout decisions.

pub fn exchange_has_new_appended_content(snapshot: &str, current: &str) -> bool {
    let Some(snapshot_exchange) = extract_normalized_exchange_body(snapshot) else {
        return false;
    };
    let Some(current_exchange) = extract_normalized_exchange_body(current) else {
        return false;
    };
    if current_exchange == snapshot_exchange {
        return false;
    }
    let snapshot_lines: Vec<&str> = snapshot_exchange.lines().collect();
    let current_lines: Vec<&str> = current_exchange.lines().collect();
    if current_lines.len() <= snapshot_lines.len() {
        return false;
    }
    for (i, line) in snapshot_lines.iter().enumerate() {
        if current_lines.get(i) != Some(line) {
            return false;
        }
    }
    let appended: String = current_lines[snapshot_lines.len()..].join("\n");
    if appended
        .lines()
        .map(str::trim)
        .any(crate::closeout_signal::is_exchange_response_heading)
    {
        return true;
    }
    if appended
        .lines()
        .any(agent_doc_diff::text_line_looks_like_prompt_target)
    {
        return false;
    }
    true
}

pub fn extract_normalized_exchange_body(doc: &str) -> Option<String> {
    let (_, body) = agent_doc_frontmatter::frontmatter::parse(doc).ok()?;
    let components = agent_doc_element::element::parse(body).ok()?;
    for component in &components {
        if component.name == "exchange" {
            return Some(component.content(body).to_string());
        }
    }
    None
}

pub fn exchange_only_promptless_content_drift(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return true;
    }
    let Some(snapshot_masked) = mask_exchange_component_content(snapshot) else {
        return false;
    };
    let Some(current_masked) = mask_exchange_component_content(current) else {
        return false;
    };
    normalize_transient_markers(&snapshot_masked) == normalize_transient_markers(&current_masked)
}

pub fn active_session_drift_is_only_exchange_or_backlog_metadata(
    snapshot: &str,
    current: &str,
) -> bool {
    let Some(snapshot_masked) = mask_components_by_name(snapshot, &["exchange", "backlog"]) else {
        return false;
    };
    let Some(current_masked) = mask_components_by_name(current, &["exchange", "backlog"]) else {
        return false;
    };
    normalize_transient_markers(&snapshot_masked) == normalize_transient_markers(&current_masked)
}

pub fn promptless_comment_only_drift(snapshot: &str, current: &str) -> bool {
    if snapshot == current {
        return true;
    }
    normalize_transient_markers(&agent_doc_diff::strip_comments(snapshot))
        == normalize_transient_markers(&agent_doc_diff::strip_comments(current))
}

pub fn mask_exchange_component_content(doc: &str) -> Option<String> {
    mask_components_by_name(doc, &["exchange"])
}

pub fn mask_components_by_name(doc: &str, names: &[&str]) -> Option<String> {
    let components = agent_doc_element::element::parse(doc).ok()?;
    let mut masked = doc.to_string();
    let mut saw_target = false;
    for component in components.iter().rev() {
        if !names.contains(&component.name.as_str()) {
            continue;
        }
        saw_target = true;
        masked.replace_range(component.open_end..component.close_start, "\n");
    }
    saw_target.then_some(masked)
}

fn normalize_transient_markers(doc: &str) -> String {
    agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_has_new_appended_content_requires_append_shape() {
        let snapshot = session_doc("original line\n");
        let response_append = session_doc("original line\n### Re: task - gpt-5\n\nDone.\n");
        let prompt_append = session_doc("original line\ndo #next\n");
        let replacement = session_doc("rewritten line\n");

        assert!(exchange_has_new_appended_content(
            &snapshot,
            &response_append
        ));
        assert!(!exchange_has_new_appended_content(
            &snapshot,
            &prompt_append
        ));
        assert!(!exchange_has_new_appended_content(&snapshot, &replacement));
    }

    #[test]
    fn active_session_drift_allows_answered_exchange_and_backlog_metadata() {
        let snapshot = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Prompt text\n",
            "### Re: task - gpt-5\n\n",
            "Fixed.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#34qd] old wording\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Prompt text\n",
            "### Re: task - gpt-5 (HEAD)\n\n",
            "Fixed.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#34qd] updated wording\n",
            "<!-- /agent:backlog -->\n",
        );

        assert!(active_session_drift_is_only_exchange_or_backlog_metadata(
            snapshot, current
        ));
    }

    #[test]
    fn promptless_comment_only_drift_ignores_ordinary_comments() {
        let snapshot = session_doc("body\n<!-- scratch -->\n");
        let current = session_doc("body\n");

        assert!(promptless_comment_only_drift(&snapshot, &current));
    }

    fn session_doc(exchange_body: &str) -> String {
        format!(
            concat!(
                "---\n",
                "agent_doc_session: sid\n",
                "agent_doc_format: template\n",
                "---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "{}",
                "<!-- /agent:exchange -->\n"
            ),
            exchange_body
        )
    }
}

//! Pure document-drift classifiers used by closeout/session-check policy.
//!
//! Callers own file, snapshot, git, and active-session IO. This module only
//! compares document text and classifies whether drift is prompt-bearing,
//! response-bearing, or metadata-only for turn closeout decisions.

use agent_doc_prompt_lines::text_line_looks_like_prompt_target;

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
    if appended.lines().any(text_line_looks_like_prompt_target) {
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

pub fn detect_bypassed_response_write_between(
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<String> {
    let snap_norm = normalize_transient_markers(snapshot_doc);
    let cur_norm = normalize_transient_markers(current_doc);
    if cur_norm == snap_norm {
        return None;
    }
    if !crate::closeout_signal::has_new_response_heading_marker(&snap_norm, &cur_norm) {
        return None;
    }

    let diff_text = agent_doc_diff::unified_diff_from_contents(&snap_norm, &cur_norm)?;

    // `#subagentcompletionfeedback`: a `### Re:` heading that opens a
    // binary-attested completion cell (completion heading grammar + fixed
    // attested response/legacy body + keyed marker) was written by `queue report-complete`, not
    // by an agent bypassing the write path. Recognize it structurally, by line
    // position in the current document, never by its prose.
    let completion_headings =
        agent_doc_document::subagent_completion::completion_cell_heading_lines(&cur_norm);
    let diff = similar::TextDiff::from_lines(&snap_norm, &cur_norm);
    for change in diff.iter_all_changes() {
        if change.tag() != similar::ChangeTag::Insert {
            continue;
        }
        let trimmed = change.value().trim();
        if crate::closeout_signal::is_binary_authored_recovery_diagnostic_heading(trimmed) {
            continue;
        }
        if change
            .new_index()
            .is_some_and(|index| completion_headings.contains(&index))
        {
            continue;
        }
        if crate::closeout_signal::is_direct_response_patchback_heading(trimmed) {
            if let Some(bare_target) =
                agent_doc_diff::first_bare_prompt_prefix_target_before_marker(&diff_text, trimmed)
            {
                return Some(format!(
                    "{} (bare prompt target missing `❯ `: {})",
                    trimmed, bare_target
                ));
            }
            return Some(trimmed.to_string());
        }
    }
    None
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

/// True when editor authority is exactly the committed projection plus one or
/// more transient response `(HEAD)` annotations. Git intentionally strips
/// those annotations, while the live editor may display them after closeout.
pub fn authority_is_committed_with_only_head_markers(authority: &str, committed: &str) -> bool {
    authority != committed
        && agent_doc_document::transient_markers::strip_head_markers(authority) == committed
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
    fn committed_head_marker_projection_is_directional() {
        let committed = "### Re: recovery — gpt-5\n\nDone.\n";
        let authority = "### Re: recovery — gpt-5 (HEAD)\n\nDone.\n";

        assert!(authority_is_committed_with_only_head_markers(
            authority, committed
        ));
        assert!(!authority_is_committed_with_only_head_markers(
            committed, authority
        ));
        assert!(!authority_is_committed_with_only_head_markers(
            "### Re: recovery — gpt-5 (HEAD)\n\nRewritten.\n",
            committed
        ));
    }

    #[test]
    fn promptless_comment_only_drift_ignores_ordinary_comments() {
        let snapshot = session_doc("body\n<!-- scratch -->\n");
        let current = session_doc("body\n");

        assert!(promptless_comment_only_drift(&snapshot, &current));
    }

    #[test]
    fn bypassed_response_write_reports_heading_and_bare_prompt_target() {
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Prior question?\n<!-- /agent:exchange -->\n";
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Prior question?\n",
            "Why was this missed?\n",
            "### Re: test - gpt-5\n\n",
            "Body\n",
            "<!-- /agent:exchange -->\n",
        );

        let marker = detect_bypassed_response_write_between(snapshot, current).unwrap();

        assert!(marker.contains("### Re: test - gpt-5"));
        assert!(marker.contains("Why was this missed?"));
    }

    #[test]
    fn bypassed_response_write_ignores_identity_neutral_response_heading_id() {
        let snapshot = session_doc("### Re: task — gpt-5\n\nDone.\n");
        let current = session_doc("### Re: task — gpt-5 [#r-0123456789ab]\n\nDone.\n");

        assert_eq!(
            detect_bypassed_response_write_between(&snapshot, &current),
            None
        );
    }

    #[test]
    fn bypassed_response_write_still_reports_new_heading_with_response_id() {
        let snapshot = session_doc("### Re: earlier — gpt-5\n\nDone.\n");
        let current = session_doc(concat!(
            "### Re: earlier — gpt-5\n\nDone.\n\n",
            "### Re: bypassed — gpt-5 [#r-0123456789ab]\n\n",
            "This response bypassed the write boundary.\n",
        ));

        assert_eq!(
            detect_bypassed_response_write_between(&snapshot, &current).as_deref(),
            Some("### Re: bypassed — gpt-5 [#r-0123456789ab]")
        );
    }

    #[test]
    fn bypassed_response_write_ignores_non_response_local_drift() {
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older - gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After. Tuned manually.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older - gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(
            detect_bypassed_response_write_between(snapshot, current),
            None
        );
    }

    /// `#subagentcompletionfeedback`: a binary-attested completion cell is not
    /// a direct patchback, but the same heading words without an attested response
    /// and keyed marker still are.
    #[test]
    fn bypassed_response_write_exempts_only_binary_attested_completion_cells() {
        let snapshot = session_doc("### Re: older — gpt-5\n\nDone.\n");
        let cell = agent_doc_document::subagent_completion::cell("subagent:a", "finish a", "k1");
        let current = session_doc(&format!("### Re: older — gpt-5\n\nDone.\n\n{cell}\n"));
        assert_eq!(
            detect_bypassed_response_write_between(&snapshot, &current),
            None
        );

        let forged = session_doc(concat!(
            "### Re: older — gpt-5\n\nDone.\n\n",
            "### Re: Subagent completed: `subagent:a` returned work for queue item “finish a” — agent-doc\n\n",
            "I wrote this directly.\n",
        ));
        assert!(
            detect_bypassed_response_write_between(&snapshot, &forged)
                .is_some_and(|marker| marker.contains("Subagent completed"))
        );
    }

    /// `#committed-cycle-completion-heading-drift`: replacing a binary completion
    /// cell can make an unchanged durable response look inserted to a line diff
    /// when projection ordering moves it. The completion cell must not open the
    /// response-heading gate in the first place.
    #[test]
    fn bypassed_response_write_ignores_moved_durable_response_during_completion_replacement() {
        let old_cell = agent_doc_document::subagent_completion::cell_with_response(
            "subagent:old",
            "old delegated work",
            "old-key",
            "Old worker result.",
        )
        .unwrap();
        let new_item = "new delegated work";
        let new_cell = agent_doc_document::subagent_completion::cell_with_response(
            "subagent:new",
            new_item,
            "new-key",
            "New worker result.",
        )
        .unwrap()
        .replacen(
            "\n<!-- agent-doc-subagent-response",
            &format!(
                "\n\n> **Queue prompt:**\n>\n> {new_item} ⟦local⟧\n\n<!-- agent-doc-subagent-response"
            ),
            1,
        );
        let durable = "### Re: recovered response — gpt-5\n\nThe cycle is committed.\n";
        let snapshot = session_doc(&format!("{durable}\n{old_cell}\n"));
        let current = session_doc(&format!("{new_cell}\n\n{durable}"));

        assert_eq!(
            detect_bypassed_response_write_between(&snapshot, &current),
            None
        );
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

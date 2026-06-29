//! Pure exchange-tail prompt and response detection.
//!
//! Orchestration owns file IO and guard wording. This module owns the
//! string-level policy for deciding whether the visible `agent:exchange` tail is
//! an unresolved user prompt, has a response heading, or ends with a
//! prompt-only closeout tail.

fn exchange_body(doc: &str) -> Option<String> {
    let body = agent_doc_frontmatter::frontmatter::parse(doc)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| doc.to_string());
    let components = agent_doc_element::element::parse(&body).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    Some(exchange.content(&body).to_string())
}

fn boundary_tail_start(lines: &[&str]) -> usize {
    lines
        .iter()
        .rposition(|line| line.trim().starts_with("<!-- agent:boundary:"))
        .map(|idx| idx + 1)
        .unwrap_or(0)
}

/// Snapshot-independent detection of a live unresolved user prompt in
/// `agent:exchange`.
///
/// A prompt is unresolved when there is user-authored, non-comment text after
/// the latest `agent:boundary` marker and no non-queue-continuation `### Re:`
/// response heading follows it in that tail segment.
pub fn unresolved_exchange_prompt_in_content(content: &str) -> Option<String> {
    let body = exchange_body(content)?;
    let lines: Vec<&str> = body.lines().collect();
    let tail_start = boundary_tail_start(&lines);
    let tail = &lines[tail_start..];

    let first_response_idx = tail
        .iter()
        .position(|line| super::closeout_signal::is_exchange_response_heading(line.trim()));
    if let Some(idx) = first_response_idx {
        let heading = tail[idx].trim();
        if !super::closeout_signal::is_queue_continuation_response_heading(heading) {
            return None;
        }
    }
    let prompt_region = match first_response_idx {
        Some(idx) => &tail[..idx],
        None => tail,
    };

    let prompt_lines: Vec<String> = prompt_region
        .iter()
        .map(|line| line.trim())
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("<!--")
                && !line.starts_with("-->")
                && !super::closeout_signal::is_exchange_response_heading(line)
                && !agent_doc_diff::line_is_binary_authored_ipc_proof_diagnostic(line)
                && !agent_doc_diff::line_is_binary_authored_compact_summary(line)
        })
        .map(super::closeout_signal::normalized_prompt_for_match)
        .filter(|line| !line.is_empty())
        .collect();
    if prompt_lines.is_empty() {
        return None;
    }
    Some(prompt_lines.join("\n"))
}

/// True when the exchange tail after the latest boundary contains an assistant
/// response heading.
pub fn exchange_tail_has_response_heading(content: &str) -> bool {
    let Some(body) = exchange_body(content) else {
        return false;
    };
    let lines: Vec<&str> = body.lines().collect();
    let tail_start = boundary_tail_start(&lines);
    lines[tail_start..]
        .iter()
        .any(|line| super::closeout_signal::is_exchange_response_heading(line.trim()))
}

/// Detect a document whose live `agent:exchange` tail ends in a prompt-looking
/// block with no later assistant response.
pub fn prompt_only_exchange_tail(doc: &str) -> Option<String> {
    let body = exchange_body(doc)?;

    let mut in_fence: Option<&'static str> = None;
    let mut prompt_preview: Option<String> = None;
    let mut in_assistant_response = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = match in_fence {
                Some("```") => None,
                None => Some("```"),
                other => other,
            };
            continue;
        }
        if trimmed.starts_with("~~~") {
            in_fence = match in_fence {
                Some("~~~") => None,
                None => Some("~~~"),
                other => other,
            };
            continue;
        }
        if in_fence.is_some() {
            continue;
        }
        if super::closeout_signal::is_exchange_response_heading(trimmed) {
            prompt_preview = None;
            in_assistant_response = true;
            continue;
        }
        if trimmed.starts_with("<!-- agent:boundary:") || trimmed == "## User" {
            in_assistant_response = false;
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with("<!--")
            || trimmed == "(HEAD)"
            || agent_doc_diff::line_looks_like_plain_response_after_prompt(trimmed)
        {
            continue;
        }
        if agent_doc_diff::text_line_looks_like_prompt_target(trimmed) {
            if in_assistant_response && !trimmed.starts_with('❯') {
                continue;
            }
            prompt_preview.get_or_insert_with(|| {
                trimmed
                    .trim_start_matches('❯')
                    .trim()
                    .chars()
                    .take(160)
                    .collect::<String>()
            });
        }
    }
    prompt_preview
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_exchange_prompt_detects_unanswered_tail_after_boundary() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ earlier prompt\n### Re: earlier — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "What are #next-steps to complete review items?\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(
            unresolved_exchange_prompt_in_content(content).as_deref(),
            Some("What are #next-steps to complete review items?")
        );
    }

    #[test]
    fn unresolved_exchange_prompt_none_when_answered_after_boundary() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ earlier\n### Re: earlier — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "new prompt\n### Re: new prompt — gpt-5\n\nAnswered too.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(unresolved_exchange_prompt_in_content(content), None);
    }

    #[test]
    fn unresolved_exchange_prompt_none_when_tail_empty_after_boundary() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ prompt\n### Re: prompt — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(unresolved_exchange_prompt_in_content(content), None);
    }

    #[test]
    fn unresolved_exchange_prompt_unmasked_by_queue_continuation_response() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- agent:boundary:committed -->\n",
            "❯ JB Run Agent Doc on sampleorders.md stalled.\n",
            "### Re: do [#6cmx] — gpt-5\n\nI gated #6cmx.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(
            unresolved_exchange_prompt_in_content(content).as_deref(),
            Some("JB Run Agent Doc on sampleorders.md stalled.")
        );
    }

    #[test]
    fn unresolved_exchange_prompt_ignores_separated_ipc_proof_diagnostic_line() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ earlier\n### Re: earlier — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "ipc_proof_insufficient file=/tmp/session.md source=socket_ack_content patch_id=abc invariant=live_prompt_drift_after_preflight recovery=visible_repair_required\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(unresolved_exchange_prompt_in_content(content), None);
    }

    #[test]
    fn unresolved_exchange_prompt_keeps_real_prompt_mentioning_ipc_drift() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ earlier\n### Re: earlier — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "The IPC drift keeps breaking finalize — please diagnose and fix the root cause.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(
            unresolved_exchange_prompt_in_content(content).as_deref(),
            Some("The IPC drift keeps breaking finalize — please diagnose and fix the root cause."),
        );
    }

    #[test]
    fn unresolved_exchange_prompt_detects_fresh_prompt_without_boundary() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "do [#xyz]\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(
            unresolved_exchange_prompt_in_content(content).as_deref(),
            Some("do [#xyz]")
        );
    }

    #[test]
    fn exchange_tail_response_heading_detects_only_tail_heading() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: old\n\nDone.\n",
            "<!-- agent:boundary:committed -->\n",
            "new prompt\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(!exchange_tail_has_response_heading(content));

        let answered = content.replace("new prompt\n", "new prompt\n### Re: new\n");
        assert!(exchange_tail_has_response_heading(&answered));
    }

    #[test]
    fn prompt_only_exchange_tail_ignores_answered_tail_prompt() {
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do [#vt-agent-deploy]. spec-test-news-commit-push\n",
            "### Re: vt agent deploy — gpt-5\n\n",
            "Deployed and verified.\n",
            "<!-- agent:boundary:tail -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(prompt_only_exchange_tail(current), None);
    }

    #[test]
    fn prompt_only_exchange_tail_ignores_assistant_closeout_status_after_response_heading() {
        let current = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: starting dispatch — gpt-5\n\n",
            "Implemented the route/startup guard and updated the regression coverage.\n\n",
            "The push is still running after closeout and should not require a repair patchback.\n",
            "<!-- agent:boundary:tail -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(prompt_only_exchange_tail(current), None);
    }
}

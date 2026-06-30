use std::collections::HashSet;

use agent_doc_frontmatter::frontmatter;
use agent_doc_prompt_lines::text_line_looks_like_prompt_target;

pub fn first_response_heading_line(response: &str) -> Option<&str> {
    response
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("### Re:"))
}

fn normalize_replay_topic(text: &str) -> String {
    let trimmed = text.trim();
    let trimmed = trimmed
        .strip_prefix("❯ ")
        .unwrap_or(trimmed)
        .strip_prefix("### Re:")
        .unwrap_or(trimmed)
        .trim();
    let trimmed = trimmed
        .split_once(" — ")
        .map(|(topic, _)| topic)
        .unwrap_or(trimmed)
        .trim();
    let trimmed = trimmed
        .strip_prefix("do ")
        .unwrap_or(trimmed)
        .trim_start_matches('#')
        .trim();

    let mut normalized = String::new();
    let mut last_was_space = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_space = false;
        } else if !last_was_space {
            normalized.push(' ');
            last_was_space = true;
        }
    }
    normalized.trim().to_string()
}

fn line_matches_historical_prompt(line: &str, topic: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("<!--")
    {
        return false;
    }
    if !(trimmed.starts_with("❯ ")
        || trimmed.starts_with('#')
        || trimmed.starts_with("do #")
        || trimmed.starts_with("preset #"))
    {
        return false;
    }

    let normalized_line = normalize_replay_topic(trimmed);
    !normalized_line.is_empty()
        && (normalized_line == topic
            || normalized_line.contains(topic)
            || topic.contains(&normalized_line))
}

pub fn has_matching_orphan_prompt_for_committed_capture(
    doc_content: &str,
    response_heading: &str,
) -> bool {
    let topic = normalize_replay_topic(response_heading);
    if topic.is_empty() {
        return false;
    }

    let body = frontmatter::parse(doc_content)
        .map(|(_, body)| body)
        .unwrap_or(doc_content);
    let exchange = if let Ok(components) = agent_doc_element::element::parse(body) {
        components
            .iter()
            .find(|component| component.name == "exchange")
            .map(|component| component.content(body).to_string())
            .unwrap_or_else(|| body.to_string())
    } else {
        body.to_string()
    };

    let mut saw_match = false;
    for line in exchange.lines() {
        let trimmed = line.trim();
        if trimmed == response_heading.trim() {
            return false;
        }
        if saw_match && trimmed.starts_with("### Re:") {
            return false;
        }
        if line_matches_historical_prompt(trimmed, &topic) {
            saw_match = true;
        }
    }

    saw_match
}

fn wrap_template_exchange_patch(body: &str) -> String {
    let mut patch = String::from("<!-- patch:exchange -->\n");
    patch.push_str(body);
    if !body.ends_with('\n') {
        patch.push('\n');
    }
    patch.push_str("<!-- /patch:exchange -->\n");
    patch
}

pub fn extract_visible_response_patch_between(
    snapshot_doc: &str,
    current_doc: &str,
    template_mode: bool,
) -> Option<String> {
    let norm =
        |s: &str| agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(s);
    let snapshot_norm = norm(snapshot_doc);
    let current_norm = norm(current_doc);
    if current_norm == snapshot_norm
        || crate::document_drift::detect_bypassed_response_write_between(
            &snapshot_norm,
            &current_norm,
        )
        .is_none()
    {
        return None;
    }

    let diff = similar::TextDiff::from_lines(&snapshot_norm, &current_norm);
    let mut collected = String::new();
    let mut collecting = false;
    for change in diff.iter_all_changes() {
        let line = change.value();
        let trimmed = line.trim_end_matches('\n').trim();
        match change.tag() {
            similar::ChangeTag::Insert => {
                if !collecting && !crate::closeout_signal::is_exchange_response_heading(trimmed) {
                    continue;
                }
                collecting = true;
                collected.push_str(line);
            }
            similar::ChangeTag::Equal if collecting => {
                if trimmed.is_empty() {
                    collected.push_str(line);
                    continue;
                }
                if trimmed.starts_with("<!-- agent:boundary:")
                    || trimmed == "<!-- /agent:exchange -->"
                    || trimmed == "<!-- /patch:exchange -->"
                    || text_line_looks_like_prompt_target(trimmed)
                    || crate::closeout_signal::is_exchange_response_heading(trimmed)
                {
                    break;
                }
                break;
            }
            _ => {}
        }
    }

    if collected.trim().is_empty() {
        return None;
    }

    Some(if template_mode {
        wrap_template_exchange_patch(&collected)
    } else {
        collected
    })
}

/// True when the live document's `agent:exchange` already contains a `### Re:`
/// response heading whose normalized topic matches `heading` — i.e. the prompt
/// the orphan answered is already answered by a landed response.
pub fn live_exchange_answers_heading(doc_content: &str, heading: &str) -> bool {
    let target = normalize_replay_topic(heading);
    if target.is_empty() {
        return false;
    }
    let Ok(components) = agent_doc_element::element::parse(doc_content) else {
        return false;
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return false;
    };
    exchange
        .content(doc_content)
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("### Re:"))
        .any(|line| normalize_replay_topic(line) == target)
}

pub fn prompt_change_is_known_response(change_text: &str, response: &str) -> bool {
    let response_lines: HashSet<String> = normalized_response_lines(response)
        .into_iter()
        .map(|line| line.trim().trim_start_matches('❯').trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    if response_lines.is_empty() {
        return false;
    }
    change_text
        .lines()
        .map(|line| line.trim().trim_start_matches('❯').trim())
        .filter(|line| !line.is_empty())
        .all(|line| response_lines.contains(line))
}

/// Returns true if the pending response content appears to already be applied to the document.
///
/// Checks whether the document contains the response's normalized visible lines
/// as one contiguous block. This tolerates blank-line separation and transient
/// ` (HEAD)` suffixes on response headings without treating scattered matching
/// phrases elsewhere in the document as an already-applied replay.
pub fn response_already_applied(doc: &str, response: &str) -> bool {
    let response_lines = normalized_response_lines(response);
    if response_lines.is_empty() {
        return false;
    }
    let doc_lines = normalized_response_lines(doc);
    doc_lines
        .windows(response_lines.len())
        .any(|window| window == response_lines.as_slice())
}

/// Accepts the response as applied when the captured `response` had spurious
/// leading `❯ ` markers that the user has since stripped from the document.
/// Compares the response's normalized lines against the document after also
/// stripping a single leading `❯ ` from response lines.
pub fn response_already_applied_after_prefix_strip(doc: &str, response: &str) -> bool {
    let response_lines: Vec<String> = response
        .lines()
        .filter_map(normalize_response_line)
        .map(|line| {
            let trimmed = line.trim_start();
            if let Some(stripped) = trimmed.strip_prefix("❯ ") {
                let indent_len = line.len() - trimmed.len();
                format!("{}{}", &line[..indent_len], stripped)
            } else {
                line
            }
        })
        .collect();
    if response_lines.is_empty() {
        return false;
    }
    let doc_lines = normalized_response_lines(doc);
    doc_lines
        .windows(response_lines.len())
        .any(|window| window == response_lines.as_slice())
}

fn normalized_response_lines(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "> **Queue prompt:**" {
            while let Some(next) = lines.peek() {
                if next.trim_start().starts_with('>') {
                    lines.next();
                } else {
                    break;
                }
            }
            continue;
        }
        if let Some(normalized) = normalize_response_line(line) {
            out.push(normalized);
        }
    }
    out
}

fn normalize_response_line(line: &str) -> Option<String> {
    let raw = line.trim_end_matches('\r');
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!-- patch:")
        || trimmed.starts_with("<!-- /patch:")
        || trimmed.starts_with("<!-- agent:")
        || trimmed.starts_with("<!-- /agent:")
    {
        return None;
    }
    Some(strip_transient_response_head_marker(raw))
}

fn strip_transient_response_head_marker(line: &str) -> String {
    if let Some(stripped) = line.strip_suffix(" (HEAD)") {
        let trimmed = stripped.trim_start();
        let is_re_heading = trimmed.starts_with("### Re:");
        let is_bold_re_heading = trimmed.starts_with("**Re:") && trimmed.ends_with("**");
        if is_re_heading || is_bold_re_heading {
            return stripped.to_string();
        }
    }
    line.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_already_applied_tolerates_queue_prompt_echo_between_heading_and_body() {
        let captured_response = "### Re: do [#thing] — opus-4-8\n\nShipped the fix.\n";
        let materialized_doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do [#thing] — opus-4-8\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> do [#thing]\n\n",
            "Shipped the fix.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(response_already_applied(
            materialized_doc,
            captured_response
        ));

        let other_doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do [#other] — opus-4-8\n\nUnrelated.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(!response_already_applied(other_doc, captured_response));
    }

    #[test]
    fn dedup_requires_contiguous_normalized_response_block() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — opus-4-6\n",
            "Implemented in `src/agent-doc`.\n",
            "- `cargo test`\n",
            "<!-- /patch:exchange -->\n"
        );
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: topic — opus-4-6 (HEAD)\n",
            "Earlier answer.\n\n",
            "Implemented in `src/agent-doc`.\n\n",
            "Unrelated text.\n",
            "- `cargo test`\n",
            "<!-- /agent:exchange -->\n"
        );

        assert!(!response_already_applied(doc, response));
    }

    #[test]
    fn dedup_short_response_still_requires_contiguous_match() {
        let response = "Implemented.\nDone.\n";
        let doc = "Implemented.\nOther line.\nDone.\n";

        assert!(!response_already_applied(doc, response));
    }

    #[test]
    fn visible_response_patch_extracts_inserted_response_body() {
        let snapshot = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do #ship\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do #ship\n",
            "### Re: do #ship — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );

        assert_eq!(
            extract_visible_response_patch_between(snapshot, current, false),
            Some("### Re: do #ship — gpt-5\n\nDone.\n".to_string())
        );
    }

    #[test]
    fn visible_response_patch_wraps_template_exchange_patch() {
        let snapshot = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do #ship\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do #ship\n",
            "### Re: do #ship — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );

        assert_eq!(
            extract_visible_response_patch_between(snapshot, current, true),
            Some(
                concat!(
                    "<!-- patch:exchange -->\n",
                    "### Re: do #ship — gpt-5\n\n",
                    "Done.\n",
                    "<!-- /patch:exchange -->\n"
                )
                .to_string()
            )
        );
    }
}

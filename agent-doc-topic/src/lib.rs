//! # Module: topic
//!
//! ## Spec
//! - Pure text parsing of `### Re:` topic sections from an exchange component body.
//! - Splits a component body into preamble (before the first `### Re:` heading), the per-topic
//!   sections, and any trailing content after a managed `<!-- agent:boundary: -->` marker.
//! - Boundary markers are managed by the binary and are never archived, so they are stripped
//!   from the parsed output.
//!
//! ## Agentic Contracts
//! - Lives in `agent-doc-topic` so compaction/archive flows can parse topic sections through the
//!   focused crate directly.
//!
//! ## Evals
//! - parse_topic_sections_basic: `### Re:` headings split into sections
//! - parse_topic_sections_strips_boundary_marker: boundary markers excluded from sections

/// Parsed topic-section view of an exchange component body.
#[derive(Debug, Default)]
pub struct TopicSections {
    /// Content before the first `### Re:` heading.
    pub preamble: String,
    /// One entry per `### Re:` topic heading, including the heading line.
    pub sections: Vec<String>,
    /// Content after a managed `<!-- agent:boundary: -->` marker.
    pub trailing: String,
}

/// Split a component body into its preamble and per-topic sections.
pub fn parse_topic_sections(content: &str) -> (String, Vec<String>) {
    let parsed = parse_topic_sections_with_tail(content);
    (parsed.preamble, parsed.sections)
}

/// Split a component body into preamble, per-topic sections, and post-boundary trailing content.
pub fn parse_topic_sections_with_tail(content: &str) -> TopicSections {
    let mut preamble = String::new();
    let mut sections: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut found_first = false;
    let mut after_boundary = false;
    let mut trailing = String::new();

    for line in content.lines() {
        // Strip boundary markers — they are managed by the binary, not archived
        if line.starts_with("<!-- agent:boundary:") {
            after_boundary = true;
            continue;
        }
        if after_boundary {
            trailing.push_str(line);
            trailing.push('\n');
            continue;
        }

        if line.starts_with("### Re:") || line.starts_with("#### Re:") || line.starts_with("## Re:")
        {
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            found_first = true;
            current = Some(format!("{}\n", line));
        } else if found_first {
            let section = current.get_or_insert_with(String::new);
            section.push_str(line);
            section.push('\n');
        } else {
            preamble.push_str(line);
            preamble.push('\n');
        }
    }

    if let Some(last) = current {
        sections.push(last);
    }

    TopicSections {
        preamble,
        sections,
        trailing,
    }
}

const COMPACT_SUMMARY_ITEM_LIMIT: usize = 3;
const COMPACT_SUMMARY_TEXT_LIMIT: usize = 120;

pub fn summarize_compacted_exchange(exchange: &str) -> Vec<String> {
    let parsed = parse_topic_sections_with_tail(exchange);
    let mut summary = Vec::new();

    let topics: Vec<String> = parsed
        .sections
        .iter()
        .filter_map(|section| summarize_response_topic(section))
        .collect();
    if !topics.is_empty() {
        let limit = COMPACT_SUMMARY_ITEM_LIMIT.min(topics.len());
        let mut item = format!(
            "Archived {} response topic(s): {}",
            topics.len(),
            topics[..limit].join("; ")
        );
        if topics.len() > limit {
            item.push_str(&format!("; {} more", topics.len() - limit));
        }
        summary.push(item);
    }

    if let Some(preamble) = summarize_prior_preamble_context(&parsed.preamble) {
        summary.push(format!("Prior summary/context: {preamble}"));
    }

    let trailing = parsed.trailing.trim();
    if !trailing.is_empty() {
        summary.push(format!(
            "Trailing prompt/context: {}",
            truncate_with_ellipsis(&collapse_whitespace(trailing), COMPACT_SUMMARY_TEXT_LIMIT)
        ));
    }

    if summary.is_empty() {
        summarize_freeform_component(exchange.trim())
    } else {
        summary
    }
}

fn summarize_response_topic(section: &str) -> Option<String> {
    let heading = section.lines().next()?.trim();
    let heading = heading.trim_start_matches('#').trim();
    let topic = heading.strip_prefix("Re:").unwrap_or(heading).trim();
    let topic = topic.strip_suffix("(HEAD)").unwrap_or(topic).trim();
    let topic = topic.split(" — ").next().unwrap_or(topic).trim();
    if topic.is_empty() {
        None
    } else {
        Some(truncate_with_ellipsis(topic, COMPACT_SUMMARY_TEXT_LIMIT))
    }
}

fn summarize_freeform_component(body: &str) -> Vec<String> {
    let excerpt = truncate_with_ellipsis(&collapse_whitespace(body), COMPACT_SUMMARY_TEXT_LIMIT);
    if excerpt.is_empty() {
        Vec::new()
    } else {
        vec![excerpt]
    }
}

fn summarize_prior_preamble_context(preamble: &str) -> Option<String> {
    let preamble = preamble.trim();
    if preamble.is_empty() {
        return None;
    }
    let is_session_summary = preamble
        .lines()
        .any(|line| line.trim() == "### Session Summary");
    if is_session_summary && let Some(summary) = summarize_prior_compact_summary(preamble) {
        return Some(summary);
    }

    let mut selected = Vec::new();
    for line in preamble.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed == "### Session Summary"
            || trimmed == "Compacted content:"
            || trimmed.starts_with("*Compacted. Content archived to `")
            || trimmed.starts_with("- Prior summary/context:")
            || trimmed.starts_with("- Trailing prompt/context:")
            || is_markdown_ordered_item(trimmed)
            || (is_session_summary && trimmed.starts_with("- "))
        {
            continue;
        }
        let item = trimmed.trim_start_matches("- ").trim().to_string();
        if !selected.iter().any(|seen| seen == &item) {
            selected.push(item);
        }
        if selected.len() >= COMPACT_SUMMARY_ITEM_LIMIT {
            break;
        }
    }

    let excerpt = if selected.is_empty() {
        if is_session_summary {
            return None;
        }
        collapse_whitespace(preamble)
    } else {
        collapse_whitespace(&selected.join(" "))
    };
    let excerpt = truncate_with_ellipsis(&excerpt, COMPACT_SUMMARY_TEXT_LIMIT);
    (!excerpt.is_empty()).then_some(excerpt)
}

fn summarize_prior_compact_summary(preamble: &str) -> Option<String> {
    let mut in_compacted_content = false;
    let mut items = Vec::new();

    for line in preamble.lines() {
        let trimmed = line.trim();
        if trimmed == "Compacted content:" {
            in_compacted_content = true;
            continue;
        }
        if !in_compacted_content || trimmed.is_empty() {
            continue;
        }
        let Some(item) = trimmed.strip_prefix("- ").map(str::trim) else {
            continue;
        };
        if item.starts_with("Prior summary/context:")
            || item.starts_with("Trailing prompt/context:")
        {
            break;
        }
        if !items.iter().any(|seen| seen == item) {
            items.push(item.to_string());
        }
        if items.len() >= COMPACT_SUMMARY_ITEM_LIMIT {
            break;
        }
    }

    if items.is_empty() {
        return None;
    }
    Some(truncate_with_ellipsis(
        &format!("prior compacted content: {}", items.join("; ")),
        COMPACT_SUMMARY_TEXT_LIMIT,
    ))
}

fn is_markdown_ordered_item(line: &str) -> bool {
    let mut chars = line.chars().peekable();
    let mut saw_digit = false;
    while matches!(chars.peek(), Some(ch) if ch.is_ascii_digit()) {
        saw_digit = true;
        chars.next();
    }
    saw_digit && chars.next() == Some('.') && matches!(chars.next(), Some(ch) if ch.is_whitespace())
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }

    let mut out: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_topic_sections_basic() {
        let content = "intro\n### Re: a\nbody a\n### Re: b\nbody b\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert_eq!(preamble.trim(), "intro");
        assert_eq!(sections.len(), 2);
        assert!(sections[0].starts_with("### Re: a"));
        assert!(sections[1].starts_with("### Re: b"));
    }

    #[test]
    fn parse_topic_sections_strips_boundary_marker() {
        let content = "### Re: a\nbody a\n<!-- agent:boundary:abcd1234 -->\ntail\n";
        let parsed = parse_topic_sections_with_tail(content);
        assert_eq!(parsed.sections.len(), 1);
        assert!(!parsed.sections[0].contains("boundary"));
        assert_eq!(parsed.trailing.trim(), "tail");
    }

    #[test]
    fn parse_topic_sections_no_re_headings() {
        let content = "just preamble\nmore preamble\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert!(sections.is_empty());
        assert_eq!(preamble.trim(), "just preamble\nmore preamble");
    }

    #[test]
    fn compact_summary_lists_topic_limit_and_more_count() {
        let content = concat!(
            "### Re: topic one\nbody\n",
            "### Re: topic two\nbody\n",
            "### Re: topic three\nbody\n",
            "### Re: topic four\nbody\n"
        );

        assert_eq!(
            summarize_compacted_exchange(content),
            vec!["Archived 4 response topic(s): topic one; topic two; topic three; 1 more"]
        );
    }

    #[test]
    fn compact_summary_dedupes_prior_compact_lists() {
        let content = concat!(
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/previous.md`*\n\n",
            "Compacted content:\n",
            "- Archived 1 response topic(s): lender bank wire instruction reveal\n",
            "- Prior summary/context: stale nested summary\n",
            "8. **Active Funding**: wire confirmed.\n",
            "- **Active**: at least one fund row is active.\n\n",
            "### Re: new topic\nbody\n"
        );

        let summary = summarize_compacted_exchange(content);

        assert_eq!(
            summary,
            vec![
                "Archived 1 response topic(s): new topic",
                "Prior summary/context: prior compacted content: Archived 1 response topic(s): lender bank wire instruction reveal"
            ]
        );
    }

    #[test]
    fn compact_summary_reports_trailing_prompt_after_boundary() {
        let content = concat!(
            "### Re: last topic\nbody\n",
            "<!-- agent:boundary:abc123 -->\n",
            "Continue with the remaining implementation.\n"
        );

        let summary = summarize_compacted_exchange(content);

        assert_eq!(
            summary,
            vec![
                "Archived 1 response topic(s): last topic",
                "Trailing prompt/context: Continue with the remaining implementation."
            ]
        );
    }

    #[test]
    fn compact_summary_uses_prior_context_for_unstructured_preamble() {
        assert_eq!(
            summarize_compacted_exchange("loose text\nwith   whitespace\n"),
            vec!["Prior summary/context: loose text with whitespace"]
        );
        assert!(summarize_compacted_exchange(" \n\t ").is_empty());
    }
}

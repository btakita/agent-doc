//! Pure document-text helpers for compacting agent-doc exchange content.

use std::collections::BTreeMap;

use agent_doc_element::element;

/// A parsed inline exchange pair (`## User` prompt + `## Assistant` response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactExchange {
    /// The user's content without the `## User` heading.
    pub user: String,
    /// The assistant's content without the `## Assistant` heading.
    pub assistant: String,
}

/// A non-exchange component opening marker that changed across compact output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonExchangeMarkerChange {
    pub component: String,
    pub before_marker: String,
    pub after_marker: String,
}

impl NonExchangeMarkerChange {
    pub fn describe(&self) -> String {
        format!(
            "{}: `{}` -> `{}`",
            self.component, self.before_marker, self.after_marker
        )
    }
}

/// Parse an inline-mode document body into complete User/Assistant exchange pairs.
///
/// A trailing `## User` section without an assistant reply is treated as live
/// prompt text and is not returned. Headings inside fenced code blocks are
/// ignored.
pub fn parse_inline_exchanges(body: &str) -> Vec<CompactExchange> {
    let mut exchanges = Vec::new();
    let mut sections: Vec<(&str, String)> = Vec::new();

    let mut current_type = "";
    let mut current_content = String::new();
    let mut in_code_block = false;

    for line in body.lines() {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
        }

        if !in_code_block {
            if line == "## User" {
                if !current_type.is_empty() {
                    sections.push((current_type, current_content.clone()));
                }
                current_type = "user";
                current_content.clear();
                continue;
            } else if line == "## Assistant" {
                if !current_type.is_empty() {
                    sections.push((current_type, current_content.clone()));
                }
                current_type = "assistant";
                current_content.clear();
                continue;
            }
        }

        if !current_type.is_empty() {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    if !current_type.is_empty() {
        sections.push((current_type, current_content));
    }

    let mut i = 0;
    while i < sections.len() {
        if sections[i].0 == "user" {
            let user = sections[i].1.trim().to_string();
            let assistant = if i + 1 < sections.len() && sections[i + 1].0 == "assistant" {
                i += 1;
                sections[i].1.trim().to_string()
            } else {
                String::new()
            };
            if !assistant.is_empty() {
                exchanges.push(CompactExchange { user, assistant });
            }
        }
        i += 1;
    }

    exchanges
}

/// Build an inline-mode compacted document with a summary pointer, kept
/// exchanges, and a trailing `## User` prompt block.
pub fn build_inline_compacted_document(
    original: &str,
    body: &str,
    kept_exchanges: &[CompactExchange],
    archive_path: &str,
    archived_count: usize,
) -> String {
    let body_start = original.len() - body.len();
    let header = &original[..body_start];

    let mut result = header.to_string();
    result.push_str(&format!(
        "*{} earlier exchange(s) archived to `{}`*\n\n",
        archived_count, archive_path
    ));

    for exchange in kept_exchanges {
        result.push_str("## User\n\n");
        result.push_str(&exchange.user);
        result.push_str("\n\n## Assistant\n\n");
        result.push_str(&exchange.assistant);
        result.push_str("\n\n");
    }

    result.push_str("## User\n\n");
    result
}

/// Split component content around the first agent boundary marker.
///
/// Content after the boundary remains live drift in the visible document and is
/// excluded from archive/snapshot summaries.
pub fn split_component_content_at_boundary(content: &str) -> (String, String) {
    let mut before = String::new();
    let mut after = String::new();
    let mut after_boundary = false;

    for line in content.lines() {
        if line.starts_with("<!-- agent:boundary:") {
            after_boundary = true;
            continue;
        }
        if after_boundary {
            after.push_str(line);
            after.push('\n');
        } else {
            before.push_str(line);
            before.push('\n');
        }
    }

    (before, after)
}

/// Find malformed compact summary lines rendered as user prompts inside
/// `agent:exchange`.
pub fn malformed_compact_summary_lines(compacted: &str) -> Vec<String> {
    let Ok(components) = element::parse(compacted) else {
        return Vec::new();
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Vec::new();
    };
    exchange
        .content(compacted)
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix('❯')?.trim_start();
            if rest.starts_with("*Compacted") || rest.contains("Content archived to") {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Verbatim opening marker line for every non-exchange component, keyed by name.
pub fn non_exchange_opening_markers(content: &str) -> BTreeMap<String, String> {
    let mut markers = BTreeMap::new();
    let Ok(components) = element::parse(content) else {
        return markers;
    };
    for comp in &components {
        if comp.name == "exchange" {
            continue;
        }
        markers.insert(
            comp.name.clone(),
            content[comp.open_start..comp.open_end].to_string(),
        );
    }
    markers
}

/// Return surviving non-exchange components whose opening marker changed.
pub fn changed_non_exchange_opening_markers(
    before: &str,
    after: &str,
) -> Vec<NonExchangeMarkerChange> {
    let before_markers = non_exchange_opening_markers(before);
    let after_markers = non_exchange_opening_markers(after);

    before_markers
        .iter()
        .filter_map(|(component, before_marker)| {
            let after_marker = after_markers.get(component)?;
            (after_marker != before_marker).then(|| NonExchangeMarkerChange {
                component: component.clone(),
                before_marker: before_marker.clone(),
                after_marker: after_marker.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_inline_exchanges_basic() {
        let body = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\nBye\n\n## Assistant\n\nGoodbye\n\n## User\n\n";
        let exchanges = parse_inline_exchanges(body);
        assert_eq!(exchanges.len(), 2);
        assert_eq!(exchanges[0].user, "Hello");
        assert_eq!(exchanges[0].assistant, "Hi there");
        assert_eq!(exchanges[1].user, "Bye");
        assert_eq!(exchanges[1].assistant, "Goodbye");
    }

    #[test]
    fn parse_inline_exchanges_ignores_code_block_headings() {
        let body = "## User\n\nHere's code:\n\n```\n## User\n## Assistant\n```\n\n## Assistant\n\nNice code\n\n## User\n\n";
        let exchanges = parse_inline_exchanges(body);
        assert_eq!(exchanges.len(), 1);
        assert!(exchanges[0].user.contains("```"));
        assert!(exchanges[0].user.contains("## User"));
    }

    #[test]
    fn parse_inline_exchanges_skips_trailing_user_prompt() {
        let body = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\nPending question\n";
        let exchanges = parse_inline_exchanges(body);
        assert_eq!(exchanges.len(), 1);
    }

    #[test]
    fn build_inline_compacted_document_format() {
        let kept = vec![CompactExchange {
            user: "Recent question".to_string(),
            assistant: "Recent answer".to_string(),
        }];
        let compacted = build_inline_compacted_document(
            "---\ntest: true\n---\n\n",
            "\n",
            &kept,
            "archive.md",
            3,
        );
        assert!(compacted.contains("3 earlier exchange(s) archived"));
        assert!(compacted.contains("## User\n\nRecent question"));
        assert!(compacted.contains("## Assistant\n\nRecent answer"));
        assert!(compacted.ends_with("## User\n\n"));
    }

    #[test]
    fn split_component_content_preserves_tail_after_boundary() {
        let (before, after) = split_component_content_at_boundary(
            "### Re: done\n\nResponse.\n<!-- agent:boundary:abc -->\ndo next\n",
        );
        assert!(before.contains("### Re: done"));
        assert!(!before.contains("agent:boundary"));
        assert_eq!(after, "do next\n");
    }

    #[test]
    fn malformed_compact_summary_detects_prompt_prefixed_summary() {
        let doc = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ *Compacted. Content archived to `.agent-doc/archives/x.md`*\n",
            "### Re: #next-steps-2 — gpt-5\n\nLeftover archived body.\n",
            "<!-- /agent:exchange -->\n",
        );
        let malformed = malformed_compact_summary_lines(doc);
        assert_eq!(malformed.len(), 1, "{malformed:?}");
        assert!(malformed[0].contains("Compacted"), "{malformed:?}");
    }

    #[test]
    fn malformed_compact_summary_accepts_clean_summary() {
        let doc = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/x.md`*\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(malformed_compact_summary_lines(doc).is_empty());
    }

    #[test]
    fn changed_non_exchange_opening_markers_reports_surviving_marker_changes() {
        let before = concat!(
            "<!-- agent:exchange -->\nx\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue priority preset=\"#x\" go -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let after = concat!(
            "<!-- agent:exchange -->\ny\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );

        let changes = changed_non_exchange_opening_markers(before, after);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].component, "queue");
        assert!(changes[0].before_marker.contains("preset=\"#x\""));
        assert_eq!(changes[0].after_marker, "<!-- agent:queue -->\n");
    }
}

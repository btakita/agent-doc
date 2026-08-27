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

/// Return the agent boundary marker line present in component content.
///
/// Compaction rebuilds the component body from scratch, so it has to carry the
/// existing marker forward explicitly. Without it the committed projection (which
/// re-inserts a boundary at commit time) and the live editor buffer (which does
/// not) disagree, leaving the document permanently dirty by a marker-only diff
/// (`#compactboundary`).
pub fn boundary_marker_line(content: &str) -> Option<String> {
    content
        .lines()
        .find(|line| line.starts_with("<!-- agent:boundary:"))
        .map(|line| line.trim_end().to_string())
}

/// Return the sortable archive timestamp from a binary-generated compact
/// exchange summary.
///
/// This deliberately recognizes only the default `### Session Summary` plus
/// archive-pointer shape. Custom compact messages and malformed/prompt-prefixed
/// summaries are not recovery authority.
pub fn compacted_exchange_archive_timestamp(content: &str) -> Option<String> {
    let components = element::parse(content).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    let exchange_content = exchange.content(content);
    let mut non_empty_lines = exchange_content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    if non_empty_lines.next()? != "### Session Summary" {
        return None;
    }

    let archive_path = non_empty_lines.find_map(|line| {
        line.strip_prefix("*Compacted. Content archived to `")
            .and_then(|rest| rest.strip_suffix("`*"))
    })?;
    let file_name = archive_path.rsplit('/').next()?;
    let stem = file_name.strip_suffix(".md")?;
    const TIMESTAMP_LEN: usize = "YYYYMMDD-HHMMSS".len();
    let timestamp_start = stem.len().checked_sub(TIMESTAMP_LEN)?;
    if timestamp_start == 0 || stem.as_bytes().get(timestamp_start - 1) != Some(&b'-') {
        return None;
    }
    let timestamp = &stem[timestamp_start..];
    if timestamp.as_bytes().get(8) != Some(&b'-')
        || !timestamp
            .bytes()
            .enumerate()
            .all(|(index, byte)| index == 8 || byte.is_ascii_digit())
    {
        return None;
    }
    Some(timestamp.to_string())
}

/// True when `current` carries a strictly newer binary compact archive than
/// `retained` for the exchange component.
pub fn newer_compacted_exchange_supersedes(retained: &str, current: &str) -> bool {
    let Some(retained_timestamp) = compacted_exchange_archive_timestamp(retained) else {
        return false;
    };
    let Some(current_timestamp) = compacted_exchange_archive_timestamp(current) else {
        return false;
    };
    current_timestamp > retained_timestamp
}

/// Rebase a retained Compact Exchange projection over the editor's current cut.
///
/// Compaction owns the exchange prefix before `agent:boundary`; the editor owns
/// the live tail after it and every sibling component. A normal component-level
/// three-way merge treats an editor cut inside the soon-to-be-compacted prefix as
/// a conflict, even though that prefix is intentionally replaced by the compact
/// summary. Rebuild only that ownership split and leave the rest of the current
/// editor projection byte-for-byte intact.
///
/// `None` means the inputs do not prove this narrow compact lineage. Callers must
/// retain the write or use their normal conflict policy instead of guessing.
pub fn rebase_retained_compact_exchange_over_editor_cut(
    merge_base: &str,
    retained_target: &str,
    editor_cut: &str,
) -> Option<String> {
    let retained_timestamp = compacted_exchange_archive_timestamp(retained_target)?;
    if compacted_exchange_archive_timestamp(editor_cut)
        .is_some_and(|current_timestamp| current_timestamp > retained_timestamp)
    {
        return Some(editor_cut.to_string());
    }

    let base_components = element::parse(merge_base).ok()?;
    let retained_components = element::parse(retained_target).ok()?;
    let editor_components = element::parse(editor_cut).ok()?;
    let base_exchange = base_components
        .iter()
        .find(|component| component.name == "exchange")?;
    let retained_exchange = retained_components
        .iter()
        .find(|component| component.name == "exchange")?;
    let editor_exchange = editor_components
        .iter()
        .find(|component| component.name == "exchange")?;
    if retained_exchange.attrs != base_exchange.attrs
        || editor_exchange.attrs != base_exchange.attrs
    {
        return None;
    }

    let base_content = base_exchange.content(merge_base);
    let retained_content = retained_exchange.content(retained_target);
    let editor_content = editor_exchange.content(editor_cut);
    boundary_marker_line(base_content)?;
    boundary_marker_line(retained_content)?;
    let editor_boundary = boundary_marker_line(editor_content)?;
    let (mut retained_prefix, _) = split_component_content_at_boundary(retained_content);
    let (_, editor_tail) = split_component_content_at_boundary(editor_content);
    if !retained_prefix.ends_with('\n') {
        retained_prefix.push('\n');
    }
    retained_prefix.push_str(&editor_boundary);
    retained_prefix.push('\n');
    retained_prefix.push_str(&editor_tail);

    Some(editor_exchange.replace_content(editor_cut, &retained_prefix))
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

    fn compacted_doc(timestamp: &str, suffix: &str) -> String {
        format!(
            concat!(
                "# Session\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted. Content archived to `.agent-doc/archives/doc-hash-{}.md`*\n\n",
                "{}",
                "<!-- /agent:exchange -->\n",
            ),
            timestamp, suffix
        )
    }

    #[test]
    fn newer_binary_compact_summary_supersedes_older_summary() {
        let older = compacted_doc("20260717-225006", "old trailing prompt\n");
        let newer = compacted_doc("20260717-225039", "new trailing prompt\n");

        assert_eq!(
            compacted_exchange_archive_timestamp(&older).as_deref(),
            Some("20260717-225006")
        );
        assert!(newer_compacted_exchange_supersedes(&older, &newer));
        assert!(!newer_compacted_exchange_supersedes(&newer, &older));
        assert!(!newer_compacted_exchange_supersedes(&newer, &newer));
    }

    #[test]
    fn retained_compact_rebase_owns_prefix_and_preserves_editor_tail_and_siblings() {
        let base = concat!(
            "# Session\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "## User\n\nquestion\n\n## Assistant\n\nanswer line one\nanswer line two\n",
            "<!-- agent:boundary:base -->\n",
            "queued before compact\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:backlog -->\n- version 1\n<!-- /agent:backlog -->\n",
        );
        let retained = concat!(
            "# Session\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/doc-20260827-231500.md`*\n",
            "<!-- agent:boundary:base -->\n",
            "queued before compact\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:backlog -->\n- version 1\n<!-- /agent:backlog -->\n",
        );
        let editor = concat!(
            "# Session\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "## User\n\nquestion\n\n## Assistant\n\nanswer line one\n",
            "<!-- agent:boundary:editor -->\n",
            "new queued editor text\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:backlog -->\n- version 2\n<!-- /agent:backlog -->\n",
        );

        let rebased = rebase_retained_compact_exchange_over_editor_cut(base, retained, editor)
            .expect("the retained compact owns the pre-boundary exchange prefix");
        assert!(rebased.contains("### Session Summary"));
        assert!(!rebased.contains("answer line one"));
        assert!(rebased.contains("<!-- agent:boundary:editor -->\nnew queued editor text\n"));
        assert!(rebased.contains("- version 2"));
        assert!(!rebased.contains("- version 1"));
    }

    #[test]
    fn retained_compact_rebase_keeps_a_newer_visible_compact() {
        let older = compacted_doc("20260717-225006", "<!-- agent:boundary:x -->\nold tail\n");
        let newer = compacted_doc("20260717-225039", "<!-- agent:boundary:y -->\nnew tail\n");

        assert_eq!(
            rebase_retained_compact_exchange_over_editor_cut(&older, &older, &newer),
            Some(newer)
        );
    }

    #[test]
    fn compact_supersession_rejects_custom_or_malformed_summaries() {
        let older = compacted_doc("20260717-225006", "");
        let custom = older.replace("### Session Summary", "### Custom summary");
        let malformed = older.replace("doc-hash-20260717-225006.md", "doc-hash-latest.md");

        assert!(compacted_exchange_archive_timestamp(&custom).is_none());
        assert!(compacted_exchange_archive_timestamp(&malformed).is_none());
        assert!(!newer_compacted_exchange_supersedes(&older, &custom));
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

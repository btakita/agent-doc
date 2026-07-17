//! Pure normalization for transient agent-doc document markers.
//!
//! These helpers operate only on document text. Git staging, realtime write
//! policy, preflight, and session checks use them to compare durable content
//! while ignoring boundary markers, `(HEAD)` annotations, guard comments, and
//! managed pipeline frontmatter.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::{HashMap, HashSet};

pub fn strip_boundary_markers(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn code_block_byte_ranges(content: &str) -> Vec<std::ops::Range<usize>> {
    let parser = Parser::new_ext(content, Options::empty()).into_offset_iter();
    let mut ranges = Vec::new();
    let mut start: Option<usize> = None;
    for (event, range) in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                start = Some(range.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(s) = start.take() {
                    ranges.push(s..range.end);
                }
            }
            _ => {}
        }
    }
    ranges
}

fn is_in_code_block(ranges: &[std::ops::Range<usize>], offset: usize) -> bool {
    ranges.iter().any(|r| r.contains(&offset))
}

/// Strip ` (HEAD)` suffix from markdown heading lines and bold-text pseudo-headers.
pub fn strip_head_markers(content: &str) -> String {
    let code_ranges = code_block_byte_ranges(content);
    let mut result_lines: Vec<&str> = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !is_in_code_block(&code_ranges, offset)
            && let Some(stripped) = line.strip_suffix(" (HEAD)")
        {
            if trimmed.starts_with('#') {
                result_lines.push(stripped);
                offset += line.len() + 1;
                continue;
            }
            let without_suffix = stripped.trim_end();
            if trimmed.starts_with("**") && without_suffix.trim_start().ends_with("**") {
                result_lines.push(stripped);
                offset += line.len() + 1;
                continue;
            }
        }
        result_lines.push(line);
        offset += line.len() + 1;
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    }
}

/// Strip per-cycle guard suppression markers from durable comparisons/commits.
pub fn strip_guard_markers(content: &str) -> String {
    const MARKERS: &[&str] = &[
        "<!-- no-pending-capture -->",
        "<!-- no-pending-done-guard -->",
    ];
    let mut result_lines: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if MARKERS.contains(&trimmed) {
            continue;
        }
        if MARKERS.iter().any(|m| line.contains(m)) {
            let mut cleaned = line.to_string();
            for marker in MARKERS {
                cleaned = cleaned.replace(marker, "");
            }
            result_lines.push(cleaned.trim_end().to_string());
        } else {
            result_lines.push(line.to_string());
        }
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    }
}

pub fn normalize_transient_agent_doc_markers(content: &str) -> String {
    agent_doc_frontmatter::frontmatter::strip_pipeline_block_lines(&strip_guard_markers(
        &strip_head_markers(&strip_exchange_active_prompt_markers(
            &strip_boundary_markers(content),
        )),
    ))
}

fn strip_active_prompt_marker_from_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent_len = line.len().saturating_sub(trimmed.len());
    if let Some(rest) = trimmed.strip_prefix("❯ 🚧 ") {
        return format!("{}❯ {}", &line[..indent_len], rest);
    }

    let heading_level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&heading_level)
        && trimmed.as_bytes().get(heading_level) == Some(&b' ')
        && let Some(rest) = trimmed[heading_level + 1..].strip_prefix("❯ 🚧 ")
    {
        return format!(
            "{}{} ❯ {}",
            &line[..indent_len],
            &trimmed[..heading_level],
            rest
        );
    }
    line.to_string()
}

/// Remove the cosmetic active-work marker only from exchange prompt prose.
/// Fenced regions are excluded so literal examples remain byte-for-byte stable.
pub fn strip_exchange_active_prompt_markers(content: &str) -> String {
    fn strip_lines(content: &str) -> String {
        let code_ranges = code_block_byte_ranges(content);
        let mut stripped = String::with_capacity(content.len());
        let mut offset = 0usize;
        for segment in content.split_inclusive('\n') {
            let (line, newline) = segment
                .strip_suffix('\n')
                .map(|line| (line, "\n"))
                .unwrap_or((segment, ""));
            if is_in_code_block(&code_ranges, offset) {
                stripped.push_str(line);
            } else {
                stripped.push_str(&strip_active_prompt_marker_from_line(line));
            }
            stripped.push_str(newline);
            offset += segment.len();
        }
        stripped
    }

    let Ok(components) = agent_doc_element::element::parse(content) else {
        return content.to_string();
    };
    let mut rebuilt = String::with_capacity(content.len());
    let mut last = 0usize;
    for component in components {
        if component.open_end < last {
            continue;
        }
        rebuilt.push_str(&content[last..component.open_end]);
        if component.name == "exchange" {
            rebuilt.push_str(&strip_lines(component.content(content)));
        } else {
            rebuilt.push_str(component.content(content));
        }
        rebuilt.push_str(&content[component.close_start..component.close_end]);
        last = component.close_end;
    }
    rebuilt.push_str(&content[last..]);
    rebuilt
}

/// Replace the `agent:queue` component with a canonical empty placeholder for
/// replay-hash comparisons.
pub fn neutralize_queue_component(content: &str) -> String {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return content.to_string();
    };
    let Some(queue) = components.iter().find(|c| c.name == "queue") else {
        return content.to_string();
    };
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..queue.open_start]);
    out.push_str("<!-- agent:queue -->\n<!-- /agent:queue -->");
    out.push_str(&content[queue.close_end..]);
    out
}

/// Drop transient queue activation frontmatter for replay-hash comparisons.
pub fn strip_queue_active_frontmatter(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !t.starts_with("queue_active:") && !t.starts_with("queue:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Drop only the deprecated `queue_active:` mirror while preserving the
/// canonical operator-owned `queue:` control. Drift guards use this narrower
/// normalization so a real go/stop edit remains visible.
pub fn strip_legacy_queue_active_frontmatter(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim_start().starts_with("queue_active:"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Normalization used for response-replay / stale-cycle hash matching.
pub fn normalize_for_replay_hash(content: &str) -> String {
    normalize_transient_agent_doc_markers(&strip_queue_active_frontmatter(
        &neutralize_queue_component(content),
    ))
}

/// Hash of the replay-normalized document form used by cycle/recovery matching.
pub fn replay_content_hash(content: &str) -> String {
    agent_doc_hash::content_hash(&normalize_for_replay_hash(content))
}

pub fn strip_re_heading_attribution(content: &str) -> String {
    let code_ranges = code_block_byte_ranges(content);
    let mut result_lines: Vec<String> = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        if !is_in_code_block(&code_ranges, offset) {
            let trimmed = line.trim_start();
            let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&hash_count) && trimmed.chars().nth(hash_count) == Some(' ') {
                let after_hash = trimmed[hash_count..].trim_start();
                if after_hash.starts_with("Re:")
                    && let Some(pos) = line.rfind(" — ")
                {
                    result_lines.push(line[..pos].to_string());
                    offset += line.len() + 1;
                    continue;
                }
            }
        }
        result_lines.push(line.to_string());
        offset += line.len() + 1;
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    }
}

pub fn normalize_post_commit_re_heading_drift(content: &str) -> String {
    strip_re_heading_attribution(&normalize_transient_agent_doc_markers(content))
}

pub fn strip_exchange_prompt_prefixes_for_compare(content: &str) -> String {
    fn strip_line(line: &str) -> String {
        let active_marker_stripped = strip_active_prompt_marker_from_line(line);
        let trimmed = active_marker_stripped.trim_start();
        let indent_len = active_marker_stripped.len().saturating_sub(trimmed.len());
        if let Some(rest) = trimmed.strip_prefix("❯ ") {
            format!("{}{}", &active_marker_stripped[..indent_len], rest)
        } else {
            let heading_level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
            if (1..=6).contains(&heading_level)
                && trimmed.as_bytes().get(heading_level) == Some(&b' ')
                && let Some(rest) = trimmed[heading_level + 1..].strip_prefix("❯ ")
            {
                format!(
                    "{}{} {}",
                    &active_marker_stripped[..indent_len],
                    &trimmed[..heading_level],
                    rest
                )
            } else {
                active_marker_stripped
            }
        }
    }

    fn strip_lines(content: &str) -> String {
        let code_ranges = code_block_byte_ranges(content);
        let mut stripped = String::with_capacity(content.len());
        let mut offset = 0usize;
        for segment in content.split_inclusive('\n') {
            let (line, newline) = segment
                .strip_suffix('\n')
                .map(|line| (line, "\n"))
                .unwrap_or((segment, ""));
            if is_in_code_block(&code_ranges, offset) {
                stripped.push_str(line);
            } else {
                stripped.push_str(&strip_line(line));
            }
            stripped.push_str(newline);
            offset += segment.len();
        }
        if !content.ends_with('\n') && content.is_empty() {
            stripped.clear();
        }
        stripped
    }

    let Ok(components) = agent_doc_element::element::parse(content) else {
        return strip_lines(content);
    };
    let mut rebuilt = String::with_capacity(content.len());
    let mut last = 0usize;
    for comp in components {
        if comp.open_end < last {
            continue;
        }
        rebuilt.push_str(&content[last..comp.open_end]);
        if comp.name == "exchange" {
            rebuilt.push_str(&strip_lines(comp.content(content)));
        } else {
            rebuilt.push_str(comp.content(content));
        }
        rebuilt.push_str(&content[comp.close_start..comp.close_end]);
        last = comp.close_end;
    }
    rebuilt.push_str(&content[last..]);
    rebuilt
}

pub fn exchange_prompt_prefix_equivalent(left: &str, right: &str) -> bool {
    strip_exchange_prompt_prefixes_for_compare(left)
        == strip_exchange_prompt_prefixes_for_compare(right)
}

fn first_hash_id(text: &str) -> Option<String> {
    let mut chars = text.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch != '#' {
            continue;
        }
        let mut id = String::new();
        while let Some((_, next)) = chars.peek().copied() {
            if next.is_ascii_alphanumeric() || next == '-' || next == '_' {
                id.push(next);
                chars.next();
            } else {
                break;
            }
        }
        if !id.is_empty() {
            return Some(id);
        }
    }
    None
}

fn normalized_response_heading_key(line: &str) -> Option<String> {
    let normalized = normalize_post_commit_re_heading_drift(line);
    let trimmed = normalized.trim();
    if trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn normalized_exchange_inventory_line(line: &str) -> Option<String> {
    let normalized = normalize_post_commit_re_heading_drift(line);
    let trimmed = normalized.trim();
    if trimmed.is_empty() || trimmed.starts_with("<!-- agent:boundary:") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn exchange_line_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for line in exchange
        .lines()
        .filter_map(normalized_exchange_inventory_line)
    {
        *counts.entry(line).or_insert(0) += 1;
    }
    counts
}

fn current_exchange_is_committed_line_subset(head_exchange: &str, current_exchange: &str) -> bool {
    let head_counts = exchange_line_counts(head_exchange);
    let current_counts = exchange_line_counts(current_exchange);
    if current_counts.is_empty() {
        return false;
    }
    current_counts
        .into_iter()
        .all(|(line, count)| head_counts.get(&line).copied().unwrap_or(0) >= count)
}

fn exchange_has_blockquoted_prompt_for_id(exchange: &str, id: &str) -> bool {
    let bracketed = format!("[#{id}]");
    let bare = format!("#{id}");
    exchange.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed
            .strip_prefix('>')
            .is_some_and(|rest| rest.contains(&bracketed) || rest.contains(&bare))
    })
}

pub fn stale_agent_response_collapse_exchange(head_exchange: &str, current_exchange: &str) -> bool {
    if normalize_post_commit_re_heading_drift(current_exchange)
        == normalize_post_commit_re_heading_drift(head_exchange)
    {
        return false;
    }
    if !current_exchange_is_committed_line_subset(head_exchange, current_exchange) {
        return false;
    }

    let head_headings: Vec<String> = head_exchange
        .lines()
        .filter_map(normalized_response_heading_key)
        .collect();
    let current_headings: HashSet<String> = current_exchange
        .lines()
        .filter_map(normalized_response_heading_key)
        .collect();

    head_headings.into_iter().any(|heading| {
        !current_headings.contains(&heading)
            && first_hash_id(&heading)
                .as_deref()
                .is_some_and(|id| exchange_has_blockquoted_prompt_for_id(current_exchange, id))
    })
}

pub fn repair_stale_agent_response_collapse_doc(
    head_doc: &str,
    current_doc: &str,
) -> Option<String> {
    if head_doc == current_doc {
        return None;
    }
    let head_exchange = agent_doc_element::element::parse(head_doc)
        .ok()?
        .into_iter()
        .find(|component| component.name == "exchange")?;
    let current_exchange = agent_doc_element::element::parse(current_doc)
        .ok()?
        .into_iter()
        .find(|component| component.name == "exchange")?;
    if !stale_agent_response_collapse_exchange(
        head_exchange.content(head_doc),
        current_exchange.content(current_doc),
    ) {
        return None;
    }
    Some(current_exchange.replace_content(current_doc, head_exchange.content(head_doc)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_head_markers_from_headings() {
        let input =
            "# Title\n### Re: Foo (HEAD)\nSome text with (HEAD) in it\n### Re: Bar (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(
            result,
            "# Title\n### Re: Foo\nSome text with (HEAD) in it\n### Re: Bar\n"
        );
    }

    #[test]
    fn strip_head_markers_preserves_non_heading_lines() {
        let input = "Normal line (HEAD)\n### Heading (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "Normal line (HEAD)\n### Heading\n");
    }

    #[test]
    fn strip_head_markers_bold_text() {
        let input = "**Re: Something** (HEAD)\nSome text.\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "**Re: Something**\nSome text.\n");
    }

    #[test]
    fn strip_head_markers_ignores_fenced_code_hash() {
        let input = "### Re: Answer (HEAD)\nResponse.\n```bash\n# comment (HEAD)\n```\n";
        let result = strip_head_markers(input);
        assert_eq!(
            result, "### Re: Answer\nResponse.\n```bash\n# comment (HEAD)\n```\n",
            "fenced (HEAD) must be preserved, got:\n{result}"
        );
    }

    #[test]
    fn strip_guard_markers_removes_standalone_lines() {
        let input = "### Re: topic\nResponse text.\n<!-- no-pending-capture -->\nMore text.\n<!-- no-pending-done-guard -->\nEnd.\n";
        let result = strip_guard_markers(input);
        assert_eq!(
            result, "### Re: topic\nResponse text.\nMore text.\nEnd.\n",
            "standalone guard markers should be removed:\n{result}"
        );
    }

    #[test]
    fn strip_guard_markers_strips_inline_content() {
        let input = "Text with <!-- no-pending-capture --> inline.\nNormal line.\n";
        let result = strip_guard_markers(input);
        assert_eq!(
            result, "Text with  inline.\nNormal line.\n",
            "inline guard markers should be stripped:\n{result}"
        );
    }

    #[test]
    fn strip_guard_markers_strips_trailing_on_content_line() {
        let input = "**All 39 variable products now have defaults set.** <!-- no-pending-capture -->\nNext line.\n";
        let result = strip_guard_markers(input);
        assert_eq!(
            result, "**All 39 variable products now have defaults set.**\nNext line.\n",
            "trailing guard marker should be stripped with trailing whitespace trimmed:\n{result}"
        );
    }

    #[test]
    fn normalize_transient_markers_strips_boundary_head_guard_and_pipeline() {
        let input = concat!(
            "---\n",
            "agent_doc_pipeline:\n",
            "  phase: responding\n",
            "title: test\n",
            "---\n\n",
            "### Re: topic (HEAD)\n",
            "Answer. <!-- no-pending-capture -->\n",
            "<!-- agent:boundary:abc -->\n"
        );

        assert_eq!(
            normalize_transient_agent_doc_markers(input),
            "---\ntitle: test\n---\n\n### Re: topic\nAnswer."
        );
    }

    #[test]
    fn normalize_for_replay_hash_neutralizes_queue_churn() {
        let with_active_queue = concat!(
            "---\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic — gpt-5\nResponse body.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test\n- do [#a]\n",
            "<!-- /agent:queue -->\n"
        );
        let with_drained_queue = concat!(
            "---\nagent_doc_format: template\nqueue_active: false\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic — gpt-5\nResponse body.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );

        assert_eq!(
            normalize_for_replay_hash(with_active_queue),
            normalize_for_replay_hash(with_drained_queue),
            "queue-only churn must not change the replay normalization"
        );

        let with_changed_response = with_active_queue.replace("Response body.", "Different body.");
        assert_ne!(
            normalize_for_replay_hash(with_active_queue),
            normalize_for_replay_hash(&with_changed_response),
            "a real response-body change must still change the replay normalization"
        );

        assert_eq!(
            replay_content_hash(with_active_queue),
            replay_content_hash(with_drained_queue),
            "replay content hash must use the same queue-neutralized normalization"
        );
    }

    #[test]
    fn strip_re_heading_attribution_ignores_code_blocks() {
        let input = concat!(
            "### Re: topic — gpt-5\n",
            "Response.\n",
            "```md\n",
            "### Re: literal — gpt-5\n",
            "```\n"
        );

        assert_eq!(
            strip_re_heading_attribution(input),
            "### Re: topic\nResponse.\n```md\n### Re: literal — gpt-5\n```\n"
        );
    }

    #[test]
    fn exchange_prompt_prefix_equivalent_only_ignores_exchange_user_prefixes() {
        let left = "\
<!-- agent:exchange -->
❯ do #one
<!-- /agent:exchange -->
<!-- agent:queue -->
❯ keep queued prefix
<!-- /agent:queue -->
";
        let right = "\
<!-- agent:exchange -->
do #one
<!-- /agent:exchange -->
<!-- agent:queue -->
❯ keep queued prefix
<!-- /agent:queue -->
";
        let changed_outside_exchange = right.replace("❯ keep queued prefix", "keep queued prefix");

        assert!(exchange_prompt_prefix_equivalent(left, right));
        assert!(!exchange_prompt_prefix_equivalent(
            left,
            &changed_outside_exchange
        ));
    }

    #[test]
    fn exchange_prompt_prefix_equivalence_ignores_active_markers_and_heading_internal_prefixes() {
        let active = concat!(
            "<!-- agent:exchange -->\n",
            "### ❯ 🚧 Heading prompt\n",
            "❯ Detail\n",
            "```md\n",
            "❯ 🚧 literal\n",
            "```\n",
            "<!-- /agent:exchange -->\n",
        );
        let bare = concat!(
            "<!-- agent:exchange -->\n",
            "### Heading prompt\n",
            "Detail\n",
            "```md\n",
            "❯ 🚧 literal\n",
            "```\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(exchange_prompt_prefix_equivalent(active, bare));
        assert_eq!(
            strip_exchange_active_prompt_markers(active),
            active.replace("### ❯ 🚧 Heading prompt", "### ❯ Heading prompt")
        );
    }

    #[test]
    fn stale_response_collapse_repair_restores_missing_exchange_response() {
        let committed = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: #first — gpt-5\n\n",
            "> [#first]\n\n",
            "First done.\n\n",
            "### Re: #second — gpt-5\n\n",
            "> [#second]\n\n",
            "Second done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
        );
        let collapsed = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: #first — gpt-5\n\n",
            "> [#second]\n\n",
            "> [#first]\n\n",
            "First done.\n\n",
            "Second done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#later]\n",
            "<!-- /agent:queue -->\n",
        );

        let repaired = repair_stale_agent_response_collapse_doc(committed, collapsed).unwrap();

        assert!(stale_agent_response_collapse_exchange(
            "<!-- agent:exchange -->\n### Re: #second — gpt-5\n\n> [#second]\n\nSecond done.\n<!-- /agent:exchange -->",
            "<!-- agent:exchange -->\n> [#second]\n\nSecond done.\n<!-- /agent:exchange -->"
        ));
        assert!(repaired.contains("### Re: #second"));
        assert!(repaired.contains("- do [#later]\n"));
    }
}

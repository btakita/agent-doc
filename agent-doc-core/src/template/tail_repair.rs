//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn is_exchange_turn_heading(trimmed: &str) -> bool {
    trimmed == "## User"
        || trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("## Re:")
}

pub(crate) fn preamble_belongs_to_exchange(preamble: &str) -> bool {
    let mut saw_nonempty = false;
    for line in preamble.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        saw_nonempty = true;
        if trimmed.starts_with("[//]:")
            || trimmed.starts_with("<!--")
            || trimmed.starts_with('#')
            || trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || trimmed.starts_with('>')
        {
            return false;
        }
    }
    saw_nonempty
}

pub(crate) fn is_non_exchange_section_heading(trimmed: &str) -> bool {
    let hashes = trimmed.chars().take_while(|&ch| ch == '#').count();
    if hashes == 0 || hashes > 3 || is_exchange_turn_heading(trimmed) {
        return false;
    }
    let rest = &trimmed[hashes..];
    rest.is_empty() || rest.starts_with(' ')
}

pub(crate) fn conversation_tail_start_in_range(
    doc: &str,
    search_start: usize,
    search_end: usize,
) -> Option<usize> {
    let code_ranges = element::find_code_ranges(doc);
    let comment_ranges = element::find_non_agent_html_comment_ranges(doc);
    let in_ignored_range = |pos: usize| {
        code_ranges
            .iter()
            .chain(comment_ranges.iter())
            .any(|&(start, end)| pos >= start && pos < end)
    };

    let mut first_nonempty_after = None;
    let mut first_heading_start = None;
    let mut offset = 0usize;
    for line in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if line_start < search_start || in_ignored_range(line_start) {
            continue;
        }
        if line_start >= search_end {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        first_nonempty_after.get_or_insert(line_start);
        if is_exchange_turn_heading(trimmed) {
            first_heading_start = Some(line_start);
            break;
        }
    }

    let heading_start = first_heading_start?;
    let first_nonempty_after = first_nonempty_after.unwrap_or(heading_start);
    if first_nonempty_after < heading_start
        && preamble_belongs_to_exchange(&doc[first_nonempty_after..heading_start])
    {
        Some(first_nonempty_after)
    } else {
        Some(heading_start)
    }
}

pub(crate) fn prompt_tail_range_in_region(
    doc: &str,
    search_start: usize,
    search_end: usize,
) -> Option<(usize, usize)> {
    let code_ranges = element::find_code_ranges(doc);
    let comment_ranges = element::find_non_agent_html_comment_ranges(doc);
    let in_ignored_range = |pos: usize| {
        code_ranges
            .iter()
            .chain(comment_ranges.iter())
            .any(|&(start, end)| pos >= start && pos < end)
    };

    let mut prompt_start = None;
    let mut prompt_end = None;
    let mut offset = 0usize;

    for line in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if line_start < search_start || in_ignored_range(line_start) {
            continue;
        }
        if line_start >= search_end {
            break;
        }

        let trimmed = line.trim();
        if prompt_start.is_none() {
            if trimmed.is_empty()
                || trimmed == "###"
                || trimmed.starts_with("<!--")
                || trimmed.starts_with("[//]:")
                || is_non_exchange_section_heading(trimmed)
            {
                continue;
            }
            if text_line_looks_like_prompt_target(trimmed) {
                prompt_start = Some(line_start);
                prompt_end = Some(offset);
                continue;
            }
            return None;
        }

        if trimmed.starts_with("<!--")
            || trimmed.starts_with("[//]:")
            || trimmed == "###"
            || is_non_exchange_section_heading(trimmed)
        {
            break;
        }
        prompt_end = Some(offset);
    }

    match (prompt_start, prompt_end) {
        (Some(start), Some(end)) if start < end => Some((start, end)),
        _ => None,
    }
}

pub(crate) fn escaped_prompt_range_outside_exchange(
    doc: &str,
    components: &[element::Component],
    exchange: &element::Component,
) -> Option<(usize, usize)> {
    let mut trailing_components: Vec<&element::Component> = components
        .iter()
        .filter(|c| c.open_start >= exchange.close_end)
        .collect();
    trailing_components.sort_by_key(|c| c.open_start);

    let mut search_start = exchange.close_end;
    for component in trailing_components {
        if search_start < component.open_start
            && let Some(range) =
                prompt_tail_range_in_region(doc, search_start, component.open_start)
        {
            return Some(range);
        }
        search_start = search_start.max(component.close_end);
    }

    if search_start < doc.len()
        && let Some(range) = prompt_tail_range_in_region(doc, search_start, doc.len())
    {
        return Some(range);
    }

    None
}

pub(crate) fn escaped_conversation_range_outside_exchange(
    doc: &str,
    components: &[element::Component],
    exchange: &element::Component,
) -> Option<(usize, usize)> {
    if let Some(range) = escaped_prompt_range_outside_exchange(doc, components, exchange) {
        return Some(range);
    }

    let mut trailing_components: Vec<&element::Component> = components
        .iter()
        .filter(|c| c.open_start >= exchange.close_end)
        .collect();
    trailing_components.sort_by_key(|c| c.open_start);

    let mut search_start = exchange.close_end;
    for component in trailing_components {
        if search_start < component.open_start
            && let Some(start) =
                conversation_tail_start_in_range(doc, search_start, component.open_start)
        {
            return Some((start, component.open_start));
        }
        search_start = search_start.max(component.close_end);
    }

    if search_start < doc.len() {
        if let Some(range) = prompt_tail_range_in_region(doc, search_start, doc.len()) {
            return Some(range);
        }
        if let Some(start) = conversation_tail_start_in_range(doc, search_start, doc.len()) {
            return Some((start, doc.len()));
        }
    }

    None
}

pub(crate) fn tail_is_safe_exchange_content(tail: &str) -> bool {
    fn fence_open(trimmed: &str) -> Option<(char, usize)> {
        let fc = trimmed.chars().next()?;
        if fc != '`' && fc != '~' {
            return None;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        if fl >= 3 { Some((fc, fl)) } else { None }
    }

    fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
        let fc = trimmed.chars().next().unwrap_or('\0');
        if fc != fence_char {
            return false;
        }
        let fl = trimmed.chars().take_while(|&c| c == fence_char).count();
        fl >= fence_len && trimmed[fl..].trim().is_empty()
    }

    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 3usize;

    for line in tail.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<!-- agent:")
            || trimmed.starts_with("<!-- /agent:")
            || trimmed.starts_with("<!-- patch:")
            || trimmed.starts_with("<!-- /patch:")
        {
            return false;
        }
        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                continue;
            }
        } else {
            if fence_close(trimmed, fence_char, fence_len) {
                in_fence = false;
            }
            continue;
        }

        if trimmed.is_empty() || is_exchange_turn_heading(trimmed) {
            continue;
        }

        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if hashes > 0 {
            if (4..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ') {
                continue;
            }
            return false;
        }
    }

    !in_fence
}

/// Fail closed when conversation content (prompts or response headings) would
/// land outside the live `<!-- agent:exchange -->` block. This is a write-path
/// guard — explicit `agent-doc repair` uses `repair_conversation_tail_outside_exchange`
/// instead.
pub fn guard_no_conversation_tail_outside_exchange(doc: &str) -> Result<()> {
    let components = element::parse(doc).context("failed to parse components")?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(());
    };

    let Some((tail_start, _tail_end)) =
        escaped_conversation_range_outside_exchange(doc, &components, exchange)
    else {
        return Ok(());
    };

    let tail = &doc[tail_start..];
    let first_line = tail.lines().next().unwrap_or("(empty)");
    anyhow::bail!(
        "prompt/response content would land outside `<!-- agent:exchange -->` — \
         the write path cannot place conversation content after the exchange close tag. \
         First escaped line: `{}`",
        first_line.chars().take(120).collect::<String>()
    );
}

/// Repair the safe malformed-template case where conversation content escaped
/// below `<!-- /agent:exchange -->` and now trails the document after sibling
/// components like pending/todo.
pub fn repair_conversation_tail_outside_exchange(doc: &str) -> Result<Option<String>> {
    let components = element::parse(doc).context("failed to parse components")?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(None);
    };

    let Some((tail_start, tail_end)) =
        escaped_conversation_range_outside_exchange(doc, &components, exchange)
    else {
        return Ok(None);
    };

    let tail = &doc[tail_start..tail_end];
    if !tail_is_safe_exchange_content(tail) {
        anyhow::bail!(
            "conversation content escaped `agent:exchange`, but the trailing document structure is ambiguous"
        );
    }

    let prefix = &doc[..tail_start];
    let suffix = &doc[tail_end..];
    let prefix_components = element::parse(prefix).context("failed to parse repair prefix")?;
    let exchange = prefix_components
        .iter()
        .find(|c| c.name == "exchange")
        .context("exchange component disappeared during repair")?;
    let escaped = tail.trim();
    if escaped.is_empty() {
        return Ok(None);
    }

    let mut repaired = if let Some(boundary_id) = find_boundary_in_component(prefix, exchange) {
        exchange.append_with_boundary(prefix, escaped, &boundary_id)
    } else {
        let new_content = if exchange.content(prefix).trim().is_empty() {
            format!("{escaped}\n")
        } else {
            format!("{}\n{}\n", exchange.content(prefix).trim_end(), escaped)
        };
        exchange.replace_content(prefix, &new_content)
    };

    repaired.push_str(suffix);
    Ok(Some(repaired))
}

/// Repair only prompt-target drift that escaped below `<!-- /agent:exchange -->`
/// while leaving later markdown section separators outside the exchange block.
pub fn repair_prompt_tail_outside_exchange(doc: &str) -> Result<Option<String>> {
    let components = element::parse(doc).context("failed to parse components")?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(None);
    };

    let Some((tail_start, tail_end)) =
        escaped_prompt_range_outside_exchange(doc, &components, exchange)
    else {
        return Ok(None);
    };

    let tail = &doc[tail_start..tail_end];
    if !tail_is_safe_exchange_content(tail) {
        anyhow::bail!(
            "prompt content escaped `agent:exchange`, but the trailing document structure is ambiguous"
        );
    }

    let prefix = &doc[..tail_start];
    let suffix = &doc[tail_end..];
    let prefix_components = element::parse(prefix).context("failed to parse repair prefix")?;
    let exchange = prefix_components
        .iter()
        .find(|c| c.name == "exchange")
        .context("exchange component disappeared during prompt repair")?;
    let escaped = tail.trim();
    if escaped.is_empty() {
        return Ok(None);
    }

    let mut repaired = if let Some(boundary_id) = find_boundary_in_component(prefix, exchange) {
        exchange.append_with_boundary(prefix, escaped, &boundary_id)
    } else {
        let new_content = if exchange.content(prefix).trim().is_empty() {
            format!("{escaped}\n")
        } else {
            format!("{}\n{}\n", exchange.content(prefix).trim_end(), escaped)
        };
        exchange.replace_content(prefix, &new_content)
    };

    repaired.push_str(suffix);
    Ok(Some(repaired))
}

/// Repair the malformed-template case where a duplicate
/// `<!-- /agent:exchange -->` lands after escaped conversation content.
///
/// This shows up as `closing marker <!-- /agent:exchange --> without matching open`
/// even though the document still has the real opening exchange marker. When the
/// text between the first and second close markers is safe exchange content, move
/// that text back into the real exchange block and drop the stray second close.
pub fn repair_duplicate_exchange_close_tail(doc: &str) -> Result<Option<String>> {
    let open_tag = "<!-- agent:exchange";
    let close_tag = "<!-- /agent:exchange -->";

    let Some(open_start) = doc.find(open_tag) else {
        return Ok(None);
    };
    let Some(open_end) = doc[open_start..]
        .find("-->")
        .map(|idx| open_start + idx + 3)
    else {
        return Ok(None);
    };
    let Some(first_close_start) = doc[open_end..].find(close_tag).map(|idx| open_end + idx) else {
        return Ok(None);
    };
    let first_close_end = first_close_start + close_tag.len();
    let Some(second_close_start) = doc[first_close_end..]
        .find(close_tag)
        .map(|idx| first_close_end + idx)
    else {
        return Ok(None);
    };
    let second_close_end = second_close_start + close_tag.len();

    let escaped = &doc[first_close_end..second_close_start];
    if !escaped.trim().is_empty() && !tail_is_safe_exchange_content(escaped) {
        anyhow::bail!(
            "conversation content escaped `agent:exchange`, but the duplicate close repair suffix is ambiguous"
        );
    }

    let prefix = &doc[..first_close_end];
    let prefix_components = element::parse(prefix).context("failed to parse repair prefix")?;
    let exchange = prefix_components
        .iter()
        .find(|c| c.name == "exchange")
        .context("exchange component disappeared during duplicate-close repair")?;

    let mut repaired = if escaped.trim().is_empty() {
        prefix.to_string()
    } else if let Some(boundary_id) = find_boundary_in_component(prefix, exchange) {
        exchange.append_with_boundary(prefix, escaped.trim(), &boundary_id)
    } else {
        let new_content = if exchange.content(prefix).trim().is_empty() {
            format!("{}\n", escaped.trim())
        } else {
            format!(
                "{}\n{}\n",
                exchange.content(prefix).trim_end(),
                escaped.trim()
            )
        };
        exchange.replace_content(prefix, &new_content)
    };

    repaired.push_str(&doc[second_close_end..]);
    Ok(Some(repaired))
}

/// Repair the malformed-template case where a duplicate template scaffold is
/// inserted between two `<!-- /agent:exchange -->` close markers.
///
/// Unlike `repair_duplicate_exchange_close_tail`, the text between the two
/// close markers is not conversation content to move back into exchange. It is
/// a duplicated outer document scaffold (`###`, queue/backlog/done components,
/// etc.) that should be dropped while preserving the first close marker and
/// the real scaffold after the second close marker.
pub fn repair_duplicate_exchange_close_scaffold(doc: &str) -> Result<Option<String>> {
    let open_tag = "<!-- agent:exchange";
    let close_tag = "<!-- /agent:exchange -->";

    let Some(open_start) = doc.find(open_tag) else {
        return Ok(None);
    };
    let Some(open_end) = doc[open_start..]
        .find("-->")
        .map(|idx| open_start + idx + 3)
    else {
        return Ok(None);
    };
    let Some(first_close_start) = doc[open_end..].find(close_tag).map(|idx| open_end + idx) else {
        return Ok(None);
    };
    let first_close_end = first_close_start + close_tag.len();
    let Some(second_close_start) = doc[first_close_end..]
        .find(close_tag)
        .map(|idx| first_close_end + idx)
    else {
        return Ok(None);
    };
    let second_close_end = second_close_start + close_tag.len();

    let duplicate_scaffold = &doc[first_close_end..second_close_start];
    if !is_safe_duplicate_template_scaffold(duplicate_scaffold) {
        return Ok(None);
    }

    let mut repaired = String::with_capacity(doc.len() - (second_close_end - first_close_end));
    repaired.push_str(&doc[..first_close_end]);
    repaired.push_str(&doc[second_close_end..]);
    Ok(Some(repaired))
}

/// Repair the malformed-template case where a duplicated scaffold contains
/// safe live conversation text before the stray second exchange close marker.
///
/// This is the mixed form of `repair_duplicate_exchange_close_scaffold`: the
/// duplicated queue/backlog/done scaffold should be dropped, but any ordinary
/// prompt text stranded in that duplicate segment still belongs in the live
/// exchange.
pub fn repair_duplicate_exchange_close_mixed_scaffold_tail(doc: &str) -> Result<Option<String>> {
    let open_tag = "<!-- agent:exchange";
    let close_tag = "<!-- /agent:exchange -->";

    let Some(open_start) = doc.find(open_tag) else {
        return Ok(None);
    };
    let Some(open_end) = doc[open_start..]
        .find("-->")
        .map(|idx| open_start + idx + 3)
    else {
        return Ok(None);
    };
    let Some(first_close_start) = doc[open_end..].find(close_tag).map(|idx| open_end + idx) else {
        return Ok(None);
    };
    let first_close_end = first_close_start + close_tag.len();
    let Some(second_close_start) = doc[first_close_end..]
        .find(close_tag)
        .map(|idx| first_close_end + idx)
    else {
        return Ok(None);
    };
    let second_close_end = second_close_start + close_tag.len();

    let duplicate_segment = &doc[first_close_end..second_close_start];
    let Some(exchange_tail) = safe_exchange_tail_from_duplicate_scaffold(duplicate_segment)? else {
        return Ok(None);
    };

    let prefix = &doc[..first_close_end];
    let mut repaired = append_tail_to_exchange_end(prefix, &exchange_tail)?;
    repaired.push_str(&doc[second_close_end..]);
    Ok(Some(repaired))
}

pub(crate) fn safe_exchange_tail_from_duplicate_scaffold(segment: &str) -> Result<Option<String>> {
    let has_scaffold_component = segment.contains("<!-- agent:queue -->")
        || segment.contains("<!-- agent:backlog")
        || segment.contains("<!-- agent:pending")
        || segment.contains("<!-- agent:done");
    if !has_scaffold_component {
        return Ok(None);
    }

    let mut residue = segment.to_string();
    if let Ok(components) = element::parse(segment) {
        let mut ranges: Vec<(usize, usize)> = components
            .iter()
            .filter(|component| {
                matches!(
                    component.name.as_str(),
                    "queue" | "backlog" | "pending" | "icebox" | "done"
                )
            })
            .map(|component| (component.open_start, component.close_end))
            .collect();
        ranges.sort_by_key(|range| std::cmp::Reverse(range.0));
        for (start, end) in ranges {
            residue.replace_range(start..end, "");
        }
    }

    let residue = strip_html_comments(&residue);
    let tail = residue
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                None
            } else {
                Some(line.trim_end())
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tail = tail.trim();
    if tail.is_empty() {
        return Ok(None);
    }
    if !tail_is_safe_exchange_content(tail) {
        anyhow::bail!(
            "conversation content escaped `agent:exchange`, but the duplicate scaffold repair suffix is ambiguous"
        );
    }
    Ok(Some(tail.to_string()))
}

pub(crate) fn append_tail_to_exchange_end(prefix: &str, tail: &str) -> Result<String> {
    let prefix_without_boundaries = remove_all_boundaries(prefix);
    let components =
        element::parse(&prefix_without_boundaries).context("failed to parse repair prefix")?;
    let exchange = components
        .iter()
        .find(|c| c.name == "exchange")
        .context("exchange component disappeared during mixed duplicate-scaffold repair")?;

    let existing = exchange.content(&prefix_without_boundaries).trim_end();
    let mut new_content = String::new();
    if !existing.is_empty() {
        new_content.push_str(existing);
        new_content.push('\n');
    }
    new_content.push_str(tail.trim_end());
    new_content.push('\n');
    new_content.push_str(&crate::id::format_boundary_marker(
        &crate::id::new_boundary_id(),
    ));
    new_content.push('\n');
    Ok(exchange.replace_content(&prefix_without_boundaries, &new_content))
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;
    #[test]
    fn repair_conversation_tail_outside_exchange_moves_tail_back_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Follow-up response.\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let pending_open = repaired.find("<!-- agent:pending -->").unwrap();
        let assistant = repaired.find("## Assistant").unwrap();

        assert!(
            assistant < exchange_close,
            "assistant tail should move back inside exchange:\n{repaired}"
        );
        assert!(
            pending_open > exchange_close,
            "pending should remain outside exchange:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- agent:boundary:").count(),
            1,
            "repair should leave exactly one boundary marker"
        );
    }
    #[test]
    fn repair_conversation_tail_outside_exchange_rejects_ambiguous_suffix() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Assistant\n\n",
            "Escaped answer.\n\n",
            "## Todo / Backlog\n\n",
            "- not conversation content\n"
        );

        let err = repair_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string().contains("escaped `agent:exchange`"),
            "unexpected error: {err}"
        );
    }
    #[test]
    fn repair_conversation_tail_outside_exchange_moves_plain_trailing_suffix_after_todo() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "## User\n",
            "compact exchange\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:todo patch=replace -->\n",
            "- [ ] backlog\n",
            "<!-- /agent:todo -->\n\n",
            "Exchange compacted. No new work was run in this turn.\n\n",
            "## Assistant\n\n",
            "Exchange compacted. No new work was run in this turn.\n\n",
            "## User\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let pending_open = repaired.find("<!-- agent:pending -->").unwrap();
        let todo_open = repaired.find("<!-- agent:todo patch=replace -->").unwrap();
        let trailing_summary = repaired
            .rfind("Exchange compacted. No new work was run in this turn.")
            .unwrap();

        assert!(
            trailing_summary < exchange_close,
            "plain trailing suffix should move back inside exchange:\n{repaired}"
        );
        assert!(
            pending_open > exchange_close && todo_open > exchange_close,
            "sibling components should stay outside exchange:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- agent:boundary:").count(),
            1,
            "repair should leave exactly one boundary marker"
        );
    }
    #[test]
    fn repair_conversation_tail_outside_exchange_ignores_comment_only_suffix() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "[//]: # (leave this note outside exchange)\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc).unwrap();
        assert!(
            repaired.is_none(),
            "comment-only suffix should stay outside exchange"
        );
    }
    #[test]
    fn repair_conversation_tail_outside_exchange_ignores_html_comment_body() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Can this stay hidden?\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc).unwrap();
        assert!(
            repaired.is_none(),
            "prompt-like text inside ordinary HTML comments must not be moved into exchange"
        );
    }
    #[test]
    fn repair_conversation_tail_outside_exchange_ignores_unterminated_html_comment_body() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Still typing this scratch note.\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc).unwrap();
        assert!(
            repaired.is_none(),
            "prompt-like text inside a transiently unclosed ordinary HTML comment must not be moved into exchange"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }
    #[test]
    fn repair_conversation_tail_outside_exchange_moves_gap_before_backlog_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "### Re: gap — gpt-5\n\n",
            "Escaped answer.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let response = repaired.find("### Re: gap — gpt-5").unwrap();
        let gap_marker = repaired.find("\n###\n\n").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            response < exchange_close,
            "gap response should move back inside exchange:\n{repaired}"
        );
        assert!(
            gap_marker > exchange_close,
            "plain gap marker should remain outside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
    }
    #[test]
    fn repair_conversation_tail_outside_exchange_moves_prompt_before_gap_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "do [#oobprompt]. spec-test-build-install-commit-push\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let prompt = repaired
            .find("do [#oobprompt]. spec-test-build-install-commit-push")
            .unwrap();
        let gap_marker = repaired.find("\n###\n\n").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            prompt < exchange_close,
            "prompt should move back inside exchange:\n{repaired}"
        );
        assert!(
            gap_marker > exchange_close,
            "plain gap marker should remain outside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
    }
    #[test]
    fn repair_duplicate_exchange_close_tail_moves_escaped_response_back_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ earlier question\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "### Re: later — gpt-5\n\n",
            "Escaped answer.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_close_tail(doc)
            .unwrap()
            .expect("duplicate close repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let response = repaired.find("### Re: later — gpt-5").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            response < exchange_close,
            "escaped response should move back inside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- /agent:exchange -->").count(),
            1,
            "repair should leave exactly one exchange close marker"
        );
    }
    #[test]
    fn repair_duplicate_exchange_close_scaffold_drops_inserted_template_shell() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "JB `Run Agent Doc` failed on this document.\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_close_scaffold(doc)
            .unwrap()
            .expect("duplicate scaffold repair should apply");

        assert_eq!(
            repaired.matches("<!-- /agent:exchange -->").count(),
            1,
            "repair should leave one exchange close:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- agent:queue -->").count(),
            1,
            "repair should drop the duplicated queue scaffold:\n{repaired}"
        );
        assert!(repaired.contains("JB `Run Agent Doc` failed on this document."));
        assert!(element::parse(&repaired).is_ok());
    }
    #[test]
    fn repair_duplicate_exchange_close_scaffold_rejects_mixed_user_text() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "c The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "corky.md The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_close_scaffold(doc).unwrap();

        assert!(
            repaired.is_none(),
            "mixed user text must not be dropped as duplicated scaffold"
        );
    }
    #[test]
    fn normalize_editor_visible_template_structure_repairs_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = normalize_editor_visible_template_structure(doc)
            .expect("safe duplicate scaffold should repair before editor write");

        assert_eq!(repaired.matches("<!-- /agent:exchange -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:queue -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:backlog -->").count(), 1);
        guard_no_conversation_tail_outside_exchange(&repaired).unwrap();
    }
    #[test]
    fn repair_duplicate_exchange_close_tail_rejects_ambiguous_suffix() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ earlier question\n",
            "<!-- /agent:exchange -->\n\n",
            "## Todo / Backlog\n\n",
            "- keep me outside exchange\n",
            "<!-- /agent:exchange -->\n"
        );

        let err = repair_duplicate_exchange_close_tail(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("duplicate close repair suffix is ambiguous"),
            "unexpected error: {err}"
        );
    }
    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_for_normal_content() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ user question\n",
            "### Re: question — opus-4-6\n\n",
            "Answer.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] some task\n",
            "<!-- /agent:backlog -->\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }
    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_tail_after_session_digest() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ earlier question\n",
            "### Re: earlier — opus-4-6\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "# Session Digest\n\n",
            "Summary of work.\n\n",
            "❯ Why were the backlog items removed?\n",
            "### Re: backlog — opus-4-6\n\n",
            "Escaped answer.\n"
        );
        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }
    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_tail_after_backlog() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Assistant\n\n",
            "Follow-up response.\n"
        );
        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }
    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_gap_before_backlog() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "### Re: gap — gpt-5\n\n",
            "Escaped answer.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }
    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_prompt_before_gap_marker() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "do [#oobprompt]. spec-test-build-install-commit-push\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }
    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_without_exchange_block() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status -->\n",
            "Active\n",
            "<!-- /agent:status -->\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }
    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_for_comment_only_tail() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "[//]: # (leave this note outside exchange)\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }
    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_for_html_comment_body() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Can this stay hidden?\n",
            "-->\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }
}

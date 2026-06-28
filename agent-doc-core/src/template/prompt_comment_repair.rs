//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub fn remove_post_exchange_duplicate_prompt_comments(doc: &str) -> Option<String> {
    remove_post_exchange_duplicate_prompt_comments_preserving_docs(doc, &[])
}

/// Like `remove_post_exchange_duplicate_prompt_comments`, but keeps duplicate-
/// looking comment lines that were already present in `preserve_doc`.
///
/// Closeout, route, and preflight use this as an ownership proof: stale
/// duplicate residue created during the current response cycle can be scrubbed,
/// while pre-existing scratch comments below `agent:exchange` remain user-owned.
pub fn remove_post_exchange_duplicate_prompt_comments_preserving(
    doc: &str,
    preserve_doc: Option<&str>,
) -> Option<String> {
    match preserve_doc {
        Some(preserve_doc) => {
            remove_post_exchange_duplicate_prompt_comments_preserving_docs(doc, &[preserve_doc])
        }
        None => remove_post_exchange_duplicate_prompt_comments_preserving_docs(doc, &[]),
    }
}

/// Like `remove_post_exchange_duplicate_prompt_comments_preserving`, but accepts
/// several ownership-proof documents and preserves the union of their ordinary
/// post-exchange comment lines.
pub fn remove_post_exchange_duplicate_prompt_comments_preserving_docs(
    doc: &str,
    preserve_docs: &[&str],
) -> Option<String> {
    let components = component::parse(doc).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    let prompts = exchange_prompt_comment_targets(exchange.content(doc));
    if prompts.is_empty() {
        return None;
    }

    let protected_ranges = components
        .iter()
        .filter(|component| component.name != "exchange")
        .map(|component| (component.open_start, component.close_end))
        .collect::<Vec<_>>();
    let mut preserved_comment_lines = HashSet::new();
    for preserve_doc in preserve_docs {
        preserved_comment_lines.extend(post_exchange_comment_line_preserve_set(preserve_doc));
    }

    let mut replacements = Vec::<(usize, usize, String)>::new();
    for (start, end) in component::find_non_agent_html_comment_ranges(doc) {
        if start < exchange.close_end {
            continue;
        }
        if protected_ranges
            .iter()
            .any(|(protected_start, protected_end)| {
                start >= *protected_start && end <= *protected_end
            })
        {
            continue;
        }
        let original = &doc[start..end];
        if !original.ends_with("-->") {
            continue;
        }
        let body = &doc[start + 4..end - 3];
        let Some(cleaned_body) =
            strip_duplicate_prompt_comment_body(body, &prompts, &preserved_comment_lines)
        else {
            replacements.push((start, end, empty_html_comment_like(body)));
            continue;
        };
        if cleaned_body != body {
            replacements.push((start, end, format!("<!--{}-->", cleaned_body)));
        }
    }

    if replacements.is_empty() {
        return None;
    }

    let mut cleaned = doc.to_string();
    for (start, end, replacement) in replacements.into_iter().rev() {
        cleaned.replace_range(start..end, &replacement);
    }
    Some(cleaned)
}

pub(crate) fn post_exchange_comment_line_preserve_set(doc: &str) -> HashSet<String> {
    let Ok(components) = component::parse(doc) else {
        return HashSet::new();
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return HashSet::new();
    };
    let protected_ranges = components
        .iter()
        .filter(|component| component.name != "exchange")
        .map(|component| (component.open_start, component.close_end))
        .collect::<Vec<_>>();

    let mut preserved = HashSet::new();
    for (start, end) in component::find_non_agent_html_comment_ranges(doc) {
        if start < exchange.close_end {
            continue;
        }
        if protected_ranges
            .iter()
            .any(|(protected_start, protected_end)| {
                start >= *protected_start && end <= *protected_end
            })
        {
            continue;
        }
        let original = &doc[start..end];
        if !original.ends_with("-->") {
            continue;
        }
        let body = &doc[start + 4..end - 3];
        for line in comment_body_lines(body) {
            if let Some(normalized) = normalize_prompt_comment_text(line) {
                preserved.insert(normalized);
            }
        }
    }
    preserved
}

/// Remove a prompt tail after the latest exchange boundary when that tail is an
/// exact duplicate of a prompt block already answered earlier in the exchange.
///
/// This covers delayed route/editor replay that re-adds the just-answered
/// prompt after closeout. The cleanup is intentionally narrow: every
/// non-comment tail line must (a) carry the `❯ ` answered-form marker — proof it
/// is a copy of an answered prompt rather than a freshly-typed live prompt — and
/// (b) match a contiguous prompt block immediately before an existing response
/// heading. A bare unprefixed post-boundary prompt is always preserved.
pub fn remove_duplicate_answered_exchange_prompt_tail(doc: &str) -> Option<String> {
    let components = component::parse(doc).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    let exchange_content = exchange.content(doc);
    let duplicate_start = duplicate_answered_exchange_prompt_tail_start(exchange_content)?;

    let mut cleaned_exchange = exchange_content[..duplicate_start].to_string();
    if !cleaned_exchange.ends_with('\n') {
        cleaned_exchange.push('\n');
    }
    Some(exchange.replace_content(doc, &cleaned_exchange))
}

pub(crate) fn duplicate_answered_exchange_prompt_tail_start(exchange: &str) -> Option<usize> {
    let lines = exchange_line_spans(exchange);
    let boundary_idx = lines
        .iter()
        .rposition(|(_, _, line)| line.trim().starts_with("<!-- agent:boundary:"))?;
    let tail_start = lines
        .get(boundary_idx)
        .map(|(_, end, _)| *end)
        .unwrap_or(exchange.len());
    let tail = duplicate_exchange_tail_prompt_lines(&lines[boundary_idx + 1..])?;
    if answered_exchange_prompt_blocks_before_boundary(&lines, boundary_idx)
        .into_iter()
        .any(|block| block == tail)
    {
        Some(tail_start)
    } else {
        None
    }
}

pub(crate) fn exchange_line_spans(text: &str) -> Vec<(usize, usize, &str)> {
    let mut spans = Vec::new();
    let mut offset = 0usize;
    for segment in text.split_inclusive('\n') {
        let start = offset;
        offset += segment.len();
        spans.push((start, offset, segment));
    }
    if spans.is_empty() && !text.is_empty() {
        spans.push((0, text.len(), text));
    }
    spans
}

pub(crate) fn duplicate_exchange_tail_prompt_lines(
    lines: &[(usize, usize, &str)],
) -> Option<Vec<String>> {
    let mut prompt_lines = Vec::new();
    for (_, _, line) in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        if is_exchange_turn_heading(trimmed) {
            return None;
        }
        // Ownership proof: only an answered-form line (carrying the `❯ ` prompt
        // marker) can be delayed-replay residue, because the marker is added by
        // the answer/normalize cycle, never by a user typing a fresh prompt. A
        // bare, unprefixed post-boundary line is a LIVE prompt the user just
        // typed — even when its text matches a previously-answered prompt (e.g.
        // a re-typed "go"/"yes"/"continue") — and must never be scrubbed.
        // Without this guard the text-only match silently ate live prompts
        // (#ipcfullprompt-recur: "go" on sampleorders.md).
        if !trimmed.starts_with('❯') {
            return None;
        }
        prompt_lines.push(normalize_duplicate_exchange_prompt_line(trimmed)?);
    }
    if prompt_lines.is_empty() {
        None
    } else {
        Some(prompt_lines)
    }
}

pub(crate) fn answered_exchange_prompt_blocks_before_boundary(
    lines: &[(usize, usize, &str)],
    boundary_idx: usize,
) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    for response_idx in 0..boundary_idx {
        if !is_exchange_turn_heading(lines[response_idx].2.trim()) {
            continue;
        }
        let mut block = Vec::new();
        let mut cursor = response_idx;
        while cursor > 0 {
            cursor -= 1;
            let trimmed = lines[cursor].2.trim();
            if trimmed.is_empty() || trimmed.starts_with("<!--") {
                if block.is_empty() {
                    continue;
                }
                break;
            }
            if trimmed.starts_with('❯')
                && let Some(normalized) = normalize_duplicate_exchange_prompt_line(trimmed)
            {
                block.push(normalized);
                continue;
            }
            if block.is_empty()
                && looks_like_prompt_comment_target(trimmed)
                && let Some(normalized) = normalize_duplicate_exchange_prompt_line(trimmed)
            {
                block.push(normalized);
                continue;
            }
            break;
        }
        if !block.is_empty() {
            block.reverse();
            blocks.push(block);
        }
    }
    blocks
}

pub(crate) fn normalize_duplicate_exchange_prompt_line(line: &str) -> Option<String> {
    let unprefixed = line.trim().strip_prefix('❯').unwrap_or(line.trim()).trim();
    let normalized = unprefixed.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Fail closed when prompt text already recorded in `agent:exchange` still
/// appears as freeform Markdown after the exchange close marker.
///
/// Ordinary post-exchange HTML comments are scrubbed by
/// `remove_post_exchange_duplicate_prompt_comments`; tracked components such as
/// backlog/queue have their own mutation rules. Anything else is ambiguous
/// enough that closeout must not silently commit or dispatch it.
pub fn guard_no_duplicate_prompt_residue_outside_exchange(doc: &str) -> Result<()> {
    let components = match component::parse(doc) {
        Ok(components) => components,
        Err(err)
            if err
                .chain()
                .any(|cause| cause.to_string().contains("without matching open")) =>
        {
            return Ok(());
        }
        Err(err) => return Err(err).context("failed to parse components"),
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(());
    };
    let prompts = exchange_prompt_comment_targets(exchange.content(doc));
    if prompts.is_empty() {
        return Ok(());
    }

    let protected_ranges = components
        .iter()
        .filter(|component| component.name != "exchange")
        .map(|component| (component.open_start, component.close_end))
        .collect::<Vec<_>>();
    let in_protected_component = |pos: usize| {
        protected_ranges
            .iter()
            .any(|(start, end)| pos >= *start && pos < *end)
    };
    let ordinary_comment_ranges = component::find_non_agent_html_comment_ranges(doc);
    let in_ordinary_comment = |pos: usize| {
        ordinary_comment_ranges
            .iter()
            .any(|(start, end)| pos >= *start && pos < *end)
    };

    let mut offset = 0usize;
    for segment in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += segment.len();
        if line_start < exchange.close_end
            || in_protected_component(line_start)
            || in_ordinary_comment(line_start)
        {
            continue;
        }
        let trimmed = segment.trim();
        if !is_duplicate_prompt_comment_text(trimmed, &prompts) {
            continue;
        }
        anyhow::bail!(
            "duplicate prompt residue outside `<!-- agent:exchange -->`; refusing to commit or dispatch ambiguous Markdown tail. First duplicate line: `{}`",
            trimmed.chars().take(120).collect::<String>()
        );
    }

    Ok(())
}

pub(crate) fn empty_html_comment_like(body: &str) -> String {
    if body.contains('\n') {
        "<!--\n-->".to_string()
    } else {
        "<!-- -->".to_string()
    }
}

pub(crate) fn strip_duplicate_prompt_comment_body(
    body: &str,
    prompts: &[String],
    preserved_comment_lines: &HashSet<String>,
) -> Option<String> {
    if !body.contains('\n') && is_duplicate_prompt_comment_text(body, prompts) {
        if normalized_comment_line_is_preserved(body, preserved_comment_lines) {
            return Some(body.to_string());
        }
        return None;
    }

    let mut changed = false;
    let mut cleaned = String::new();
    for segment in body.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if is_duplicate_prompt_comment_text(line, prompts) {
            if normalized_comment_line_is_preserved(line, preserved_comment_lines) {
                cleaned.push_str(segment);
                continue;
            }
            changed = true;
            continue;
        }
        cleaned.push_str(segment);
    }

    if !changed {
        return Some(body.to_string());
    }
    if cleaned.trim().is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

pub(crate) fn comment_body_lines(body: &str) -> Vec<&str> {
    if body.contains('\n') {
        body.split_inclusive('\n')
            .map(|segment| segment.strip_suffix('\n').unwrap_or(segment))
            .collect()
    } else {
        vec![body]
    }
}

pub(crate) fn normalized_comment_line_is_preserved(
    line: &str,
    preserved_comment_lines: &HashSet<String>,
) -> bool {
    normalize_prompt_comment_text(line)
        .map(|normalized| preserved_comment_lines.contains(&normalized))
        .unwrap_or(false)
}

pub(crate) fn exchange_prompt_comment_targets(exchange: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    let mut in_response_block = false;
    let mut in_fence = false;

    for line in exchange.lines() {
        let trimmed = line.trim();
        if is_fence_delimiter(trimmed) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.starts_with("<!-- agent:boundary:") {
            in_response_block = false;
            continue;
        }
        if trimmed.starts_with("### Re:") || trimmed.starts_with("## Assistant") {
            in_response_block = true;
            continue;
        }
        if in_response_block {
            continue;
        }
        let Some(normalized) = normalize_prompt_comment_text(trimmed) else {
            continue;
        };
        if seen.insert(normalized.clone()) {
            targets.push(normalized);
        }
    }

    targets
}

pub(crate) fn looks_like_prompt_comment_target(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    trimmed.starts_with('❯')
        || trimmed.contains('#')
        || trimmed.ends_with('?')
        || lower.starts_with("do ")
        || lower.starts_with("fix ")
        || lower.starts_with("run ")
        || lower.starts_with("please ")
        || lower.contains(" spec-test-")
        || lower.contains(" reproduce ")
}

pub(crate) fn normalize_prompt_comment_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("## User")
        || is_markdown_heading(trimmed)
    {
        return None;
    }
    let unprefixed = trimmed.strip_prefix('❯').unwrap_or(trimmed).trim();
    let collapsed = unprefixed.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() < 32 {
        None
    } else {
        Some(collapsed)
    }
}

pub(crate) fn is_markdown_heading(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ')
}

pub(crate) fn is_fence_delimiter(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if first != '`' && first != '~' {
        return false;
    }
    trimmed.chars().take_while(|ch| *ch == first).count() >= 3
}

pub(crate) fn is_duplicate_prompt_comment_text(candidate: &str, prompts: &[String]) -> bool {
    let Some(candidate) = normalize_prompt_comment_text(candidate) else {
        return false;
    };
    let candidate_lower = candidate.to_lowercase();
    let candidate_tokens = prompt_comment_tokens(&candidate);
    if candidate_tokens.len() < 8 {
        return false;
    }

    prompts.iter().any(|prompt| {
        let prompt_lower = prompt.to_lowercase();
        if candidate_lower == prompt_lower
            || prompt_lower.contains(&candidate_lower)
            || candidate_lower.contains(&prompt_lower)
        {
            return true;
        }

        let prompt_tokens = prompt_comment_tokens(prompt);
        if prompt_tokens.len() < 8 {
            return false;
        }
        ordered_token_coverage(&candidate_tokens, &prompt_tokens) >= 0.85
            || ordered_token_coverage(&prompt_tokens, &candidate_tokens) >= 0.85
    })
}

pub(crate) fn prompt_comment_tokens(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

pub(crate) fn ordered_token_coverage(needle: &[String], haystack: &[String]) -> f64 {
    if needle.is_empty() {
        return 0.0;
    }
    let mut matched = 0usize;
    for token in haystack {
        if needle.get(matched).is_some_and(|needle| needle == token) {
            matched += 1;
            if matched == needle.len() {
                break;
            }
        }
    }
    matched as f64 / needle.len() as f64
}

pub(crate) fn is_safe_duplicate_template_scaffold(segment: &str) -> bool {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains("### Re:")
        || trimmed.contains("## User")
        || trimmed.contains("## Assistant")
        || trimmed.contains("❯ ")
    {
        return false;
    }

    let has_scaffold_component = trimmed.contains("<!-- agent:queue -->")
        || trimmed.contains("<!-- agent:backlog")
        || trimmed.contains("<!-- agent:pending")
        || trimmed.contains("<!-- agent:done");
    if !has_scaffold_component {
        return false;
    }
    if !duplicate_scaffold_has_only_structural_residue(trimmed) {
        return false;
    }

    let wrapped = format!("<!-- agent:scaffold -->\n{trimmed}\n<!-- /agent:scaffold -->\n");
    let Ok(components) = component::parse(&wrapped) else {
        return false;
    };
    let allowed = ["scaffold", "queue", "backlog", "pending", "icebox", "done"];
    components
        .iter()
        .all(|component| allowed.contains(&component.name.as_str()))
}

pub(crate) fn duplicate_scaffold_has_only_structural_residue(segment: &str) -> bool {
    let mut residue = segment.to_string();
    if let Ok(components) = component::parse(segment) {
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
    residue.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty() || trimmed.starts_with('#')
    })
}

pub(crate) fn strip_html_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(start) = rest.find("<!--") else {
            result.push_str(rest);
            break;
        };
        result.push_str(&rest[..start]);
        let after_start = &rest[start + 4..];
        let Some(end) = after_start.find("-->") else {
            break;
        };
        rest = &after_start[end + 3..];
    }
    result
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;
    #[test]
    fn guard_no_duplicate_prompt_residue_outside_exchange_rejects_plain_markdown_duplicate() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: duplicate residue — gpt-5\n\n",
                "Answered.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "# Notes\n\n",
                "{prompt}\n"
            ),
            prompt = prompt
        );

        let err = guard_no_duplicate_prompt_residue_outside_exchange(&doc).unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue outside"),
            "unexpected error: {err}"
        );
    }
    #[test]
    fn guard_no_duplicate_prompt_residue_outside_exchange_allows_tracked_components() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: duplicate residue — gpt-5\n\n",
                "Answered.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:backlog -->\n",
                "{prompt}\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        guard_no_duplicate_prompt_residue_outside_exchange(&doc).unwrap();
    }
    #[test]
    fn remove_duplicate_answered_tail_scrubs_prefixed_replay_residue() {
        // Answered-form residue (carries `❯ `) re-added below the boundary is
        // safely removable replay residue.
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ go\n",
            "### Re: go — gpt-5\n\n",
            "Did the thing.\n",
            "<!-- agent:boundary:head -->\n",
            "❯ go\n",
            "<!-- /agent:exchange -->\n",
        );
        let cleaned = remove_duplicate_answered_exchange_prompt_tail(doc)
            .expect("prefixed answered-form residue should be scrubbed");
        assert!(
            !cleaned.contains("head -->\n❯ go"),
            "answered-form residue tail must be removed:\n{cleaned}"
        );
        assert!(
            cleaned.contains("❯ go\n### Re: go"),
            "answered history must be preserved:\n{cleaned}"
        );
    }
    #[test]
    fn remove_duplicate_answered_tail_preserves_unprefixed_live_prompt() {
        // #ipcfullprompt-recur: a freshly-typed prompt (no `❯ `) that matches a
        // previously-answered prompt is a LIVE prompt and must never be scrubbed.
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ go\n",
            "### Re: go — gpt-5\n\n",
            "Did the thing.\n",
            "<!-- agent:boundary:head -->\n",
            "go\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(
            remove_duplicate_answered_exchange_prompt_tail(doc).is_none(),
            "a bare re-typed live prompt must be preserved"
        );
    }
}

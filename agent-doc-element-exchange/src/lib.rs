//! Exchange element descriptor.

use std::collections::{HashMap, HashSet};

use agent_doc_element::{
    Component, ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy, element,
};
use agent_doc_prompt_lines::{
    line_looks_like_markdown_list_item, line_looks_like_plain_response_after_prompt,
    line_looks_like_prompt_prefix_repair_start,
    line_looks_like_targeted_or_prefixed_prompt_repair_start,
    line_looks_like_targeted_prompt_prefix_repair_start,
};
use anyhow::{Context, Result};
use similar::{ChangeTag, TextDiff};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "exchange",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::SharedOperatorAuthoritative,
    write_policy: ElementWritePolicy::MergeOnly,
    scheduling_role: ElementSchedulingRole::None,
    realtime_model: ElementRealtimeModel::Exchange,
    composition_role: ElementCompositionRole::LocalOnly,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

pub fn insert_prompt_line_before_boundary(doc: &str, prompt_line: &str) -> Result<String> {
    let components = element::parse(doc).context("failed to parse document components")?;
    let exchange = components
        .iter()
        .find(|comp| comp.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no `agent:exchange` component"))?;
    let existing = exchange.content(doc);
    if existing.lines().any(|line| line.trim() == prompt_line) {
        return Ok(doc.to_string());
    }

    let relative_boundary = existing
        .lines()
        .scan(0usize, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .filter_map(|(start, line)| {
            line.trim()
                .starts_with("<!-- agent:boundary:")
                .then_some(start)
        })
        .last();

    let insert_at = relative_boundary
        .map(|rel| exchange.open_end + rel)
        .unwrap_or(exchange.close_start);
    let mut result = String::with_capacity(doc.len() + prompt_line.len() + 4);
    result.push_str(&doc[..insert_at]);
    if insert_at > exchange.open_end && !result.ends_with('\n') {
        result.push('\n');
    }
    if insert_at > exchange.open_end && !result.ends_with("\n\n") {
        result.push('\n');
    }
    result.push_str(prompt_line);
    result.push('\n');
    result.push_str(&doc[insert_at..]);
    Ok(result)
}

pub fn append_deduped_content_to_exchange(
    doc: &str,
    dedupe_key: &str,
    content: &str,
) -> Result<Option<String>> {
    if doc.contains(dedupe_key) {
        return Ok(None);
    }
    let components = element::parse(doc).context("failed to parse document components")?;
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let updated = exchange.append_with_caret(doc, content, None);
    if updated == doc {
        Ok(None)
    } else {
        Ok(Some(updated))
    }
}

/// Extract the byte length of the exchange component's trimmed content.
/// Returns 0 if no exchange component is found or component parsing fails.
pub fn exchange_content_len(doc: &str) -> usize {
    exchange_content(doc)
        .map(|content| content.trim().len())
        .unwrap_or(0)
}

pub fn exchange_content(doc: &str) -> Option<&str> {
    exchange_component(doc).map(|component| component.content(doc))
}

pub fn exchange_component(doc: &str) -> Option<Component> {
    agent_doc_element::element::parse(doc)
        .ok()?
        .into_iter()
        .find(|component| component.name == "exchange")
}

/// Strip user content from the exchange component, leaving just the markers.
///
/// This creates a snapshot baseline that treats existing user text as a diff.
pub fn strip_exchange_content(content: &str) -> String {
    exchange_component(content)
        .map(|exchange| exchange.replace_content(content, "\n"))
        .unwrap_or_else(|| content.to_string())
}

pub fn redact_exchange_component_content(doc: &str) -> Option<String> {
    let components = agent_doc_element::element::parse(doc).ok()?;
    let mut redacted = doc.to_string();
    for component in components.iter().rev() {
        if component.name == "exchange" {
            redacted = component.replace_content(&redacted, "");
        }
    }
    Some(redacted)
}

pub fn post_commit_ipc_reposition_only_exchange_safe(parent_doc: &str, head_doc: &str) -> bool {
    let Some(parent_redacted) = redact_exchange_component_content(parent_doc) else {
        return false;
    };
    let Some(head_redacted) = redact_exchange_component_content(head_doc) else {
        return false;
    };
    parent_redacted == head_redacted
}

pub fn normalized_prompt_text(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("## User")
        || is_markdown_heading_line(trimmed)
    {
        return None;
    }
    Some(
        trimmed
            .strip_prefix('❯')
            .unwrap_or(trimmed)
            .trim()
            .to_string(),
    )
}

pub fn is_markdown_heading_line(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ')
}

pub fn normalized_prompt_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for line in exchange.lines() {
        if let Some(text) = normalized_prompt_text(line) {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

pub fn response_aware_user_prompt_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for info in exchange_prompt_reconciliation_infos(exchange, None) {
        if let Some(text) = info.normalized {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

pub fn user_prompt_count_growth(reference: &str, candidate: &str) -> usize {
    let (Some(reference_exchange), Some(candidate_exchange)) =
        (exchange_content(reference), exchange_content(candidate))
    else {
        return 0;
    };
    let reference_counts = response_aware_user_prompt_counts(reference_exchange);
    let candidate_counts = response_aware_user_prompt_counts(candidate_exchange);
    candidate_counts
        .iter()
        .map(|(line, candidate_count)| {
            let reference_count = reference_counts.get(line).copied().unwrap_or(0);
            candidate_count.saturating_sub(reference_count)
        })
        .sum()
}

pub fn exchange_has_live_user_edit(baseline: Option<&str>, before: &str) -> bool {
    let Some(base) = baseline else {
        return false;
    };
    let Some(base_exchange) = exchange_content(base) else {
        return false;
    };
    let Some(before_exchange) = exchange_content(before) else {
        return false;
    };
    strip_exchange_boundary_markers_for_dedup(base_exchange)
        != strip_exchange_boundary_markers_for_dedup(before_exchange)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExchangeShrinkGuardBlock {
    pub old_exchange_len: usize,
    pub new_exchange_len: usize,
    pub ratio: f64,
}

/// Return shrink details when a candidate document would truncate the exchange
/// below the caller's accepted ratio.
pub fn exchange_shrink_guard_block(
    content_at_start: &str,
    content_ours: &str,
    min_bytes: usize,
    max_ratio: f64,
) -> Option<ExchangeShrinkGuardBlock> {
    let old_exchange_len = exchange_content_len(content_at_start);
    let new_exchange_len = exchange_content_len(content_ours);

    if old_exchange_len < min_bytes {
        return None;
    }

    let ratio = new_exchange_len as f64 / old_exchange_len as f64;
    (ratio < max_ratio).then_some(ExchangeShrinkGuardBlock {
        old_exchange_len,
        new_exchange_len,
        ratio,
    })
}

/// True when a CPC delivery consumed the patch while live exchange edits were
/// present, but the caller lacks visible-write proof and the resulting exchange
/// text did not materialize those live edits.
pub fn live_exchange_without_visible_write_retry_required(
    baseline: Option<&str>,
    before: Option<&str>,
    after: &str,
    visible_write_proven: bool,
) -> bool {
    if visible_write_proven {
        return false;
    }
    let Some(before) = before else {
        return false;
    };
    if !exchange_has_live_user_edit(baseline, before) {
        return false;
    }
    let (Some(before_exchange), Some(after_exchange)) =
        (exchange_content(before), exchange_content(after))
    else {
        return false;
    };
    strip_exchange_boundary_markers_for_dedup(before_exchange)
        == strip_exchange_boundary_markers_for_dedup(after_exchange)
}

fn strip_exchange_boundary_markers_for_dedup(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn exchange_prompt_prefix_count(exchange: &str) -> usize {
    exchange
        .lines()
        .filter(|line| line.trim_start().starts_with("❯ "))
        .count()
}

pub fn exchange_prompt_text_duplicated(before: &str, after: &str) -> bool {
    let Some(before_exchange) = exchange_content(before) else {
        return false;
    };
    let Some(after_exchange) = exchange_content(after) else {
        return false;
    };
    let before_counts = normalized_prompt_counts(before_exchange);
    let after_counts = normalized_prompt_counts(after_exchange);
    after_counts.iter().any(|(line, after_count)| {
        let before_count = before_counts.get(line).copied().unwrap_or(0);
        before_count > 0 && *after_count > before_count
    })
}

pub fn split_line_segment(segment: &str) -> (&str, &str) {
    segment
        .strip_suffix('\n')
        .map(|line| (line, "\n"))
        .unwrap_or((segment, ""))
}

pub fn is_code_fence_delimiter(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if first != '`' && first != '~' {
        return false;
    }
    trimmed.chars().take_while(|ch| *ch == first).count() >= 3
}

pub fn normalization_target_counts(lines: &[String]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        *counts.entry(trimmed.to_string()).or_default() += 1;
    }
    counts
}

pub fn exchange_user_region(content: &str) -> &str {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut boundary_pos = content.len();
    let mut offset = 0;
    for line in content.lines() {
        if line.trim().starts_with(boundary_prefix) {
            boundary_pos = offset;
        }
        offset += line.len() + 1;
    }
    &content[..boundary_pos]
}

pub fn is_exchange_response_heading_for_prefix_repair(trimmed: &str) -> bool {
    let trimmed = trimmed.strip_prefix("❯ ").unwrap_or(trimmed);
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

pub fn is_prefixed_exchange_response_heading_for_prefix_repair(trimmed: &str) -> bool {
    let Some(stripped) = trimmed.strip_prefix("❯ ") else {
        return false;
    };
    is_exchange_response_heading_for_prefix_repair(stripped)
}

pub fn normalization_target_matches_line(
    line: &str,
    target_counts: &HashMap<String, usize>,
) -> bool {
    let normalized = line.trim_end();
    target_counts.contains_key(normalized)
        || normalized
            .strip_prefix("❯ ")
            .is_some_and(|stripped| target_counts.contains_key(stripped))
}

#[derive(Clone, Debug)]
struct ExchangeLineSegment {
    segment: String,
    line: String,
}

fn split_exchange_line_segments(content: &str) -> Vec<ExchangeLineSegment> {
    content
        .split_inclusive('\n')
        .map(|segment| {
            let line = segment
                .strip_suffix('\n')
                .map(str::to_string)
                .unwrap_or_else(|| segment.to_string());
            ExchangeLineSegment {
                segment: segment.to_string(),
                line,
            }
        })
        .collect()
}

fn line_is_exchange_boundary(trimmed: &str) -> bool {
    trimmed.starts_with("<!-- agent:boundary:")
}

fn normalized_response_signature_lines(response: Option<&str>) -> HashSet<String> {
    response
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("<!--"))
        .filter(|line| *line != "Done.")
        .map(|line| line.trim_start_matches('❯').trim().to_string())
        .collect()
}

fn exchange_response_block_matches_signature(
    segments: &[ExchangeLineSegment],
    heading_idx: usize,
    prompt_idx: usize,
    signature: &HashSet<String>,
    response: Option<&str>,
) -> bool {
    let Some(response) = response else {
        return false;
    };
    if response.trim().is_empty() {
        return false;
    }
    let heading = segments[heading_idx].line.trim();
    if response.contains(heading) {
        return true;
    }
    if signature.is_empty() {
        return false;
    }
    segments[heading_idx..prompt_idx].iter().any(|segment| {
        let normalized = segment
            .line
            .trim()
            .trim_start_matches('❯')
            .trim()
            .to_string();
        !normalized.is_empty() && signature.contains(&normalized)
    })
}

fn find_response_precedes_prompt_candidate(
    exchange_content: &str,
    response: Option<&str>,
) -> Option<(usize, usize, usize)> {
    let segments = split_exchange_line_segments(exchange_content);
    let signature = normalized_response_signature_lines(response);

    for heading_idx in 0..segments.len() {
        let heading = segments[heading_idx].line.trim();
        if !is_exchange_response_heading_for_prefix_repair(heading) {
            continue;
        }
        let mut saw_boundary_after_heading = false;
        for idx in (heading_idx + 1)..segments.len() {
            let trimmed = segments[idx].line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if line_is_exchange_boundary(trimmed) {
                saw_boundary_after_heading = true;
                continue;
            }
            if trimmed.starts_with("<!--") {
                continue;
            }
            if is_exchange_response_heading_for_prefix_repair(trimmed) {
                break;
            }
            let normalized = trimmed.trim_start_matches('❯').trim();
            let is_target = signature.contains(normalized);
            if saw_boundary_after_heading
                && line_looks_like_prompt_prefix_repair_start(trimmed, is_target)
                && exchange_response_block_matches_signature(
                    &segments,
                    heading_idx,
                    idx,
                    &signature,
                    response,
                )
            {
                let mut prompt_end = segments.len();
                for (next_idx, next) in segments.iter().enumerate().skip(idx + 1) {
                    if is_exchange_response_heading_for_prefix_repair(next.line.trim()) {
                        prompt_end = next_idx;
                        break;
                    }
                }
                return Some((heading_idx, idx, prompt_end));
            }
        }
    }
    None
}

pub fn response_precedes_prompt_in_exchange(
    doc: &str,
    response: Option<&str>,
    prompt_must_exist_in: Option<&str>,
) -> bool {
    let Some(exchange) = exchange_component(doc) else {
        return false;
    };
    let exchange_content = exchange.content(doc);
    let Some((_, prompt_idx, prompt_end)) =
        find_response_precedes_prompt_candidate(exchange_content, response)
    else {
        return false;
    };
    if let Some(required_doc) = prompt_must_exist_in {
        let segments = split_exchange_line_segments(exchange_content);
        let prompt_lines =
            normalized_non_boundary_exchange_lines(&segments[prompt_idx..prompt_end]);
        return exchange_contains_normalized_line_sequence(required_doc, &prompt_lines);
    }
    true
}

pub fn repair_response_precedes_prompt_in_exchange(
    doc: &str,
    response: Option<&str>,
    prompt_must_exist_in: Option<&str>,
) -> Result<Option<String>> {
    let components = agent_doc_element::element::parse(doc)?;
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let exchange_content = exchange.content(doc);
    let Some((heading_idx, prompt_idx, prompt_end)) =
        find_response_precedes_prompt_candidate(exchange_content, response)
    else {
        return Ok(None);
    };
    let segments = split_exchange_line_segments(exchange_content);
    if let Some(required_doc) = prompt_must_exist_in {
        let prompt_lines =
            normalized_non_boundary_exchange_lines(&segments[prompt_idx..prompt_end]);
        if !exchange_contains_normalized_line_sequence(required_doc, &prompt_lines) {
            return Ok(None);
        }
    }
    let boundary_id =
        agent_doc_element_boundary::boundary::find_boundary_id_in_component(doc, exchange);
    let boundary_marker = boundary_id
        .as_deref()
        .map(agent_doc_element_boundary::boundary::format_marker)
        .unwrap_or_else(|| {
            agent_doc_element_boundary::boundary::format_marker(
                &agent_doc_element_boundary::boundary::new_id(),
            )
        });

    let keep_non_boundary =
        |segment: &ExchangeLineSegment| !line_is_exchange_boundary(segment.line.trim());
    let prefix = segments[..heading_idx]
        .iter()
        .filter(|segment| keep_non_boundary(segment))
        .map(|segment| segment.segment.as_str())
        .collect::<String>();
    let response_block = segments[heading_idx..prompt_idx]
        .iter()
        .filter(|segment| keep_non_boundary(segment))
        .map(|segment| segment.segment.as_str())
        .collect::<String>();
    let prompt_block = segments[prompt_idx..prompt_end]
        .iter()
        .filter(|segment| keep_non_boundary(segment))
        .map(|segment| segment.segment.as_str())
        .collect::<String>();
    let suffix = segments[prompt_end..]
        .iter()
        .filter(|segment| keep_non_boundary(segment))
        .map(|segment| segment.segment.as_str())
        .collect::<String>();

    let mut repaired_exchange = String::new();
    repaired_exchange.push_str(&prefix);
    if !repaired_exchange.is_empty()
        && !repaired_exchange.ends_with('\n')
        && !prompt_block.is_empty()
    {
        repaired_exchange.push('\n');
    }
    repaired_exchange.push_str(&prompt_block);
    if !repaired_exchange.is_empty()
        && !repaired_exchange.ends_with('\n')
        && !response_block.is_empty()
    {
        repaired_exchange.push('\n');
    }
    repaired_exchange.push_str(&response_block);
    if !repaired_exchange.ends_with('\n') {
        repaired_exchange.push('\n');
    }
    repaired_exchange.push_str(&boundary_marker);
    repaired_exchange.push('\n');
    repaired_exchange.push_str(&suffix);

    let repaired = exchange.replace_content(doc, &repaired_exchange);
    if repaired == doc {
        return Ok(None);
    }
    Ok(Some(repaired))
}

/// Strip leaked harness user-prompt markers (`❯ `) from response body lines of
/// every `### Re: ...` response block inside `agent:exchange`.
///
/// Response bodies do not use `❯ ` as a paragraph marker. Strip a leading run
/// of prefixed response-body lines until the first unprefixed body line, and
/// also strip later prefixed proof/list lines that the response classifier
/// recognizes as assistant-owned content. Prompt-like `❯ ` text after the
/// response body starts is preserved as quoted/user-visible prose. Returns
/// `Some(repaired)` when any prefix was stripped, `None` when the document is
/// clean.
pub fn strip_prompt_prefix_from_response_body_first_lines(content: &str) -> Option<String> {
    let exchange = exchange_component(content)?;
    let exchange_body = exchange.content(content);

    let mut repaired_lines: Vec<String> = Vec::with_capacity(exchange_body.lines().count());
    let mut in_response_block = false;
    let mut saw_unprefixed_response_body_line = false;
    let mut stripped_any = false;
    for line in exchange_body.lines() {
        let trimmed_start = line.trim_start();
        let is_response_heading = trimmed_start.starts_with("### Re:");
        let is_other_heading = trimmed_start.starts_with("###") && !is_response_heading
            || trimmed_start.starts_with("## ")
            || trimmed_start.starts_with("# ");
        let is_exchange_marker = trimmed_start.starts_with("<!-- agent:")
            || trimmed_start.starts_with("<!-- /agent:")
            || trimmed_start.starts_with("<!-- agent:boundary:");

        if is_response_heading {
            in_response_block = true;
            saw_unprefixed_response_body_line = false;
            repaired_lines.push(line.to_string());
            continue;
        }
        if is_other_heading || is_exchange_marker {
            in_response_block = false;
            saw_unprefixed_response_body_line = false;
            repaired_lines.push(line.to_string());
            continue;
        }
        if in_response_block && !line.trim().is_empty() {
            if !saw_unprefixed_response_body_line
                && line_looks_like_prompt_prefix_repair_start(trimmed_start, false)
            {
                in_response_block = false;
                saw_unprefixed_response_body_line = false;
                repaired_lines.push(line.to_string());
                continue;
            }
            if let Some(rest) = line.strip_prefix("❯ ") {
                let response_shaped_tail =
                    line_looks_like_plain_response_after_prompt(rest.trim_start());
                if !saw_unprefixed_response_body_line || response_shaped_tail {
                    stripped_any = true;
                    repaired_lines.push(rest.to_string());
                    continue;
                }
            }
            if line.trim_start() == "❯" && !saw_unprefixed_response_body_line {
                stripped_any = true;
                repaired_lines.push(String::new());
                continue;
            }
            saw_unprefixed_response_body_line = true;
        }
        repaired_lines.push(line.to_string());
    }
    if !stripped_any {
        return None;
    }
    let mut repaired_body = repaired_lines.join("\n");
    if exchange_body.ends_with('\n') && !repaired_body.ends_with('\n') {
        repaired_body.push('\n');
    }
    Some(exchange.replace_content(content, &repaired_body))
}

fn normalized_non_boundary_exchange_lines(segments: &[ExchangeLineSegment]) -> Vec<String> {
    segments
        .iter()
        .filter_map(|segment| {
            let trimmed = segment.line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("<!--")
                || line_is_exchange_boundary(trimmed)
            {
                return None;
            }
            Some(trimmed.trim_start_matches('❯').trim().to_string())
        })
        .collect()
}

fn exchange_contains_normalized_line_sequence(doc: &str, needle: &[String]) -> bool {
    if needle.is_empty() {
        return false;
    }
    let Some(exchange) = exchange_component(doc) else {
        return false;
    };
    let haystack = split_exchange_line_segments(exchange.content(doc));
    let haystack = normalized_non_boundary_exchange_lines(&haystack);
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

pub fn exchange_prompt_prefix_eligible_lines<'a>(
    content: &'a str,
    target_counts: Option<&HashMap<String, usize>>,
) -> Vec<&'a str> {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut eligible = Vec::new();
    let mut in_response_block = false;
    let mut response_heading_was_prefixed = false;

    for line in exchange_user_region(content).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(boundary_prefix) {
            in_response_block = false;
            response_heading_was_prefixed = false;
            continue;
        }
        if is_exchange_response_heading_for_prefix_repair(trimmed) {
            in_response_block = true;
            response_heading_was_prefixed =
                is_prefixed_exchange_response_heading_for_prefix_repair(trimmed);
            continue;
        }
        if line_looks_like_markdown_list_item(trimmed) {
            continue;
        }

        let is_target =
            target_counts.is_some_and(|counts| normalization_target_matches_line(line, counts));
        if in_response_block {
            let starts_prompt = if target_counts.is_some() {
                line_looks_like_targeted_or_prefixed_prompt_repair_start(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                )
            } else {
                line_looks_like_prompt_prefix_repair_start(trimmed, false)
            };
            if starts_prompt {
                in_response_block = false;
                response_heading_was_prefixed = false;
            } else {
                continue;
            }
        }

        eligible.push(line);
    }

    eligible
}

#[derive(Clone, Debug)]
pub struct PromptLineInfo {
    pub segment: String,
    pub normalized: Option<String>,
    pub prefixed: bool,
    pub remove: bool,
}

pub fn exchange_prompt_reconciliation_infos(
    exchange: &str,
    target_counts: Option<&HashMap<String, usize>>,
) -> Vec<PromptLineInfo> {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut in_response_block = false;
    let mut response_heading_was_prefixed = false;
    let mut in_code_fence = false;
    let mut infos = Vec::new();

    for segment in exchange.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        let trimmed = line.trim();
        let is_fence = is_code_fence_delimiter(trimmed);
        let was_in_code_fence = in_code_fence;
        let mut eligible = !(was_in_code_fence || is_fence);
        if eligible {
            if trimmed.starts_with(boundary_prefix) {
                in_response_block = false;
                response_heading_was_prefixed = false;
                eligible = false;
            } else if is_exchange_response_heading_for_prefix_repair(trimmed) {
                in_response_block = true;
                response_heading_was_prefixed =
                    is_prefixed_exchange_response_heading_for_prefix_repair(trimmed);
                eligible = false;
            } else if in_response_block {
                let is_target = target_counts
                    .is_some_and(|counts| normalization_target_matches_line(line, counts));
                if line_looks_like_targeted_or_prefixed_prompt_repair_start(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                ) {
                    in_response_block = false;
                    response_heading_was_prefixed = false;
                } else {
                    eligible = false;
                }
            }
        }

        let normalized = if eligible {
            normalized_prompt_text(line)
        } else {
            None
        };
        infos.push(PromptLineInfo {
            segment: segment.to_string(),
            normalized,
            prefixed: trimmed.starts_with("❯ "),
            remove: false,
        });
        if is_fence {
            in_code_fence = !in_code_fence;
        }
    }

    infos
}

pub fn prompt_reconciliation_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for info in exchange_prompt_reconciliation_infos(exchange, None) {
        if let Some(text) = info.normalized {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

pub fn last_exchange_boundary_tail_start(exchange: &str) -> Option<usize> {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut offset = 0usize;
    let mut tail_start = None;
    for segment in exchange.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        if line.trim().starts_with(boundary_prefix) {
            tail_start = Some(offset + segment.len());
        }
        offset += segment.len();
    }
    tail_start
}

pub fn probable_live_prompt_prefix_variant(shorter: &str, longer: &str) -> bool {
    let shorter = shorter.trim();
    let longer = longer.trim();
    if shorter.len() < 16 || longer.len() <= shorter.len() + 2 {
        return false;
    }
    if !longer.starts_with(shorter) || !longer.is_char_boundary(shorter.len()) {
        return false;
    }
    if matches!(
        shorter.chars().last(),
        Some('.' | '!' | '?' | ':' | ';' | ')' | ']')
    ) {
        return false;
    }
    true
}

pub fn dedupe_live_prompt_prefix_variants_in_exchange_tail(exchange: &str) -> Option<String> {
    let tail_start = last_exchange_boundary_tail_start(exchange)?;
    let tail = &exchange[tail_start..];
    if tail.trim().is_empty() {
        return None;
    }

    #[derive(Clone, Debug)]
    struct TailLine {
        segment: String,
        normalized: Option<String>,
        remove: bool,
    }

    let mut in_fence = false;
    let mut lines = Vec::<TailLine>::new();
    for segment in tail.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        let trimmed = line.trim();
        let is_fence = is_code_fence_delimiter(trimmed);
        let normalized = if !in_fence && !is_fence {
            normalized_prompt_text(line)
        } else {
            None
        };
        lines.push(TailLine {
            segment: segment.to_string(),
            normalized,
            remove: false,
        });
        if is_fence {
            in_fence = !in_fence;
        }
    }
    if !tail.ends_with('\n') && !tail.is_empty() {
        let consumed: usize = lines.iter().map(|line| line.segment.len()).sum();
        if consumed < tail.len() {
            let rest = &tail[consumed..];
            lines.push(TailLine {
                segment: rest.to_string(),
                normalized: normalized_prompt_text(rest),
                remove: false,
            });
        }
    }

    let mut changed = false;
    for idx in 0..lines.len().saturating_sub(1) {
        if lines[idx].remove || lines[idx + 1].remove {
            continue;
        }
        let Some(left) = lines[idx].normalized.as_deref() else {
            continue;
        };
        let Some(right) = lines[idx + 1].normalized.as_deref() else {
            continue;
        };
        let left_prefixed = lines[idx].segment.trim_start().starts_with("❯ ");
        let right_prefixed = lines[idx + 1].segment.trim_start().starts_with("❯ ");
        if left == right && left_prefixed != right_prefixed {
            if left_prefixed {
                lines[idx + 1].remove = true;
            } else {
                lines[idx].remove = true;
            }
            changed = true;
        } else if probable_live_prompt_prefix_variant(left, right) {
            lines[idx].remove = true;
            changed = true;
        } else if probable_live_prompt_prefix_variant(right, left) {
            lines[idx + 1].remove = true;
            changed = true;
        }
    }

    if !changed {
        return None;
    }

    let repaired_tail = lines
        .into_iter()
        .filter(|line| !line.remove)
        .map(|line| line.segment)
        .collect::<String>();
    Some(format!("{}{}", &exchange[..tail_start], repaired_tail))
}

pub fn dedupe_live_prompt_prefix_variants_in_doc(content: &str) -> Option<String> {
    let exchange = exchange_component(content)?;
    let repaired_exchange =
        dedupe_live_prompt_prefix_variants_in_exchange_tail(exchange.content(content))?;
    Some(exchange.replace_content(content, &repaired_exchange))
}

pub fn dedupe_adjacent_prompt_prefix_duplicates_in_exchange(exchange: &str) -> Option<String> {
    let mut lines = exchange_prompt_reconciliation_infos(exchange, None);
    let mut changed = false;

    for idx in 0..lines.len().saturating_sub(1) {
        if lines[idx].remove || lines[idx + 1].remove {
            continue;
        }
        let Some(left) = lines[idx].normalized.as_deref() else {
            continue;
        };
        let Some(right) = lines[idx + 1].normalized.as_deref() else {
            continue;
        };
        if left == right && lines[idx].prefixed != lines[idx + 1].prefixed {
            if lines[idx].prefixed {
                lines[idx + 1].remove = true;
            } else {
                lines[idx].remove = true;
            }
            changed = true;
        }
    }

    if !changed {
        return None;
    }

    Some(
        lines
            .into_iter()
            .filter(|line| !line.remove)
            .map(|line| line.segment)
            .collect::<String>(),
    )
}

pub fn dedupe_adjacent_prompt_prefix_duplicates_in_doc(content: &str) -> Option<String> {
    let exchange = exchange_component(content)?;
    let repaired_exchange =
        dedupe_adjacent_prompt_prefix_duplicates_in_exchange(exchange.content(content))?;
    Some(exchange.replace_content(content, &repaired_exchange))
}

pub fn dedupe_prompt_lines_against_before_exchange(
    before_exchange: &str,
    after_exchange: &str,
) -> Option<String> {
    let before_counts = prompt_reconciliation_counts(before_exchange);
    if before_counts.is_empty() {
        return None;
    }
    let mut lines = exchange_prompt_reconciliation_infos(after_exchange, Some(&before_counts));

    let mut by_text: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, line) in lines.iter().enumerate() {
        if let Some(text) = line.normalized.as_ref() {
            by_text.entry(text.clone()).or_default().push(idx);
        }
    }

    let mut changed = false;
    for (text, indexes) in by_text {
        let allowed = before_counts.get(&text).copied().unwrap_or(0);
        if allowed == 0 || indexes.len() <= allowed {
            continue;
        }

        let mut excess = indexes.len() - allowed;
        if indexes.iter().any(|idx| lines[*idx].prefixed) {
            let unprefixed_indexes: Vec<usize> = indexes
                .iter()
                .copied()
                .filter(|idx| !lines[*idx].prefixed)
                .collect();
            for idx in unprefixed_indexes {
                if excess == 0 {
                    break;
                }
                lines[idx].remove = true;
                excess -= 1;
                changed = true;
            }
        }
        if excess > 0 {
            for idx in indexes.iter().rev().copied() {
                if excess == 0 {
                    break;
                }
                if lines[idx].remove {
                    continue;
                }
                lines[idx].remove = true;
                excess -= 1;
                changed = true;
            }
        }
    }

    if !changed {
        return None;
    }

    Some(
        lines
            .into_iter()
            .filter(|line| !line.remove)
            .map(|line| line.segment)
            .collect::<String>(),
    )
}

pub fn dedupe_prompt_lines_against_before_doc(before: &str, after: &str) -> Option<String> {
    let before_exchange = exchange_content(before)?;
    let after_exchange = exchange_component(after)?;
    let repaired_exchange = dedupe_prompt_lines_against_before_exchange(
        before_exchange,
        after_exchange.content(after),
    )?;
    Some(after_exchange.replace_content(after, &repaired_exchange))
}

/// Compare the committed/snapshot document against the working tree and return
/// exchange user-region lines that should regain a missing `❯ ` prefix.
pub fn extract_post_commit_normalization_targets(committed: &str, working: &str) -> Vec<String> {
    let committed_exc = exchange_content(committed).unwrap_or("");
    let working_exc = exchange_content(working).unwrap_or("");

    if committed_exc == working_exc {
        return vec![];
    }

    let mut working_prefixed = HashMap::<String, usize>::new();
    let mut working_unprefixed = HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(working_exc, None) {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix("❯ ") {
            *working_prefixed.entry(stripped.to_string()).or_default() += 1;
        } else {
            *working_unprefixed.entry(trimmed.to_string()).or_default() += 1;
        }
    }

    let mut committed_prefixed = HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(committed_exc, None) {
        let Some(stripped) = line.strip_prefix("❯ ") else {
            continue;
        };
        let normalized = stripped.trim_end();
        if normalized.is_empty() {
            continue;
        }
        *committed_prefixed
            .entry(normalized.to_string())
            .or_default() += 1;
    }

    let mut missing_counts = HashMap::<String, usize>::new();
    for (line, committed_count) in committed_prefixed {
        let working_prefixed_count = working_prefixed.get(&line).copied().unwrap_or(0);
        let working_unprefixed_count = working_unprefixed.get(&line).copied().unwrap_or(0);
        let missing = committed_count.saturating_sub(working_prefixed_count);
        let repairable = missing.min(working_unprefixed_count);
        if repairable > 0 {
            missing_counts.insert(line, repairable);
        }
    }

    let mut targets = Vec::new();
    for line in exchange_prompt_prefix_eligible_lines(committed_exc, None) {
        let Some(stripped) = line.strip_prefix("❯ ") else {
            continue;
        };
        let normalized = stripped.trim_end();
        let Some(remaining) = missing_counts.get_mut(normalized) else {
            continue;
        };
        if *remaining == 0 {
            continue;
        }
        targets.push(stripped.to_string());
        *remaining -= 1;
    }

    targets
}

/// Apply `❯ ` prefix normalization to matching lines in the exchange user
/// region of a full document.
pub fn normalize_exchange_prefixes_for_targets(doc: &str, prefix_lines: &[String]) -> String {
    if prefix_lines.is_empty() {
        return doc.to_string();
    }

    let open_tag = "<!-- agent:exchange";
    let close_tag = "<!-- /agent:exchange -->";
    let boundary_prefix = "<!-- agent:boundary:";

    let Some(open_match) = doc.find(open_tag) else {
        return doc.to_string();
    };
    let Some(close_idx) = doc[open_match..]
        .find(close_tag)
        .map(|idx| open_match + idx)
    else {
        return doc.to_string();
    };
    let Some(open_end) = doc[open_match..]
        .find("-->")
        .map(|idx| open_match + idx + 3)
    else {
        return doc.to_string();
    };

    let before_exchange = &doc[..open_end];
    let exchange_content = &doc[open_end..close_idx];
    let after_exchange = &doc[close_idx..];

    let mut user_region_end = exchange_content.len();
    let mut offset = 0;
    for line in exchange_content.lines() {
        if line.trim().starts_with(boundary_prefix) {
            user_region_end = offset;
        }
        offset += line.len() + 1;
    }
    let user_region = &exchange_content[..user_region_end];
    let agent_region = &exchange_content[user_region_end..];

    let mut remaining = normalization_target_counts(prefix_lines);
    if remaining.is_empty() {
        return doc.to_string();
    }

    let mut in_response_block = false;
    let mut response_heading_was_prefixed = false;
    let normalized_user_region = user_region
        .split('\n')
        .map(|doc_line| {
            let trimmed = doc_line.trim();
            if trimmed.starts_with(boundary_prefix) {
                in_response_block = false;
                response_heading_was_prefixed = false;
                return doc_line.to_string();
            }
            if is_exchange_response_heading_for_prefix_repair(trimmed) {
                in_response_block = true;
                response_heading_was_prefixed =
                    is_prefixed_exchange_response_heading_for_prefix_repair(trimmed);
                return doc_line.to_string();
            }
            let normalized = doc_line.trim_end();
            let is_target = normalization_target_matches_line(doc_line, &remaining);
            if in_response_block {
                if line_looks_like_targeted_or_prefixed_prompt_repair_start(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                ) {
                    in_response_block = false;
                    response_heading_was_prefixed = false;
                } else {
                    return doc_line.to_string();
                }
            }
            if normalized.starts_with("❯ ")
                || line_looks_like_plain_response_after_prompt(normalized)
            {
                return doc_line.to_string();
            }
            let Some(remaining_count) = remaining.get_mut(normalized) else {
                return doc_line.to_string();
            };
            if *remaining_count == 0 {
                return doc_line.to_string();
            }
            *remaining_count -= 1;
            format!("❯ {doc_line}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("{before_exchange}{normalized_user_region}{agent_region}{after_exchange}")
}

/// Return required and observed prompt-prefix counts for normalization targets.
///
/// Falls back to inspecting the full content when the exchange component cannot
/// be parsed or found.
pub fn normalization_prefix_observation_counts(
    content: &str,
    normalize_prefix_lines: &[String],
) -> (usize, usize) {
    let target_counts = normalization_target_counts(normalize_prefix_lines);
    let required = target_counts.values().sum();
    if required == 0 {
        return (0, 0);
    }

    let exchange = exchange_content(content).unwrap_or(content);
    let mut observed_counts = HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(exchange, Some(&target_counts)) {
        let Some(stripped) = line.trim_end().strip_prefix("❯ ") else {
            continue;
        };
        if target_counts.contains_key(stripped) {
            *observed_counts.entry(stripped.to_string()).or_default() += 1;
        }
    }

    let observed = target_counts
        .iter()
        .map(|(target, required)| {
            observed_counts
                .get(target)
                .copied()
                .unwrap_or(0)
                .min(*required)
        })
        .sum();
    (required, observed)
}

/// Count duplicate eligible prompt lines after normalizing optional `❯ ` prefixes.
///
/// Falls back to inspecting the full content when the exchange component cannot
/// be parsed or found.
pub fn duplicate_prompt_line_count(content: &str) -> usize {
    let exchange = exchange_content(content).unwrap_or(content);
    let mut counts = HashMap::<String, usize>::new();
    let mut duplicates = 0;
    for line in exchange_prompt_prefix_eligible_lines(exchange, None) {
        let normalized = line
            .trim_end()
            .strip_prefix("❯ ")
            .unwrap_or(line.trim_end())
            .trim();
        if normalized.is_empty() {
            continue;
        }
        let count = counts.entry(normalized.to_string()).or_default();
        *count += 1;
        if *count > 1 {
            duplicates += 1;
        }
    }
    duplicates
}

/// Extract lines that were normalized by [`normalize_user_prompts_in_exchange`].
///
/// Compares the exchange content line-by-line and returns lines where `before`
/// had plain text and `after` has `❯ <text>` at the same position. Positional
/// comparison preserves duplicate prompt lines.
pub fn extract_normalization_targets(before: &str, after: &str) -> Vec<String> {
    let before_exc = exchange_content(before).unwrap_or("");
    let after_exc = exchange_content(after).unwrap_or("");

    if before_exc == after_exc {
        return vec![];
    }

    let mut targets = Vec::new();
    for (before_line, after_line) in before_exc.lines().zip(after_exc.lines()) {
        if let Some(stripped) = after_line.strip_prefix("❯ ")
            && before_line == stripped
        {
            targets.push(stripped.to_string());
        }
    }
    targets
}

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
    let fl = trimmed.chars().take_while(|&c| c == fc).count();
    fl >= fence_len && trimmed[fl..].trim().is_empty()
}

fn heading_level(trimmed: &str) -> Option<usize> {
    let n = trimmed.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&n) && trimmed.as_bytes().get(n) == Some(&b' ') {
        Some(n)
    } else {
        None
    }
}

fn strip_exchange_boundary_markers(content: &str) -> String {
    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
        .collect();
    let mut out = filtered.join("\n");
    if content.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Add `❯ ` to user-added exchange lines by comparing the current baseline to
/// the previous snapshot. This is a pure document transformation; effectful
/// safety rails and logging live in orchestration.
pub fn normalize_user_prompts_in_exchange(content: &str, baseline: &str, snapshot: &str) -> String {
    let Ok(content_comps) = agent_doc_element::element::parse(content) else {
        return content.to_string();
    };
    let baseline_comps = agent_doc_element::element::parse(baseline).unwrap_or_default();
    let snap_comps = agent_doc_element::element::parse(snapshot).unwrap_or_default();

    let Some(exchange) = content_comps.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };

    let baseline_exc = baseline_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|e| e.content(baseline))
        .unwrap_or("");
    let snap_exc = snap_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|e| e.content(snapshot))
        .unwrap_or("");

    let exc_content = exchange.content(content);
    let content_user_region = exchange_user_region(exc_content);
    let content_agent_region = &exc_content[content_user_region.len()..];

    let baseline_stripped = strip_exchange_boundary_markers(baseline_exc);
    let snap_stripped = strip_exchange_boundary_markers(snap_exc);
    let diff_text = agent_doc_diff::unified_diff_from_contents(&snap_stripped, &baseline_stripped);
    let prompt_prefix_targets = diff_text
        .as_deref()
        .map(agent_doc_diff::prompt_prefix_normalization_targets)
        .unwrap_or_default();

    let diff = TextDiff::from_lines(snap_stripped.as_str(), baseline_stripped.as_str());
    let mut user_added = HashSet::<String>::new();
    let mut agent_inserted = HashSet::<String>::new();
    let mut in_baseline_fence = false;
    let mut baseline_fence_char = '`';
    let mut baseline_fence_len = 3usize;
    let mut in_agent_block = false;
    let mut saw_deleted_heading = false;
    let mut in_re_block = false;
    let mut re_block_saw_body_delete = false;
    for change in diff.iter_all_changes() {
        let line = change.value().trim_end_matches('\n');
        let trimmed = line.trim();
        let is_heading = heading_level(trimmed).is_some();
        let was_in_fence = in_baseline_fence;
        if change.tag() == ChangeTag::Delete {
            saw_deleted_heading = !in_baseline_fence && is_heading;
            if in_re_block
                && !in_baseline_fence
                && !is_heading
                && !trimmed.is_empty()
                && !trimmed.starts_with("<!--")
                && fence_open(trimmed).is_none()
            {
                re_block_saw_body_delete = true;
            }
            continue;
        }
        let heading_replaces_deleted_heading =
            change.tag() == ChangeTag::Insert && is_heading && saw_deleted_heading;
        saw_deleted_heading = false;
        if change.tag() != ChangeTag::Delete {
            if !in_baseline_fence {
                if let Some((fc, fl)) = fence_open(trimmed) {
                    in_baseline_fence = true;
                    baseline_fence_char = fc;
                    baseline_fence_len = fl;
                }
            } else if fence_close(trimmed, baseline_fence_char, baseline_fence_len) {
                in_baseline_fence = false;
            }
            if !in_baseline_fence {
                if heading_level(trimmed).is_some() {
                    in_agent_block =
                        change.tag() == ChangeTag::Insert && !heading_replaces_deleted_heading;
                    in_re_block = trimmed.starts_with("### Re:");
                    re_block_saw_body_delete = false;
                } else if in_agent_block && trimmed.is_empty() {
                    // Blank assistant-response lines do not prove the next line is user input.
                } else if in_agent_block
                    && (line_looks_like_targeted_prompt_prefix_repair_start(trimmed, true)
                        || trimmed.starts_with('❯')
                        || trimmed.starts_with("<!--"))
                {
                    in_agent_block = false;
                }
                if in_re_block
                    && (line_looks_like_targeted_prompt_prefix_repair_start(trimmed, true)
                        || trimmed.starts_with('❯'))
                {
                    in_re_block = false;
                    re_block_saw_body_delete = false;
                }
            }
        }
        let is_fence_delim = fence_open(trimmed).is_some()
            || (was_in_fence && fence_close(trimmed, baseline_fence_char, baseline_fence_len));
        let is_re_block_replacement = in_re_block && re_block_saw_body_delete;
        if change.tag() == ChangeTag::Insert
            && !in_baseline_fence
            && !in_agent_block
            && !is_re_block_replacement
            && !heading_replaces_deleted_heading
            && !trimmed.is_empty()
            && !trimmed.starts_with('❯')
            && !trimmed.starts_with("<!--")
            && !is_fence_delim
            && !agent_doc_diff::line_is_binary_authored_compact_summary(trimmed)
        {
            user_added.insert(line.to_string());
        } else if change.tag() == ChangeTag::Insert && (in_agent_block || is_re_block_replacement) {
            agent_inserted.insert(line.to_string());
        }
    }

    for line in prompt_prefix_targets {
        if !agent_inserted.contains(&line) {
            user_added.insert(line);
        }
    }
    if user_added.is_empty() {
        return content.to_string();
    }

    let mut in_content_fence = false;
    let mut content_fence_char = '`';
    let mut content_fence_len = 3usize;
    let mut normalized_user = String::new();
    for line in content_user_region.lines() {
        let trimmed = line.trim();
        if !in_content_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_content_fence = true;
                content_fence_char = fc;
                content_fence_len = fl;
            }
        } else if fence_close(trimmed, content_fence_char, content_fence_len) {
            in_content_fence = false;
        }
        if !in_content_fence && user_added.contains(line) {
            normalized_user.push_str("❯ ");
        }
        normalized_user.push_str(line);
        normalized_user.push('\n');
    }
    if !content_user_region.is_empty() && !content_user_region.ends_with('\n') {
        normalized_user.truncate(normalized_user.len() - 1);
    }
    if content_user_region.is_empty() {
        normalized_user.clear();
    }

    exchange.replace_content(content, &format!("{normalized_user}{content_agent_region}"))
}

/// Preserve committed exchange prompt-prefix state after normalization repairs.
pub fn preserve_head_exchange_prompt_prefix_state(content: &str, head: &str) -> String {
    let Some(head_exchange) = exchange_component(head) else {
        return content.to_string();
    };
    let mut head_unprefixed = HashMap::<String, usize>::new();
    let mut head_prefixed = HashMap::<String, usize>::new();
    for line in head_exchange.content(head).lines() {
        let line = line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('❯')
            || trimmed.starts_with("<!--")
            || is_exchange_response_heading_for_prefix_repair(trimmed)
        {
            continue;
        }
        *head_unprefixed.entry(line.to_string()).or_default() += 1;
    }
    for line in exchange_prompt_prefix_eligible_lines(head_exchange.content(head), None) {
        let line = line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        if let Some(stripped) = line.strip_prefix("❯ ") {
            *head_prefixed.entry(stripped.to_string()).or_default() += 1;
        }
    }
    if head_unprefixed.is_empty() && head_prefixed.is_empty() {
        return content.to_string();
    }

    let Some(exchange) = exchange_component(content) else {
        return content.to_string();
    };
    let exchange_content = exchange.content(content);
    let mut changed = false;
    let mut rebuilt = String::with_capacity(exchange_content.len());
    let target_counts =
        normalization_target_counts(&head_prefixed.keys().cloned().collect::<Vec<String>>());
    let mut in_response_block = false;
    for segment in exchange_content.split_inclusive('\n') {
        let (line, newline) = split_line_segment(segment);
        let trimmed = line.trim();
        if trimmed.starts_with("<!-- agent:boundary:") {
            in_response_block = false;
        } else if is_exchange_response_heading_for_prefix_repair(trimmed) {
            in_response_block = true;
        }
        let is_target = target_counts
            .get(line.trim_end())
            .copied()
            .unwrap_or_default()
            > 0;
        let eligible = if in_response_block {
            line_looks_like_prompt_prefix_repair_start(trimmed, is_target)
        } else {
            true
        };
        if let Some(unprefixed) = line.strip_prefix("❯ ")
            && let Some(remaining) = head_unprefixed.get_mut(unprefixed)
            && *remaining > 0
        {
            rebuilt.push_str(unprefixed);
            *remaining -= 1;
            changed = true;
        } else if eligible
            && !line.starts_with("❯ ")
            && let Some(remaining) = head_prefixed.get_mut(line)
            && *remaining > 0
        {
            rebuilt.push_str("❯ ");
            rebuilt.push_str(line);
            *remaining -= 1;
            changed = true;
        } else {
            rebuilt.push_str(line);
        }
        if in_response_block
            && eligible
            && line_looks_like_prompt_prefix_repair_start(trimmed, is_target)
        {
            in_response_block = false;
        }
        rebuilt.push_str(newline);
    }
    if !changed {
        return content.to_string();
    }
    exchange.replace_content(content, &rebuilt)
}

/// Verify that editor-visible content preserved every expected `❯ ` prompt prefix.
pub fn verify_visible_normalization(visible: &str, normalize_prefix_lines: &[String]) -> bool {
    if normalize_prefix_lines.is_empty() {
        return true;
    }

    let visible_exchange = exchange_content(visible)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| visible.to_string());
    let target_counts = normalization_target_counts(normalize_prefix_lines);

    let mut prefixed_counts = HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(&visible_exchange, Some(&target_counts)) {
        let trimmed = line.trim_end();
        if let Some(stripped) = trimmed.strip_prefix("❯ ") {
            *prefixed_counts.entry(stripped.to_string()).or_default() += 1;
        }
    }

    for (target, required) in target_counts {
        if prefixed_counts.get(&target).copied().unwrap_or(0) < required {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_content_len_reports_trimmed_exchange_body() {
        let doc = "<!-- agent:exchange -->\nHello world\n<!-- /agent:exchange -->\n";
        assert_eq!(exchange_content_len(doc), "Hello world".len());

        let empty = "<!-- agent:exchange -->\n\n<!-- /agent:exchange -->\n";
        assert_eq!(exchange_content_len(empty), 0);

        let no_exchange = "Just text.";
        assert_eq!(exchange_content_len(no_exchange), 0);
    }

    #[test]
    fn insert_prompt_line_before_boundary_inserts_before_latest_boundary() {
        let doc = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "old response\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let updated = insert_prompt_line_before_boundary(doc, "❯ do #gkke").unwrap();

        let prompt_pos = updated.find("❯ do #gkke").unwrap();
        let boundary_pos = updated.find("<!-- agent:boundary:keep -->").unwrap();
        assert!(prompt_pos < boundary_pos);
    }

    #[test]
    fn normalized_prompt_text_ignores_exchange_structure() {
        assert_eq!(
            normalized_prompt_text("❯ ship it").as_deref(),
            Some("ship it")
        );
        assert_eq!(
            normalized_prompt_text("ship it").as_deref(),
            Some("ship it")
        );
        assert_eq!(normalized_prompt_text("### Re: ship it"), None);
        assert_eq!(normalized_prompt_text("## User"), None);
        assert_eq!(normalized_prompt_text("## Heading"), None);
        assert_eq!(normalized_prompt_text("<!-- agent:boundary:x -->"), None);
    }

    #[test]
    fn normalized_prompt_counts_counts_equivalent_prefixed_lines() {
        let counts = normalized_prompt_counts("❯ ship it\nship it\n### Re: ship it\n");

        assert_eq!(counts.get("ship it").copied(), Some(2));
    }

    #[test]
    fn response_aware_counts_skip_response_blocks_and_fences() {
        let exchange = concat!(
            "❯ ship it\n",
            "### Re: ship it\n",
            "assistant text\n",
            "❯ do next\n",
            "```\n",
            "not a prompt\n",
            "```\n",
        );

        let counts = response_aware_user_prompt_counts(exchange);

        assert_eq!(counts.get("ship it").copied(), Some(1));
        assert_eq!(counts.get("do next").copied(), Some(1));
        assert!(!counts.contains_key("assistant text"));
        assert!(!counts.contains_key("not a prompt"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_strips_leaked_marker() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #respfx. spec-test-build-install-commit-push
### Re: #respfx — opus-4-7

❯ Landed Phase 1 only this cycle. Item stays open.

#### Details

`agent-doc <FILE>` now accepts `--wait-for-ready <SECONDS>`.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leaked prompt marker on response body first line must be stripped");
        assert!(
            repaired.contains("\nLanded Phase 1 only this cycle. Item stays open.\n"),
            "stripped response body should start with original prose, got:\n{repaired}"
        );
        assert!(
            !repaired.contains("❯ Landed"),
            "leaked marker must be removed, got:\n{repaired}"
        );
        assert!(repaired.contains("❯ do #respfx. spec-test-build-install-commit-push"));
        assert!(repaired.contains("### Re: #respfx — opus-4-7"));
        assert!(repaired.contains("#### Details"));
        assert!(repaired.contains("`agent-doc <FILE>` now accepts `--wait-for-ready <SECONDS>`."));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_strips_leading_run() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #leading-run. spec-test-build-install-commit-push
### Re: #leading-run — gpt-5

❯ First response paragraph.

❯ Second response paragraph.
❯ - Proof line.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leading response-body prompt markers must be stripped");

        assert!(repaired.contains("\nFirst response paragraph.\n"));
        assert!(repaired.contains("\nSecond response paragraph.\n- Proof line.\n"));
        assert!(!repaired.contains("❯ First response paragraph."));
        assert!(!repaired.contains("❯ Second response paragraph."));
        assert!(!repaired.contains("❯ - Proof line."));
        assert!(repaired.contains("❯ do #leading-run. spec-test-build-install-commit-push"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_skips_when_clean() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #clean. spec-test-build-install-commit-push
### Re: #clean — opus-4-7

Landed cleanly.
<!-- /agent:exchange -->
";
        let result = strip_prompt_prefix_from_response_body_first_lines(content);
        assert!(
            result.is_none(),
            "clean document must not trigger the strip path"
        );
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_preserves_inner_prompt_like_lines() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #inner. spec-test-build-install-commit-push
### Re: #inner — opus-4-7

❯ first line gets stripped

The user said:
❯ this quoted line stays
because it is not the first body line.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leaked first-line prompt marker must be stripped");
        assert!(repaired.contains("\nfirst line gets stripped\n"));
        assert!(!repaired.contains("❯ first line gets stripped"));
        assert!(repaired.contains("❯ this quoted line stays"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_strips_late_proof_lines() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #tail. spec-test-build-install-commit-push
### Re: #tail — gpt-5

Changed behavior:
❯ - First proof line.
❯ - Second proof line.

Verification passed:
❯ - `make check`

The user said:
❯ this quoted line stays
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("late response proof lines must be stripped");

        assert!(repaired.contains("\n- First proof line.\n- Second proof line.\n"));
        assert!(repaired.contains("\n- `make check`\n"));
        assert!(!repaired.contains("❯ - First proof line."));
        assert!(!repaired.contains("❯ - Second proof line."));
        assert!(!repaired.contains("❯ - `make check`"));
        assert!(repaired.contains("❯ this quoted line stays"));
        assert!(repaired.contains("❯ do #tail. spec-test-build-install-commit-push"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_handles_multiple_re_blocks() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #a
### Re: #a — opus-4-7

❯ first response

❯ do #b
### Re: #b — opus-4-7

❯ second response
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("multiple leaks must be stripped");
        assert!(repaired.contains("\nfirst response\n"));
        assert!(repaired.contains("\nsecond response\n"));
        assert!(!repaired.contains("❯ first response"));
        assert!(!repaired.contains("❯ second response"));
        assert!(repaired.contains("❯ do #a"));
        assert!(repaired.contains("❯ do #b"));
    }

    #[test]
    fn prompt_growth_counts_new_response_aware_prompt_instances() {
        let reference = "\
<!-- agent:exchange -->
❯ ship it
<!-- /agent:exchange -->
";
        let candidate = "\
<!-- agent:exchange -->
❯ ship it
ship it
### Re: ship it
assistant text
<!-- /agent:exchange -->
";

        assert_eq!(user_prompt_count_growth(reference, candidate), 1);
    }

    #[test]
    fn exchange_live_user_edit_ignores_boundary_id_churn() {
        let baseline = "\
<!-- agent:exchange -->
same prompt
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let boundary_only = "\
<!-- agent:exchange -->
same prompt
<!-- agent:boundary:new -->
<!-- /agent:exchange -->
";
        let edited = "\
<!-- agent:exchange -->
same prompt
new prompt
<!-- agent:boundary:new -->
<!-- /agent:exchange -->
";

        assert!(!exchange_has_live_user_edit(Some(baseline), boundary_only));
        assert!(exchange_has_live_user_edit(Some(baseline), edited));
    }

    #[test]
    fn exchange_shrink_guard_block_reports_substantial_truncation() {
        let old_exchange = "a]".repeat(250);
        let old = format!(
            "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            old_exchange
        );
        let new = "<!-- agent:exchange -->\n.\n<!-- /agent:exchange -->\n";

        let block = exchange_shrink_guard_block(&old, new, 100, 0.10).unwrap();

        assert_eq!(block.old_exchange_len, 500);
        assert_eq!(block.new_exchange_len, 1);
        assert!(block.ratio < 0.01);
    }

    #[test]
    fn exchange_shrink_guard_allows_normal_small_or_missing_exchange() {
        let old_text = "x".repeat(200);
        let new_text = "y".repeat(100);
        let old = format!(
            "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            old_text
        );
        let new = format!(
            "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            new_text
        );
        assert!(exchange_shrink_guard_block(&old, &new, 100, 0.10).is_none());

        let small = "\
<!-- agent:exchange -->
Small content here, not much.
<!-- /agent:exchange -->
";
        assert!(exchange_shrink_guard_block(small, new.as_str(), 100, 0.10).is_none());
        assert!(exchange_shrink_guard_block("# Heading\nbody\n", ".\n", 100, 0.10).is_none());
    }

    #[test]
    fn live_exchange_without_visible_write_retry_requires_unmaterialized_live_edit() {
        let baseline = "\
<!-- agent:exchange -->
same prompt
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let before = "\
<!-- agent:exchange -->
same prompt
new prompt
<!-- agent:boundary:before -->
<!-- /agent:exchange -->
";
        let after_same = "\
<!-- agent:exchange -->
same prompt
new prompt
<!-- agent:boundary:after -->
<!-- /agent:exchange -->
";
        let after_changed = "\
<!-- agent:exchange -->
same prompt
new prompt
### Re: new prompt
Done.
<!-- agent:boundary:after -->
<!-- /agent:exchange -->
";

        assert!(live_exchange_without_visible_write_retry_required(
            Some(baseline),
            Some(before),
            after_same,
            false
        ));
        assert!(!live_exchange_without_visible_write_retry_required(
            Some(baseline),
            Some(before),
            after_same,
            true
        ));
        assert!(!live_exchange_without_visible_write_retry_required(
            Some(baseline),
            Some(before),
            after_changed,
            false
        ));
        assert!(!live_exchange_without_visible_write_retry_required(
            Some(before),
            Some(before),
            after_same,
            false
        ));
    }

    #[test]
    fn prompt_duplication_and_prefix_counts_are_response_aware() {
        let before = "\
<!-- agent:exchange -->
❯ ship it
<!-- /agent:exchange -->
";
        let after = "\
<!-- agent:exchange -->
❯ ship it
ship it
### Re: ship it
ship it
<!-- /agent:exchange -->
";

        assert!(exchange_prompt_text_duplicated(before, after));
        assert_eq!(
            exchange_prompt_prefix_count(exchange_content(after).unwrap()),
            1
        );
    }

    #[test]
    fn code_fence_delimiter_detects_common_fences() {
        assert!(is_code_fence_delimiter("```"));
        assert!(is_code_fence_delimiter("~~~rust"));
        assert!(!is_code_fence_delimiter("``"));
        assert!(!is_code_fence_delimiter("text"));
    }

    #[test]
    fn response_heading_policy_accepts_prefixed_headings() {
        assert!(is_exchange_response_heading_for_prefix_repair(
            "### Re: task"
        ));
        assert!(is_exchange_response_heading_for_prefix_repair(
            "❯ ### Re: task"
        ));
        assert!(is_prefixed_exchange_response_heading_for_prefix_repair(
            "❯ ## Assistant"
        ));
        assert!(!is_exchange_response_heading_for_prefix_repair("## Notes"));
    }

    #[test]
    fn response_prompt_order_detects_prompt_after_response_boundary() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do #next\n",
            "\n",
            "Done.\n",
            "<!-- agent:boundary:b0 -->\n",
            "do #next\n",
            "<!-- /agent:exchange -->\n",
        );
        let required = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "do #next\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(response_precedes_prompt_in_exchange(
            doc,
            Some("### Re: do #next\n\nDone."),
            Some(required)
        ));
    }

    #[test]
    fn response_prompt_order_repair_moves_prompt_before_response() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do #next\n",
            "\n",
            "Done.\n",
            "<!-- agent:boundary:b0 -->\n",
            "do #next\n",
            "<!-- /agent:exchange -->\n",
        );

        let repaired = repair_response_precedes_prompt_in_exchange(
            doc,
            Some("### Re: do #next\n\nDone."),
            None,
        )
        .unwrap()
        .expect("repair should move prompt before response");

        let prompt_pos = repaired.find("do #next\n### Re:").unwrap();
        let boundary_pos = repaired.find("<!-- agent:boundary:b0 -->").unwrap();
        assert!(prompt_pos < boundary_pos, "{repaired}");
        assert!(repaired.contains("Done.\n<!-- agent:boundary:b0 -->"));
        assert!(!response_precedes_prompt_in_exchange(
            &repaired,
            Some("### Re: do #next\n\nDone."),
            None
        ));
    }

    #[test]
    fn response_prompt_order_repair_requires_prompt_in_authority_doc() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do #next\n",
            "\n",
            "Done.\n",
            "<!-- agent:boundary:b0 -->\n",
            "do #next\n",
            "<!-- /agent:exchange -->\n",
        );
        let unrelated = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "do #other\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(!response_precedes_prompt_in_exchange(
            doc,
            Some("### Re: do #next\n\nDone."),
            Some(unrelated)
        ));
        assert!(
            repair_response_precedes_prompt_in_exchange(
                doc,
                Some("### Re: do #next\n\nDone."),
                Some(unrelated)
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn exchange_prompt_prefix_eligible_lines_skips_response_lists() {
        let exchange = concat!(
            "❯ do #item\n",
            "### Re: item\n",
            "- verified\n",
            "do #next\n",
            "<!-- agent:boundary:x -->\n",
            "after boundary\n",
        );

        let eligible = exchange_prompt_prefix_eligible_lines(exchange, None);

        assert!(eligible.contains(&"❯ do #item"));
        assert!(eligible.contains(&"do #next"));
        assert!(!eligible.contains(&"- verified"));
        assert!(!eligible.contains(&"after boundary"));
    }

    #[test]
    fn prompt_reconciliation_infos_tracks_removable_prompt_lines() {
        let exchange = concat!(
            "❯ do #item\n",
            "do #item\n",
            "### Re: item\n",
            "assistant text\n",
        );

        let infos = exchange_prompt_reconciliation_infos(exchange, None);

        assert_eq!(infos.len(), 4);
        assert_eq!(infos[0].normalized.as_deref(), Some("do #item"));
        assert!(infos[0].prefixed);
        assert_eq!(infos[1].normalized.as_deref(), Some("do #item"));
        assert!(!infos[1].prefixed);
        assert!(infos[2].normalized.is_none());
        assert!(infos[3].normalized.is_none());
        assert_eq!(
            prompt_reconciliation_counts(exchange).get("do #item"),
            Some(&2)
        );
    }

    #[test]
    fn tail_start_and_live_prefix_variant_policy_are_stable() {
        let exchange = "old\n<!-- agent:boundary:x -->\ntail\n";

        assert_eq!(
            last_exchange_boundary_tail_start(exchange),
            Some("old\n<!-- agent:boundary:x -->\n".len())
        );
        assert!(probable_live_prompt_prefix_variant(
            "agent-doc on corky running opencode, the key log shows re",
            "agent-doc on corky running opencode, the key log shows received"
        ));
        assert!(!probable_live_prompt_prefix_variant(
            "short",
            "short extended"
        ));
        assert!(!probable_live_prompt_prefix_variant(
            "complete sentence.",
            "complete sentence. more"
        ));
    }

    #[test]
    fn exchange_tail_repair_dedupes_live_prefix_variants() {
        let shorter = "agent-doc on corky running opencode, the key log shows re";
        let longer = "agent-doc on corky running opencode, the key log shows received";
        let exchange = format!("old\n<!-- agent:boundary:x -->\n{shorter}\n{longer}\n");

        let repaired = dedupe_live_prompt_prefix_variants_in_exchange_tail(&exchange).unwrap();

        assert!(!repaired.contains(&format!("\n{shorter}\n")));
        assert!(repaired.contains(&format!("\n{longer}\n")));
        assert!(repaired.starts_with("old\n<!-- agent:boundary:x -->\n"));
    }

    #[test]
    fn document_exchange_tail_repair_replaces_component_content() {
        let shorter = "agent-doc on corky running opencode, the key log shows re";
        let longer = "agent-doc on corky running opencode, the key log shows received";
        let doc = format!(
            "<!-- agent:exchange -->\nold\n<!-- agent:boundary:x -->\n{shorter}\n{longer}\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n- keep\n<!-- /agent:backlog -->\n"
        );

        let repaired = dedupe_live_prompt_prefix_variants_in_doc(&doc).unwrap();

        assert!(!repaired.contains(&format!("\n{shorter}\n")));
        assert!(repaired.contains(&format!("\n{longer}\n")));
        assert!(repaired.contains("<!-- agent:backlog -->\n- keep\n<!-- /agent:backlog -->"));
    }

    #[test]
    fn strip_exchange_content_removes_user_text() {
        let content = "---\nagent_doc_session: abc\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nUser prompt here.\n<!-- /agent:exchange -->\n";
        let result = strip_exchange_content(content);
        assert!(result.contains("<!-- agent:exchange"));
        assert!(!result.contains("User prompt here."));
    }

    #[test]
    fn strip_exchange_content_preserves_no_exchange() {
        let content = "---\nagent_doc_session: abc\n---\n\nJust text.\n";
        let result = strip_exchange_content(content);
        assert_eq!(result, content);
    }

    #[test]
    fn post_commit_ipc_reposition_safe_when_only_exchange_changes() {
        let before = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: previous\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#head] current work\n",
            "<!-- /agent:backlog -->\n",
        );
        let after = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: previous\n",
            "Done.\n\n",
            "### Re: latest\n",
            "Also done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#head] current work\n",
            "<!-- /agent:backlog -->\n",
        );

        assert!(post_commit_ipc_reposition_only_exchange_safe(before, after));
    }

    #[test]
    fn post_commit_ipc_reposition_unsafe_when_queue_or_backlog_changes() {
        let before = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: previous\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#head] current work\n",
            "<!-- /agent:backlog -->\n",
        );
        let after = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: previous\n",
            "Done.\n\n",
            "### Re: latest\n",
            "Also done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#head]\n",
            "- do [#agentsignals]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#head] current work\n",
            "- [ ] [#agentsignals] add realtime signals\n",
            "<!-- /agent:backlog -->\n",
        );

        assert!(!post_commit_ipc_reposition_only_exchange_safe(
            before, after
        ));
    }

    #[test]
    fn adjacent_prompt_duplicate_repair_prefers_prefixed_line() {
        let exchange = "❯ do #item\ndo #item\n### Re: item\nDone.\n";

        let repaired = dedupe_adjacent_prompt_prefix_duplicates_in_exchange(exchange).unwrap();

        assert!(repaired.contains("❯ do #item\n### Re: item"));
        assert!(!repaired.contains("❯ do #item\ndo #item"));
    }

    #[test]
    fn document_adjacent_prompt_duplicate_repair_replaces_component_content() {
        let doc = "\
<!-- agent:exchange -->
❯ do #item
do #item
### Re: item
Done.
<!-- /agent:exchange -->
";

        let repaired = dedupe_adjacent_prompt_prefix_duplicates_in_doc(doc).unwrap();

        assert!(repaired.contains("❯ do #item\n### Re: item"));
        assert!(!repaired.contains("❯ do #item\ndo #item"));
    }

    #[test]
    fn prompt_lines_against_before_repair_removes_excess_unprefixed_copy() {
        let before = "live prompt\n";
        let after = "❯ live prompt\nlive prompt\n### Re: response\nDone.\n";

        let repaired = dedupe_prompt_lines_against_before_exchange(before, after).unwrap();

        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(!repaired.contains("❯ live prompt\nlive prompt"));
        assert_eq!(
            response_aware_user_prompt_counts(&repaired)
                .get("live prompt")
                .copied(),
            Some(1)
        );
    }

    #[test]
    fn document_prompt_lines_against_before_repair_replaces_component_content() {
        let before = "\
<!-- agent:exchange -->
live prompt
<!-- /agent:exchange -->
";
        let after = "\
<!-- agent:exchange -->
❯ live prompt
live prompt
### Re: response
Done.
<!-- /agent:exchange -->
";

        let repaired = dedupe_prompt_lines_against_before_doc(before, after).unwrap();

        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(!repaired.contains("❯ live prompt\nlive prompt"));
        assert_eq!(
            response_aware_user_prompt_counts(exchange_content(&repaired).unwrap())
                .get("live prompt")
                .copied(),
            Some(1)
        );
    }

    #[test]
    fn extract_post_commit_normalization_targets_finds_missing_working_tree_prefix() {
        let committed = "\
<!-- agent:exchange -->
❯ do #spfxnorm. spec-test-build-install-commit-push
### Re: #spfxnorm - gpt-5
Implemented.
<!-- agent:boundary:clean123 -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange -->
do #spfxnorm. spec-test-build-install-commit-push
### Re: #spfxnorm - gpt-5 (HEAD)
Implemented.
<!-- agent:boundary:dirty123 -->
<!-- /agent:exchange -->
";

        assert_eq!(
            extract_post_commit_normalization_targets(committed, working),
            vec!["do #spfxnorm. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_only_updates_exchange_user_region() {
        let working = "\
<!-- agent:exchange -->
do #spfxnorm. spec-test-build-install-commit-push
<!-- agent:boundary:dirty123 -->
do #spfxnorm. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &["do #spfxnorm. spec-test-build-install-commit-push".to_string()],
        );

        assert!(repaired.contains("❯ do #spfxnorm. spec-test-build-install-commit-push"));
        assert!(
            repaired.contains("<!-- agent:boundary:dirty123 -->\ndo #spfxnorm. spec-test-build-install-commit-push"),
            "agent region after the boundary must remain untouched: {repaired}"
        );
    }

    #[test]
    fn normalization_prefix_observation_counts_counts_exchange_targets() {
        let content = "\
<!-- agent:exchange -->
❯ do #one
❯ do #one
do #two
### Re: response
❯ do #two
<!-- /agent:exchange -->
";

        assert_eq!(
            normalization_prefix_observation_counts(
                content,
                &[
                    "do #one".to_string(),
                    "do #one".to_string(),
                    "do #two".to_string(),
                ],
            ),
            (3, 3)
        );
    }

    #[test]
    fn observation_helpers_fall_back_to_full_content_without_exchange_component() {
        let content = "\
❯ do #one
do #one
❯ do #two
";

        assert_eq!(
            normalization_prefix_observation_counts(
                content,
                &["do #one".to_string(), "do #two".to_string()],
            ),
            (2, 2)
        );
        assert_eq!(duplicate_prompt_line_count(content), 1);
    }

    #[test]
    fn duplicate_prompt_line_count_normalizes_prefixes_for_eligible_lines() {
        let content = "\
<!-- agent:exchange -->
❯ do #one
do #one
### Re: response
do #one
<!-- /agent:exchange -->
";

        assert_eq!(duplicate_prompt_line_count(content), 2);
    }

    #[test]
    fn append_deduped_content_to_exchange_appends_and_dedupes() {
        let doc = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );
        let dedupe_key = "dedupe-key-123";
        let note = "### Re: diagnostic - gpt-5\n\nIssue: dedupe-key-123";

        let updated = append_deduped_content_to_exchange(doc, dedupe_key, note)
            .unwrap()
            .expect("expected content to append");

        assert!(updated.contains(note));
        assert!(
            updated.find(note).unwrap() < updated.find("<!-- /agent:exchange -->").unwrap(),
            "content must stay inside agent:exchange"
        );
        assert!(
            updated.contains("- do [#next]\n<!-- /agent:queue -->"),
            "queue content must be preserved"
        );

        let second = append_deduped_content_to_exchange(&updated, dedupe_key, note).unwrap();
        assert!(second.is_none(), "same dedupe key should not duplicate");
    }

    #[test]
    fn append_deduped_content_to_exchange_noops_without_exchange_component() {
        let doc = "<!-- agent:queue -->\n- do [#next]\n<!-- /agent:queue -->\n";
        let updated =
            append_deduped_content_to_exchange(doc, "dedupe-key-123", "### Re: diagnostic")
                .unwrap();

        assert!(updated.is_none());
    }

    #[test]
    fn appended_recovery_artifact_is_not_prompt_bearing() {
        let before = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );
        let note = "### Re: IPC proof diagnostic (interrupted-cycle recovery) - agent-doc\n\n```text\nIssue class: `ipc_proof_insufficient`\nipc_proof_insufficient file=/tmp/session.md\n```";

        let updated =
            append_deduped_content_to_exchange(before, "ipc_proof_insufficient file=", note)
                .unwrap()
                .expect("expected note to append");

        let diff_text = agent_doc_diff::unified_diff_from_contents(before, &updated)
            .expect("expected a non-empty diff after appending the note");
        let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff_text);
        assert!(
            !changes.iter().any(|c| matches!(
                c.kind,
                agent_doc_diff::PromptBearingChangeKind::PromptTarget
            )),
            "diagnostic note must not classify as a PromptTarget: {changes:?}"
        );
        assert!(
            changes.iter().any(|c| matches!(
                c.kind,
                agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
            )),
            "diagnostic note must classify as a RecoveryArtifact: {changes:?}"
        );
        assert!(
            agent_doc_diff::prompt_prefix_normalization_targets(&diff_text).is_empty(),
            "diagnostic note must not trigger prompt-prefix normalization"
        );
        assert!(
            agent_doc_turn::exchange_tail::prompt_only_exchange_tail(&updated).is_none(),
            "diagnostic note must not leave a prompt-only exchange tail"
        );
    }
}

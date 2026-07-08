//! Pure document normalization helpers shared by write/recovery adapters.

use anyhow::{Context, Result};
use std::collections::HashSet;

use crate::transient_markers::normalize_transient_agent_doc_markers;
use agent_doc_element::element::{self, is_backlog_component};

pub const AGENT_RESPONSE_COMPONENT: &str = "exchange";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBacklogPromptCleanup {
    pub content: String,
    pub removed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplicePendingComponentWarning {
    SourceParseFailed(String),
    TargetParseFailed(String),
    TargetMissingBacklogComponent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplicePendingComponentResult {
    pub content: String,
    pub warning: Option<SplicePendingComponentWarning>,
}

fn normalized_prompt_line(line: &str) -> String {
    line.trim()
        .strip_prefix('❯')
        .unwrap_or_else(|| line.trim())
        .trim()
        .to_string()
}

fn prompt_target_lines(target: &str) -> Vec<String> {
    target
        .lines()
        .map(normalized_prompt_line)
        .filter(|line| !line.is_empty())
        .collect()
}

fn prompt_target_matches_at(
    segments: &[&str],
    removed: &[bool],
    start: usize,
    target: &[String],
) -> bool {
    if start + target.len() > segments.len() {
        return false;
    }
    target.iter().enumerate().all(|(offset, expected)| {
        let idx = start + offset;
        !removed[idx] && normalized_prompt_line(segments[idx].trim_end_matches('\n')) == *expected
    })
}

pub fn remove_prompt_target_blocks_from_body(body: &str, targets: &[String]) -> (String, usize) {
    let segments: Vec<&str> = body.split_inclusive('\n').collect();
    if segments.is_empty() || targets.is_empty() {
        return (body.to_string(), 0);
    }

    let mut removed = vec![false; segments.len()];
    let mut removed_count = 0usize;
    let target_lines: Vec<Vec<String>> = targets
        .iter()
        .map(|target| prompt_target_lines(target))
        .filter(|lines| !lines.is_empty())
        .collect();

    for target in &target_lines {
        if let Some(start) = (0..segments.len())
            .rev()
            .find(|&idx| prompt_target_matches_at(&segments, &removed, idx, target))
        {
            for slot in removed.iter_mut().skip(start).take(target.len()) {
                *slot = true;
            }
            removed_count += 1;
        }
    }

    if removed_count == 0 {
        return (body.to_string(), 0);
    }

    let mut cleaned = String::with_capacity(body.len());
    for (idx, segment) in segments.iter().enumerate() {
        if !removed[idx] {
            cleaned.push_str(segment);
        }
    }
    (cleaned, removed_count)
}

fn prompt_targets_added_to_backlog(
    base: &str,
    current: &str,
) -> Result<Vec<(String, Vec<String>)>> {
    let base_components = element::parse(base).context("failed to parse baseline components")?;
    let current_components =
        element::parse(current).context("failed to parse current components")?;
    let mut targets = Vec::new();

    for current_component in current_components
        .iter()
        .filter(|component| is_backlog_component(&component.name))
    {
        let base_body = base_components
            .iter()
            .find(|component| component.name == current_component.name)
            .map(|component| component.content(base))
            .unwrap_or("");
        let current_body = current_component.content(current);
        let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(base_body, current_body)
        else {
            continue;
        };
        let component_targets: Vec<String> =
            agent_doc_diff::classify_prompt_bearing_changes(&diff_text)
                .into_iter()
                .filter(|change| {
                    change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
                })
                .map(|change| change.text)
                .collect();
        if !component_targets.is_empty() {
            targets.push((current_component.name.clone(), component_targets));
        }
    }

    Ok(targets)
}

pub fn cleanup_resolved_backlog_prompts_after_response(
    base: &str,
    current: &str,
    final_content: &str,
) -> Result<Option<ResolvedBacklogPromptCleanup>> {
    let targets = prompt_targets_added_to_backlog(base, current)?;
    if targets.is_empty() {
        return Ok(None);
    }

    let mut result = final_content.to_string();
    let mut removed_total = 0usize;
    for (component_name, component_targets) in targets {
        let components = element::parse(&result)
            .context("failed to parse final components for prompt cleanup")?;
        let Some(component) = components
            .iter()
            .find(|component| component.name == component_name)
        else {
            continue;
        };
        let body = component.content(&result);
        let (cleaned_body, removed_count) =
            remove_prompt_target_blocks_from_body(body, &component_targets);
        if removed_count == 0 {
            continue;
        }
        result = component.replace_content(&result, &cleaned_body);
        removed_total += removed_count;
    }

    if removed_total == 0 {
        return Ok(None);
    }

    Ok(Some(ResolvedBacklogPromptCleanup {
        content: result,
        removed: removed_total,
    }))
}

pub fn latest_response_block_missing_from_current(head: &str, current: &str) -> Option<String> {
    if current_exchange_is_compacted_summary(current) {
        return None;
    }
    let heading = latest_response_heading_missing_from_current(head, current)?;
    let head_components = element::parse(head).ok()?;
    let head_exchange = head_components
        .iter()
        .find(|component| component.name == "exchange")?;
    latest_response_block_from_exchange_body(head_exchange.content(head)).filter(|block| {
        block
            .lines()
            .next()
            .is_some_and(|line| line.trim() == heading)
    })
}

fn current_exchange_is_compacted_summary(current: &str) -> bool {
    let Ok(components) = element::parse(current) else {
        return false;
    };
    components
        .iter()
        .find(|component| component.name == "exchange")
        .map(|exchange| {
            let body = exchange.content(current);
            body.lines()
                .any(|line| line.trim() == "### Session Summary")
                && body
                    .lines()
                    .any(|line| line.contains("Compacted. Content archived"))
        })
        .unwrap_or(false)
}

pub fn response_marker_present_in_content(content: &str, marker: &str) -> bool {
    let needle = normalize_transient_agent_doc_markers(marker);
    let needle = needle.trim();
    if needle.is_empty() {
        return false;
    }
    let content_norm = normalize_transient_agent_doc_markers(content);
    content_norm.lines().any(|line| line.trim() == needle)
}

pub fn latest_response_heading_missing_from_current(head: &str, current: &str) -> Option<String> {
    let head_norm = normalize_transient_agent_doc_markers(head);
    let heading = head_norm
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("### Re:").then(|| trimmed.to_string())
        })
        .next_back()?;
    if response_marker_present_in_content(current, &heading) {
        return None;
    }
    Some(heading)
}

pub fn latest_response_block_from_exchange_body(body: &str) -> Option<String> {
    let lines: Vec<&str> = body.lines().collect();
    let start = lines
        .iter()
        .rposition(|line| line.trim().starts_with("### Re:"))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(idx, line)| {
            let trimmed = line.trim();
            (trimmed.starts_with("### Re:") || trimmed.starts_with("<!-- agent:boundary:"))
                .then_some(idx)
        })
        .unwrap_or(lines.len());
    let block = lines[start..end].join("\n");
    let block = block.trim();
    if block.is_empty() {
        return None;
    }
    Some(format!("{block}\n"))
}

pub fn splice_response_block_into_current_exchange(
    current: &str,
    response_block: &str,
) -> Option<String> {
    let components = element::parse(current).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    let body = exchange.content(current);
    let insert_at = body.rfind("<!-- agent:boundary:").unwrap_or(body.len());
    let (before_boundary, boundary_and_after) = body.split_at(insert_at);
    let mut new_body = before_boundary.to_string();
    if !new_body.ends_with('\n') {
        new_body.push('\n');
    }
    if !new_body.ends_with("\n\n") {
        new_body.push('\n');
    }
    new_body.push_str(response_block.trim());
    new_body.push('\n');
    if !boundary_and_after.is_empty() {
        new_body.push_str(boundary_and_after);
    }
    Some(exchange.replace_content(current, &new_body))
}

/// Blank the content of every component whose name is not in `keep`, preserving
/// component markers and non-component regions for structure-aware comparison.
pub fn blank_components_except(doc: &str, keep: &[&str]) -> Option<String> {
    let components = element::parse(doc).ok()?;
    let mut spans: Vec<(usize, usize)> = components
        .iter()
        .filter(|component| !keep.contains(&component.name.as_str()))
        .map(|component| (component.open_end, component.close_start))
        .collect();
    spans.sort_by_key(|(start, _)| *start);
    let mut out = doc.to_string();
    for (start, end) in spans.into_iter().rev() {
        if start <= end
            && end <= out.len()
            && out.is_char_boundary(start)
            && out.is_char_boundary(end)
        {
            out.replace_range(start..end, "");
        }
    }
    Some(out)
}

pub fn convergence_recovered_editor_wins_outside_response(recovered: &str, snapshot: &str) -> bool {
    let (Some(rec_blanked), Some(snap_blanked)) = (
        blank_components_except(recovered, &[AGENT_RESPONSE_COMPONENT]),
        blank_components_except(snapshot, &[AGENT_RESPONSE_COMPONENT]),
    ) else {
        return false;
    };
    normalize_transient_agent_doc_markers(&rec_blanked)
        == normalize_transient_agent_doc_markers(&snap_blanked)
        && normalize_transient_agent_doc_markers(recovered)
            != normalize_transient_agent_doc_markers(snapshot)
}

pub fn convergence_recovered_editor_wins_for_payload(
    recovered: &str,
    target: &str,
    payload: &serde_json::Value,
) -> bool {
    if normalize_transient_agent_doc_markers(recovered)
        == normalize_transient_agent_doc_markers(target)
    {
        return false;
    }

    let Some(node_patches) = agent_doc_markdown_ast::mutations::parse_node_patches_payload(payload)
    else {
        return false;
    };
    if !node_patches.is_empty()
        && !agent_doc_markdown_ast::mutations::node_patches_already_landed(recovered, &node_patches)
    {
        return false;
    }

    let Some(strict_components) = convergence_strict_components(payload) else {
        return false;
    };
    let strict_component_refs = strict_components
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let (Some(recovered_blanked), Some(target_blanked)) = (
        blank_components_except(recovered, &strict_component_refs),
        blank_components_except(target, &strict_component_refs),
    ) else {
        return false;
    };
    normalize_transient_agent_doc_markers(&recovered_blanked)
        == normalize_transient_agent_doc_markers(&target_blanked)
}

fn convergence_strict_components(payload: &serde_json::Value) -> Option<Vec<String>> {
    let mut strict_components = vec![AGENT_RESPONSE_COMPONENT.to_string()];
    if let Some(patches_value) = payload.get("patches") {
        let patches = patches_value.as_array()?;
        for patch in patches {
            let component = patch
                .get("component")
                .or_else(|| patch.get("name"))
                .and_then(|value| value.as_str())?;
            if !strict_components
                .iter()
                .any(|existing| existing == component)
            {
                strict_components.push(component.to_string());
            }
        }
    }
    Some(strict_components)
}

pub fn reconcile_postcommit_exchange_to_head(working: &str, head: &str) -> Option<String> {
    let working_components = element::parse(working).ok()?;
    let head_components = element::parse(head).ok()?;
    let head_exchange = head_components
        .iter()
        .find(|component| component.name == AGENT_RESPONSE_COMPONENT)?;
    let working_exchange = working_components
        .iter()
        .find(|component| component.name == AGENT_RESPONSE_COMPONENT)?;
    let head_body = head_exchange.content(head);
    let working_body = working_exchange.content(working);
    let head_norm = normalize_transient_agent_doc_markers(head_body);
    let working_norm = normalize_transient_agent_doc_markers(working_body);
    if head_norm == working_norm {
        return None;
    }
    let replace_exchange_with_head = || {
        let start = working_exchange.open_end;
        let end = working_exchange.close_start;
        if !(start <= end
            && end <= working.len()
            && working.is_char_boundary(start)
            && working.is_char_boundary(end))
        {
            return None;
        }
        let mut out = working.to_string();
        out.replace_range(start..end, head_body);
        Some(out)
    };
    if stale_prompt_targets_are_committed_queue_echoes(head_body, working_body) {
        return replace_exchange_with_head();
    }
    let head_lines: HashSet<&str> = head_norm
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let working_lines: HashSet<&str> = working_norm
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if !head_lines.iter().all(|line| working_lines.contains(line)) {
        return None;
    }
    let working_only: Vec<&str> = working_lines.difference(&head_lines).copied().collect();
    if working_only.is_empty() || !working_only.iter().all(|line| line.starts_with('>')) {
        return None;
    }
    if prompt_bearing_user_changes_between(head_body, working_body)
        .iter()
        .any(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget)
    {
        return None;
    }
    replace_exchange_with_head()
}

fn stale_prompt_targets_are_committed_queue_echoes(head_body: &str, working_body: &str) -> bool {
    let (head_without_queue_proofs, _) = remove_committed_queue_prompt_proofs(head_body);
    let (working_without_queue_proofs, _) = remove_committed_queue_prompt_proofs(working_body);
    let response_headings = response_heading_match_keys(&head_without_queue_proofs);
    let (working_without_stale_prompts, removed_prompt_target) = remove_stale_prompt_target_lines(
        &working_without_queue_proofs,
        &head_without_queue_proofs,
        &response_headings,
    );
    if !removed_prompt_target {
        return false;
    }
    compact_exchange_for_compare(&head_without_queue_proofs)
        == compact_exchange_for_compare(&working_without_stale_prompts)
}

fn remove_committed_queue_prompt_proofs(body: &str) -> (String, bool) {
    let mut out = String::with_capacity(body.len());
    let mut in_queue_prompt = false;
    let mut removed = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed == "> **Queue prompt:**" {
            in_queue_prompt = true;
            removed = true;
            continue;
        }
        if !in_queue_prompt {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('>') {
            removed = true;
            continue;
        } else {
            in_queue_prompt = false;
            out.push_str(line);
            out.push('\n');
        }
    }
    if body.ends_with('\n') || out.is_empty() {
        (out, removed)
    } else {
        (out.trim_end_matches('\n').to_string(), removed)
    }
}

fn remove_stale_prompt_target_lines(
    body: &str,
    head_without_queue_proofs: &str,
    response_headings: &[String],
) -> (String, bool) {
    let head_prompt_targets: HashSet<String> = head_without_queue_proofs
        .lines()
        .filter(|line| line.trim_start().starts_with('❯'))
        .map(|line| line.trim().to_string())
        .collect();
    let mut out = String::with_capacity(body.len());
    let mut removed = false;
    for line in body.lines() {
        if line.trim_start().starts_with('❯')
            && !head_prompt_targets.contains(line.trim())
            && prompt_target_matches_response_heading(line, response_headings)
        {
            removed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if body.ends_with('\n') || out.is_empty() {
        (out, removed)
    } else {
        (out.trim_end_matches('\n').to_string(), removed)
    }
}

fn response_heading_match_keys(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(response_heading_match_key)
        .collect()
}

fn response_heading_match_key(line: &str) -> Option<String> {
    let normalized = normalize_transient_agent_doc_markers(line);
    let trimmed = normalized.trim_start();
    let hash_count = trimmed.chars().take_while(|&ch| ch == '#').count();
    if !(1..=6).contains(&hash_count) {
        return None;
    }
    let rest = trimmed.get(hash_count..)?.trim_start();
    let title = rest.strip_prefix("Re:")?.trim();
    let title = title
        .split(" — ")
        .next()
        .unwrap_or(title)
        .split(" - ")
        .next()
        .unwrap_or(title);
    let key = prompt_match_key(title);
    if key.is_empty() { None } else { Some(key) }
}

fn prompt_target_matches_response_heading(line: &str, response_headings: &[String]) -> bool {
    let prompt = prompt_match_key(&normalized_prompt_line(line));
    if prompt.is_empty() {
        return false;
    }
    response_headings.iter().any(|heading| {
        if heading.is_empty() {
            return false;
        }
        if prompt == *heading || prompt.contains(heading) || heading.contains(&prompt) {
            return true;
        }
        let heading_tokens = heading.split_whitespace().collect::<Vec<_>>();
        heading_tokens.len() >= 2 && heading_tokens.iter().all(|token| prompt.contains(token))
    })
}

fn prompt_match_key(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_exchange_for_compare(body: &str) -> Vec<String> {
    normalize_transient_agent_doc_markers(body)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("<!-- agent:boundary:"))
        .map(ToOwned::to_owned)
        .collect()
}

fn prompt_bearing_user_changes_between(
    base: &str,
    current: &str,
) -> Vec<agent_doc_diff::PromptBearingChange> {
    let base_norm = crate::transient_markers::strip_boundary_markers(base);
    let current_norm = crate::transient_markers::strip_boundary_markers(current);
    let base_prompt_norm = agent_doc_diff::strip_comments(&base_norm);
    let current_prompt_norm = agent_doc_diff::strip_comments(&current_norm);
    let Some(diff_text) =
        agent_doc_diff::unified_diff_from_contents(&base_prompt_norm, &current_prompt_norm)
    else {
        return Vec::new();
    };
    let mut changes: Vec<_> = agent_doc_diff::classify_prompt_bearing_changes(&diff_text)
        .into_iter()
        .filter(|change| {
            matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::PromptTarget
                    | agent_doc_diff::PromptBearingChangeKind::ContentEdit
            )
        })
        .collect();
    if diff_text.lines().any(|line| {
        let Some(added) = line.strip_prefix('+') else {
            return false;
        };
        if line.starts_with("+++") {
            return false;
        }
        let trimmed = added.trim();
        trimmed.starts_with('❯')
            || agent_doc_prompt_lines::text_line_looks_like_prompt_target(trimmed)
    }) {
        for line in diff_text.lines() {
            let Some(added) = line.strip_prefix('+') else {
                continue;
            };
            if line.starts_with("+++") {
                continue;
            }
            let trimmed = added.trim();
            if trimmed.starts_with('❯')
                || agent_doc_prompt_lines::text_line_looks_like_prompt_target(trimmed)
            {
                let text = trimmed
                    .strip_prefix('❯')
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string();
                if !changes.iter().any(|change| {
                    change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
                        && change.text.trim() == text
                }) {
                    changes.push(agent_doc_diff::PromptBearingChange {
                        kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
                        text,
                    });
                }
            }
        }
    }
    changes
}

pub fn editor_buffer_preserved_head_exchange(flushed: &str, head: &str) -> bool {
    let (Ok(flushed_components), Ok(head_components)) =
        (element::parse(flushed), element::parse(head))
    else {
        return false;
    };
    let (Some(head_exchange), Some(flushed_exchange)) = (
        head_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
        flushed_components
            .iter()
            .find(|component| component.name == AGENT_RESPONSE_COMPONENT),
    ) else {
        return false;
    };
    let head_norm = normalize_transient_agent_doc_markers(head_exchange.content(head));
    let flushed_norm = normalize_transient_agent_doc_markers(flushed_exchange.content(flushed));
    let flushed_lines: HashSet<&str> = flushed_norm
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    head_norm
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .all(|line| flushed_lines.contains(line))
}

pub fn lift_pending_from_exchange(content: &str) -> Option<String> {
    let components = match element::parse(content) {
        Ok(c) => c,
        Err(_) => return None,
    };
    let exchange = components.iter().find(|c| c.name == "exchange")?;
    let pending = components.iter().find(|c| is_backlog_component(&c.name))?;

    if pending.open_start >= exchange.close_end {
        return None;
    }
    if pending.open_start < exchange.open_end {
        return None;
    }

    let pending_block = &content[pending.open_start..pending.close_end];
    let mut result = String::with_capacity(content.len() + 4);
    result.push_str(&content[..pending.open_start]);
    result.push_str(&content[pending.close_end..exchange.close_end]);
    result.push('\n');
    result.push_str(pending_block);
    result.push_str(&content[exchange.close_end..]);
    Some(result)
}

/// Maximum number of `❯ `-prefix lines a single normalization cycle may add.
///
/// A legitimate user input rarely produces more than a few dozen prefixed lines
/// in one write cycle. When this threshold is exceeded, it indicates snapshot/
/// baseline divergence rather than genuine user input.
pub const MAX_NORMALIZE_USER_LINES: usize = 50;

pub fn count_user_prompt_prefixes(content: &str) -> usize {
    let mut count = content.matches("\n❯ ").count();
    if content.starts_with("❯ ") {
        count += 1;
    }
    count
}

pub fn normalize_user_prompt_prefixes_applied(before_content: &str, after_content: &str) -> usize {
    count_user_prompt_prefixes(after_content)
        .saturating_sub(count_user_prompt_prefixes(before_content))
}

pub fn normalize_user_prompt_prefix_application_exceeds_threshold(applied: usize) -> bool {
    applied > MAX_NORMALIZE_USER_LINES
}

pub fn strip_boundary_for_dedup(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn splice_pending_component(target: &str, source: &str) -> SplicePendingComponentResult {
    let source_comps = match element::parse(source) {
        Ok(c) => c,
        Err(e) => {
            return SplicePendingComponentResult {
                content: target.to_string(),
                warning: Some(SplicePendingComponentWarning::SourceParseFailed(
                    e.to_string(),
                )),
            };
        }
    };
    let source_pending = source_comps.iter().find(|c| is_backlog_component(&c.name));
    let Some(src_comp) = source_pending else {
        return SplicePendingComponentResult {
            content: target.to_string(),
            warning: None,
        };
    };
    let source_content = &source[src_comp.open_end..src_comp.close_start];

    let target_comps = match element::parse(target) {
        Ok(c) => c,
        Err(e) => {
            return SplicePendingComponentResult {
                content: target.to_string(),
                warning: Some(SplicePendingComponentWarning::TargetParseFailed(
                    e.to_string(),
                )),
            };
        }
    };
    let target_pending = target_comps.iter().find(|c| is_backlog_component(&c.name));
    match target_pending {
        Some(tgt_comp) => SplicePendingComponentResult {
            content: tgt_comp.replace_content(target, source_content),
            warning: None,
        },
        None => SplicePendingComponentResult {
            content: target.to_string(),
            warning: Some(SplicePendingComponentWarning::TargetMissingBacklogComponent),
        },
    }
}

/// Count lines that open a fenced code block: a line whose first non-whitespace
/// run starts with three or more backticks or three or more tildies. Mirrors
/// CommonMark's fence-open recognition loosely and intentionally over-counts.
pub fn count_code_fence_openings(content: &str) -> usize {
    content
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("```") {
                rest.is_empty()
                    || rest.starts_with(|c: char| !c.is_whitespace() && c != '`')
                    || rest.starts_with(char::is_whitespace)
            } else if let Some(rest) = trimmed.strip_prefix("~~~") {
                rest.is_empty()
                    || rest.starts_with(|c: char| !c.is_whitespace() && c != '~')
                    || rest.starts_with(char::is_whitespace)
            } else {
                false
            }
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_queue_and_exchange(queue_body: &str, response: &str) -> String {
        format!(
            "---\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange -->\n{response}\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n{queue_body}\n<!-- /agent:queue -->\n"
        )
    }

    fn doc_with_exchange(exchange_body: &str, queue_body: &str) -> String {
        format!(
            "---\nagent_doc_format: template\n---\n<!-- agent:exchange -->\n{exchange_body}\n<!-- /agent:exchange -->\n## Queue\n<!-- agent:queue -->\n{queue_body}\n<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn cleanup_resolved_backlog_prompts_removes_only_prompt_targets() {
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep this tracked item\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep this tracked item\n",
            "commit + push uncommitted files\n",
            "<!-- /agent:backlog -->\n",
        );
        let final_content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: backlog prompt - gpt-5\n\n",
            "Committed and pushed.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#keep1] Keep this tracked item\n",
            "commit + push uncommitted files\n",
            "<!-- /agent:backlog -->\n",
        );

        let cleaned = cleanup_resolved_backlog_prompts_after_response(base, current, final_content)
            .unwrap()
            .expect("prompt target should be cleaned");

        assert_eq!(cleaned.removed, 1);
        assert!(cleaned.content.contains("### Re: backlog prompt - gpt-5"));
        assert!(
            cleaned
                .content
                .contains("- [x] [#keep1] Keep this tracked item")
        );
        assert!(!cleaned.content.contains("commit + push uncommitted files"));
    }

    #[test]
    fn cleanup_resolved_backlog_prompts_preserves_non_prompt_backlog_edits() {
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Existing item\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Existing item\n",
            "- [ ] [#new1] Added tracked item\n",
            "<!-- /agent:backlog -->\n",
        );

        let cleaned =
            cleanup_resolved_backlog_prompts_after_response(base, current, current).unwrap();
        assert!(
            cleaned.is_none(),
            "ordinary tracked backlog additions are not prompt cleanup targets"
        );
    }

    #[test]
    fn response_block_recovery_splices_latest_missing_response() {
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: old - gpt-5\n\n",
            "Done.\n\n",
            "<!-- agent:boundary:abc -->\n",
            "### Re: latest - gpt-5\n\n",
            "Recovered.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: old - gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:def -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let block = latest_response_block_missing_from_current(head, current).expect("block");
        let recovered =
            splice_response_block_into_current_exchange(current, &block).expect("splice");

        assert!(recovered.contains("### Re: latest - gpt-5"));
        assert!(recovered.contains("Recovered."));
        assert!(recovered.contains("<!-- agent:boundary:def -->"));
    }

    #[test]
    fn response_block_recovery_skips_compacted_current_exchange() {
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one - gpt-5\n\n",
            "Archived response.\n\n",
            "### Re: topic two - gpt-5\n\n",
            "Also archived.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\n",
            "- Archived 2 response topic(s): topic one; topic two\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(
            latest_response_block_missing_from_current(head, current),
            None,
            "compacted exchange cells are intentionally removed and must not be replayed from HEAD"
        );
    }

    #[test]
    fn response_marker_present_in_content_normalizes_transient_markers() {
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "<!-- agent:boundary:abc -->\n",
            "### Re: shipped the fix - gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(response_marker_present_in_content(
            content,
            "### Re: shipped the fix - gpt-5 (HEAD)"
        ));
        assert!(!response_marker_present_in_content(
            content,
            "### Re: never committed - gpt-5"
        ));
        assert!(!response_marker_present_in_content(content, "   "));
    }

    #[test]
    fn latest_response_heading_missing_from_current_reports_latest_head_heading() {
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: old - gpt-5\n\n",
            "Done.\n\n",
            "### Re: latest - gpt-5 (HEAD)\n\n",
            "Recovered.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: old - gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(
            latest_response_heading_missing_from_current(head, current).as_deref(),
            Some("### Re: latest - gpt-5")
        );
        assert_eq!(
            latest_response_heading_missing_from_current(head, head),
            None
        );
    }

    #[test]
    fn convergence_recovered_editor_wins_accepts_editor_buffer_when_only_queue_differs() {
        let snapshot =
            doc_with_queue_and_exchange("- a free-text head\n", "### Re: topic\n\nAnswered.");
        let recovered =
            doc_with_queue_and_exchange("- ~~a free-text head~~\n", "### Re: topic\n\nAnswered.");

        assert!(
            convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "queue-only divergence with matching response must be accepted"
        );
    }

    #[test]
    fn convergence_recovered_editor_wins_accepts_arbitrary_plugin_component() {
        let doc = |panel: &str| {
            format!(
                "---\nq: 1\n---\n\n<!-- agent:exchange -->\n### Re: x\n\nbody\n<!-- /agent:exchange -->\n\n<!-- agent:pluginpanel -->\n{panel}\n<!-- /agent:pluginpanel -->\n"
            )
        };
        let snapshot = doc("plugin state v1");
        let recovered = doc("plugin state v2 (editor-updated)");

        assert!(
            convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "a plugin-defined component must be editor-authoritative without an allowlist"
        );
    }

    #[test]
    fn convergence_recovered_editor_wins_rejects_when_response_differs() {
        let snapshot =
            doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered correctly.");
        let recovered =
            doc_with_queue_and_exchange("- ~~head~~\n", "### Re: topic\n\nAnswered DIFFERENTLY.");

        assert!(
            !convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "a response divergence must fail closed"
        );
    }

    #[test]
    fn convergence_recovered_editor_wins_rejects_when_identical() {
        let doc = doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered.");

        assert!(
            !convergence_recovered_editor_wins_outside_response(&doc, &doc),
            "identical docs are handled by the strict path"
        );
    }

    #[test]
    fn convergence_recovered_editor_wins_rejects_when_non_component_region_differs() {
        let snapshot = doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered.");
        let mut recovered =
            doc_with_queue_and_exchange("- ~~head~~\n", "### Re: topic\n\nAnswered.");
        recovered = recovered.replace("## Queue", "## Queue (tampered interstitial)");

        assert!(
            !convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "a non-component-region divergence must fail closed"
        );
    }

    #[test]
    fn convergence_recovered_editor_wins_rejects_structural_component_add() {
        let snapshot = doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered.");
        let recovered = format!(
            "{}\n<!-- agent:extra -->\nnew\n<!-- /agent:extra -->\n",
            doc_with_queue_and_exchange("- head\n", "### Re: topic\n\nAnswered.").trim_end()
        );

        assert!(
            !convergence_recovered_editor_wins_outside_response(&recovered, &snapshot),
            "a structural component add must fail closed"
        );
    }

    #[test]
    fn reconcile_postcommit_exchange_adopts_head_exchange_and_preserves_queue() {
        let head =
            doc_with_queue_and_exchange("- a live head\n", "### Re: topic\n\nAnswered cleanly.");
        let working = doc_with_queue_and_exchange(
            "- ~~a live head~~\n",
            "### Re: topic\n\nAnswered cleanly.\n\n> **Queue prompt:** stale leftover from a prior cycle",
        );

        let reconciled = reconcile_postcommit_exchange_to_head(&working, &head)
            .expect("stale exchange blockquote must reconcile to HEAD");

        assert!(
            !reconciled.contains("stale leftover from a prior cycle"),
            "the stale exchange blockquote must be dropped"
        );
        assert!(
            reconciled.contains("- ~~a live head~~"),
            "the editor-owned queue must be preserved"
        );
        assert!(
            reconcile_postcommit_exchange_to_head(&reconciled, &head).is_none(),
            "reconcile must converge"
        );
    }

    #[test]
    fn reconcile_postcommit_exchange_adopts_head_when_consumed_queue_prompt_echoes_stale_target() {
        let head = doc_with_queue_and_exchange(
            "- ~~do #fix1~~\n- do #fix2\n",
            "❯ describe the project\n\n### Re: do #fix1 - gpt-5\n\n> **Queue prompt:**\n>\n> do #fix1\n\nImplemented.",
        );
        let working = doc_with_queue_and_exchange(
            "- ~~do #fix1~~\n- do #fix2\n",
            "❯ describe the project\n\n### Re: do #fix1 - gpt-5 (HEAD)\nImplemented.\n❯ do #fix1",
        );

        let reconciled = reconcile_postcommit_exchange_to_head(&working, &head)
            .expect("stale consumed queue prompt target must reconcile to HEAD exchange");

        assert!(
            !reconciled.contains("❯ do #fix1"),
            "stale prompt target must be removed:\n{reconciled}"
        );
        assert!(
            reconciled.contains("> do #fix1"),
            "committed queue prompt proof should be preserved:\n{reconciled}"
        );
        assert!(
            reconciled.contains("❯ describe the project"),
            "baseline prompt target should be preserved:\n{reconciled}"
        );
        assert!(
            reconciled.contains("- ~~do #fix1~~"),
            "queue mutation outside exchange must be preserved:\n{reconciled}"
        );
    }

    #[test]
    fn reconcile_postcommit_exchange_adopts_head_when_batch_prompt_echo_is_only_live_drift() {
        let head = doc_with_queue_and_exchange(
            "",
            "❯ why did the queue stop?\n\n### Re: queued batch - gpt-5\n\n> **Queue prompt:**\n>\n> do [#cspe]\n>\n> do [#ctes]\n\nChanged paths: specs.md.\nCommands: cargo test queue_batch.\nVerification: passed.",
        );
        let working = doc_with_queue_and_exchange(
            "",
            "❯ why did the queue stop?\n\n### Re: queued batch - gpt-5 (HEAD)\n\n> **Queue prompt:**\n>\n> do [#cspe]\n>\n> do [#ctes]\n\nChanged paths: specs.md.\nCommands: cargo test queue_batch.\nVerification: passed.\n❯ Handle the whole queued batch in this response.",
        );

        let reconciled = reconcile_postcommit_exchange_to_head(&working, &head)
            .expect("batch prompt target echo must reconcile to HEAD exchange");

        assert!(
            !reconciled.contains("Handle the whole queued batch"),
            "stale batch prompt target must be removed:\n{reconciled}"
        );
        assert!(
            reconciled.contains("> do [#cspe]"),
            "committed queue proof should remain:\n{reconciled}"
        );
    }

    #[test]
    fn reconcile_postcommit_exchange_rejects_unrelated_late_prompt_target() {
        let head = doc_with_queue_and_exchange(
            "",
            "❯ Please reply\n\n### Re: Please reply - gpt-5\nAnswered only the original prompt.",
        );
        let working = doc_with_queue_and_exchange(
            "",
            "❯ Please reply\n\n### Re: Please reply - gpt-5 (HEAD)\nAnswered only the original prompt.\n❯ What remains after this response?",
        );

        assert!(
            reconcile_postcommit_exchange_to_head(&working, &head).is_none(),
            "an unrelated late prompt target must stay visible for session-check"
        );
    }

    #[test]
    fn reconcile_postcommit_exchange_returns_none_when_only_queue_differs() {
        let head = doc_with_queue_and_exchange("- a head\n", "### Re: x\n\nbody");
        let working = doc_with_queue_and_exchange("- ~~a head~~\n", "### Re: x\n\nbody");

        assert!(
            reconcile_postcommit_exchange_to_head(&working, &head).is_none(),
            "queue-only divergence with a matching exchange must not reconcile"
        );
    }

    #[test]
    fn reconcile_postcommit_exchange_fails_closed_on_new_user_prompt() {
        let head = doc_with_queue_and_exchange("- a head\n", "### Re: x\n\nbody");
        let working = doc_with_queue_and_exchange(
            "- a head\n",
            "### Re: x\n\nbody\n\n❯ do [#followup] a new directive",
        );

        assert!(
            reconcile_postcommit_exchange_to_head(&working, &head).is_none(),
            "a new user PromptTarget in the working exchange must fail closed"
        );
    }

    #[test]
    fn blank_components_except_clears_others_keeps_exchange() {
        let doc = doc_with_queue_and_exchange("- some head\n", "### Re: x\n\nbody");
        let blanked = blank_components_except(&doc, &[AGENT_RESPONSE_COMPONENT]).unwrap();

        assert!(
            !blanked.contains("some head"),
            "queue content must be blanked"
        );
        assert!(
            blanked.contains("### Re: x"),
            "response content must be preserved"
        );
        assert!(
            blanked.contains("<!-- agent:queue -->"),
            "queue markers stay"
        );
    }

    #[test]
    fn editor_buffer_preserved_head_exchange_accepts_buffer_with_head_response_plus_editor_edits() {
        let head = doc_with_exchange("### Re: topic\n\nThe committed answer.", "- do [#a]");
        let flushed = doc_with_exchange(
            "### Re: topic\n\nThe committed answer.",
            "- do [#a]\n- a new operator queue line",
        );

        assert!(editor_buffer_preserved_head_exchange(&flushed, &head));
    }

    #[test]
    fn editor_buffer_preserved_head_exchange_rejects_buffer_that_dropped_committed_response() {
        let head = doc_with_exchange(
            "### Re: topic\n\nThe committed answer.\n\nA second committed paragraph.",
            "- do [#a]",
        );
        let flushed = doc_with_exchange("### Re: topic\n\nThe committed answer.", "- do [#a]");

        assert!(!editor_buffer_preserved_head_exchange(&flushed, &head));
    }

    #[test]
    fn editor_buffer_preserved_head_exchange_ignores_boundary_markers() {
        let head = doc_with_exchange("### Re: topic\n\nThe committed answer.", "- do [#a]");
        let flushed =
            doc_with_exchange("### Re: topic (HEAD)\n\nThe committed answer.", "- do [#a]");

        assert!(editor_buffer_preserved_head_exchange(&flushed, &head));
    }

    #[test]
    fn lift_pending_moves_nested_backlog_component_after_exchange() {
        let nested = concat!(
            "<!-- agent:exchange -->\n",
            "response\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#a] task\n",
            "<!-- /agent:pending -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let lifted = lift_pending_from_exchange(nested).expect("lifted");

        let exchange_close = lifted.find("<!-- /agent:exchange -->").unwrap();
        let pending_open = lifted.find("<!-- agent:pending -->").unwrap();
        assert!(
            exchange_close < pending_open,
            "pending component should be moved after exchange:\n{lifted}"
        );
    }

    #[test]
    fn count_user_prompt_prefixes_includes_first_line_and_embedded_lines() {
        assert_eq!(count_user_prompt_prefixes("❯ first\nplain\n❯ second\n"), 2);
        assert_eq!(count_user_prompt_prefixes("plain\n❯ second\n"), 1);
        assert_eq!(count_user_prompt_prefixes("plain\nnot ❯ prompt\n"), 0);
    }

    #[test]
    fn normalize_user_prompt_prefixes_applied_saturates_on_fewer_prefixes() {
        assert_eq!(
            normalize_user_prompt_prefixes_applied("❯ old\n❯ other\n", "❯ old\n"),
            0
        );
        assert_eq!(
            normalize_user_prompt_prefixes_applied("❯ old\n", "❯ old\n❯ new\n"),
            1
        );
    }

    #[test]
    fn normalize_user_prompt_prefix_application_threshold_is_strictly_greater() {
        assert!(!normalize_user_prompt_prefix_application_exceeds_threshold(
            MAX_NORMALIZE_USER_LINES
        ));
        assert!(normalize_user_prompt_prefix_application_exceeds_threshold(
            MAX_NORMALIZE_USER_LINES + 1
        ));
    }

    #[test]
    fn strip_boundary_for_dedup_removes_markers() {
        let with_boundary = "Hello\n<!-- agent:boundary:abc123 -->\nWorld\n";
        let without = strip_boundary_for_dedup(with_boundary);
        assert!(!without.contains("agent:boundary"));
        assert!(without.contains("Hello"));
        assert!(without.contains("World"));
    }

    #[test]
    fn splice_pending_replaces_content_when_both_have_pending() {
        let target = concat!(
            "<!-- agent:exchange -->\n",
            "response content\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] old item\n",
            "<!-- /agent:pending -->\n",
        );
        let source = concat!(
            "<!-- agent:exchange -->\n",
            "original content\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:pending -->\n",
            "- [x] [#aaaa] old item\n",
            "<!-- /agent:pending -->\n",
        );

        let result = splice_pending_component(target, source);

        assert_eq!(result.warning, None);
        assert!(result.content.contains("response content"));
        assert!(result.content.contains("- [x] [#aaaa] old item"));
        assert!(!result.content.contains("- [ ] [#aaaa] old item"));
    }

    #[test]
    fn splice_pending_noop_when_source_has_no_pending() {
        let target = concat!(
            "<!-- agent:exchange -->\n",
            "response\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#bbbb] task\n",
            "<!-- /agent:pending -->\n",
        );
        let source = concat!(
            "<!-- agent:exchange -->\n",
            "original\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = splice_pending_component(target, source);

        assert_eq!(result.content, target);
        assert_eq!(result.warning, None);
    }

    #[test]
    fn splice_pending_reports_when_target_missing_pending() {
        let target = concat!(
            "<!-- agent:exchange -->\n",
            "response\n",
            "<!-- /agent:exchange -->\n",
        );
        let source = concat!(
            "<!-- agent:exchange -->\n",
            "original\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:pending -->\n",
            "- [x] [#cccc] done item\n",
            "<!-- /agent:pending -->\n",
        );

        let result = splice_pending_component(target, source);

        assert_eq!(result.content, target);
        assert_eq!(
            result.warning,
            Some(SplicePendingComponentWarning::TargetMissingBacklogComponent)
        );
    }

    #[test]
    fn count_code_fence_openings_handles_backtick_and_tilde() {
        assert_eq!(count_code_fence_openings("```\ncode\n```\n"), 2);
        assert_eq!(count_code_fence_openings("~~~\ncode\n~~~\n"), 2);
        assert_eq!(
            count_code_fence_openings("  ```js\nconst x = 1;\n  ```\n"),
            2
        );
        assert_eq!(count_code_fence_openings("no fences here"), 0);
        assert_eq!(count_code_fence_openings("```python\nprint('hi')\n```"), 2);
        assert_eq!(
            count_code_fence_openings("``````\nnot a fence open by CommonMark\n``````\n"),
            0
        );
    }
}

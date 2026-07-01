//! Pure document normalization helpers shared by write/recovery adapters.

use anyhow::{Context, Result};

use crate::transient_markers::normalize_transient_agent_doc_markers;
use agent_doc_element::element::{self, is_backlog_component};

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

//! Exchange element descriptor.

use std::collections::HashMap;

use agent_doc_element::{
    Component, ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};

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

pub fn starts_prompt_run_after_response(trimmed: &str, is_target: bool) -> bool {
    agent_doc_diff::line_looks_like_prompt_prefix_repair_start(trimmed, is_target)
}

pub fn starts_targeted_prompt_repair_after_response(trimmed: &str, is_target: bool) -> bool {
    agent_doc_diff::line_looks_like_targeted_prompt_prefix_repair_start(trimmed, is_target)
}

pub fn starts_targeted_or_prefixed_prompt_repair_after_response(
    trimmed: &str,
    is_target: bool,
) -> bool {
    starts_targeted_prompt_repair_after_response(
        trimmed,
        is_target || trimmed.trim_start().starts_with('❯'),
    )
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
        if agent_doc_diff::line_looks_like_markdown_list_item(trimmed) {
            continue;
        }

        let is_target =
            target_counts.is_some_and(|counts| normalization_target_matches_line(line, counts));
        if in_response_block {
            let starts_prompt = if target_counts.is_some() {
                starts_targeted_or_prefixed_prompt_repair_after_response(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                )
            } else {
                starts_prompt_run_after_response(trimmed, false)
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
                if starts_targeted_or_prefixed_prompt_repair_after_response(
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
                if starts_targeted_or_prefixed_prompt_repair_after_response(
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
                || agent_doc_diff::line_looks_like_plain_response_after_prompt(normalized)
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
}

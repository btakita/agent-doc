use anyhow::Result;
use indexmap::IndexMap;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

const BACKLOG_SIGNALS: &[&str] = &[
    "tasks",
    "todo",
    "backlog",
    "what's next",
    "what should we do",
    "what do we need",
    "recommendations",
    "recommend",
    "next steps",
    "action items",
    "what's left",
    "what remains",
    "what else",
    "add to backlog",
    "add to pending",
    "follow-up backlog",
    "follow up backlog",
    "follow-up items",
    "follow up items",
];

const NO_FOLLOWUP_PHRASES: &[&str] = &[
    "no actionable follow-up",
    "no actionable follow up",
    "no follow-up items",
    "no follow up items",
    "no new backlog item came out of this change",
    "no new backlog items came out of this change",
    "nothing to add to the backlog",
    "nothing new to add to the backlog",
    "no backlog items to add",
    "no follow-up work to track",
    "no follow up work to track",
    "no new follow-up work",
    "no new follow up work",
];

const FORMAT_REQUIREMENT_COMPONENT_SIGNALS: &[&str] = &[
    "backlog", "pending", "todo", "icebox", "exchange", "response",
];

const FORMAT_REQUIREMENT_SHAPE_SIGNALS: &[&str] = &[
    "2-level",
    "two-level",
    "two level",
    "numeric list",
    "numbered list",
    "bullet list",
    "organize",
    "format",
    "grouped",
    "section",
    "sections",
    "heading",
    "headings",
    "priority",
    "top",
];

pub fn prompt_requests_plan_work(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
) -> bool {
    effective_prompt_texts(prompt_targets, added_diff_lines, prompt_presets)
        .iter()
        .any(|text| text_requests_plan_work(text))
}

pub fn prompt_requests_backlog_work(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
) -> bool {
    effective_prompt_texts(prompt_targets, added_diff_lines, prompt_presets)
        .iter()
        .any(|text| text_requests_backlog_work(text))
}

pub fn prompt_targets_reference_preset(
    prompt_targets: &[String],
    prompt_presets: &IndexMap<String, String>,
    preset_name: &str,
) -> bool {
    effective_prompt_references_preset(prompt_targets, &[], prompt_presets, preset_name)
}

pub fn requested_prompt_presets(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
) -> Vec<String> {
    let mut requested = Vec::new();
    for text in prompt_targets.iter().chain(added_diff_lines.iter()) {
        let text = without_prompt_preset_definition_lines(text, prompt_presets);
        if text.trim().is_empty() {
            continue;
        }
        for preset in referenced_presets_in_text(&text, prompt_presets) {
            if !requested.iter().any(|existing| existing == &preset) {
                requested.push(preset);
            }
        }
    }
    requested
}

pub fn explicit_backlog_targets(
    current_file: &Path,
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
) -> Result<Vec<PathBuf>> {
    let mut targets = Vec::new();
    for text in effective_prompt_texts(prompt_targets, added_diff_lines, prompt_presets) {
        let Some(target) = explicit_backlog_target_in_text(current_file, &text)? else {
            continue;
        };
        if !targets.iter().any(|existing| existing == &target) {
            targets.push(target);
        }
    }
    Ok(targets)
}

pub fn response_explicitly_has_no_followups(response_text: &str) -> bool {
    let lower = response_text.to_ascii_lowercase();
    NO_FOLLOWUP_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

pub fn format_active_format_requirements(content: &str) -> Option<String> {
    let requirements = collect_active_format_requirements(content);
    if requirements.is_empty() {
        return None;
    }

    let mut rendered = String::from(
        "Active document-level formatting / structure requirements carried forward from earlier user prompts:\n\
         Preserve these when they still apply to the component or response you are updating.\n\
         If your response-format rules prevent an exact match, say that explicitly instead of silently flattening the structure.\n\n",
    );
    for (idx, requirement) in requirements.iter().enumerate() {
        rendered.push_str(&format!(
            "<requirement index=\"{}\">\n{}\n</requirement>\n\n",
            idx + 1,
            requirement
        ));
    }
    Some(rendered)
}

pub fn required_explicit_backlog_item_count(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
    prompt_bearing_changes: &[crate::diff::PromptBearingChange],
) -> usize {
    if !effective_prompt_references_preset(
        prompt_targets,
        added_diff_lines,
        prompt_presets,
        "#agent-doc-bug",
    ) {
        return 0;
    }

    required_issue_unit_count(prompt_bearing_changes)
}

pub fn required_plan_reference_count(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
    prompt_bearing_changes: &[crate::diff::PromptBearingChange],
) -> usize {
    if !effective_prompt_references_preset(
        prompt_targets,
        added_diff_lines,
        prompt_presets,
        "#agent-doc-bug",
    ) {
        return 0;
    }
    if !prompt_requests_plan_work(prompt_targets, added_diff_lines, prompt_presets) {
        return 0;
    }

    required_issue_unit_count(prompt_bearing_changes)
}

pub fn ordered_issue_units_for_agent_doc_bug(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
    prompt_bearing_changes: &[crate::diff::PromptBearingChange],
) -> Vec<String> {
    if !effective_prompt_references_preset(
        prompt_targets,
        added_diff_lines,
        prompt_presets,
        "#agent-doc-bug",
    ) {
        return Vec::new();
    }

    let content_edit_units = ordered_issue_units_from_changes(
        prompt_bearing_changes,
        crate::diff::PromptBearingChangeKind::ContentEdit,
    );
    if !content_edit_units.is_empty() {
        return content_edit_units;
    }

    let prompt_target_units = ordered_issue_units_from_changes(
        prompt_bearing_changes,
        crate::diff::PromptBearingChangeKind::PromptTarget,
    );
    if !prompt_target_units.is_empty() {
        return prompt_target_units;
    }

    vec!["#agent-doc-bug".to_string()]
}

fn effective_prompt_texts(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
) -> Vec<String> {
    let mut queue = prompt_targets.iter().cloned().collect::<VecDeque<_>>();
    queue.extend(added_diff_lines.iter().cloned());
    let mut seen_presets = HashSet::new();
    let mut texts = Vec::new();

    while let Some(text) = queue.pop_front() {
        let text = without_prompt_preset_definition_lines(&text, prompt_presets);
        if text.trim().is_empty() {
            continue;
        }

        texts.push(text.clone());
        for preset in referenced_presets_in_text(&text, prompt_presets) {
            if seen_presets.insert(preset.clone())
                && let Some(body) = prompt_presets.get(&preset)
            {
                queue.push_back(body.clone());
            }
        }
    }

    texts
}

fn collect_active_format_requirements(content: &str) -> Vec<String> {
    let mut requirements = Vec::new();
    let mut current_block = Vec::new();

    for raw_line in content.lines() {
        let trimmed_start = raw_line.trim_start();
        if let Some(rest) = trimmed_start.strip_prefix('❯') {
            if !current_block.is_empty() {
                maybe_push_format_requirement(&mut requirements, &current_block.join("\n"));
                current_block.clear();
            }
            let line = rest.trim_start();
            if !line.is_empty() {
                current_block.push(line.to_string());
            }
            continue;
        }

        if current_block.is_empty() {
            continue;
        }

        if trimmed_start.is_empty() {
            maybe_push_format_requirement(&mut requirements, &current_block.join("\n"));
            current_block.clear();
            continue;
        }

        if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            current_block.push(trimmed_start.to_string());
            continue;
        }

        maybe_push_format_requirement(&mut requirements, &current_block.join("\n"));
        current_block.clear();
    }

    if !current_block.is_empty() {
        maybe_push_format_requirement(&mut requirements, &current_block.join("\n"));
    }

    requirements
}

fn maybe_push_format_requirement(requirements: &mut Vec<String>, block: &str) {
    if looks_like_format_requirement(block)
        && !requirements.iter().any(|existing| existing == block)
    {
        requirements.push(block.to_string());
    }
}

fn looks_like_format_requirement(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let references_component = FORMAT_REQUIREMENT_COMPONENT_SIGNALS
        .iter()
        .any(|signal| lower.contains(signal));
    let references_shape = FORMAT_REQUIREMENT_SHAPE_SIGNALS
        .iter()
        .any(|signal| lower.contains(signal));
    let imperative_shape = lower.contains("use a ")
        || lower.contains("use an ")
        || lower.contains("keep ")
        || lower.contains("place ")
        || lower.contains("preserve ");

    references_component && (references_shape || imperative_shape)
}

fn effective_prompt_references_preset(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
    preset_name: &str,
) -> bool {
    effective_prompt_texts(prompt_targets, added_diff_lines, prompt_presets)
        .iter()
        .any(|text| {
            referenced_presets_in_text(text, prompt_presets)
                .iter()
                .any(|preset| preset == preset_name)
        })
}

pub fn collect_added_diff_lines(diff_text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;

    for line in diff_text.lines() {
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
            continue;
        }
        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        let content = &line[1..];
        let trimmed = content.trim_start();

        if !in_fence {
            let first = trimmed.chars().next().unwrap_or('\0');
            if first == '`' || first == '~' {
                let count = trimmed.chars().take_while(|&c| c == first).count();
                if count >= 3 {
                    in_fence = true;
                    fence_char = first;
                    fence_len = count;
                    continue;
                }
            }
        } else {
            let first = trimmed.chars().next().unwrap_or('\0');
            if first == fence_char {
                let count = trimmed.chars().take_while(|&c| c == first).count();
                if count >= fence_len && trimmed[count..].trim().is_empty() {
                    in_fence = false;
                }
            }
            continue;
        }

        if content.starts_with('>') {
            continue;
        }
        lines.push(content.to_string());
    }

    lines
}

fn referenced_presets_in_text(
    text: &str,
    prompt_presets: &IndexMap<String, String>,
) -> Vec<String> {
    let mut referenced = Vec::new();

    for line in text.lines() {
        if line_defines_prompt_preset(line, prompt_presets) {
            continue;
        }

        for preset in crate::diff::extract_prompt_preset_requests_from_text(line) {
            if let Some(preset) =
                crate::frontmatter::resolve_prompt_preset_key(prompt_presets, &preset)
                && !referenced.iter().any(|existing| existing == &preset)
            {
                referenced.push(preset);
            }
        }

        for token in extract_hashtag_tokens(line) {
            if prompt_presets.contains_key(token.as_str())
                && !referenced.iter().any(|existing| existing == &token)
            {
                referenced.push(token);
            }
        }
    }

    referenced
}

fn without_prompt_preset_definition_lines(
    text: &str,
    prompt_presets: &IndexMap<String, String>,
) -> String {
    text.lines()
        .filter(|line| !line_defines_prompt_preset(line, prompt_presets))
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_defines_prompt_preset(line: &str, prompt_presets: &IndexMap<String, String>) -> bool {
    let trimmed = line.trim_start();
    prompt_presets
        .keys()
        .any(|preset| line_starts_with_yaml_key(trimmed, preset))
}

fn line_starts_with_yaml_key(line: &str, key: &str) -> bool {
    if let Some(rest) = line.strip_prefix(key) {
        return rest.trim_start().starts_with(':');
    }

    for quote in ['\'', '"'] {
        let Some(rest) = line.strip_prefix(quote) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(key) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(quote) else {
            continue;
        };
        if rest.trim_start().starts_with(':') {
            return true;
        }
    }

    false
}

fn extract_hashtag_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut idx = 0usize;

    while idx < chars.len() {
        let (byte_idx, ch) = chars[idx];
        if ch != '#' {
            idx += 1;
            continue;
        }

        let start = byte_idx;
        let mut end = start + ch.len_utf8();
        let mut cursor = idx + 1;
        while cursor < chars.len() {
            let (next_byte, next_ch) = chars[cursor];
            if next_ch.is_ascii_alphanumeric() || next_ch == '-' || next_ch == '_' {
                end = next_byte + next_ch.len_utf8();
                cursor += 1;
                continue;
            }
            break;
        }

        if end > start + 1 {
            let token = text[start..end].to_string();
            if !tokens.iter().any(|existing| existing == &token) {
                tokens.push(token);
            }
        }
        idx = cursor;
    }

    tokens
}

fn text_requests_backlog_work(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    BACKLOG_SIGNALS.iter().any(|signal| lower.contains(signal))
}

fn text_requests_plan_work(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("create a plan") || lower.contains("write a plan")
}

fn explicit_backlog_target_in_text(current_file: &Path, text: &str) -> Result<Option<PathBuf>> {
    let lower = text.to_ascii_lowercase();
    let references_backlog_target = lower.contains("add to the backlog of")
        || lower.contains("add to backlog of")
        || lower.contains("backlog of ");
    if !references_backlog_target {
        return Ok(None);
    }
    crate::security::referenced_markdown_path_checked(current_file, text)
}

fn ordered_issue_units_from_changes(
    prompt_bearing_changes: &[crate::diff::PromptBearingChange],
    kind: crate::diff::PromptBearingChangeKind,
) -> Vec<String> {
    prompt_bearing_changes
        .iter()
        .filter(|change| change.kind == kind)
        .flat_map(|change| issue_units_in_text(&change.text))
        .collect()
}

fn issue_units_in_text(text: &str) -> Vec<String> {
    let mut in_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;
    let mut list_items = Vec::new();
    let mut substantive_groups: Vec<Vec<String>> = Vec::new();
    let mut current_substantive_group: Vec<String> = Vec::new();

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        let trimmed = trimmed.strip_prefix('❯').unwrap_or(trimmed).trim_start();
        if trimmed.is_empty() || trimmed.starts_with("<!--") || trimmed.starts_with('>') {
            continue;
        }

        if !in_fence {
            let first = trimmed.chars().next().unwrap_or('\0');
            if first == '`' || first == '~' {
                let count = trimmed.chars().take_while(|&c| c == first).count();
                if count >= 3 {
                    in_fence = true;
                    fence_char = first;
                    fence_len = count;
                    continue;
                }
            }
        } else {
            let first = trimmed.chars().next().unwrap_or('\0');
            if first == fence_char {
                let count = trimmed.chars().take_while(|&c| c == first).count();
                if count >= fence_len && trimmed[count..].trim().is_empty() {
                    in_fence = false;
                }
            }
            continue;
        }

        if is_top_level_issue_list_item(trimmed) {
            list_items.push(strip_top_level_issue_marker(trimmed).to_string());
            continue;
        }

        if trimmed == "---" {
            if !current_substantive_group.is_empty() {
                substantive_groups.push(std::mem::take(&mut current_substantive_group));
            }
            continue;
        }

        if !trimmed.starts_with('/') && !trimmed.eq_ignore_ascii_case("#agent-doc-bug") {
            current_substantive_group.push(trimmed.to_string());
        }
    }
    if !current_substantive_group.is_empty() {
        substantive_groups.push(current_substantive_group);
    }

    if !list_items.is_empty() {
        return list_items;
    }
    substantive_groups
        .into_iter()
        .map(|group| group.join("\n"))
        .collect()
}

fn count_issue_units_in_text(text: &str) -> usize {
    issue_units_in_text(text).len()
}

fn required_issue_unit_count(prompt_bearing_changes: &[crate::diff::PromptBearingChange]) -> usize {
    let content_edit_count = prompt_bearing_changes
        .iter()
        .filter(|change| change.kind == crate::diff::PromptBearingChangeKind::ContentEdit)
        .map(|change| count_issue_units_in_text(&change.text))
        .sum::<usize>();
    if content_edit_count > 0 {
        return content_edit_count;
    }

    let prompt_target_count = prompt_bearing_changes
        .iter()
        .filter(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
        .map(|change| count_issue_units_in_text(&change.text))
        .sum::<usize>();
    if prompt_target_count > 0 {
        return prompt_target_count;
    }

    1
}

fn is_top_level_issue_list_item(trimmed: &str) -> bool {
    trimmed.starts_with("- ") || trimmed.starts_with("* ") || is_numbered_list_item(trimmed)
}

fn strip_top_level_issue_marker(trimmed: &str) -> &str {
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return rest.trim_start();
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return rest.trim_start();
    }
    let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits > 0
        && trimmed
            .as_bytes()
            .get(digits)
            .is_some_and(|byte| *byte == b'.')
        && trimmed
            .as_bytes()
            .get(digits + 1)
            .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        return trimmed[digits + 1..].trim_start();
    }
    trimmed
}

fn is_numbered_list_item(trimmed: &str) -> bool {
    let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    digits > 0
        && trimmed
            .as_bytes()
            .get(digits)
            .is_some_and(|byte| *byte == b'.')
        && trimmed
            .as_bytes()
            .get(digits + 1)
            .is_some_and(|byte| byte.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backlog_request_detected_from_direct_prompt_text() {
        assert!(prompt_requests_backlog_work(
            &["What should we do next? Any recommendations?".to_string()],
            &[],
            &IndexMap::new()
        ));
    }

    #[test]
    fn backlog_request_detected_via_recursive_prompt_preset_expansion() {
        let presets = IndexMap::from([
            (
                "#code-review".to_string(),
                "Please review the codebase. #follow-up-backlog".to_string(),
            ),
            (
                "#follow-up-backlog".to_string(),
                "Any follow-up items to place in the backlog?".to_string(),
            ),
        ]);

        assert!(prompt_requests_backlog_work(
            &["#code-review".to_string()],
            &[],
            &presets
        ));
    }

    #[test]
    fn plan_request_detected_via_recursive_prompt_preset_expansion() {
        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                .to_string(),
        )]);

        assert!(prompt_requests_plan_work(
            &["Please report this agent-doc missing feature. #agent-doc-bug".to_string()],
            &[],
            &presets
        ));
    }

    #[test]
    fn plan_request_ignores_copied_prompt_preset_definitions() {
        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                .to_string(),
        )]);

        assert!(!prompt_requests_plan_work(
            &["do #tmuxplanscope. spec-test-build-install-commit-push".to_string()],
            &[
                "prompt_presets:".to_string(),
                "  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                    .to_string(),
            ],
            &presets
        ));
        assert!(!prompt_requests_backlog_work(
            &["do #tmuxplanscope. spec-test-build-install-commit-push".to_string()],
            &[
                "prompt_presets:".to_string(),
                "  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                    .to_string(),
            ],
            &presets
        ));
    }

    #[test]
    fn prompt_targets_reference_preset_only_considers_prompt_targets() {
        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                .to_string(),
        )]);

        assert!(prompt_targets_reference_preset(
            &["Please report this agent-doc missing feature. #agent-doc-bug".to_string()],
            &presets,
            "#agent-doc-bug",
        ));
        assert!(!prompt_targets_reference_preset(
            &["do #pbct. spec-test-build-install-commit-push".to_string()],
            &presets,
            "#agent-doc-bug",
        ));
    }

    #[test]
    fn requested_prompt_presets_detects_inline_hashtag_request() {
        let presets = IndexMap::from([(
            "#next-steps".to_string(),
            "Any follow-up items to place in the backlog?".to_string(),
        )]);

        assert_eq!(
            requested_prompt_presets(
                &[
                    "Please analyze failed orders and bot traffic on monsterrodholders.com. #next-steps"
                        .to_string()
                ],
                &[],
                &presets,
            ),
            vec!["#next-steps".to_string()]
        );
    }

    #[test]
    fn requested_prompt_presets_ignores_frontmatter_definition_lines() {
        let presets = IndexMap::from([(
            "#next-steps".to_string(),
            "Any follow-up items to place in the backlog?".to_string(),
        )]);

        assert!(
            requested_prompt_presets(
                &[],
                &[
                    "prompt_presets:".to_string(),
                    "  '#next-steps': Any follow-up items to place in the backlog?".to_string(),
                ],
                &presets,
            )
            .is_empty()
        );
    }

    #[test]
    fn no_followups_detection_accepts_explicit_proof_phrases() {
        assert!(response_explicitly_has_no_followups(
            "No new backlog item came out of this change."
        ));
        assert!(response_explicitly_has_no_followups(
            "There were no actionable follow-up items to capture."
        ));
    }

    #[test]
    fn no_followups_detection_ignores_unrelated_prose() {
        assert!(!response_explicitly_has_no_followups(
            "I did not find a third issue in this pass."
        ));
    }

    #[test]
    fn format_active_format_requirements_surfaces_prior_backlog_shape_directive() {
        let doc = concat!(
            "❯ Please organize the backlog into a 2-level list. ",
            "Place the urgent-security matters at the top. ",
            "Use a numeric list where appropriate.\n",
            "### Re: backlog organization — gpt-5\n",
            "I reorganized the backlog.\n",
        );

        let rendered =
            format_active_format_requirements(doc).expect("expected formatting requirements");

        assert!(
            rendered.contains(
                "Active document-level formatting / structure requirements carried forward"
            )
        );
        assert!(rendered.contains(
            "Please organize the backlog into a 2-level list. Place the urgent-security matters at the top. Use a numeric list where appropriate."
        ));
        assert!(
            rendered.contains(
                "If your response-format rules prevent an exact match, say that explicitly"
            )
        );
    }

    #[test]
    fn format_active_format_requirements_ignores_agent_confirmation_prose() {
        let doc = concat!(
            "### Re: backlog organization — gpt-5\n",
            "I reorganized the backlog into numbered sections with urgent work at the top.\n",
        );

        assert!(format_active_format_requirements(doc).is_none());
    }

    #[test]
    fn explicit_backlog_target_detected_via_prompt_preset_expansion() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let current = dir.path().join("tasks/source.md");
        let target = dir.path().join("tasks/bugs.md");
        std::fs::write(&current, "# source\n").unwrap();
        std::fs::write(&target, "# bugs\n").unwrap();

        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                .to_string(),
        )]);

        let targets =
            explicit_backlog_targets(&current, &["#agent-doc-bug".to_string()], &[], &presets)
                .unwrap();

        assert_eq!(targets, vec![target]);
    }

    #[test]
    fn explicit_backlog_target_strips_redundant_project_prefix_from_preset() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("agent-loop");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/software")).unwrap();
        let current = root.join("tasks/software/tmux-router.md");
        let target = root.join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::write(&current, "# source\n").unwrap();
        std::fs::write(&target, "# bugs\n").unwrap();

        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md"
                .to_string(),
        )]);

        let targets =
            explicit_backlog_targets(&current, &["#agent-doc-bug".to_string()], &[], &presets)
                .unwrap();

        assert_eq!(targets, vec![target.canonicalize().unwrap()]);
    }

    #[test]
    fn explicit_backlog_target_uses_parent_project_prefix_from_nested_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("agent-loop");
        let nested = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join("tasks")).unwrap();
        let current = nested.join("tasks/root.md");
        let target = root.join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::write(&current, "# source\n").unwrap();
        std::fs::write(&target, "# parent bugs\n").unwrap();
        std::fs::write(
            nested.join("tasks/agent-doc/agent-doc-bugs2.md"),
            "# nested bugs\n",
        )
        .unwrap();

        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md"
                .to_string(),
        )]);

        let targets =
            explicit_backlog_targets(&current, &["#agent-doc-bug".to_string()], &[], &presets)
                .unwrap();

        assert_eq!(targets, vec![target.canonicalize().unwrap()]);
    }

    #[test]
    fn explicit_backlog_target_fails_on_ambiguous_nested_task_tree() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("agent-loop");
        let nested = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join("tasks")).unwrap();
        let current = nested.join("tasks/root.md");
        std::fs::write(&current, "# source\n").unwrap();
        std::fs::write(
            root.join("tasks/agent-doc/agent-doc-bugs2.md"),
            "# parent bugs\n",
        )
        .unwrap();
        std::fs::write(
            nested.join("tasks/agent-doc/agent-doc-bugs2.md"),
            "# nested bugs\n",
        )
        .unwrap();

        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md"
                .to_string(),
        )]);

        let err =
            explicit_backlog_targets(&current, &["#agent-doc-bug".to_string()], &[], &presets)
                .unwrap_err();

        assert!(
            err.to_string().contains("ambiguous markdown reference"),
            "{err:#}"
        );
    }

    #[test]
    fn required_explicit_backlog_item_count_prefers_content_edits() {
        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                .to_string(),
        )]);
        let changes = vec![
            crate::diff::PromptBearingChange {
                kind: crate::diff::PromptBearingChangeKind::PromptTarget,
                text: "Please report this agent-doc missing feature. #agent-doc-bug".to_string(),
            },
            crate::diff::PromptBearingChange {
                kind: crate::diff::PromptBearingChangeKind::ContentEdit,
                text: "1. First missing transfer\n2. Second missing transfer\n3. Third missing transfer"
                    .to_string(),
            },
        ];

        let count = required_explicit_backlog_item_count(
            &["Please report this agent-doc missing feature. #agent-doc-bug".to_string()],
            &[],
            &presets,
            &changes,
        );

        assert_eq!(count, 3);
    }

    #[test]
    fn required_explicit_backlog_item_count_falls_back_to_single_prompt_target() {
        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                .to_string(),
        )]);
        let changes = vec![crate::diff::PromptBearingChange {
            kind: crate::diff::PromptBearingChangeKind::PromptTarget,
            text: "Please report this agent-doc missing feature. #agent-doc-bug".to_string(),
        }];

        let count = required_explicit_backlog_item_count(
            &["Please report this agent-doc missing feature. #agent-doc-bug".to_string()],
            &[],
            &presets,
            &changes,
        );

        assert_eq!(count, 1);
    }

    #[test]
    fn required_plan_reference_count_tracks_issue_inventory_for_agent_doc_bug() {
        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                .to_string(),
        )]);
        let changes = vec![
            crate::diff::PromptBearingChange {
                kind: crate::diff::PromptBearingChangeKind::PromptTarget,
                text: "Please report this agent-doc missing feature. #agent-doc-bug".to_string(),
            },
            crate::diff::PromptBearingChange {
                kind: crate::diff::PromptBearingChangeKind::ContentEdit,
                text: "1. First missing transfer\n2. Second missing transfer\n3. Third missing transfer"
                    .to_string(),
            },
        ];

        let count = required_plan_reference_count(
            &["Please report this agent-doc missing feature. #agent-doc-bug".to_string()],
            &[],
            &presets,
            &changes,
        );

        assert_eq!(count, 3);
    }

    #[test]
    fn ordered_issue_units_preserve_declaration_chain_order() {
        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                .to_string(),
        )]);
        let prompt_targets = vec![
            "First issue. #agent-doc-bug".to_string(),
            "Second issue. #agent-doc-bug".to_string(),
            "Third issue. #agent-doc-bug".to_string(),
        ];
        let changes = prompt_targets
            .iter()
            .map(|text| crate::diff::PromptBearingChange {
                kind: crate::diff::PromptBearingChangeKind::PromptTarget,
                text: text.clone(),
            })
            .collect::<Vec<_>>();

        let units = ordered_issue_units_for_agent_doc_bug(&prompt_targets, &[], &presets, &changes);

        assert_eq!(
            units,
            vec![
                "First issue. #agent-doc-bug",
                "Second issue. #agent-doc-bug",
                "Third issue. #agent-doc-bug"
            ]
        );
    }

    #[test]
    fn ordered_issue_units_split_top_level_lists_in_order() {
        let presets = IndexMap::from([(
            "#agent-doc-bug".to_string(),
            "Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md"
                .to_string(),
        )]);
        let prompt_targets = vec!["#agent-doc-bug\n1. First issue\n2. Second issue".to_string()];
        let changes = vec![crate::diff::PromptBearingChange {
            kind: crate::diff::PromptBearingChangeKind::PromptTarget,
            text: prompt_targets[0].clone(),
        }];

        let units = ordered_issue_units_for_agent_doc_bug(&prompt_targets, &[], &presets, &changes);

        assert_eq!(units, vec!["First issue", "Second issue"]);
    }
}

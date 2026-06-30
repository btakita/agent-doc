use anyhow::Result;
use indexmap::IndexMap;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

pub mod harness_prompt;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPresetRequestResolution {
    pub requested: Vec<String>,
    pub missing: Vec<String>,
}

pub fn resolve_prompt_preset_requests(
    prompt_diff: Option<&str>,
    harness_diff: Option<&str>,
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
) -> PromptPresetRequestResolution {
    let mut requested = prompt_diff
        .map(agent_doc_diff::detect_prompt_preset_requests)
        .unwrap_or_default();
    if let Some(harness_diff) = harness_diff {
        push_unique_strings(
            &mut requested,
            agent_doc_diff::detect_prompt_preset_requests(harness_diff),
        );
    }
    push_unique_strings(
        &mut requested,
        requested_prompt_presets(prompt_targets, added_diff_lines, prompt_presets),
    );

    let requested = requested
        .into_iter()
        .map(|name| {
            agent_doc_frontmatter::frontmatter::resolve_prompt_preset_key(prompt_presets, &name)
                .unwrap_or(name)
        })
        .fold(Vec::new(), |mut acc, name| {
            if !acc.iter().any(|existing| existing == &name) {
                acc.push(name);
            }
            acc
        });
    let missing = requested
        .iter()
        .filter(|name| !prompt_presets.contains_key(name.as_str()))
        .cloned()
        .collect();

    PromptPresetRequestResolution { requested, missing }
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

pub fn required_explicit_backlog_item_count(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
    prompt_bearing_changes: &[agent_doc_diff::PromptBearingChange],
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
    prompt_bearing_changes: &[agent_doc_diff::PromptBearingChange],
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
    prompt_bearing_changes: &[agent_doc_diff::PromptBearingChange],
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
        agent_doc_diff::PromptBearingChangeKind::ContentEdit,
    );
    if !content_edit_units.is_empty() {
        return content_edit_units;
    }

    let prompt_target_units = ordered_issue_units_from_changes(
        prompt_bearing_changes,
        agent_doc_diff::PromptBearingChangeKind::PromptTarget,
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

fn push_unique_strings(target: &mut Vec<String>, values: impl IntoIterator<Item = String>) {
    for value in values {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
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

        for preset in agent_doc_diff::extract_prompt_preset_requests_from_text(line) {
            if let Some(preset) = agent_doc_frontmatter::frontmatter::resolve_prompt_preset_key(
                prompt_presets,
                &preset,
            ) && !referenced.iter().any(|existing| existing == &preset)
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
    agent_doc_fs::referenced_markdown_path_checked(current_file, text)
}

fn ordered_issue_units_from_changes(
    prompt_bearing_changes: &[agent_doc_diff::PromptBearingChange],
    kind: agent_doc_diff::PromptBearingChangeKind,
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

fn required_issue_unit_count(
    prompt_bearing_changes: &[agent_doc_diff::PromptBearingChange],
) -> usize {
    let content_edit_count = prompt_bearing_changes
        .iter()
        .filter(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::ContentEdit)
        .map(|change| count_issue_units_in_text(&change.text))
        .sum::<usize>();
    if content_edit_count > 0 {
        return content_edit_count;
    }

    let prompt_target_count = prompt_bearing_changes
        .iter()
        .filter(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget)
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
    fn requested_prompt_presets_detects_inline_hashtag_request() {
        let presets = IndexMap::from([(
            "#next-steps".to_string(),
            "Any follow-up items to place in the backlog?".to_string(),
        )]);

        assert_eq!(
            requested_prompt_presets(
                &[
                    "Please analyze failed orders and bot traffic on sampleorders.com. #next-steps"
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
    fn resolve_prompt_preset_requests_canonicalizes_aliases_and_dedupes() {
        let presets = IndexMap::from([
            ("#spec-test".to_string(), "Run checks.".to_string()),
            ("release-check".to_string(), "Prepare release.".to_string()),
        ]);
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n+preset spec-test\n+preset #spec-test\n";

        let resolution = resolve_prompt_preset_requests(
            Some(diff),
            None,
            &["Please also use #spec-test.".to_string()],
            &[],
            &presets,
        );

        assert_eq!(resolution.requested, vec!["#spec-test".to_string()]);
        assert!(resolution.missing.is_empty());
    }

    #[test]
    fn resolve_prompt_preset_requests_reports_missing_diff_request() {
        let presets = IndexMap::new();
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+preset missing-check\n";

        let resolution = resolve_prompt_preset_requests(Some(diff), None, &[], &[], &presets);

        assert_eq!(resolution.requested, vec!["missing-check".to_string()]);
        assert_eq!(resolution.missing, vec!["missing-check".to_string()]);
    }

    #[test]
    fn resolve_prompt_preset_requests_preserves_mixed_diff_harness_prompt_order() {
        let presets = IndexMap::from([
            ("first".to_string(), "First preset.".to_string()),
            ("second".to_string(), "Second preset.".to_string()),
            ("#third".to_string(), "Third preset.".to_string()),
        ]);
        let prompt_diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+preset first\n";
        let harness_diff = "--- snapshot\n+++ harness\n@@ -1 +1,2 @@\n ctx\n+presets second\n";

        let resolution = resolve_prompt_preset_requests(
            Some(prompt_diff),
            Some(harness_diff),
            &["Use #third after the diff-level presets.".to_string()],
            &[],
            &presets,
        );

        assert_eq!(
            resolution.requested,
            vec![
                "first".to_string(),
                "second".to_string(),
                "#third".to_string()
            ]
        );
        assert!(resolution.missing.is_empty());
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
            agent_doc_diff::PromptBearingChange {
                kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
                text: "Please report this agent-doc missing feature. #agent-doc-bug".to_string(),
            },
            agent_doc_diff::PromptBearingChange {
                kind: agent_doc_diff::PromptBearingChangeKind::ContentEdit,
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
        let changes = vec![agent_doc_diff::PromptBearingChange {
            kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
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
            agent_doc_diff::PromptBearingChange {
                kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
                text: "Please report this agent-doc missing feature. #agent-doc-bug".to_string(),
            },
            agent_doc_diff::PromptBearingChange {
                kind: agent_doc_diff::PromptBearingChangeKind::ContentEdit,
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
            .map(|text| agent_doc_diff::PromptBearingChange {
                kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
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
        let changes = vec![agent_doc_diff::PromptBearingChange {
            kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
            text: prompt_targets[0].clone(),
        }];

        let units = ordered_issue_units_for_agent_doc_bug(&prompt_targets, &[], &presets, &changes);

        assert_eq!(units, vec!["First issue", "Second issue"]);
    }
}

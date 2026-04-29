use indexmap::IndexMap;
use std::collections::{HashSet, VecDeque};

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

pub(crate) fn prompt_requests_backlog_work(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
) -> bool {
    effective_prompt_texts(prompt_targets, added_diff_lines, prompt_presets)
        .iter()
        .any(|text| text_requests_backlog_work(text))
}

pub(crate) fn response_explicitly_has_no_followups(response_text: &str) -> bool {
    let lower = response_text.to_ascii_lowercase();
    NO_FOLLOWUP_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
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
        texts.push(text.clone());
        for preset in referenced_presets_in_text(&text, prompt_presets) {
            if seen_presets.insert(preset.clone()) {
                if let Some(body) = prompt_presets.get(&preset) {
                    queue.push_back(body.clone());
                }
            }
        }
    }

    texts
}

pub(crate) fn collect_added_diff_lines(diff_text: &str) -> Vec<String> {
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

    for preset in crate::diff::extract_prompt_preset_requests_from_text(text) {
        if prompt_presets.contains_key(preset.as_str())
            && !referenced.iter().any(|existing| existing == &preset)
        {
            referenced.push(preset);
        }
    }

    for token in extract_hashtag_tokens(text) {
        if prompt_presets.contains_key(token.as_str())
            && !referenced.iter().any(|existing| existing == &token)
        {
            referenced.push(token);
        }
    }

    referenced
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
}

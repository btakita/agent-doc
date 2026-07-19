//! Pure queue prompt drift accounting.
//!
//! This module owns the queue-body prompt counts used by orchestration to
//! decide whether live IPC/editor content would delete queue work. Callers
//! still own file IO, snapshots, editor redelivery, and logging.

use std::collections::HashMap;

use crate::document_queue::{self, QueueEntry};

pub fn queue_component_text(doc: &str) -> String {
    let Ok(components) = agent_doc_element::element::parse(doc) else {
        return String::new();
    };
    components
        .iter()
        .find(|component| component.name == "queue")
        .map(|component| component.content(doc).to_string())
        .unwrap_or_default()
}

pub fn queue_prompt_texts(body: &str) -> Vec<String> {
    let Ok(entries) = document_queue::parse(body) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) if !prompt.multiline => {
                let text = prompt.text.trim().to_string();
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        })
        .collect()
}

/// Active + consumed (struck) queue prompt texts.
///
/// A queue item struck this cycle (`QueueEntry::Completed`) was consumed, not
/// dropped, so it must count toward content-ours coverage when deciding whether
/// adopting content-ours would drop a user-added candidate prompt.
pub fn queue_prompt_texts_including_consumed(body: &str) -> Vec<String> {
    let Ok(entries) = document_queue::parse(body) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) if !prompt.multiline => {
                let text = prompt.text.trim().to_string();
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        })
        .collect()
}

fn queue_prompt_counts(prompts: &[String]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for prompt in prompts {
        *counts.entry(prompt.clone()).or_insert(0) += 1;
    }
    counts
}

fn queue_prompt_count(counts: &HashMap<String, usize>, prompt: &str) -> usize {
    counts.get(prompt).copied().unwrap_or(0)
}

/// Queue prompt texts that existed at baseline but are absent from the live IPC
/// candidate.
///
/// The candidate may be a stale editor buffer, not an intentional operator
/// queue edit. Treating these missing lines as authoritative silently deletes
/// queue work from content-ours. The safe default is to keep baseline queue
/// prompts in content-ours and let the normal queue-consume / done-id paths
/// remove them with proof.
pub fn queue_prompt_deletions_between(baseline: &str, candidate: &str) -> Vec<String> {
    let baseline_prompts = queue_prompt_texts(&queue_component_text(baseline));
    if baseline_prompts.is_empty() {
        return Vec::new();
    }
    let candidate_prompts = queue_prompt_texts(&queue_component_text(candidate));
    let mut candidate_counts = queue_prompt_counts(&candidate_prompts);
    let mut deleted = Vec::new();
    for prompt in baseline_prompts {
        let remaining = candidate_counts.entry(prompt.clone()).or_insert(0);
        if *remaining > 0 {
            *remaining -= 1;
        } else {
            deleted.push(prompt);
        }
    }
    deleted
}

/// User-authored queue prompt lines present in the IPC candidate that
/// content-ours does not own; these would be silently deleted when content-ours
/// is adopted.
///
/// Scoped to `agent:queue` so exchange/backlog drift is not misread. A struck
/// queue item in content-ours counts as covered because it was answered rather
/// than dropped.
pub fn dropped_queue_prompt_lines_after_content_ours(
    baseline: &str,
    candidate: &str,
    content_ours: &str,
) -> Vec<String> {
    let baseline_q = queue_component_text(baseline);
    let candidate_q = queue_component_text(candidate);
    let content_ours_q = queue_component_text(content_ours);

    let baseline_prompts = queue_prompt_texts(&baseline_q);
    let candidate_prompts = queue_prompt_texts(&candidate_q);
    let content_ours_prompts = queue_prompt_texts_including_consumed(&content_ours_q);
    if candidate_prompts.is_empty() {
        return Vec::new();
    }

    let baseline_counts = queue_prompt_counts(&baseline_prompts);
    let content_ours_counts = queue_prompt_counts(&content_ours_prompts);
    let mut candidate_seen = HashMap::new();
    let mut dropped = Vec::new();

    for prompt in candidate_prompts {
        let seen = candidate_seen.entry(prompt.clone()).or_insert(0);
        *seen += 1;

        let baseline_count = queue_prompt_count(&baseline_counts, &prompt);
        if *seen <= baseline_count {
            continue;
        }

        let candidate_added_index = *seen - baseline_count;
        let content_ours_added_count =
            queue_prompt_count(&content_ours_counts, &prompt).saturating_sub(baseline_count);
        if candidate_added_index > content_ours_added_count {
            dropped.push(prompt);
        }
    }

    dropped
}

/// Return content-ours unchanged while detecting unproven live queue deletions.
///
/// Live editor/ack content can be stale. Preserve content-ours; callers log the
/// ignored deletion count and keep the deletion out of forward-merge unions.
pub fn preserve_content_ours_over_live_queue_deletions(
    baseline: &str,
    live_candidate: &str,
    content_ours: &str,
) -> (String, Vec<String>) {
    (
        content_ours.to_string(),
        queue_prompt_deletions_between(baseline, live_candidate),
    )
}

pub fn merge_visible_queue_additions_into_content_ours(
    base: &str,
    candidate: &str,
    content_ours: &str,
    dropped_queue: &[String],
) -> Option<String> {
    if dropped_queue.is_empty() {
        return None;
    }

    let base_queue = queue_component_text(base);
    let candidate_queue = queue_component_text(candidate);
    let content_ours_queue = queue_component_text(content_ours);
    let candidate_entries = document_queue::parse(&candidate_queue).ok()?;
    let mut merged_entries = document_queue::parse(&content_ours_queue).ok()?;

    let baseline_counts = queue_prompt_counts(&queue_prompt_texts(&base_queue));
    let mut content_ours_counts =
        queue_prompt_counts(&queue_prompt_texts_including_consumed(&content_ours_queue));
    let mut candidate_seen = HashMap::new();
    let mut appended = Vec::new();

    for entry in candidate_entries {
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        if prompt.multiline {
            continue;
        }
        let text = prompt.text.trim().to_string();
        if text.is_empty() {
            continue;
        }

        let seen = candidate_seen.entry(text.clone()).or_insert(0);
        *seen += 1;
        let baseline_count = queue_prompt_count(&baseline_counts, &text);
        if *seen <= baseline_count {
            continue;
        }

        let candidate_added_index = *seen - baseline_count;
        let content_ours_added_count =
            queue_prompt_count(&content_ours_counts, &text).saturating_sub(baseline_count);
        if candidate_added_index > content_ours_added_count {
            appended.push(QueueEntry::Prompt(document_queue::QueuePrompt {
                text: text.clone(),
                multiline: false,
                indent: 0,
            }));
            *content_ours_counts.entry(text).or_insert(0) += 1;
        }
    }

    if appended.len() != dropped_queue.len() {
        return None;
    }
    merged_entries.extend(appended);
    let merged_queue = document_queue::render(&merged_entries);
    let components = agent_doc_element::element::parse(content_ours).ok()?;
    let queue_component = components
        .iter()
        .find(|component| component.name == "queue")?;
    Some(queue_component.replace_content(content_ours, &merged_queue))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_queue_prompt_lines_after_content_ours_captures_adjacent_free_text_items() {
        let baseline = concat!(
            "<!-- agent:queue auto -->\n",
            "- item being edited\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "<!-- agent:queue auto -->\n",
            "- previous user queue item\n",
            "- item being edited\n",
            "- next user queue item\n",
            "<!-- /agent:queue -->\n",
        );

        let dropped = dropped_queue_prompt_lines_after_content_ours(baseline, candidate, baseline);
        assert_eq!(
            dropped,
            vec![
                "previous user queue item".to_string(),
                "next user queue item".to_string()
            ]
        );
    }

    #[test]
    fn dropped_queue_prompt_lines_after_content_ours_empty_when_items_are_owned() {
        let baseline = concat!(
            "<!-- agent:queue auto -->\n",
            "- item being edited\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "<!-- agent:queue auto -->\n",
            "- previous user queue item\n",
            "- item being edited\n",
            "- next user queue item\n",
            "<!-- /agent:queue -->\n",
        );

        let dropped = dropped_queue_prompt_lines_after_content_ours(baseline, candidate, candidate);
        assert!(dropped.is_empty());
    }

    #[test]
    fn dropped_queue_prompt_lines_after_content_ours_excludes_consumed_struck_item() {
        let baseline = concat!(
            "<!-- agent:queue auto -->\n",
            "- keep me\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "<!-- agent:queue auto -->\n",
            "- do [#consumed]\n",
            "- keep me\n",
            "<!-- /agent:queue -->\n",
        );
        let content_ours = concat!(
            "<!-- agent:queue auto -->\n",
            "- ~~do [#consumed]~~\n",
            "- keep me\n",
            "<!-- /agent:queue -->\n",
        );
        let dropped =
            dropped_queue_prompt_lines_after_content_ours(baseline, candidate, content_ours);
        assert!(
            dropped.is_empty(),
            "a struck/consumed item is answered, not dropped: {dropped:?}"
        );
    }

    #[test]
    fn dropped_queue_prompt_lines_after_content_ours_counts_duplicate_user_items() {
        let baseline = concat!(
            "<!-- agent:queue auto -->\n",
            "- repeat me\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "<!-- agent:queue auto -->\n",
            "- repeat me\n",
            "- repeat me\n",
            "- repeat me\n",
            "<!-- /agent:queue -->\n",
        );

        let dropped = dropped_queue_prompt_lines_after_content_ours(baseline, candidate, baseline);
        assert_eq!(
            dropped,
            vec!["repeat me".to_string(), "repeat me".to_string()]
        );
    }

    #[test]
    fn queue_prompt_deletions_between_counts_missing_baseline_prompts() {
        let baseline = concat!(
            "<!-- agent:queue auto -->\n",
            "- keep me\n",
            "- delete me\n",
            "<!-- /agent:queue -->\n",
        );
        let candidate = concat!(
            "<!-- agent:queue auto -->\n",
            "- keep me\n",
            "<!-- /agent:queue -->\n",
        );

        assert_eq!(
            queue_prompt_deletions_between(baseline, candidate),
            vec!["delete me".to_string()]
        );
    }

    #[test]
    fn preserve_content_ours_over_live_queue_deletions_keeps_baseline_prompts() {
        let content_ours = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: response\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#head]\n",
            "- do [#deleted]\n",
            "<!-- /agent:queue -->\n",
        );
        let live_candidate = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "live prompt after preflight\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto priority -->\n",
            "- do [#manual]\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n",
        );

        let (reconciled, ignored) = preserve_content_ours_over_live_queue_deletions(
            content_ours,
            live_candidate,
            content_ours,
        );

        assert!(reconciled.contains("### Re: response"));
        assert!(!reconciled.contains("live prompt after preflight"));
        assert!(reconciled.contains("<!-- agent:queue auto -->"));
        assert!(
            !reconciled.contains("do [#manual]"),
            "live queue additions stay next-cycle visible, not in content_ours:\n{reconciled}"
        );
        assert!(reconciled.contains("do [#head]"));
        assert!(
            reconciled.contains("do [#deleted]"),
            "unproven live queue deletion must not be folded into content_ours:\n{reconciled}"
        );
        assert_eq!(ignored, vec!["do [#deleted]".to_string()]);
    }
}

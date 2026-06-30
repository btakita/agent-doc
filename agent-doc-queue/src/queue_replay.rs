//! Queue replay-normalization policy.
//!
//! These helpers decide when queue-only visible drift was neutralized by replay
//! hashing but should still be committed because it preserves operator-added
//! prompt work. Callers own git, snapshots, files, and ops-log side effects.

use std::collections::HashMap;

use crate::document_queue::QueueEntry;

pub fn queue_entry_commit_signature(entry: &QueueEntry) -> String {
    match entry {
        QueueEntry::Prompt(prompt) => {
            format!("prompt:{}:{}", prompt.multiline, prompt.text.trim())
        }
        QueueEntry::Completed(prompt) => {
            format!("completed:{}:{}", prompt.multiline, prompt.text.trim())
        }
        QueueEntry::Preset(preset) => format!("preset:{}", preset.trim()),
        QueueEntry::Dispatch(dispatch) => format!("dispatch:{}", dispatch.trim()),
        QueueEntry::StartFence(Some(at)) => format!("start:{}", at.trim()),
        QueueEntry::StartFence(None) => "start:".to_string(),
        QueueEntry::StopFence => "stop:".to_string(),
        QueueEntry::Freeform(line) => format!("freeform:{}", line.trim()),
    }
}

fn queue_entry_count_map(entries: &[QueueEntry]) -> HashMap<String, (usize, bool)> {
    let mut counts = HashMap::new();
    for entry in entries {
        let key = queue_entry_commit_signature(entry);
        let is_prompt = matches!(entry, QueueEntry::Prompt(_));
        let slot = counts.entry(key).or_insert((0usize, is_prompt));
        slot.0 += 1;
        slot.1 |= is_prompt;
    }
    counts
}

/// Return the number of prompt entries added only in the queue component when
/// replay normalization otherwise considers the documents equivalent.
pub fn preserved_queue_additions_neutralized_by_replay(
    head_doc: &str,
    current_doc: &str,
) -> Option<usize> {
    if head_doc == current_doc {
        return None;
    }
    if agent_doc_document::transient_markers::normalize_for_replay_hash(head_doc)
        != agent_doc_document::transient_markers::normalize_for_replay_hash(current_doc)
    {
        return None;
    }

    let head_components = agent_doc_element::element::parse(head_doc).ok()?;
    let current_components = agent_doc_element::element::parse(current_doc).ok()?;
    let head_queue = head_components
        .iter()
        .find(|component| component.name == "queue")?;
    let current_queue = current_components
        .iter()
        .find(|component| component.name == "queue")?;
    let head_entries = crate::document_queue::parse(head_queue.content(head_doc)).ok()?;
    let current_entries = crate::document_queue::parse(current_queue.content(current_doc)).ok()?;
    let head_counts = queue_entry_count_map(&head_entries);
    let current_counts = queue_entry_count_map(&current_entries);

    for (key, (head_count, _)) in &head_counts {
        let current_count = current_counts
            .get(key)
            .map(|(count, _)| *count)
            .unwrap_or(0);
        if current_count < *head_count {
            return None;
        }
    }

    let mut added_prompts = 0usize;
    for (key, (current_count, is_prompt)) in &current_counts {
        let head_count = head_counts.get(key).map(|(count, _)| *count).unwrap_or(0);
        if *current_count <= head_count {
            continue;
        }
        if !*is_prompt {
            return None;
        }
        added_prompts += current_count - head_count;
    }

    (added_prompts > 0).then_some(added_prompts)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD_DOC: &str = concat!(
        "---\nagent_doc_session: test\nqueue_active: true\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior\n",
        "response body\n",
        "<!-- /agent:exchange -->\n",
        "<!-- agent:queue priority go -->\n",
        "- do [#existing]\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn detects_preserved_queue_prompt_additions_neutralized_by_replay() {
        let current = HEAD_DOC.replace(
            "- do [#existing]\n",
            "- do [#existing]\n- :pushpin: do [#advance-review]\n",
        );

        assert_eq!(
            preserved_queue_additions_neutralized_by_replay(HEAD_DOC, &current),
            Some(1)
        );
    }

    #[test]
    fn rejects_queue_deletions_or_non_prompt_additions() {
        let deleted = HEAD_DOC.replace("- do [#existing]\n", "");
        let preset_added = HEAD_DOC.replace(
            "- do [#existing]\n",
            "- do [#existing]\npreset #spec-test\n",
        );

        assert_eq!(
            preserved_queue_additions_neutralized_by_replay(HEAD_DOC, &deleted),
            None
        );
        assert_eq!(
            preserved_queue_additions_neutralized_by_replay(HEAD_DOC, &preset_added),
            None
        );
    }
}

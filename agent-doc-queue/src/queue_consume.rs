//! Pure queue prompt consumption policy.
//!
//! This module owns entry-level consume planning helpers. Callers still own
//! file IO, snapshots, IPC node operations, and editor convergence.

use std::collections::HashSet;

use crate::{
    document_queue::{self, QueueEntry},
    queue_response::{normalize_done_id, queue_prompt_done_id},
};

pub fn first_n_queue_prompt_texts(entries: &[QueueEntry], count: usize) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => {
                Some(document_queue::strip_in_progress_marker(&prompt.text))
            }
            _ => None,
        })
        .take(count)
        .collect()
}

pub fn queue_consume_count_for_done_ids(entries: &[QueueEntry], done_ids: &[String]) -> usize {
    if done_ids.is_empty() {
        return 0;
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();
    let mut count = 0usize;
    for entry in entries {
        let QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        let Some(id) = queue_prompt_done_id(&prompt.text) else {
            break;
        };
        if done_ids.contains(&id) {
            count += 1;
            continue;
        }
        break;
    }
    count
}

pub fn queue_prompt_texts_match_for_consumption(left: &str, right: &str) -> bool {
    document_queue::strip_priority_markers(left) == document_queue::strip_priority_markers(right)
}

pub fn mark_first_matching_prompts_completed_by_texts(
    entries: &[QueueEntry],
    target_texts: &[String],
) -> Option<Vec<QueueEntry>> {
    let mut remaining_targets = target_texts.to_vec();
    let mut marked = Vec::with_capacity(target_texts.len());
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        if let QueueEntry::Prompt(prompt) = entry
            && let Some(pos) = remaining_targets
                .iter()
                .position(|target| queue_prompt_texts_match_for_consumption(&prompt.text, target))
        {
            let mut completed = prompt.clone();
            completed.text = document_queue::strip_in_progress_marker(&completed.text);
            marked.push(remaining_targets.remove(pos));
            result.push(QueueEntry::Completed(completed));
            continue;
        }
        result.push(entry.clone());
    }
    if marked.len() == target_texts.len() {
        Some(result)
    } else {
        None
    }
}

pub fn mark_entries_completed_by_done_ids(
    entries: &[QueueEntry],
    done_ids: &[String],
) -> (Vec<QueueEntry>, Vec<String>) {
    if done_ids.is_empty() {
        return (entries.to_vec(), Vec::new());
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();
    let mut marked_texts = Vec::new();
    let entries = entries
        .iter()
        .map(|entry| match entry {
            QueueEntry::Prompt(prompt)
                if queue_prompt_done_id(&prompt.text).is_some_and(|id| done_ids.contains(&id)) =>
            {
                let mut completed = prompt.clone();
                completed.text = document_queue::strip_in_progress_marker(&completed.text);
                marked_texts.push(completed.text.clone());
                QueueEntry::Completed(completed)
            }
            _ => entry.clone(),
        })
        .collect();
    (entries, marked_texts)
}

pub fn normalized_done_id_bag(texts: &[String]) -> Vec<String> {
    let mut ids = texts
        .iter()
        .filter_map(|text| queue_prompt_done_id(text))
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(body: &str) -> Vec<QueueEntry> {
        document_queue::parse(body).unwrap()
    }

    #[test]
    fn first_n_queue_prompt_texts_skips_non_prompts_and_strips_in_progress_marker() {
        let body = format!(
            "--- stop\n- {} do [#head]\n- do [#tail]\n",
            document_queue::IN_PROGRESS_MARKER,
        );
        let entries = entries(&body);

        assert_eq!(
            first_n_queue_prompt_texts(&entries, 1),
            vec!["do [#head]".to_string()]
        );
    }

    #[test]
    fn queue_consume_count_for_done_ids_counts_leading_matching_id_prompts_only() {
        let entries = entries(concat!(
            "- do [#head]\n",
            "- do [#tail]\n",
            "- do [#other]\n",
        ));

        assert_eq!(
            queue_consume_count_for_done_ids(&entries, &["tail".to_string(), "head".to_string()]),
            2
        );
        assert_eq!(
            queue_consume_count_for_done_ids(&entries, &["tail".to_string()]),
            0
        );
    }

    #[test]
    fn mark_entries_completed_by_done_ids_marks_matching_live_prompts_only() {
        let entries = entries(concat!(
            "- do [#head]\n",
            "- do [#opportunistic]\n",
            "- ~~do [#already]~~\n",
            "- do [#tail]\n",
        ));

        let (updated, marked) =
            mark_entries_completed_by_done_ids(&entries, &["opportunistic".to_string()]);

        assert_eq!(marked, vec!["do [#opportunistic]".to_string()]);
        assert_eq!(
            document_queue::render(&updated),
            concat!(
                "- do [#head]\n",
                "- ~~do [#opportunistic]~~\n",
                "- ~~do [#already]~~\n",
                "- do [#tail]\n",
            )
        );
    }

    #[test]
    fn mark_entries_completed_by_done_ids_ignores_already_completed_prompt() {
        let entries = entries(concat!(
            "- do [#head]\n",
            "- ~~do [#opportunistic]~~\n",
            "- do [#tail]\n",
        ));

        let (updated, marked) =
            mark_entries_completed_by_done_ids(&entries, &["opportunistic".to_string()]);
        assert!(marked.is_empty());
        assert_eq!(updated, entries);
    }

    #[test]
    fn mark_first_matching_prompts_completed_by_texts_matches_priority_marker_identity() {
        let entries = entries("- :pushpin: deploy\n- do [#tail]\n");

        let updated =
            mark_first_matching_prompts_completed_by_texts(&entries, &["deploy".to_string()])
                .unwrap();

        assert_eq!(
            document_queue::render(&updated),
            "- ~~:pushpin: deploy~~\n- do [#tail]\n"
        );
    }

    #[test]
    fn normalized_done_id_bag_sorts_ids_from_prompt_texts() {
        assert_eq!(
            normalized_done_id_bag(&["do [#b]".to_string(), "do #a".to_string()]),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}

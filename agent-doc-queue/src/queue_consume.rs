//! Pure queue prompt consumption policy.
//!
//! This module owns entry-level consume planning helpers. Callers still own
//! file IO, snapshots, IPC node operations, and editor convergence.

use std::collections::HashSet;

use crate::{
    document_queue::{self, QueueEntry},
    queue_response::{normalize_done_id, queue_prompt_done_id, queue_prompt_text_is_free_text},
};

/// The deterministic, visible explanation appended to a struck free-text queue
/// head (`#qstrikenote`). It is fixed text and lives outside the `~~...~~`
/// wrapper so the original head text stays struck and readable.
pub const STRUCK_FREE_TEXT_NOTE: &str = "answered this cycle (#ftstrike)";

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

/// Given a single queue line, append the deterministic auto-struck explanation
/// when the line is a struck free-text head that is not already annotated.
pub fn annotate_struck_free_text_line(line: &str) -> String {
    let (core, newline) = match line.strip_suffix('\n') {
        Some(rest) => (rest, "\n"),
        None => (line, ""),
    };
    let trimmed_end = core.trim_end();
    let trailing_ws = &core[trimmed_end.len()..];
    if trimmed_end.contains(agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR) {
        return line.to_string();
    }
    if !trimmed_end.ends_with("~~") {
        return line.to_string();
    }
    let content = strip_list_bullet_prefix(trimmed_end);
    let Some(inner) = content
        .strip_prefix("~~")
        .and_then(|rest| rest.strip_suffix("~~"))
    else {
        return line.to_string();
    };
    if inner.trim().is_empty() {
        return line.to_string();
    }
    format!(
        "{trimmed_end}{}{STRUCK_FREE_TEXT_NOTE}{trailing_ws}{newline}",
        agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR
    )
}

/// Strip a leading markdown list bullet from a line's content.
fn strip_list_bullet_prefix(line: &str) -> &str {
    let t = line.trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    if let Some(dot) = t.find(". ")
        && t[..dot].chars().all(|c| c.is_ascii_digit())
        && !t[..dot].is_empty()
    {
        return t[dot + 2..].trim_start();
    }
    t
}

/// Apply the auto-struck annotation to every free-text queue head that became
/// struck between `before` and `after`.
pub fn annotate_newly_struck_free_text_heads(before: &str, after: &str) -> anyhow::Result<String> {
    let struck_before: HashSet<String> =
        agent_doc_markdown_ast::mutations::item_nodes(before, "queue")
            .map(|nodes| {
                nodes
                    .into_iter()
                    .filter(|n| n.item.struck)
                    .map(|n| n.node_key)
                    .collect()
            })
            .unwrap_or_default();

    let nodes = match agent_doc_markdown_ast::mutations::item_nodes(after, "queue") {
        Ok(nodes) => nodes,
        Err(_) => return Ok(after.to_string()),
    };

    let mut edits: Vec<(usize, usize)> = Vec::new();
    for node in &nodes {
        if !node.item.struck {
            continue;
        }
        let text = node.item.text.trim();
        if text.is_empty() || !queue_prompt_text_is_free_text(after, text) {
            continue;
        }
        if struck_before.contains(&node.node_key) {
            continue;
        }
        let line = &after[node.item.start_byte..node.item.end_byte];
        if line.contains(agent_doc_markdown_ast::overlay::STRUCK_ANNOTATION_SEPARATOR) {
            continue;
        }
        edits.push((node.item.start_byte, node.item.end_byte));
    }

    if edits.is_empty() {
        return Ok(after.to_string());
    }
    edits.sort_by_key(|(start, _)| *start);
    let mut out = after.to_string();
    for (start, end) in edits.into_iter().rev() {
        let annotated = annotate_struck_free_text_line(&out[start..end]);
        out.replace_range(start..end, &annotated);
    }
    Ok(out)
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

    #[test]
    fn annotate_struck_free_text_line_is_idempotent_and_targeted() {
        assert_eq!(
            annotate_struck_free_text_line("- ~~foo~~"),
            "- ~~foo~~ — auto-struck: answered this cycle (#ftstrike)"
        );
        let once = annotate_struck_free_text_line("- ~~foo~~");
        assert_eq!(annotate_struck_free_text_line(&once), once);
        assert_eq!(annotate_struck_free_text_line("- foo"), "- foo");
        assert_eq!(
            annotate_struck_free_text_line("~~bar baz~~"),
            "~~bar baz~~ — auto-struck: answered this cycle (#ftstrike)"
        );
        assert_eq!(
            annotate_struck_free_text_line("- ~~foo~~\n"),
            "- ~~foo~~ — auto-struck: answered this cycle (#ftstrike)\n"
        );
        assert_eq!(annotate_struck_free_text_line("- ~~~~"), "- ~~~~");
    }

    #[test]
    fn annotated_struck_line_still_parses_as_struck_node() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- ~~answered free-text head~~ — auto-struck: answered this cycle (#ftstrike)\n",
            "<!-- /agent:queue -->\n",
        );
        let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue").unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].item.struck, "annotated head must parse as struck");
        assert_eq!(
            nodes[0].item.text.trim(),
            "answered free-text head",
            "the inner text must exclude both the wrapper and the note"
        );
    }

    #[test]
    fn annotate_newly_struck_free_text_heads_only_marks_new_queue_strikes() {
        let before = concat!(
            "<!-- agent:queue -->\n",
            "- open report\n",
            "- ~~old report~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:exchange -->\n",
            "unchanged\n",
            "<!-- /agent:exchange -->\n",
        );
        let after = concat!(
            "<!-- agent:queue -->\n",
            "- ~~open report~~\n",
            "- ~~old report~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:exchange -->\n",
            "unchanged\n",
            "<!-- /agent:exchange -->\n",
        );

        let annotated = annotate_newly_struck_free_text_heads(before, after).unwrap();

        assert!(
            annotated.contains("- ~~open report~~ — auto-struck: answered this cycle (#ftstrike)")
        );
        assert!(annotated.contains("- ~~old report~~\n"));
        assert!(annotated.contains("<!-- agent:exchange -->\nunchanged\n<!-- /agent:exchange -->"));
        assert_eq!(
            annotate_newly_struck_free_text_heads(after, &annotated).unwrap(),
            annotated
        );
    }
}

//! Queue convergence helpers for reconciling pre-maintenance baselines with
//! post-maintenance queue state.
//!
//! Callers provide document text. File IO, snapshot persistence, and preflight
//! side effects stay in orchestration.

use std::collections::HashSet;

use agent_doc_document::queue_projection::strip_priority_markers;
use agent_doc_element::element;
use agent_doc_element_backlog::backlog;

use crate::document_queue;

/// Splice the converged `agent:queue` component into `current`.
///
/// Returns `None` when the queue component is unchanged or either document lacks
/// a parseable queue component.
pub fn realign_baseline_to_converged_queue(baseline: &str, converged: &str) -> Option<String> {
    let cur_comps = element::parse(baseline).ok()?;
    let conv_comps = element::parse(converged).ok()?;
    let cur_q = cur_comps.iter().find(|c| c.name == "queue")?;
    let conv_q = conv_comps.iter().find(|c| c.name == "queue")?;
    let cur_q_text = &baseline[cur_q.open_start..cur_q.close_end];
    let conv_q_text = &converged[conv_q.open_start..conv_q.close_end];
    if cur_q_text == conv_q_text {
        return None;
    }
    let mut spliced = String::with_capacity(baseline.len() + conv_q_text.len());
    spliced.push_str(&baseline[..cur_q.open_start]);
    spliced.push_str(conv_q_text);
    spliced.push_str(&baseline[cur_q.close_end..]);
    for component_name in ["backlog", "pending", "icebox"] {
        if let Some(next) = realign_component_when_only_in_progress_marker_changed(
            &spliced,
            converged,
            component_name,
        ) {
            spliced = next;
        }
    }
    if let Ok((frontmatter, _)) = agent_doc_frontmatter::frontmatter::parse(converged)
        && let Some(control) = frontmatter.queue.as_deref()
        && let Ok(with_control) =
            agent_doc_frontmatter::frontmatter::merge_queue_control(&spliced, control)
    {
        spliced = with_control;
    }
    Some(spliced)
}

/// True when the only meaningful document difference is non-selected future
/// queue state, with exchange boundary marker churn ignored.
pub fn queue_body_diff_is_non_selected_future_state(
    previous: &str,
    current: &str,
    selected_head: &str,
) -> bool {
    let Some(previous_head) = first_queue_prompt_identity(previous) else {
        return false;
    };
    if previous_head != queue_prompt_identity(selected_head) {
        return false;
    }
    let Some(previous_outside_queue_body) = content_without_queue_body(previous) else {
        return false;
    };
    let Some(current_outside_queue_body) = content_without_queue_body(current) else {
        return false;
    };
    if strip_exchange_boundary_lines(&previous_outside_queue_body)
        != strip_exchange_boundary_lines(&current_outside_queue_body)
    {
        return false;
    }
    queue_body(previous) != queue_body(current)
}

/// True when the active queue head still matches the queue head captured in the
/// snapshot content.
pub fn selected_queue_head_unchanged_in_snapshot(
    snapshot_content: &str,
    current_entries: &[document_queue::QueueEntry],
) -> bool {
    let current_prompts = document_queue::prompts(current_entries);
    let Some(current_head) = current_prompts.first() else {
        return false;
    };
    let Some(snapshot_entries) = queue_entries(snapshot_content) else {
        return false;
    };
    let snapshot_prompts = document_queue::prompts(&snapshot_entries);
    let Some(snapshot_head) = snapshot_prompts.first() else {
        return false;
    };
    strip_priority_markers(&snapshot_head.text) == strip_priority_markers(&current_head.text)
}

/// True when the full `agent:queue` component region differs between the
/// snapshot and current document content.
pub fn queue_region_differs_from_snapshot(snapshot_content: &str, current_content: &str) -> bool {
    let Some(current_queue) = queue_region(current_content) else {
        return false;
    };
    let Some(snapshot_queue) = queue_region(snapshot_content) else {
        return false;
    };
    current_queue != snapshot_queue
}

/// True when an inactive queue's current entries differ from the queue body in
/// snapshot content.
///
/// Comparison is normalized through queue parsing/rendering so insignificant
/// body formatting churn does not register as an operator edit. Missing or
/// unparseable snapshot queue content is treated as changed so a newly populated
/// inactive queue still warns.
pub fn inactive_queue_changed_vs_snapshot(
    snapshot_content: &str,
    current_entries: &[document_queue::QueueEntry],
) -> bool {
    let Some(snapshot_entries) = queue_entries(snapshot_content) else {
        return true;
    };
    document_queue::render(&snapshot_entries) != document_queue::render(current_entries)
}

/// True when the queue body contains only drained non-live residue.
pub fn queue_entries_are_drained_residue(entries: &[document_queue::QueueEntry]) -> bool {
    !entries.is_empty()
        && entries.iter().all(|entry| {
            matches!(
                entry,
                document_queue::QueueEntry::Completed(_)
                    | document_queue::QueueEntry::Preset(_)
                    | document_queue::QueueEntry::Dispatch(_)
            )
        })
}

fn first_queue_prompt_identity(content: &str) -> Option<String> {
    let components = element::parse(content).ok()?;
    let queue = components
        .iter()
        .find(|component| component.name == "queue")?;
    let body = &content[queue.open_end..queue.close_start];
    let entries = document_queue::parse(body).ok()?;
    document_queue::prompts(&entries)
        .first()
        .map(|prompt| queue_prompt_identity(&prompt.text))
}

fn queue_prompt_identity(prompt: &str) -> String {
    strip_priority_markers(prompt)
}

fn content_without_queue_body(content: &str) -> Option<String> {
    let components = element::parse(content).ok()?;
    let queue = components
        .iter()
        .find(|component| component.name == "queue")?;
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..queue.open_end]);
    out.push_str(&content[queue.close_start..]);
    Some(out)
}

fn queue_body(content: &str) -> Option<&str> {
    let components = element::parse(content).ok()?;
    let queue = components
        .iter()
        .find(|component| component.name == "queue")?;
    Some(&content[queue.open_end..queue.close_start])
}

fn queue_region(content: &str) -> Option<&str> {
    let components = element::parse(content).ok()?;
    let queue = components
        .iter()
        .find(|component| component.name == "queue")?;
    Some(&content[queue.open_start..queue.close_end])
}

fn queue_entries(content: &str) -> Option<Vec<document_queue::QueueEntry>> {
    document_queue::parse(queue_body(content)?).ok()
}

fn strip_exchange_boundary_lines(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for segment in content.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\r', '\n']);
        if line.trim_start().starts_with("<!-- agent:boundary:") {
            continue;
        }
        out.push_str(segment);
    }
    out
}

fn realign_component_when_only_in_progress_marker_changed(
    current: &str,
    converged: &str,
    component_name: &str,
) -> Option<String> {
    let current_components = element::parse(current).ok()?;
    let converged_components = element::parse(converged).ok()?;
    let current_component = current_components
        .iter()
        .find(|component| component.name == component_name)?;
    let converged_component = converged_components
        .iter()
        .find(|component| component.name == component_name)?;
    let current_body = current_component.content(current);
    let converged_body = converged_component.content(converged);
    if current_body == converged_body {
        return None;
    }
    if body_without_in_progress_markers(current_body)
        != body_without_in_progress_markers(converged_body)
    {
        return None;
    }
    Some(current_component.replace_content(current, converged_body))
}

fn body_without_in_progress_markers(body: &str) -> String {
    let ids = HashSet::new();
    backlog::set_in_progress_item_ids(body, &ids).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_body_diff_future_state_ignores_exchange_boundary_churn() {
        let previous = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- 🚧 do [#active]\n",
            "- do [#old]\n",
            "<!-- /agent:queue -->\n"
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Done.\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- 🚧 do [#active]\n",
            "- do [#new]\n",
            "<!-- /agent:queue -->\n"
        );

        assert!(queue_body_diff_is_non_selected_future_state(
            previous,
            current,
            "do [#active]"
        ));
        let current_with_exchange_edit = current.replace("Done.", "Done.\nOperator follow-up.");
        assert!(!queue_body_diff_is_non_selected_future_state(
            previous,
            &current_with_exchange_edit,
            "do [#active]"
        ));
    }

    #[test]
    fn snapshot_predicates_compare_queue_content_without_io() {
        let snapshot = concat!(
            "before\n",
            "<!-- agent:queue priority -->\n",
            "- :pushpin: do [#alpha]\n",
            "- do [#old]\n",
            "<!-- /agent:queue -->\n",
            "after\n",
        );
        let current_entries = document_queue::parse("- do [#alpha]\n- do [#new]\n").unwrap();

        assert!(selected_queue_head_unchanged_in_snapshot(
            snapshot,
            &current_entries
        ));

        let current_with_queue_edit = snapshot.replace("- do [#old]", "- do [#new]");
        assert!(queue_region_differs_from_snapshot(
            snapshot,
            &current_with_queue_edit
        ));

        let current_with_outside_edit = snapshot.replace("before", "changed before");
        assert!(!queue_region_differs_from_snapshot(
            snapshot,
            &current_with_outside_edit
        ));
    }

    #[test]
    fn inactive_queue_changed_vs_snapshot_normalizes_entries() {
        let snapshot = concat!(
            "<!-- agent:queue -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n",
        );
        let same_entries = document_queue::parse("- do [#alpha]\n").unwrap();
        let changed_entries = document_queue::parse("- do [#beta]\n").unwrap();

        assert!(!inactive_queue_changed_vs_snapshot(snapshot, &same_entries));
        assert!(inactive_queue_changed_vs_snapshot(
            snapshot,
            &changed_entries
        ));
        assert!(inactive_queue_changed_vs_snapshot(
            "no queue component",
            &same_entries
        ));
    }

    #[test]
    fn queue_entries_are_drained_residue_requires_non_live_entries() {
        let completed = document_queue::QueueEntry::Completed(document_queue::QueuePrompt {
            text: "do [#done]".to_string(),
            multiline: false,
            indent: 0,
        });
        let prompt = document_queue::QueueEntry::Prompt(document_queue::QueuePrompt {
            text: "do [#live]".to_string(),
            multiline: false,
            indent: 0,
        });

        assert!(queue_entries_are_drained_residue(&[
            completed.clone(),
            document_queue::QueueEntry::Preset("#spec".to_string()),
            document_queue::QueueEntry::Dispatch("@review".to_string()),
        ]));
        assert!(!queue_entries_are_drained_residue(&[]));
        assert!(!queue_entries_are_drained_residue(&[completed, prompt]));
    }

    #[test]
    fn realign_baseline_splices_converged_queue_and_in_progress_only_components() {
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] run the alpha task\n",
            "<!-- /agent:backlog -->\n"
        );
        let converged = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Done.\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#alpha] run the alpha task\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] 🚧 [#alpha] run the alpha task\n",
            "<!-- /agent:backlog -->\n"
        );

        let realigned = realign_baseline_to_converged_queue(current, converged)
            .expect("queue convergence should realign");

        assert!(realigned.contains("- do [#alpha] run the alpha task"));
        assert!(realigned.contains("- [ ] 🚧 [#alpha] run the alpha task"));
        assert!(!realigned.contains("<!-- agent:boundary:test -->"));
    }
}

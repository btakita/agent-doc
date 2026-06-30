//! Queue convergence helpers for reconciling pre-maintenance baselines with
//! post-maintenance queue state.
//!
//! Callers provide document text. File IO, snapshot persistence, and preflight
//! side effects stay in orchestration.

use std::collections::HashSet;

use agent_doc_element::element;
use agent_doc_element_backlog::backlog;

use crate::document_queue;

/// Splice the converged `agent:queue` component into `current`.
///
/// Returns `None` when the queue component is unchanged or either document lacks
/// a parseable queue component.
pub fn realign_baseline_to_converged_queue(current: &str, converged: &str) -> Option<String> {
    let cur_comps = element::parse(current).ok()?;
    let conv_comps = element::parse(converged).ok()?;
    let cur_q = cur_comps.iter().find(|c| c.name == "queue")?;
    let conv_q = conv_comps.iter().find(|c| c.name == "queue")?;
    let cur_q_text = &current[cur_q.open_start..cur_q.close_end];
    let conv_q_text = &converged[conv_q.open_start..conv_q.close_end];
    if cur_q_text == conv_q_text {
        return None;
    }
    let mut spliced = String::with_capacity(current.len() + conv_q_text.len());
    spliced.push_str(&current[..cur_q.open_start]);
    spliced.push_str(conv_q_text);
    spliced.push_str(&current[cur_q.close_end..]);
    for component_name in ["backlog", "pending", "icebox"] {
        if let Some(next) = realign_component_when_only_in_progress_marker_changed(
            &spliced,
            converged,
            component_name,
        ) {
            spliced = next;
        }
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
    document_queue::strip_priority_markers(prompt)
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

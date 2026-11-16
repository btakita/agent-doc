//! `#todolink`: a todo's own id and the queue row that names that id are one link.
//!
//! Checking the todo strikes the linked row. A completion strike checks the
//! owning todo. Both happen in the binary, on the document bytes, not as a
//! second hand edit. A row annotated `acknowledged;` (`#nestedechostrike`) is
//! not a completion strike: the tracked item stays open, and reopening the
//! todo does not unstrike that row.

use std::collections::{HashMap, HashSet};

use agent_doc_element::element;
use agent_doc_element_queue::QueueItemIdentity;
use agent_doc_markdown_ast::mutations::{
    MutationError, MutationItemNode, MutationNodePatch, MutationNodePatchOp, apply_node_patches,
    consume_nodes_all, item_nodes_all,
};
use anyhow::Result;

use crate::ack_stays_open::ACK_STAYS_OPEN_NOTE_LEAD;
use crate::queue_continuation::{
    active_tracked_ids, gated_review_ids, head_carries_operator_verdict, operator_answered_head_ids,
};
use crate::queue_response::head_id_is_registered_preset;

/// Strike live queue rows linked to a checked todo. `Ok(None)` when nothing changed.
pub fn strike_queue_for_checked_todos(content: &str) -> Result<Option<String>> {
    let checked = eligible_checked_todo_ids(content);
    if checked.is_empty() {
        return Ok(None);
    }
    let Some(nodes) = queue_item_nodes(content)? else {
        return Ok(None);
    };
    let answered = operator_answered_head_ids(content);
    // A verdict on any live linked copy keeps every copy of that id live, matching
    // the preflight `todo_checked` exclusion of `operator_answered_head_ids`.
    let verdict_ids: HashSet<String> = nodes
        .iter()
        .filter(|node| !node.item.struck && head_carries_operator_verdict(&node.item.text))
        .filter_map(|node| linked_queue_todo_id(&node.item.text))
        .collect();
    let keys: Vec<String> = nodes
        .iter()
        .filter(|node| !node.item.struck)
        .filter(|node| !row_keeps_work_open(content, node))
        .filter_map(|node| {
            let id = linked_queue_todo_id(&node.item.text)?;
            if !checked.contains(&id) || answered.contains(&id) || verdict_ids.contains(&id) {
                return None;
            }
            Some(node.node_key.clone())
        })
        .collect();
    if keys.is_empty() {
        return Ok(None);
    }
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let next = consume_nodes_all(content, "queue", &key_refs)
        .map_err(|err| anyhow::anyhow!("#todolink queue strike: {err}"))?;
    unchanged(content, next)
}

/// Check a todo whose every linked queue row is a completion strike.
/// `Ok(None)` when no owning todo changed.
pub fn check_todos_for_struck_queue(content: &str) -> Result<Option<String>> {
    let Some(nodes) = queue_item_nodes(content)? else {
        return Ok(None);
    };
    let open = agent_doc_element::todo::open_todo_ids(content);
    if open.is_empty() {
        return Ok(None);
    }
    let active = active_tracked_ids(content);
    let gated = gated_review_ids(content);
    let mut linked: HashMap<String, LinkStrike> = HashMap::new();
    for node in &nodes {
        let Some(id) = linked_queue_todo_id(&node.item.text) else {
            continue;
        };
        if !open.contains(&id) || link_blocked(content, &id, &active, &gated) {
            continue;
        }
        let entry = linked.entry(id).or_default();
        entry.saw_row = true;
        if !node.item.struck {
            entry.live = true;
        }
        if row_keeps_work_open(content, node) {
            entry.ack = true;
        }
    }
    let mut ids: Vec<String> = linked
        .into_iter()
        .filter(|(_, state)| state.saw_row && !state.live && !state.ack)
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    let mut current = content.to_string();
    let mut changed = false;
    for id in ids {
        if let Some(next) = agent_doc_element::todo::check_off_owning_todo_id(&current, &id)? {
            current = next;
            changed = true;
        }
    }
    if changed { Ok(Some(current)) } else { Ok(None) }
}

/// Strike for checked todos, then check todos for completion strikes, until stable.
pub fn sync_linked_todo_queue(content: &str) -> Result<Option<String>> {
    let mut current = content.to_string();
    let mut changed = false;
    for _ in 0..4 {
        let mut pass = false;
        if let Some(next) = strike_queue_for_checked_todos(&current)? {
            current = next;
            pass = true;
        }
        if let Some(next) = check_todos_for_struck_queue(&current)? {
            current = next;
            pass = true;
        }
        if !pass {
            break;
        }
        changed = true;
    }
    if changed { Ok(Some(current)) } else { Ok(None) }
}

/// Unstrike completion-linked rows for todos that just became open.
///
/// `before` is the document before the checkbox write; `after` is the document
/// after it. Ack stays-open rows, open tracked work, gated review, presets,
/// and ids already in `agent:done` stay struck.
pub fn restore_linked_queue_after_todo_reopen(before: &str, after: &str) -> Result<Option<String>> {
    let reopened: HashSet<String> = agent_doc_element::todo::open_todo_ids(after)
        .difference(&agent_doc_element::todo::open_todo_ids(before))
        .cloned()
        .collect();
    if reopened.is_empty() {
        return Ok(None);
    }
    let Some(nodes) = queue_item_nodes(after)? else {
        return Ok(None);
    };
    let active = active_tracked_ids(after);
    let gated = gated_review_ids(after);
    let done = agent_doc_element_done::collect_done_document_own_ids(after);
    let patches: Vec<MutationNodePatch> = nodes
        .iter()
        .filter(|node| node.item.struck)
        .filter(|node| !row_keeps_work_open(after, node))
        .filter_map(|node| {
            let id = linked_queue_todo_id(&node.item.text)?;
            if !reopened.contains(&id)
                || active.contains(&id)
                || gated.contains(&id)
                || done.contains(&id)
                || head_id_is_registered_preset(after, &id)
            {
                return None;
            }
            Some(unstrike_patch(&node.node_key))
        })
        .collect();
    if patches.is_empty() {
        return Ok(None);
    }
    let next = apply_node_patches(after, &patches)
        .map_err(|err| anyhow::anyhow!("#todolink queue restore: {err}"))?;
    unchanged(after, next)
}

#[derive(Default)]
struct LinkStrike {
    saw_row: bool,
    live: bool,
    ack: bool,
}

fn eligible_checked_todo_ids(content: &str) -> HashSet<String> {
    let active = active_tracked_ids(content);
    let gated = gated_review_ids(content);
    agent_doc_element::todo::checked_todo_own_ids(content)
        .into_iter()
        .map(|id| id.to_ascii_lowercase())
        .filter(|id| !link_blocked(content, id, &active, &gated))
        .collect()
}

fn link_blocked(
    content: &str,
    id: &str,
    active: &HashSet<String>,
    gated: &HashSet<String>,
) -> bool {
    active.contains(id) || gated.contains(id) || head_id_is_registered_preset(content, id)
}

/// Work identity of a queue row when it names a todo's own id.
///
/// [`queue_head_identity`](agent_doc_element_queue::queue_head_identity) covers
/// `do [#id]`, `[#id]`, and `do [#id]: note`. A leading `[#id]:` head is the
/// same link. A later citation, and a generated `[#q-…]` correlation id, are not.
fn linked_queue_todo_id(work_text: &str) -> Option<String> {
    let head = agent_doc_element_queue::subprompts::head_without_subprompts(work_text);
    match agent_doc_element_queue::queue_head_identity(head) {
        QueueItemIdentity::Id(id) => {
            if generated_correlation_only(head, &id) {
                None
            } else {
                Some(id.to_ascii_lowercase())
            }
        }
        QueueItemIdentity::FreeText(_) => leading_bracket_colon_id(head),
    }
}

fn generated_correlation_only(head: &str, id: &str) -> bool {
    matches!(
        agent_doc_element_queue::queue_prompt_stable_identity(head),
        QueueItemIdentity::FreeText(key) if key == format!("queue-id:{id}")
    )
}

fn leading_bracket_colon_id(text: &str) -> Option<String> {
    let visible = agent_doc_element_queue::strip_intent_tags(text);
    let rest = visible.trim().strip_prefix("[#")?;
    let (id, tail) = rest.split_once(']')?;
    if !is_link_id(id) {
        return None;
    }
    let tail = tail.trim_start();
    (tail.is_empty() || tail.starts_with(':')).then(|| id.to_ascii_lowercase())
}

fn is_link_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn row_keeps_work_open(source: &str, node: &MutationItemNode) -> bool {
    node.item.raw.contains(ACK_STAYS_OPEN_NOTE_LEAD)
        || source
            .get(node.item.start_byte..node.item.end_byte)
            .is_some_and(|span| span.contains(ACK_STAYS_OPEN_NOTE_LEAD))
}

fn queue_item_nodes(content: &str) -> Result<Option<Vec<MutationItemNode>>> {
    let components = element::parse(content)?;
    if !components.iter().any(|component| component.name == "queue") {
        return Ok(None);
    }
    match item_nodes_all(content, "queue") {
        Ok(nodes) => Ok(Some(nodes)),
        Err(MutationError::ComponentNotFound(_)) => Ok(None),
        Err(err) => Err(anyhow::anyhow!("#todolink: {err}")),
    }
}

fn unstrike_patch(node_key: &str) -> MutationNodePatch {
    MutationNodePatch {
        component: "queue".to_string(),
        node_key: node_key.to_string(),
        op: MutationNodePatchOp::Unstrike,
        content: None,
        expected_content: None,
        before: None,
        after: None,
        order: Vec::new(),
    }
}

fn unchanged(before: &str, after: String) -> Result<Option<String>> {
    if after == before {
        Ok(None)
    } else {
        Ok(Some(after))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(front: &str, queue: &str, todo: &str, rest: &str) -> String {
        format!(
            "---\nagent_doc_session: todolink\n{front}---\n\n\
             ## Run\n\n<!-- agent:run subagents -->\n{queue}<!-- /agent:run -->\n\n\
             ## Todo\n\n<!-- agent:todo -->\n{todo}<!-- /agent:todo -->\n{rest}"
        )
    }

    fn strike(content: &str) -> String {
        strike_queue_for_checked_todos(content)
            .unwrap()
            .unwrap_or_else(|| content.to_string())
    }

    #[test]
    fn a_checked_todo_strikes_its_queue_row_and_leaves_an_unrelated_row() {
        let content = document(
            "",
            "- do [#alpha]\n- do [#beta]\n",
            "- [x] [#alpha] finish alpha\n- [ ] [#beta] leave beta\n",
            "",
        );
        let next = strike(&content);
        assert!(next.contains("~~do [#alpha]~~"), "{next}");
        assert!(!next.contains("~~do [#beta]~~"), "{next}");
        assert!(next.contains("- [x] [#alpha]"), "{next}");
        assert!(next.contains("- [ ] [#beta]"), "{next}");
    }

    #[test]
    fn a_leading_bracket_colon_head_is_the_same_link() {
        // `[#id]:` is an operator-verdict shape, so a checked todo does not
        // auto-strike it. A completion strike of that head still checks the todo.
        let live = document(
            "",
            "- [#alpha]: rename the sdk\n",
            "- [x] [#alpha] rename\n",
            "",
        );
        assert!(strike_queue_for_checked_todos(&live).unwrap().is_none());
        let struck = document(
            "",
            "- ### ~~[#alpha]: rename the sdk~~\n",
            "- [ ] [#alpha] rename\n",
            "",
        );
        let next = check_todos_for_struck_queue(&struck).unwrap().unwrap();
        assert!(next.contains("- [x] [#alpha]"), "{next}");
    }

    #[test]
    fn a_struck_queue_row_checks_the_owning_todo() {
        let content = document(
            "",
            "- ### ~~do [#alpha]~~\n- do [#beta]\n",
            "- [ ] [#alpha] finish alpha\n- [ ] [#beta] leave beta\n",
            "",
        );
        let next = check_todos_for_struck_queue(&content).unwrap().unwrap();
        assert!(next.contains("- [x] [#alpha]"), "{next}");
        assert!(next.contains("- [ ] [#beta]"), "{next}");
        assert!(next.contains("~~do [#alpha]~~"), "{next}");
        assert!(!next.contains("~~do [#beta]~~"), "{next}");
    }

    #[test]
    fn an_open_todo_with_a_live_row_is_unchanged() {
        let content = document("", "- do [#alpha]\n", "- [ ] [#alpha] still open\n", "");
        assert!(strike_queue_for_checked_todos(&content).unwrap().is_none());
        assert!(check_todos_for_struck_queue(&content).unwrap().is_none());
    }

    #[test]
    fn a_missing_queue_is_a_no_op() {
        let content = "<!-- agent:todo -->\n- [x] [#alpha] done\n<!-- /agent:todo -->\n";
        assert!(strike_queue_for_checked_todos(content).unwrap().is_none());
        assert!(check_todos_for_struck_queue(content).unwrap().is_none());
    }

    #[test]
    fn a_checked_todo_sharing_an_open_backlog_id_does_not_strike() {
        let content = document(
            "",
            "- do [#alpha]\n",
            "- [x] [#alpha] approve the backlog work\n",
            "\n## Backlog\n\n<!-- agent:backlog -->\n- [ ] [#alpha] tracked work\n<!-- /agent:backlog -->\n",
        );
        assert!(strike_queue_for_checked_todos(&content).unwrap().is_none());
    }

    #[test]
    fn a_struck_backlog_row_does_not_check_a_todo_that_shares_the_open_id() {
        let content = document(
            "",
            "- ### ~~do [#alpha]~~\n",
            "- [ ] [#alpha] approve the backlog work\n",
            "\n## Backlog\n\n<!-- agent:backlog -->\n- [ ] [#alpha] tracked work\n<!-- /agent:backlog -->\n",
        );
        assert!(check_todos_for_struck_queue(&content).unwrap().is_none());
    }

    #[test]
    fn a_citation_is_not_a_link() {
        let content = document(
            "",
            "- see [#alpha] in the notes\n",
            "- [x] [#alpha] finish alpha\n",
            "",
        );
        assert!(strike_queue_for_checked_todos(&content).unwrap().is_none());
        let struck = document(
            "",
            "- ### ~~see [#alpha] in the notes~~\n",
            "- [ ] [#alpha] finish alpha\n",
            "",
        );
        assert!(check_todos_for_struck_queue(&struck).unwrap().is_none());
    }

    #[test]
    fn an_operator_verdict_head_is_not_auto_struck() {
        let content = document(
            "",
            "- do [#alpha]: still waiting on the hold\n",
            "- [x] [#alpha] finish alpha\n",
            "",
        );
        assert!(strike_queue_for_checked_todos(&content).unwrap().is_none());
        let struck = document(
            "",
            "- ### ~~do [#alpha]: still waiting on the hold~~\n",
            "- [ ] [#alpha] finish alpha\n",
            "",
        );
        let next = check_todos_for_struck_queue(&struck).unwrap().unwrap();
        assert!(next.contains("- [x] [#alpha]"), "{next}");
    }

    #[test]
    fn an_acknowledged_strike_does_not_check_the_todo() {
        let content = document(
            "",
            "- ### ~~do [#alpha]~~ — auto-struck: acknowledged; Todo #alpha stays open: the check is still open\n",
            "- [ ] [#alpha] finish alpha\n",
            "",
        );
        assert!(check_todos_for_struck_queue(&content).unwrap().is_none());
    }

    #[test]
    fn a_live_duplicate_wins_over_a_struck_copy() {
        let content = document(
            "",
            "- do [#alpha]\n- ### ~~do [#alpha]~~\n",
            "- [ ] [#alpha] finish alpha\n",
            "",
        );
        assert!(check_todos_for_struck_queue(&content).unwrap().is_none());
    }

    #[test]
    fn both_directions_meet_in_one_document() {
        let content = document(
            "",
            "- do [#alpha]\n- ### ~~do [#beta]~~\n",
            "- [x] [#alpha] finish alpha\n- [ ] [#beta] finish beta\n",
            "",
        );
        let next = sync_linked_todo_queue(&content).unwrap().unwrap();
        assert!(next.contains("~~do [#alpha]~~"), "{next}");
        assert!(next.contains("~~do [#beta]~~"), "{next}");
        assert!(next.contains("- [x] [#alpha]"), "{next}");
        assert!(next.contains("- [x] [#beta]"), "{next}");
    }

    #[test]
    fn reopen_unstrikes_a_completion_row_and_a_later_check_does_not_redo_it() {
        let before = document(
            "",
            "- ### ~~do [#alpha]~~\n",
            "- [x] [#alpha] finish alpha\n",
            "",
        );
        let after = before.replace("- [x] [#alpha]", "- [ ] [#alpha]");
        let restored = restore_linked_queue_after_todo_reopen(&before, &after)
            .unwrap()
            .unwrap();
        assert!(!restored.contains("~~"), "{restored}");
        assert!(restored.contains("do [#alpha]"), "{restored}");
        assert!(restored.contains("- [ ] [#alpha]"), "{restored}");
        assert!(check_todos_for_struck_queue(&restored).unwrap().is_none());
    }

    #[test]
    fn reopen_does_not_unstrike_an_ack_row_or_a_done_tracked_row() {
        let ack_before = document(
            "",
            "- ### ~~do [#alpha]~~ — auto-struck: acknowledged; Todo #alpha stays open: still open\n",
            "- [x] [#alpha] finish alpha\n",
            "",
        );
        let ack_after = ack_before.replace("- [x] [#alpha]", "- [ ] [#alpha]");
        assert!(
            restore_linked_queue_after_todo_reopen(&ack_before, &ack_after)
                .unwrap()
                .is_none()
        );

        let done_before = document(
            "",
            "- ### ~~do [#alpha]~~\n",
            "- [x] [#alpha] finish alpha\n",
            "\n## Done\n\n<!-- agent:done -->\n- [x] [#alpha] shipped\n<!-- /agent:done -->\n",
        );
        let done_after = done_before.replace("- [x] [#alpha] finish", "- [ ] [#alpha] finish");
        assert!(
            restore_linked_queue_after_todo_reopen(&done_before, &done_after)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_registered_preset_is_not_linked() {
        let content = document(
            "prompt_presets:\n  \"#auth\": \"I authorize you to do this.\"\n",
            "- do [#auth]\n",
            "- [x] [#auth] authorize\n",
            "",
        );
        assert!(strike_queue_for_checked_todos(&content).unwrap().is_none());
    }
}

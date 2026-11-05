//! Whether a queue tree (or one subtree of it) can be deleted, and which row
//! keeps it open (`#answerack`).
//!
//! The queue legend says a prompt tree is safe to delete only when every row
//! in it is struck. A closeout that tells the operator "Safe to delete" from
//! the response text alone (no next actions, no request) can be wrong: the
//! same tree may still hold a live sibling, an operator reply nothing picked
//! up, a live worker attempt, an open operator request, or a struck
//! Waiting-on-you head whose `agent:todo` item is still open. This module is
//! the one computation over the parsed queue tree. The closeout response
//! projection and the consumed-answer acknowledgement both render
//! [`closability_sentence`] from it, so the binary, never the agent's prose,
//! states when a subtree or the whole tree is closable or names the one row
//! that keeps it open.
//!
//! A row is open when it is:
//!
//! - a live queue prompt outside a waiting packet (unstruck work);
//! - a live worker attempt inside a waiting packet;
//! - an operator reply inside a waiting packet that no attempt, response or
//!   acknowledgement has picked up;
//! - any unstruck row that references an open reserved operator request;
//! - a Waiting-on-you head (live, or struck while its todo is still open).
//!
//! Generated rows are never open: response metadata, offered choices, legacy
//! binary follow-ups, the triage's own Next/Roll-up/Subtree rows, and the
//! subtrees of `**Response [#r-…]**`, `**Closed by:**` and `**Consumed by:**`
//! projections (references to a response, which carry no work of their own and
//! would otherwise make a re-run of the closeout disagree with its first run).
//!
//! This module is pure: it reads parsed queue entries and never writes them.

use std::collections::HashSet;

use agent_doc_document::queue_projection::is_reserved_operator_action_id;

use crate::document_queue::QueueEntry;
use crate::waiting_packet::{
    self, WaitingMember, classify_member, entry_list_indent, is_waiting_item_head, subtree_end,
    waiting_owner,
};
use crate::waiting_subtree::{
    attempt_on, entry_body, is_closed_by, is_struck_body, is_triage_output, reply_picked_up,
    root_label,
};

/// Label of the binary-owned row beneath an operator answer that a response
/// consumed (`#answerack`).
pub const CONSUMED_BY_LABEL: &str = "**Consumed by:**";

/// Whether `body` is a `**Consumed by:** [#r-…]` acknowledgement row.
pub fn is_consumed_by(body: &str) -> bool {
    body.trim()
        .strip_prefix(CONSUMED_BY_LABEL)
        .is_some_and(|rest| rest.trim_start().starts_with("[#r-"))
}

/// One row that keeps a tree open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRow {
    /// Entry index of the row.
    pub index: usize,
    /// Short display label of the row.
    pub label: String,
    /// Why it is open, as a clause (`your reply has not been picked up`).
    pub reason: String,
    /// A container row (a waiting head) is named only when nothing more
    /// specific beneath it is open.
    container: bool,
}

/// The closability of the subtree rooted at one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closability {
    pub root: usize,
    /// Every open row, in document order.
    pub open_rows: Vec<OpenRow>,
}

impl Closability {
    /// Whether every row is struck, integrated, or generated.
    pub fn closable(&self) -> bool {
        self.open_rows.is_empty()
    }

    /// The one row to name: the first specific open row, else the first
    /// container.
    pub fn keeper(&self) -> Option<&OpenRow> {
        self.open_rows
            .iter()
            .find(|row| !row.container)
            .or_else(|| self.open_rows.first())
    }

    /// `“label” keeps it open (reason)`, plus how many more rows are open.
    fn keeper_clause(&self) -> Option<String> {
        let keeper = self.keeper()?;
        let mut clause = format!("“{}” keeps it open ({})", keeper.label, keeper.reason);
        let more = self
            .open_rows
            .iter()
            .filter(|row| row.index != keeper.index)
            .count();
        match more {
            0 => {}
            1 => clause.push_str("; 1 more row is open"),
            count => clause.push_str(&format!("; {count} more rows are open")),
        }
        Some(clause)
    }
}

/// Lowercased ids referenced as `[#id]` by `text`.
fn reference_ids(text: &str) -> Vec<String> {
    crate::document_queue::queue_prompt_reference_ids(text)
        .into_iter()
        .map(|id| id.to_ascii_lowercase())
        .collect()
}

/// The ids a waiting head names for itself (its own todo id and grafted
/// request ids), which are never "open requests" inside its own tree.
fn head_own_ids(entries: &[QueueEntry], head: usize) -> HashSet<String> {
    let text = match entries.get(head) {
        Some(QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt)) => prompt.text.as_str(),
        _ => return HashSet::new(),
    };
    reference_ids(agent_doc_element_queue::strip_attempt_projection_prefix(
        text,
    ))
    .into_iter()
    .collect()
}

/// The head's own todo id: its leading `[#id]`.
fn head_todo_id(entries: &[QueueEntry], head: usize) -> Option<String> {
    let text = match entries.get(head)? {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => prompt.text.as_str(),
        _ => return None,
    };
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text).trim();
    let work = work.strip_prefix("~~").unwrap_or(work);
    let work = crate::waiting_status::without_status_token(work);
    work.trim()
        .strip_prefix("[#")
        .and_then(|rest| rest.split_once(']'))
        .map(|(id, _)| id.to_ascii_lowercase())
}

/// A generated response projection whose subtree is a reference, not work.
fn is_response_reference(body: &str) -> bool {
    let body = body.trim();
    is_closed_by(body)
        || is_consumed_by(body)
        || body
            .strip_prefix("- ")
            .unwrap_or(body)
            .strip_prefix("**Response [#r-")
            .is_some()
}

fn attempt_reason(text: &str) -> String {
    match attempt_on(text).and_then(|attempt| attempt.owner) {
        Some(owner) => format!("worker attempt `{owner}` is still live"),
        None => "a worker attempt is still live".to_string(),
    }
}

/// Whether the live prompt at `index` is an inert group parent that the
/// queue's lifecycle settling strikes once its last live leaf closes
/// (`settle_inert_group_parents`): it has children, none of them is a live
/// leaf, and it carries no lifecycle marker or open operator action that would
/// make settling synthesize a follow-up child instead.
fn settles_with_leaves(
    entries: &[QueueEntry],
    index: usize,
    open_todo_ids: &HashSet<String>,
) -> bool {
    let QueueEntry::Prompt(prompt) = &entries[index] else {
        return false;
    };
    if !crate::document_queue::entry_is_group_parent(entries, index)
        || agent_doc_document::queue_projection::lifecycle_marker(&prompt.text).is_some()
        || agent_doc_document::queue_projection::operator_action_ids(&prompt.text)
            .iter()
            .any(|id| open_todo_ids.contains(&id.to_ascii_lowercase()))
    {
        return false;
    }
    !(index + 1..subtree_end(entries, index)).any(|descendant| {
        matches!(entries[descendant], QueueEntry::Prompt(_))
            && !crate::document_queue::entry_is_group_parent(entries, descendant)
    })
}

/// The closability of the subtree rooted at `root`. `open_todo_ids` holds the
/// lowercased ids of every open `agent:todo` item.
pub fn tree_closability(
    entries: &[QueueEntry],
    root: usize,
    open_todo_ids: &HashSet<String>,
) -> Closability {
    let mut out = Closability {
        root,
        open_rows: Vec::new(),
    };
    if root >= entries.len() {
        return out;
    }
    let end = subtree_end(entries, root);
    let push = |out: &mut Closability, index: usize, reason: String, container: bool| {
        if out.open_rows.iter().any(|row| row.index == index) {
            return;
        }
        out.open_rows.push(OpenRow {
            index,
            label: root_label(entries, index),
            reason,
            container,
        });
    };
    let mut index = root;
    while index < end {
        let entry = &entries[index];
        let body = entry_body(entry).unwrap_or_default();
        if index != root && (is_triage_output(entries, index) || is_response_reference(body)) {
            index = subtree_end(entries, index);
            continue;
        }
        let owner = waiting_owner(entries, index);
        let packet_row = owner.is_some_and(|owner| owner != index);
        let own_ids = owner
            .map(|owner| head_own_ids(entries, owner))
            .unwrap_or_default();
        let struck = matches!(entry, QueueEntry::Completed(_)) || is_struck_body(body);
        if is_waiting_item_head(entry) {
            let todo = head_todo_id(entries, index);
            let todo_open = todo.as_ref().is_some_and(|id| open_todo_ids.contains(id));
            match (struck, todo) {
                (true, Some(id)) if todo_open => push(
                    &mut out,
                    index,
                    format!("it is struck, but its todo `#{id}` is still open"),
                    true,
                ),
                (true, _) => {}
                (false, Some(id)) if todo_open => push(
                    &mut out,
                    index,
                    format!("its todo `#{id}` is still open"),
                    true,
                ),
                (false, _) => push(&mut out, index, "it is not struck yet".to_string(), true),
            }
        } else if !struck {
            match entry {
                QueueEntry::Prompt(prompt) if packet_row => match classify_member(prompt) {
                    WaitingMember::Attempt(_) => {
                        push(&mut out, index, attempt_reason(&prompt.text), false)
                    }
                    WaitingMember::Operator => {
                        let reply = agent_doc_element_queue::struck_work_text(&prompt.text).trim();
                        if !waiting_packet::is_agent_doc_reply_entry(entries, index)
                            && !reply.is_empty()
                            && !reply.starts_with("**")
                            && !reply_picked_up(entries, index)
                        {
                            push(
                                &mut out,
                                index,
                                "your reply has not been picked up".to_string(),
                                false,
                            );
                        }
                    }
                    WaitingMember::BinaryFollowUp | WaitingMember::OfferedChoice => {}
                },
                // An inert group parent closes with its last leaf.
                QueueEntry::Prompt(_) if settles_with_leaves(entries, index, open_todo_ids) => {}
                QueueEntry::Prompt(prompt) => {
                    let reason = if attempt_on(&prompt.text).is_some() {
                        attempt_reason(&prompt.text)
                    } else {
                        "it is not struck yet".to_string()
                    };
                    // A parent is named only when no leaf beneath it is open.
                    let container = crate::document_queue::entry_is_group_parent(entries, index);
                    push(&mut out, index, reason, container);
                }
                QueueEntry::Freeform(_)
                    if packet_row
                        && waiting_packet::operator_freeform_text(entries, index).is_some()
                        && !reply_picked_up(entries, index) =>
                {
                    push(
                        &mut out,
                        index,
                        "your reply has not been picked up".to_string(),
                        false,
                    );
                }
                _ => {}
            }
        }
        if !struck {
            for id in reference_ids(body) {
                if is_reserved_operator_action_id(&id)
                    && open_todo_ids.contains(&id)
                    && !own_ids.contains(&id)
                {
                    push(
                        &mut out,
                        index,
                        format!("operator request `#{id}` is still open"),
                        false,
                    );
                    break;
                }
            }
        }
        index += 1;
    }
    out
}

/// The root of the whole tree `index` belongs to: its waiting item when it
/// sits in one (the Waiting-on-you heading is a container, never a tree to
/// delete), else its top-level ancestor.
pub fn whole_tree_root(entries: &[QueueEntry], index: usize) -> usize {
    if let Some(owner) = waiting_owner(entries, index) {
        return owner;
    }
    let mut root = index;
    let Some(mut depth) = entries.get(index).and_then(entry_list_indent) else {
        return index;
    };
    for candidate in (0..index).rev() {
        let Some(indent) = entry_list_indent(&entries[candidate]) else {
            continue;
        };
        if indent < depth {
            root = candidate;
            depth = indent;
            if indent == 0 {
                break;
            }
        }
    }
    root
}

/// The binary-computed deletion guidance for the row at `index`: whether its
/// whole tree, or only its own subtree, can be deleted, or which one row keeps
/// it open.
pub fn closability_sentence(
    entries: &[QueueEntry],
    index: usize,
    open_todo_ids: &HashSet<String>,
) -> String {
    let root = whole_tree_root(entries, index);
    let whole = tree_closability(entries, root, open_todo_ids);
    if whole.closable() {
        return "You can delete this whole tree: every row in it is struck or integrated."
            .to_string();
    }
    let whole_clause = whole.keeper_clause().unwrap_or_default();
    if root != index {
        let own = tree_closability(entries, index, open_todo_ids);
        if own.closable() {
            return format!("You can delete this subtree, but keep the tree: {whole_clause}.");
        }
        if let Some(own_clause) = own.keeper_clause() {
            return format!("Keep this subtree: {own_clause}.");
        }
    }
    format!("Keep this tree: {whole_clause}.")
}

/// [`closability_sentence`] for the queue row that starts at byte `start` of a
/// whole document, with the document's open todos. `None` when no queue row
/// starts there.
pub fn closability_sentence_at(document: &str, start: usize) -> Option<String> {
    let queue = agent_doc_element::element::parse(document)
        .ok()?
        .into_iter()
        .filter(|component| component.name == "queue")
        .find(|component| component.open_end <= start && start < component.close_start)?;
    let local = start - queue.open_end;
    let spans = crate::document_queue::parse_spans(queue.content(document)).ok()?;
    let index = spans
        .iter()
        .position(|(_, span)| span.start <= local && local < span.end.max(span.start + 1))?;
    let entries = spans
        .into_iter()
        .map(|(entry, _)| entry)
        .collect::<Vec<_>>();
    Some(closability_sentence(
        &entries,
        index,
        &crate::answer_ack::open_todo_ids(document),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_queue;

    fn open(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    fn index_of(entries: &[QueueEntry], needle: &str) -> usize {
        entries
            .iter()
            .position(|entry| match entry {
                QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
                    prompt.text.contains(needle)
                }
                QueueEntry::Freeform(line) => line.contains(needle),
                _ => false,
            })
            .unwrap_or_else(|| panic!("no row contains {needle}: {entries:#?}"))
    }

    /// The `tdclireleasedownloads` shape from `sdk-dev.md`: a struck waiting
    /// copy holding the operator's answer, whose todo is still open.
    const STRUCK_COPY: &str = concat!(
        "- ### 📥 Waiting on you — answer by adding a child line under an item [#q-4775b8697412]\n",
        "  - ~~[#dl] Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-item -->~~\n",
        "    - **Response:** Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-metadata -->\n",
        "    - **Next actions:** <!-- agent-doc:waiting-metadata -->\n",
        "        - **Action:** Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-metadata -->\n",
        "          - haiven-cli will be public + open source [#q-f09f08bcf721]\n",
    );

    #[test]
    fn an_unpicked_answer_keeps_a_struck_copy_open_and_is_named() {
        let entries = document_queue::parse(STRUCK_COPY).unwrap();
        let answer = index_of(&entries, "q-f09f08bcf721");
        let head = index_of(&entries, "[#dl]");
        let status = tree_closability(&entries, head, &open(&[]));
        assert!(!status.closable());
        let keeper = status.keeper().unwrap();
        assert_eq!(keeper.index, answer);
        assert_eq!(keeper.reason, "your reply has not been picked up");
        assert_eq!(whole_tree_root(&entries, answer), head);
    }

    #[test]
    fn a_consumed_answer_under_a_struck_copy_with_an_open_todo_names_the_head() {
        let consumed = STRUCK_COPY.replace(
            "          - haiven-cli will be public + open source [#q-f09f08bcf721]\n",
            concat!(
                "          - ~~haiven-cli will be public + open source [#q-f09f08bcf721]~~\n",
                "            - **Consumed by:** [#r-0123456789ab]\n",
            ),
        );
        let entries = document_queue::parse(&consumed).unwrap();
        let answer = index_of(&entries, "q-f09f08bcf721");
        let sentence = closability_sentence(&entries, answer, &open(&["dl"]));
        assert_eq!(
            sentence,
            "You can delete this subtree, but keep the tree: “`#dl` Decide how private haiven-cli serves release downloads” keeps it open (it is struck, but its todo `#dl` is still open)."
        );
        // Once the todo closes, the whole copy is deletable.
        let sentence = closability_sentence(&entries, answer, &open(&[]));
        assert_eq!(
            sentence,
            "You can delete this whole tree: every row in it is struck or integrated."
        );
    }

    #[test]
    fn a_struck_row_with_a_live_sibling_names_the_sibling() {
        let entries = document_queue::parse(concat!(
            "- Release train\n",
            "  - ~~Publish the release.~~\n",
            "  - Announce the release.\n",
        ))
        .unwrap();
        let publish = index_of(&entries, "Publish");
        let sentence = closability_sentence(&entries, publish, &open(&[]));
        assert!(
            sentence
                == "You can delete this subtree, but keep the tree: “Announce the release.” keeps it open (it is not struck yet); 1 more row is open.",
            "{sentence}"
        );
        let closed = document_queue::parse(concat!(
            "- ~~Release train~~\n",
            "  - ~~Publish the release.~~\n",
        ))
        .unwrap();
        assert_eq!(
            closability_sentence(&closed, 1, &open(&[])),
            "You can delete this whole tree: every row in it is struck or integrated."
        );
    }

    #[test]
    fn an_open_request_and_a_live_attempt_keep_the_tree_open() {
        let entries = document_queue::parse(concat!(
            "- [#rel] Release App access <!-- agent-doc:waiting-item -->\n",
            "  - ~~tmstorm [#q-569d77146187]~~\n",
            "  - ☑️ [#subagent:c252releaseapp#1] do [#q-c4dbe51b8e36]\n",
            "  - **Request**: [#subagent-operator-request-pick]\n",
        ))
        .unwrap();
        let status = tree_closability(&entries, 0, &open(&["subagent-operator-request-pick"]));
        let reasons = status
            .open_rows
            .iter()
            .map(|row| row.reason.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            reasons,
            vec![
                "it is not struck yet",
                "worker attempt `subagent:c252releaseapp#1` is still live",
                "operator request `#subagent-operator-request-pick` is still open",
            ]
        );
        // The specific attempt is named before the container head.
        assert_eq!(status.keeper().unwrap().index, 2);
    }

    #[test]
    fn response_projections_never_keep_a_tree_open() {
        let entries = document_queue::parse(concat!(
            "- ~~do [#alpha]~~\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Choices:**\n",
            "      - **Request**: [#subagent-operator-request-pick]\n",
        ))
        .unwrap();
        assert!(
            tree_closability(&entries, 0, &open(&["subagent-operator-request-pick"])).closable()
        );
    }
}

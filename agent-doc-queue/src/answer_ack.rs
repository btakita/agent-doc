//! Visible acknowledgement of an operator answer a response consumed
//! (`#answerack`).
//!
//! An operator answers a waiting item by typing a row inside its packet
//! (`haiven-cli will be public + open source [#q-f09f08bcf721]`). A closeout
//! that used that answer (it cites the answer's id, or it terminalized the row
//! the answer sits under) used to leave the row exactly as typed, so the
//! operator could not tell whether it had been read, and the tree it sat in
//! never looked deletable.
//!
//! The acknowledgement is the record the answer ledger already reads: a struck
//! operator row is a consumed answer (`agent-doc-answer-ledger`,
//! [`extract`](../../agent_doc_answer_ledger/extract/index.html)). This module
//! strikes the row (without the `auto-struck` note, which the ledger would
//! discard) and adds one binary-owned child naming the response that consumed
//! it plus the binary-computed closability of its tree:
//!
//! ```text
//! - ~~haiven-cli will be public + open source [#q-f09f08bcf721]~~
//!   - **Consumed by:** [#r-…]. You can delete this subtree, but keep the tree: …
//! ```
//!
//! Only live operator rows inside a Waiting-on-you packet are eligible; a top
//! level queue row is consumed by ordinary queue consumption. A row carrying a
//! reserved operator-request id is left alone: it is the request's own answer
//! surface, which the request projection owns.
//!
//! This module is pure: it maps a document string to a document string.

use std::collections::{BTreeMap, HashSet};

use agent_doc_document::queue_projection::is_reserved_operator_action_id;
use agent_doc_element::element;
use anyhow::Result;

use crate::document_queue::{self, QueueEntry};
use crate::tree_closability::{CONSUMED_BY_LABEL, closability_sentence};
use crate::waiting_packet::{self, WaitingMember, classify_member, is_packet_row, subtree_end};
use crate::waiting_subtree::reply_picked_up;

/// One live operator answer row: the `nth` queue component of the document
/// and the entry index inside its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnswerRow {
    pub component: usize,
    pub entry: usize,
}

/// A live operator answer inside a waiting packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAnswer {
    pub row: AnswerRow,
    /// Stable `q-…` ids the row carries (its identity for citation).
    pub ids: Vec<String>,
    /// Whether an attempt, response, or generated row already sits beneath it.
    pub picked_up: bool,
}

/// The operator text of a live packet row, when it is an answer.
fn live_answer_text(entries: &[QueueEntry], index: usize) -> Option<&str> {
    if !is_packet_row(entries, index) {
        return None;
    }
    let text = match &entries[index] {
        QueueEntry::Prompt(prompt) => {
            if classify_member(prompt) != WaitingMember::Operator
                || waiting_packet::is_agent_doc_reply_entry(entries, index)
            {
                return None;
            }
            agent_doc_element_queue::struck_work_text(&prompt.text).trim()
        }
        QueueEntry::Freeform(_) => waiting_packet::operator_freeform_text(entries, index)?,
        _ => return None,
    };
    let first = text.lines().next().unwrap_or_default().trim();
    (!first.is_empty() && !first.starts_with("**") && !first.contains("~~")).then_some(first)
}

/// Every live operator answer row in every waiting packet of `document`.
pub fn live_packet_answers(document: &str) -> Result<Vec<LiveAnswer>> {
    let mut out = Vec::new();
    for (component, queue) in element::parse(document)?
        .into_iter()
        .filter(|component| component.name == "queue")
        .enumerate()
    {
        let entries = document_queue::parse(queue.content(document))?;
        for index in 0..entries.len() {
            let Some(text) = live_answer_text(&entries, index) else {
                continue;
            };
            let ids = document_queue::queue_prompt_reference_ids(text);
            if ids.iter().any(|id| is_reserved_operator_action_id(id)) {
                continue;
            }
            out.push(LiveAnswer {
                row: AnswerRow {
                    component,
                    entry: index,
                },
                ids: ids.into_iter().filter(|id| id.starts_with("q-")).collect(),
                picked_up: reply_picked_up(&entries, index),
            });
        }
    }
    Ok(out)
}

/// Whether `body` cites `id` (`[#id]`, `` `#id` ``, or `#id`).
pub fn cites_id(body: &str, id: &str) -> bool {
    document_queue::queue_prompt_reference_ids(body)
        .iter()
        .any(|cited| cited.eq_ignore_ascii_case(id))
}

/// The live answer rows inside the subtree that starts at byte `start` of the
/// document, when that byte begins a row of a queue component.
pub fn answers_under_row(
    document: &str,
    start: usize,
    answers: &[LiveAnswer],
) -> Result<Vec<AnswerRow>> {
    let mut out = Vec::new();
    for (component, queue) in element::parse(document)?
        .into_iter()
        .filter(|component| component.name == "queue")
        .enumerate()
    {
        if start < queue.open_end || start >= queue.close_start {
            continue;
        }
        let local = start - queue.open_end;
        let spans = document_queue::parse_spans(queue.content(document))?;
        let Some(root) = spans
            .iter()
            .position(|(_, span)| span.start <= local && local < span.end.max(span.start + 1))
        else {
            continue;
        };
        let entries = spans
            .into_iter()
            .map(|(entry, _)| entry)
            .collect::<Vec<_>>();
        let end = subtree_end(&entries, root);
        out.extend(
            answers
                .iter()
                .filter(|answer| answer.row.component == component)
                .filter(|answer| answer.row.entry > root && answer.row.entry < end)
                .map(|answer| answer.row),
        );
    }
    Ok(out)
}

/// Lowercased ids of every open `agent:todo` item.
pub fn open_todo_ids(document: &str) -> HashSet<String> {
    let Ok(components) = element::parse(document) else {
        return HashSet::new();
    };
    let mut ids = HashSet::new();
    for component in components
        .iter()
        .filter(|component| component.name == "todo")
    {
        let (_, items, _) =
            agent_doc_element_backlog::backlog::parse_items(component.content(document));
        for item in items {
            if !item.is_done() && !item.id.is_empty() {
                ids.insert(item.id.to_ascii_lowercase());
            }
        }
    }
    ids
}

/// Strike the first line of one list row in place: `- text` -> `- ~~text~~`.
fn strike_first_line(line: &str) -> Option<String> {
    let (content, newline) = match line.strip_suffix('\n') {
        Some(content) => (content, "\n"),
        None => (line, ""),
    };
    let trimmed = content.trim_start_matches([' ', '\t']);
    let indent = &content[..content.len() - trimmed.len()];
    let marker_len = ["- ", "* ", "+ "]
        .iter()
        .find(|marker| trimmed.starts_with(**marker))
        .map(|marker| marker.len())?;
    let (marker, rest) = trimmed.split_at(marker_len);
    let rest = rest.trim_end();
    if rest.is_empty() || rest.contains("~~") {
        return None;
    }
    Some(format!("{indent}{marker}~~{rest}~~{newline}"))
}

/// Strike each acknowledged answer row and add its `**Consumed by:**` child,
/// naming `response_id` and the closability of the tree the row sits in.
/// Rows that are no longer live answers are skipped, so applying the same
/// acknowledgements twice is a no-op.
pub fn apply_answer_acknowledgements(
    document: &str,
    acks: &[(AnswerRow, String)],
) -> Result<String> {
    let mut by_component = BTreeMap::<usize, BTreeMap<usize, String>>::new();
    for (row, response_id) in acks {
        by_component
            .entry(row.component)
            .or_default()
            .entry(row.entry)
            .or_insert_with(|| response_id.clone());
    }
    if by_component.is_empty() {
        return Ok(document.to_string());
    }
    let open_ids = open_todo_ids(document);
    let mut edits = Vec::new();
    for (component, queue) in element::parse(document)?
        .into_iter()
        .filter(|component| component.name == "queue")
        .enumerate()
    {
        let Some(rows) = by_component.get(&component) else {
            continue;
        };
        let body = queue.content(document);
        let spans = document_queue::parse_spans(body)?;
        let entries = spans
            .iter()
            .map(|(entry, _)| entry.clone())
            .collect::<Vec<_>>();
        // Pass 1: strike each row and insert its child without guidance.
        let mut struck_body = body.to_string();
        let mut inserted = Vec::new();
        for (&entry, response_id) in rows.iter().rev() {
            if live_answer_text(&entries, entry).is_none() {
                continue;
            }
            let span = spans[entry].1.clone();
            let line_end = body[span.start..]
                .find('\n')
                .map_or(body.len(), |offset| span.start + offset + 1);
            let line = &body[span.start..line_end];
            let Some(struck) = strike_first_line(line) else {
                continue;
            };
            let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            let mut replacement = struck;
            if !replacement.ends_with('\n') {
                replacement.push('\n');
            }
            replacement.push_str(&format!(
                "{}- {CONSUMED_BY_LABEL} {response_id}\n",
                " ".repeat(indent + 2)
            ));
            if line_end == body.len() && !line.ends_with('\n') {
                replacement.pop();
            }
            struck_body.replace_range(span.start..line_end, &replacement);
            inserted.push(entry);
        }
        if inserted.is_empty() {
            continue;
        }
        // Pass 2: compute each tree's closability on the acknowledged body
        // and append it to the child row.
        inserted.sort_unstable();
        let acked_spans = document_queue::parse_spans(&struck_body)?;
        let acked_entries = acked_spans
            .iter()
            .map(|(entry, _)| entry.clone())
            .collect::<Vec<_>>();
        let mut final_body = struck_body.clone();
        for (offset, entry) in inserted.iter().enumerate().rev() {
            let answer = entry + offset;
            let child = answer + 1;
            let Some((_, child_span)) = acked_spans.get(child) else {
                continue;
            };
            let child_line_end = struck_body[child_span.start..]
                .find('\n')
                .map_or(struck_body.len(), |at| child_span.start + at);
            let child_line = &struck_body[child_span.start..child_line_end];
            if !child_line
                .trim_start()
                .starts_with(&format!("- {CONSUMED_BY_LABEL}"))
            {
                continue;
            }
            let sentence = closability_sentence(&acked_entries, answer, &open_ids);
            final_body.insert_str(child_line_end, &format!(". {sentence}"));
        }
        edits.push((queue.open_end, queue.close_start, final_body));
    }
    let mut out = document.to_string();
    for (start, end, body) in edits.into_iter().rev() {
        out.replace_range(start..end, &body);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = concat!(
        "## Todo <!-- agent:todo -->\n",
        "- [ ] [#dl] Decide how private haiven-cli serves release downloads\n",
        "<!-- /agent:todo -->\n",
        "\n",
        "## Main <!-- agent:queue id=\"main\" -->\n",
        "- ### 📥 Waiting on you — answer by adding a child line under an item [#q-4775b8697412]\n",
        "  - [#dl] Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-item -->\n",
        "    - **Response:** Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-metadata -->\n",
        "    - **Next actions:** <!-- agent-doc:waiting-metadata -->\n",
        "        - **Action:** Decide how private haiven-cli serves release downloads <!-- agent-doc:waiting-metadata -->\n",
        "          - haiven-cli will be public + open source [#q-f09f08bcf721]\n",
        "          - mark [#dl] as done unless there's more left to do\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn only_packet_operator_rows_are_live_answers() {
        let answers = live_packet_answers(DOC).unwrap();
        assert_eq!(answers.len(), 2, "{answers:#?}");
        assert_eq!(answers[0].ids, vec!["q-f09f08bcf721".to_string()]);
        assert!(answers[1].ids.is_empty());
        assert!(answers.iter().all(|answer| !answer.picked_up));
    }

    #[test]
    fn a_cited_answer_is_struck_and_names_its_response_and_tree() {
        let answers = live_packet_answers(DOC).unwrap();
        assert!(cites_id(
            "**Deferred:** The operator reply is public (`[#q-f09f08bcf721]`).",
            "q-f09f08bcf721"
        ));
        assert!(!cites_id("see [#q-f09f08bcf7210]", "q-f09f08bcf721"));
        let acked = apply_answer_acknowledgements(
            DOC,
            &[(answers[0].row, "[#r-0123456789ab]".to_string())],
        )
        .unwrap();
        assert!(
            acked.contains(concat!(
                "          - ~~haiven-cli will be public + open source [#q-f09f08bcf721]~~\n",
                "            - **Consumed by:** [#r-0123456789ab]. You can delete this subtree, but keep the tree: “mark `#dl` as done unless there's more left to do” keeps it open (your reply has not been picked up); 1 more row is open.\n",
                "          - mark [#dl] as done unless there's more left to do\n",
            )),
            "{acked}"
        );
        // The acknowledgement is idempotent: the struck row is no longer live.
        let again = apply_answer_acknowledgements(
            &acked,
            &[(answers[0].row, "[#r-0123456789ab]".to_string())],
        )
        .unwrap();
        assert_eq!(again, acked);
        // The answer ledger reads the struck row as a consumed answer.
        let remaining = live_packet_answers(&acked).unwrap();
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn acknowledging_every_answer_names_the_open_todo_head() {
        let answers = live_packet_answers(DOC).unwrap();
        let acks = answers
            .iter()
            .map(|answer| (answer.row, "[#r-0123456789ab]".to_string()))
            .collect::<Vec<_>>();
        let acked = apply_answer_acknowledgements(DOC, &acks).unwrap();
        assert_eq!(
            acked
                .matches("- **Consumed by:** [#r-0123456789ab]. You can delete this subtree, but keep the tree: “`#dl` Decide how private haiven-cli serves release downloads” keeps it open (its todo `#dl` is still open).")
                .count(),
            2,
            "{acked}"
        );
        let closed = acked.replace("- [ ] [#dl]", "- [x] [#dl]");
        let entries = document_queue::parse(
            element::parse(&closed)
                .unwrap()
                .into_iter()
                .find(|component| component.name == "queue")
                .unwrap()
                .content(&closed),
        )
        .unwrap();
        // With the todo closed, only the live head row itself keeps the tree.
        let sentence = closability_sentence(&entries, 1, &open_todo_ids(&closed));
        assert_eq!(
            sentence,
            "Keep this tree: “`#dl` Decide how private haiven-cli serves release downloads” keeps it open (it is not struck yet)."
        );
    }
}

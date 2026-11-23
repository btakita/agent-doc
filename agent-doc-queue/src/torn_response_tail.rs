//! `#tornsplice`: drop what a torn response-tree splice left behind.
//!
//! Observed in a session document on 2026-11-23 (anonymized in the tests):
//! one Remaining re-render computed two edits against the same text, a
//! nested answer response's new Remaining/State/Guidance and the enclosing
//! response's whole Remaining list, which contains it. The nested edits
//! landed first and moved the text; the enclosing replacement was then
//! spliced at its old offsets, so it ended short and left the tail of the
//! nested response's re-rendered State line behind
//! (`ing on operator input.`, the `Wait` cut off) plus a copy of its
//! Guidance line, both under the newly listed Remaining entry. The column-0
//! fragment read as more text of the entry above it, and the Guidance line
//! as the entry's answer, so every later pass kept them, and the operator's
//! answer to the entry ended up nested under the stray Guidance.
//! [`crate::response_remaining::reconcile_response_remaining`] no longer
//! splices overlapping edits; [`drop_torn_response_tails`] repairs documents
//! a torn splice already wrote:
//!
//! - **Torn tails** (`torn_response_tail`). A non-list line directly after a
//!   generated response-detail row (`… <!-- agent-doc:response-detail -->`)
//!   or a generated `**Label:**` row, whose text is a strict tail of a
//!   generated `- **State:** …` line (any state the binary renders) or of a
//!   generated `- **Guidance:** …` line in the same queue, is dropped.
//! - **Duplicate entry guidance** (`duplicate_entry_guidance`). A generated
//!   `- **Guidance:** …` line that is the first child of a Remaining entry
//!   (a response-detail row, never response structure), and whose value
//!   another Guidance line in the same queue already carries, is dropped;
//!   so is a response tally (`🙋 1/2 done, …`) as the first child of an
//!   entry in a `**Remaining:**` list, even after the response it was
//!   copied from re-rendered its own (`#answeredentry`).
//!   Rows nested under it move up by the same amount to be the entry's
//!   answers, their text unchanged.
//!
//! Operator rows are never dropped or rewritten: only a non-list fragment of
//! generated text and a duplicated generated line go, and only leading
//! spaces of the rows under that line change. Fenced code is left alone. A
//! second run changes nothing.

use agent_doc_markdown_ast::overlay::{
    RESPONSE_DETAIL_MARKER, is_generated_response_structure_row,
};

use crate::queue_consume::{
    RESPONSE_STATE_COMPLETE, RESPONSE_STATE_FOLLOW_UP, RESPONSE_STATE_UNPUBLISHED,
    RESPONSE_STATE_WAITING_ON_OPERATOR,
};
use crate::self_repair::{SelfRepair, SelfRepairKind, edit_queue_bodies};

const STATE_LEAD: &str = "- **State:** ";
const GUIDANCE_LEAD: &str = "- **Guidance:** ";

/// The States a binary pass renders.
const GENERATED_STATES: [&str; 4] = [
    RESPONSE_STATE_COMPLETE,
    RESPONSE_STATE_FOLLOW_UP,
    RESPONSE_STATE_WAITING_ON_OPERATOR,
    RESPONSE_STATE_UNPUBLISHED,
];

/// The fewest letters or digits a torn tail must carry: a lone `.` is not
/// proof of a tear.
const MIN_TAIL_ALNUM: usize = 3;

struct Line<'a> {
    start: usize,
    end: usize,
    indent: usize,
    /// Trimmed text (no indent, no trailing whitespace).
    text: &'a str,
    fenced: bool,
}

fn lines_of(body: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut cursor = 0;
    let mut fenced = false;
    for raw in body.split_inclusive('\n') {
        let start = cursor;
        cursor += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        let text = line.trim_start_matches(' ');
        let fence = text.starts_with("```") || text.starts_with("~~~");
        out.push(Line {
            start,
            end: cursor,
            indent: line.len() - text.len(),
            text: text.trim_end(),
            fenced: fenced || fence || text.starts_with('\t'),
        });
        if fence {
            fenced = !fenced;
        }
    }
    out
}

fn is_list_row(text: &str) -> bool {
    text.starts_with("- ") || text.starts_with("* ") || text.starts_with("+ ") || text == "-"
}

/// A generated row a torn tail can follow: a response-detail row, or a
/// `**Label:**` response-structure row.
fn is_generated_row(text: &str) -> bool {
    let Some(item) = text.strip_prefix("- ") else {
        return false;
    };
    item.ends_with(RESPONSE_DETAIL_MARKER) || is_generated_response_structure_row(item)
}

/// A Remaining entry: a response-detail row that is not response structure.
fn is_remaining_entry(text: &str) -> bool {
    text.strip_prefix("- ").is_some_and(|item| {
        item.ends_with(RESPONSE_DETAIL_MARKER) && !item.trim_start().starts_with("**")
    })
}

/// Whether the row at `row` is an entry of a `**Remaining:**` list: its
/// parent line is the list's label.
fn in_remaining_list(lines: &[Line<'_>], row: usize) -> bool {
    let indent = lines[row].indent;
    lines[..row]
        .iter()
        .rev()
        .find(|line| !line.text.is_empty() && !line.fenced && line.indent < indent)
        .is_some_and(|parent| {
            parent.text == format!("- {}", crate::response_remaining::REMAINING_LABEL)
        })
}

fn generated_guidance(text: &str) -> Option<&str> {
    text.strip_prefix(GUIDANCE_LEAD)
        .filter(|value| crate::response_remaining::is_generated_guidance(value))
}

/// Drop torn State/Guidance tails and the Guidance line a torn splice
/// duplicated under a Remaining entry, in every queue component.
pub fn drop_torn_response_tails(document: &str) -> (String, Vec<SelfRepair>) {
    if !document.contains("- **State:** ") && !document.contains(GUIDANCE_LEAD) {
        return (document.to_string(), Vec::new());
    }
    edit_queue_bodies(document, repair_body)
}

fn repair_body(body: &str, repairs: &mut Vec<SelfRepair>) -> String {
    let lines = lines_of(body);
    // Every whole generated line a tail can be cut from.
    let mut whole: Vec<String> = GENERATED_STATES
        .iter()
        .map(|state| format!("{STATE_LEAD}{state}"))
        .collect();
    let guidance_values: Vec<&str> = lines
        .iter()
        .filter(|line| !line.fenced)
        .filter_map(|line| generated_guidance(line.text))
        .collect();
    whole.extend(
        guidance_values
            .iter()
            .map(|value| format!("{GUIDANCE_LEAD}{value}")),
    );
    let is_torn_tail = |text: &str| {
        !text.is_empty()
            && !is_list_row(text)
            && text.chars().filter(char::is_ascii_alphanumeric).count() >= MIN_TAIL_ALNUM
            && whole
                .iter()
                .any(|line| line.len() > text.len() && line.ends_with(text))
    };

    let mut drop = vec![false; lines.len()];
    let mut shift = vec![0usize; lines.len()];
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        if line.fenced || line.text.is_empty() || !is_generated_row(line.text) {
            index += 1;
            continue;
        }
        let row = index;
        let mut next = index + 1;
        while next < lines.len() && !lines[next].fenced && is_torn_tail(lines[next].text) {
            drop[next] = true;
            repairs.push(SelfRepair {
                kind: SelfRepairKind::TornResponseTail,
                detail: lines[next].text.to_string(),
                rewritten: None,
            });
            next += 1;
        }
        // A generated Guidance line as the entry's first child, carrying a
        // value another Guidance line in the queue already has, or a
        // response tally under an entry of a Remaining list: a tally belongs
        // to a response, never to an entry, so it is a copy even once the
        // response it was copied from re-rendered its own.
        if is_remaining_entry(lines[row].text)
            && let Some(guidance) = lines.get(next)
            && !guidance.fenced
            && guidance.indent > lines[row].indent
            && let Some(value) = generated_guidance(guidance.text)
            && (guidance_values
                .iter()
                .filter(|other| **other == value)
                .count()
                > 1
                || (crate::response_remaining::is_tally_guidance(value)
                    && in_remaining_list(&lines, row)))
        {
            drop[next] = true;
            repairs.push(SelfRepair {
                kind: SelfRepairKind::DuplicateEntryGuidance,
                detail: guidance.text.to_string(),
                rewritten: None,
            });
            // Its rows become the entry's answers, one level below it.
            let by = guidance.indent - lines[row].indent;
            let mut child = next + 1;
            while child < lines.len()
                && (lines[child].text.is_empty() || lines[child].indent > guidance.indent)
            {
                if !lines[child].text.is_empty() {
                    shift[child] = by;
                }
                child += 1;
            }
            next = child;
        }
        index = next.max(row + 1);
    }
    if !drop.iter().any(|dropped| *dropped) {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    for (at, line) in lines.iter().enumerate() {
        if drop[at] {
            continue;
        }
        out.push_str(&body[line.start + shift[at]..line.end]);
    }
    out
}

#[cfg(test)]
#[path = "torn_response_tail_tests.rs"]
mod tests;

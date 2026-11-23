//! `#followupmovedstate`: a struck row whose action items became their own
//! follow-up queue rows says so in its response structure.
//!
//! `queue report-complete` turns each **Action items** entry of a worker
//! hand-back into a top-level follow-up row placed right after the
//! originating head (`#workerresponsestructure`). The coordinator then strikes
//! that head, and the strike copies the hand-back onto it inside its
//! `**Response [#r-…]**` child (`#handbackrowstructure`). That child used to
//! say `**State:** Follow-up work remains.` and `**Guidance:** More work
//! remains. Keep this tree and follow the action items in this response.`
//! whenever the hand-back listed action items, so a closed (struck) row's own
//! response told the operator to keep it open and work from it, while the
//! work had already moved to the follow-up rows beside it, some of them
//! already done. `queue tree-status` read the same State and left the struck
//! row in `needs agent review` forever (`#treestatusquery`).
//!
//! Each action item now has exactly one place that tracks it. When every
//! action item of the response has its follow-up row in the document, by the
//! same hold check report-complete uses to avoid a duplicate row
//! ([`crate::handback_followups`]): live or struck in a queue, or quoted as
//! the queue prompt of a dispatch or completion cell in `agent:exchange`
//! after the operator deleted the finished row from the queue, the struck row's
//! response says `**State:** Complete.` and its Guidance says the action
//! items moved to their own follow-up rows, followed by the usual deletion
//! guidance for the row ([`crate::queue_consume`]). While any action item has
//! no row (placement was refused and the printed `queue follow-up` command has
//! not run yet, or the item carries a component marker a live row must not
//! hold), the response keeps `Follow-up work remains.`, because this tree is
//! then the only place that item is written down.
//!
//! The strike renders that state directly. [`reconcile_moved_followup_states`]
//! updates trees written before this rule, and trees whose follow-up rows were
//! placed after the strike; preflight queue maintenance runs it every cycle.
//! It rewrites only the State and Guidance values of a struck row's own
//! response child, only when they are exactly the follow-up form above, so it
//! is idempotent and never touches operator text.

use agent_doc_markdown_ast::overlay::{ItemSurface, RESPONSE_DETAIL_MARKER};

use crate::followup_owner::mentions_followup_row;
use crate::handback_followups::followup_row_held;
use crate::queue_consume::{
    RESPONSE_STATE_COMPLETE, RESPONSE_STATE_FOLLOW_UP, completed_row_deletion_guidance,
};

/// The Guidance a struck row's response carries while its action items are
/// still written down only in that response.
pub const FOLLOW_UP_ACTION_ITEMS_GUIDANCE: &str =
    "More work remains. Keep this tree and follow the action items in this response.";

/// The first sentence of the Guidance once every action item had its own
/// follow-up row, before `#treefinish` put the keep/delete indicator first.
/// Still recognized as generated guidance.
pub const FOLLOW_UP_MOVED_GUIDANCE_LEAD: &str =
    "Its action items moved to their own follow-up queue rows after this tree.";

/// The note after the keep/delete indicator once every action item has its
/// own follow-up row (`#treefinish`).
pub const FOLLOW_UP_MOVED_GUIDANCE_NOTE: &str = "Its action items moved to follow-up rows.";

/// Guidance for a struck row whose action items all have follow-up rows: the
/// row's keep/delete indicator, then [`FOLLOW_UP_MOVED_GUIDANCE_NOTE`].
pub fn moved_followup_guidance(closure: Option<&crate::queue_tree::RowClosure>) -> String {
    format!(
        "{} {FOLLOW_UP_MOVED_GUIDANCE_NOTE}",
        completed_row_deletion_guidance(closure)
    )
}

/// True when every one of `action_items` already has its follow-up row
/// (`[#<origin>] follow-up: <item>`, or the legacy `follow up on an action
/// item from <origin>: <item>`) in `document`: live
/// or struck in a queue, or quoted as a queue prompt in `agent:exchange`.
/// `origin_row` is the originating row's text.
///
/// The check is report-complete's own hold check, so it is true exactly when
/// a repeated report would add no row. An empty list is never "moved", and an
/// item carrying an HTML comment or component marker never has a live row.
pub fn action_items_held_as_rows(
    document: &str,
    origin_row: &str,
    action_items: &[String],
) -> bool {
    if action_items.is_empty() {
        return false;
    }
    let rows = followup_queue_rows(document);
    if rows.is_empty() {
        return false;
    }
    let origins = origin_candidates(origin_row);
    action_items.iter().all(|item| {
        !(item.contains("<!--") || item.contains("-->") || item.contains("&lt;!--"))
            && origins.iter().any(|origin| {
                rows.iter()
                    .any(|line| followup_row_held(line, origin, item))
            })
    })
}

/// The texts a follow-up row may have quoted for `origin_row`: the row as
/// parsed, and the same without its heading and its trailing report-state
/// annotation (`⟦published⟧`), which a strike can add after the report.
fn origin_candidates(origin_row: &str) -> Vec<String> {
    let text = origin_row.trim();
    let mut out = vec![text.to_string()];
    let bare = text.strip_prefix("### ").unwrap_or(text).trim();
    for candidate in [
        bare,
        agent_doc_element_queue::strip_queue_report_state_annotation(bare).trim(),
    ] {
        if !candidate.is_empty() && !out.iter().any(|have| have == candidate) {
            out.push(candidate.to_string());
        }
    }
    out
}

/// Whitespace-collapsed follow-up row texts the document holds: every queue
/// line that is a follow-up row, live or struck, and every follow-up row a
/// `> ` queue-prompt quote in `agent:exchange` records. A completed follow-up
/// row the operator deleted from the queue is still a row its action item
/// moved to; its dispatch and completion cells quote it there. Generated
/// response details and quotes inside the queue are not rows.
fn followup_queue_rows(document: &str) -> std::sync::Arc<Vec<String>> {
    crate::derived_memo::followup_rows(document, followup_queue_rows_uncached)
}

fn followup_queue_rows_uncached(document: &str) -> Vec<String> {
    let Ok(components) = agent_doc_element::element::parse(document) else {
        return Vec::new();
    };
    let collapse = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut rows = Vec::new();
    for component in &components {
        let queue = component.name == "queue";
        if !queue && component.name != "exchange" {
            continue;
        }
        for line in component.content(document).lines() {
            let trimmed = line.trim();
            let row = if queue {
                if trimmed.starts_with('>') || trimmed.ends_with(RESPONSE_DETAIL_MARKER) {
                    continue;
                }
                trimmed
            } else {
                let Some(quoted) = trimmed.strip_prefix('>') else {
                    continue;
                };
                let quoted = quoted.trim_start();
                quoted
                    .strip_prefix("**Queue prompt:**")
                    .unwrap_or(quoted)
                    .trim_start()
            };
            if !queue && !is_quoted_followup_row(row) {
                continue;
            }
            if mentions_followup_row(row) {
                rows.push(collapse(row));
            }
        }
    }
    rows
}

/// Whether an `agent:exchange` queue-prompt quote is a follow-up row in
/// either form (`#followupshorttitle`).
fn is_quoted_followup_row(quoted: &str) -> bool {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(quoted);
    crate::followup_owner::followup_row_action_item(work).is_some()
}

/// The follow-up form of a struck row's own response child.
struct FollowUpCell {
    /// Byte range of the State value (`Follow-up work remains.`).
    state_value: std::ops::Range<usize>,
    /// Byte range of the Guidance value.
    guidance_value: std::ops::Range<usize>,
    /// The rendered Action items entries, whitespace collapsed.
    action_items: Vec<String>,
}

/// One document line: its start offset, its indent, and its trimmed text.
struct Line<'a> {
    start: usize,
    indent: usize,
    text: &'a str,
}

/// The lines of `document` from byte `from`, lazily: a reader that stops at
/// the end of a row's scope never splits the rest of the document
/// (`#incrementalreconcile`).
fn lines_from(document: &str, from: usize) -> impl Iterator<Item = Line<'_>> {
    let mut cursor = from;
    document[from..].split_inclusive('\n').map(move |raw| {
        let start = cursor;
        cursor += raw.len();
        let body = raw.trim_end_matches(['\n', '\r']);
        let text = body.trim_start_matches([' ', '\t']);
        Line {
            start,
            indent: body.len() - text.len(),
            text: text.trim_end(),
        }
    })
}

/// The entry text of a list line (`- x`, `* x`, `+ x`, `1. x`), else `None`.
fn list_entry(text: &str) -> Option<&str> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = text.strip_prefix(bullet) {
            return Some(rest);
        }
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        return text[digits..].strip_prefix(". ");
    }
    None
}

fn detail_text(text: &str) -> String {
    text.trim()
        .strip_suffix(RESPONSE_DETAIL_MARKER)
        .unwrap_or(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The byte range and text of the value of a `- <label> <value>` line.
fn value_range<'a>(line: &Line<'a>, label: &str) -> (std::ops::Range<usize>, Option<&'a str>) {
    let lead = format!("- {label} ");
    let offset = line.start + line.indent + lead.len();
    let end = line.start + line.indent + line.text.len();
    (offset..end, line.text.strip_prefix(lead.as_str()))
}

/// The struck row's own response child, when it is in the follow-up form.
///
/// `item_start` / `item_end` are the row's byte span. Only the
/// `**Response [#r-…]**` child at the row's first child level counts; a
/// response under a nested row belongs to that row.
fn followup_cell(document: &str, item_start: usize, item_end: usize) -> Option<FollowUpCell> {
    let line_start = document[..item_start].rfind('\n').map_or(0, |at| at + 1);
    let item_indent = document[line_start..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    let after_item = if document[..item_end].ends_with('\n') {
        item_end
    } else {
        document[item_end..]
            .find('\n')
            .map_or(document.len(), |at| item_end + at + 1)
    };
    let mut scope = Vec::new();
    for line in lines_from(document, after_item) {
        if line.text.is_empty() {
            continue;
        }
        if line.indent <= item_indent {
            break;
        }
        scope.push(line);
    }
    let child_indent = scope.iter().map(|line| line.indent).min()?;
    let response_at = scope.iter().position(|line| {
        line.indent == child_indent && line.text.starts_with("- **Response [#r-")
    })?;
    let response_indent = scope[response_at].indent;
    let block: Vec<&Line<'_>> = scope[response_at + 1..]
        .iter()
        .take_while(|line| line.indent > response_indent)
        .collect();
    let detail_indent = block.iter().map(|line| line.indent).min()?;

    let mut state_value = None;
    let mut guidance_value = None;
    let mut action_items = Vec::new();
    let mut index = 0;
    while index < block.len() {
        let line = block[index];
        index += 1;
        if line.indent != detail_indent {
            continue;
        }
        if let (range, Some(value)) = value_range(line, "**State:**") {
            if value != RESPONSE_STATE_FOLLOW_UP {
                return None;
            }
            state_value = Some(range);
        } else if let (range, Some(value)) = value_range(line, "**Guidance:**") {
            if value != FOLLOW_UP_ACTION_ITEMS_GUIDANCE {
                return None;
            }
            guidance_value = Some(range);
        } else if let Some(rest) = line.text.strip_prefix("- **Action items:**") {
            let inline = detail_text(rest);
            if !inline.is_empty() {
                action_items.push(inline);
            }
            let mut entry_indent = None;
            while index < block.len() && block[index].indent > detail_indent {
                let entry = block[index];
                index += 1;
                let indent = *entry_indent.get_or_insert(entry.indent);
                match list_entry(entry.text) {
                    Some(text) if entry.indent <= indent => action_items.push(detail_text(text)),
                    _ => match action_items.last_mut() {
                        Some(last) => {
                            last.push(' ');
                            last.push_str(&detail_text(entry.text));
                        }
                        None => action_items.push(detail_text(entry.text)),
                    },
                }
            }
        }
    }
    action_items.retain(|item| !item.is_empty());
    Some(FollowUpCell {
        state_value: state_value?,
        guidance_value: guidance_value?,
        action_items,
    })
}

/// Rewrite the State and Guidance of every struck row whose response still
/// says its action items are follow-up work in this tree, when each of those
/// action items already has its own follow-up row (`#followupmovedstate`).
///
/// Returns the updated document and how many rows changed, or `None` when
/// nothing changes. A second pass changes nothing.
pub fn reconcile_moved_followup_states(document: &str) -> Option<(String, usize)> {
    let nodes = crate::derived_memo::queue_items(document)?;
    let mut tree = None;
    let line_index = crate::queue_tree::LineIndex::new(document);
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut rows = 0;
    for node in nodes.iter() {
        if !node.struck || node.surface == ItemSurface::MultilinePrompt {
            continue;
        }
        let Some(cell) = followup_cell(document, node.start_byte, node.end_byte) else {
            continue;
        };
        if !action_items_held_as_rows(document, &node.text, &cell.action_items) {
            continue;
        }
        let tree = tree.get_or_insert_with(|| {
            crate::queue_tree::queue_tree(document, &crate::queue_tree::QueueTreeOptions::default())
                .ok()
        });
        let closure = tree.as_ref().and_then(|tree| {
            crate::queue_tree::row_closure_at_indexed_offset(tree, &line_index, node.start_byte)
        });
        edits.push((cell.state_value, RESPONSE_STATE_COMPLETE.to_string()));
        edits.push((
            cell.guidance_value,
            moved_followup_guidance(closure.as_ref()),
        ));
        rows += 1;
    }
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut out = document.to_string();
    for (range, value) in edits {
        out.replace_range(range, &value);
    }
    Some((out, rows))
}

#[cfg(test)]
#[path = "followup_moved_state_tests.rs"]
mod tests;

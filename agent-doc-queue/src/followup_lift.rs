//! `#parentstrike`: follow-up rows never nest under an unrelated action row,
//! and a finished parent row has an agent close path.
//!
//! Live shape (sdk-dev.md, 2026-11-23/24, anonymized in the tests): the
//! operator typed `#justdoit` under a `🙋 [#q-…] follow-up: operator: sign in
//! … [operator-verify]` row. Follow-up placement keeps a nested origin's rows
//! at that origin's depth ([`crate::queue_inbound`]), so every follow-up of
//! `#justdoit`, and every follow-up of those follow-ups, landed as a child of
//! the sign-in row: 18 open `🙋` rows from other origins (reviews, decisions,
//! rotations, infra steps) under one operator action. The sign-in was done,
//! but the row had become a group parent: `queue consume --ack-text` matches
//! only executable leaves, so no agent path could close it, and the agent
//! asked the operator to strike it. Striking it as a subtree would have
//! closed or hidden the 18 open rows with it.
//!
//! A **standalone action row** is one unit of work, never a group label: a
//! follow-up row (`[#<origin>] follow-up: …`, or the legacy `follow up on an
//! action item from …`) or a row tagged `[operator-verify]`. This module owns
//! three rules about them:
//!
//! - **Placement** ([`hoisted_followup_slot`]): a follow-up whose origin is a
//!   direct child of a standalone action row lands after that row's subtree,
//!   at that row's depth, never inside it. Repeated while the new parent is
//!   standalone too.
//! - **Self-repair** ([`lift_misplaced_followups`]): a follow-up row that
//!   already sits directly under a standalone action row moves up to be that
//!   row's sibling, right after its subtree, with its own subtree and in
//!   document order. Only unclaimed rows move: a row whose subtree carries a
//!   worker attempt, or that the claim ledger holds, keeps its place (its
//!   claim is keyed by its nest vector).
//! - **Parent close** ([`plan_parent_close`]): an explicit agent
//!   acknowledgement of a row with live rows nested under it lifts every
//!   unclaimed follow-up and operator-verify row out first (to the parent's
//!   depth, after its subtree), so only the parent is struck. Any other live
//!   row under it (an operator sub-prompt, running work) keeps the parent
//!   open, and the plan names each one, so the refusal can list them and the
//!   agent command that closes them. Nothing here ever asks the operator to
//!   strike or delete a row.
//!
//! Generated response structure (`**Response [#r-…]**`, `**Summary:** …`,
//! response-detail rows, follow-up link rows) is never work and never moves
//! by itself. Each pass is pure; a second run changes nothing.

use agent_doc_markdown_ast::overlay::is_generated_response_structure_row;

use crate::child_row_renest::{Row, rows_of};
use crate::self_repair::{SelfRepair, SelfRepairKind};

/// Bound on lift rounds in one self-repair pass (each round lifts every
/// misplaced row under one parent).
const MAX_LIFT_ROUNDS: usize = 64;

/// A row's text with the `### ` navigation heading removed.
fn unheaded(item: &str) -> &str {
    let text = item.trim();
    text.strip_prefix("### ").unwrap_or(text).trim_start()
}

/// Whether a list row's content is struck (closed work).
pub fn is_struck_row(item: &str) -> bool {
    let text = unheaded(item);
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    text.starts_with("~~")
        || work.starts_with("~~")
        || crate::document_queue::parse_completed_inline(text).is_some()
}

/// A row's work text: heading, lifecycle projection and strike removed.
fn work_text(item: &str) -> &str {
    let text = agent_doc_element_queue::strip_attempt_projection_prefix(unheaded(item)).trim();
    let text = text
        .strip_prefix("~~")
        .and_then(|inner| inner.rfind("~~").map(|close| &inner[..close]))
        .unwrap_or(text);
    agent_doc_element_queue::strip_attempt_projection_prefix(text).trim()
}

/// Generated response structure: never work, never a parent of its own.
fn is_response_structure(item: &str) -> bool {
    let text = item.trim();
    text.starts_with("**Response ") || is_generated_response_structure_row(text)
}

/// Whether `item` is a follow-up row (either title form), struck or live.
pub fn is_followup_row(item: &str) -> bool {
    !is_response_structure(item)
        && crate::followup_owner::followup_row_action_item(work_text(item)).is_some()
}

/// A standalone action row: a follow-up row or an `[operator-verify]` row.
/// It is one unit of work, so nothing else nests under it as its child.
pub fn is_standalone_action_row(item: &str) -> bool {
    !is_response_structure(item)
        && (is_followup_row(item)
            || crate::followup_owner::carries_operator_owned_tag(work_text(item)))
}

/// Index of the last row of the subtree under `index` (blank lines inside it
/// count, trailing ones do not). A line at `index`'s depth or shallower ends
/// it.
fn subtree_last(rows: &[Row<'_>], index: usize) -> usize {
    let indent = rows[index].indent;
    let mut last = index;
    for (at, row) in rows.iter().enumerate().skip(index + 1) {
        if row.blank {
            continue;
        }
        if row.indent <= indent {
            break;
        }
        last = at;
    }
    last
}

/// Byte offset one past the line at `index`.
fn line_end(rows: &[Row<'_>], body: &str, index: usize) -> usize {
    rows.get(index + 1).map_or(body.len(), |next| next.start)
}

/// The nearest shallower list row above `index`, or `None` when a shallower
/// non-list line (prose, a heading) ends the tree first.
fn direct_parent(rows: &[Row<'_>], index: usize) -> Option<usize> {
    let indent = rows[index].indent;
    for at in (0..index).rev() {
        let row = &rows[at];
        if row.blank || row.indent >= indent {
            continue;
        }
        row.item?;
        return Some(at);
    }
    None
}

/// Every ancestor list row of `index`, nearest first.
fn ancestors(rows: &[Row<'_>], index: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = index;
    while let Some(parent) = direct_parent(rows, at) {
        out.push(parent);
        at = parent;
    }
    out
}

/// A Waiting-on-you tree mirrors open Todo state; it is binary-owned and
/// keeps its own shape.
fn in_waiting_tree(rows: &[Row<'_>], index: usize) -> bool {
    std::iter::once(index)
        .chain(ancestors(rows, index))
        .filter_map(|at| rows[at].item)
        .any(|item| {
            item.contains(crate::queue_consume::WAITING_ON_YOU_ITEM_MARKER)
                || crate::waiting_packet::is_waiting_on_you_root_text(item)
        })
}

/// A parent row that can take its own rows' follow-ups as siblings: a
/// dash-list standalone action row (a numbered step keeps its follow-ups,
/// `#orderedsteps`).
fn is_standalone_parent(row: &Row<'_>) -> bool {
    row.marker == 2 && !row.tabbed && !row.fenced && row.item.is_some_and(is_standalone_action_row)
}

/// Whether the subtree `[index, last]` may move: no fenced or tab-indented
/// line, and no live row in it carries a worker attempt or a ledger claim.
fn subtree_movable(
    rows: &[Row<'_>],
    index: usize,
    last: usize,
    is_claimed: &dyn Fn(&str) -> bool,
) -> bool {
    use agent_doc_document::queue_projection::{
        QueueLifecycleMarker, lifecycle_marker, owner_projection,
    };
    rows[index..=last].iter().all(|row| {
        if row.fenced || row.tabbed {
            return false;
        }
        let Some(item) = row.item else {
            return true;
        };
        if is_struck_row(item) || is_response_structure(item) {
            return true;
        }
        let text = unheaded(item);
        owner_projection(text).is_none()
            && !matches!(
                lifecycle_marker(text),
                Some(
                    QueueLifecycleMarker::Claimed
                        | QueueLifecycleMarker::Running
                        | QueueLifecycleMarker::Reported
                        | QueueLifecycleMarker::Lost
                        | QueueLifecycleMarker::LostClaimed
                        | QueueLifecycleMarker::LostRunning
                        | QueueLifecycleMarker::LostReported
                        | QueueLifecycleMarker::Reconciling
                )
            )
            && !is_claimed(item)
    })
}

/// `text` with up to `delta` leading spaces removed from every line.
fn dedent(text: &str, delta: usize) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let spaces = line.len() - line.trim_start_matches(' ').len();
        out.push_str(&line[spaces.min(delta)..]);
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Move the row blocks `blocks` (row index ranges inside `parent`'s
/// subtree, in document order) to right after `parent`'s subtree, at
/// `parent`'s depth.
fn move_blocks_after(
    body: &str,
    rows: &[Row<'_>],
    parent: usize,
    blocks: &[(usize, usize)],
) -> String {
    let parent_end = line_end(rows, body, subtree_last(rows, parent));
    let mut out = String::with_capacity(body.len() + 1);
    let mut moved = String::new();
    let mut cursor = 0;
    for &(first, last) in blocks {
        let start = rows[first].start;
        let end = line_end(rows, body, last);
        out.push_str(&body[cursor..start]);
        cursor = end;
        let delta = rows[first].indent - rows[parent].indent;
        moved.push_str(&dedent(&body[start..end], delta));
    }
    out.push_str(&body[cursor..parent_end]);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&moved);
    out.push_str(&body[parent_end..]);
    out
}

/// Placement (`#parentstrike`): where follow-ups of the row at line `origin`
/// of `body` go when that row is a direct child of a standalone action row.
/// Returns the body-relative insertion offset and the line prefix, after the
/// outermost such ancestor's subtree; `None` when the origin's own slot
/// stands.
pub fn hoisted_followup_slot(body: &str, origin: usize) -> Option<(usize, String)> {
    let rows = rows_of(body);
    if origin >= rows.len() || in_waiting_tree(&rows, origin) {
        return None;
    }
    let mut at = origin;
    let mut slot = None;
    while let Some(parent) = direct_parent(&rows, at)
        && is_standalone_parent(&rows[parent])
    {
        let end = line_end(&rows, body, subtree_last(&rows, parent));
        let line = &body[rows[parent].start..];
        let prefix_len = line.len() - line.trim_start_matches([' ', '\t']).len();
        slot = Some((end, line[..prefix_len].to_string()));
        at = parent;
    }
    slot
}

/// One round: the first standalone parent holding misplaced follow-up rows,
/// with those rows' blocks. A row is misplaced when its direct parent is a
/// standalone action row; it moves with its subtree when nothing in that
/// subtree is claimed.
fn misplaced_round(
    rows: &[Row<'_>],
    is_claimed: &dyn Fn(&str) -> bool,
) -> Option<(usize, Vec<(usize, usize)>)> {
    let mut target: Option<usize> = None;
    let mut blocks = Vec::new();
    let mut skip_until = 0;
    for index in 0..rows.len() {
        if index < skip_until {
            continue;
        }
        let row = &rows[index];
        let Some(item) = row.item else {
            continue;
        };
        if row.marker != 2 || row.fenced || is_struck_row(item) || !is_followup_row(item) {
            continue;
        }
        let Some(parent) = direct_parent(rows, index) else {
            continue;
        };
        if !is_standalone_parent(&rows[parent]) || in_waiting_tree(rows, index) {
            continue;
        }
        if target.is_some_and(|held| held != parent) {
            continue;
        }
        let last = subtree_last(rows, index);
        if !subtree_movable(rows, index, last, is_claimed) {
            continue;
        }
        target = Some(parent);
        blocks.push((index, last));
        skip_until = last + 1;
    }
    target.map(|parent| (parent, blocks))
}

fn lift_in_body(
    body: &str,
    is_claimed: &dyn Fn(&str) -> bool,
    repairs: &mut Vec<SelfRepair>,
) -> String {
    let mut current = body.to_string();
    for _ in 0..MAX_LIFT_ROUNDS {
        let rows = rows_of(&current);
        let Some((parent, blocks)) = misplaced_round(&rows, is_claimed) else {
            break;
        };
        let parent_text = rows[parent].item.unwrap_or_default().to_string();
        for &(first, _) in &blocks {
            repairs.push(SelfRepair {
                kind: SelfRepairKind::MisplacedFollowupRow,
                detail: format!(
                    "{} (was under {})",
                    rows[first].item.unwrap_or_default(),
                    parent_text
                ),
                rewritten: None,
            });
        }
        let next = move_blocks_after(&current, &rows, parent, &blocks);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// Self-repair (`#parentstrike`): lift every unclaimed follow-up row that
/// sits directly under a standalone action row up to be that row's sibling,
/// right after its subtree, in document order. `is_claimed(row)` is the
/// claim ledger's answer for a row's text; a claimed row keeps its place.
pub fn lift_misplaced_followups(
    document: &str,
    is_claimed: &dyn Fn(&str) -> bool,
) -> (String, Vec<SelfRepair>) {
    crate::self_repair::edit_queue_bodies(document, |body, repairs| {
        lift_in_body(body, is_claimed, repairs)
    })
}

/// What closing one parent row needs (`#parentstrike`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentClose {
    /// The document with every liftable row moved out from under the parent
    /// (unchanged when nothing moved, or when `open_rows` is not empty).
    pub document: String,
    /// The parent row's text (after its bullet).
    pub parent: String,
    /// Rows lifted to the parent's depth, after its subtree, in order.
    pub lifted: Vec<String>,
    /// Live rows that stay under the parent and keep it open: operator
    /// sub-prompts, running or claimed work. Empty when the parent can close.
    pub open_rows: Vec<String>,
}

/// Plan closing the live row that starts at byte `row_start` of `document`
/// (an absolute offset inside a queue component): lift every live,
/// unclaimed follow-up and `[operator-verify]` row nested anywhere under it
/// to the parent's depth, after its subtree, and name every other live row
/// still under it. `None` when no queue row starts there.
pub fn plan_parent_close(
    document: &str,
    row_start: usize,
    is_claimed: &dyn Fn(&str) -> bool,
) -> Option<ParentClose> {
    let components = agent_doc_element::element::parse(document).ok()?;
    let queue = components.iter().find(|component| {
        component.name == "queue"
            && component.open_end <= row_start
            && row_start < component.close_start
    })?;
    let body = queue.content(document);
    let relative = row_start - queue.open_end;
    let rows = rows_of(body);
    let parent = rows
        .iter()
        .enumerate()
        .rev()
        .find(|(_, row)| row.start <= relative)
        .map(|(index, _)| index)?;
    let parent_text = rows[parent].item?.to_string();
    let last = subtree_last(&rows, parent);
    let mut blocks = Vec::new();
    let mut lifted = Vec::new();
    let mut open_rows = Vec::new();
    let mut index = parent + 1;
    while index <= last {
        let row = &rows[index];
        let Some(item) = row.item.filter(|_| !row.fenced) else {
            index += 1;
            continue;
        };
        let content = item.trim();
        if is_struck_row(item)
            || is_response_structure(item)
            || content.starts_with("%%")
            || content.is_empty()
        {
            index += 1;
            continue;
        }
        let block_last = subtree_last(&rows, index);
        if row.marker == 2
            && is_standalone_action_row(item)
            && subtree_movable(&rows, index, block_last, is_claimed)
        {
            blocks.push((index, block_last));
            lifted.push(content.to_string());
            index = block_last + 1;
            continue;
        }
        open_rows.push(content.to_string());
        index += 1;
    }
    let document = if open_rows.is_empty() && !blocks.is_empty() {
        let moved = move_blocks_after(body, &rows, parent, &blocks);
        let mut out = document.to_string();
        out.replace_range(queue.open_end..queue.close_start, &moved);
        out
    } else {
        document.to_string()
    };
    Some(ParentClose {
        document,
        parent: parent_text,
        lifted: if open_rows.is_empty() {
            lifted
        } else {
            Vec::new()
        },
        open_rows,
    })
}

#[cfg(test)]
#[path = "followup_lift_tests.rs"]
mod tests;

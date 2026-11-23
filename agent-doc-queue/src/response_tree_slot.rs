//! `#selfrepaircleanup`: a row's response tree sits one level below the row,
//! after the row's operator comment and child rows.
//!
//! Every writer that attaches a `**Response [#r-…]**` tree to a queue row
//! (the projection of a response, `queue consume --ack-text`'s summary tree,
//! a stopped row's explanation, a worker hand-back) renders it at the row's
//! indent plus 2 and inserts it after the row's existing descendants. The
//! bytes can still drift from that slot afterwards. Observed in a session
//! document on 2026-11-23 (anonymized in the tests): an `--ack-text` strike
//! put a summary tree under `  - ~~squash and merge~~`, a child of a column-0
//! PR row. Its editor delivery was deferred, and the retained replay that
//! later landed it merged with the operator's deletion of that row, of the
//! PR row's `%% … %%` comment and of the next PR row. The tree was left 4
//! spaces under the column-0 row (one level skipped), and the newline after
//! its `**Guidance:**` line was gone, so the following PR row was glued onto
//! it (`… Safe to delete this tree.- ### 🔒 https://…/pull/377`) and that
//! row's comment and child rows read as the first PR row's, after its
//! response.
//!
//! [`slot_response_trees`] repairs both, changing nothing but line breaks and
//! leading spaces:
//!
//! - **Glued rows** (`glued_guidance_row`). A generated `**Guidance:**`
//!   value that ends in a known indicator (`🗑️ Safe to delete this tree.`,
//!   `✅ Done.`, or the moved-action-items note) immediately followed by a
//!   list bullet (`- `, after any spaces) gets its line break back. The text
//!   after the indicator is the next line exactly as it was, its indentation
//!   included, so one inserted `\n` restores it. An operator's own Guidance,
//!   or a value whose end the binary cannot prove (`🔒 Keep: … — <row>.`),
//!   is never split.
//! - **Misplaced response trees** (`response_tree_slot`). A response tree
//!   more than one level below its parent row (its own row was deleted, or a
//!   writer put it there) moves to the end of the parent row's subtree, one
//!   level below the row, with its own lines shifted by the same amount. A
//!   response tree at the right depth that is followed by the row's
//!   `%% … %%` comment rows moves after them. Other child rows that follow a
//!   tree at the right depth stay where they are: they may be replies the
//!   operator added after the response.
//!
//! Fail closed like [`crate::child_row_renest`]: tabs, fenced code, a
//! response under generated response structure (that is
//! [`crate::queue_consume::retire_misplaced_queue_response_projections`]'s
//! case), and a tree directly after a column-0 row that may have been torn
//! off the tree above (`#e107moved`) are left alone. A second run changes
//! nothing.

use agent_doc_element::element;
use agent_doc_markdown_ast::overlay::is_generated_response_structure_row;

use crate::child_row_renest::{
    Row, child_indent, may_be_torn_off_the_tree_above, parents, rows_of,
};
use crate::followup_moved_state::FOLLOW_UP_MOVED_GUIDANCE_NOTE;
use crate::queue_consume::{ROW_DONE_GUIDANCE, TREE_DELETE_GUIDANCE};
use crate::self_repair::{SelfRepair, SelfRepairKind};

const GUIDANCE_LEAD: &str = "- **Guidance:** ";

/// Generated Guidance endings whose last byte the binary can prove.
const PROVABLE_GUIDANCE_ENDINGS: [&str; 3] = [
    TREE_DELETE_GUIDANCE,
    ROW_DONE_GUIDANCE,
    FOLLOW_UP_MOVED_GUIDANCE_NOTE,
];

/// The absolute offset where a glued row starts inside the generated
/// Guidance line `line` (which starts at `line_start`), if one does.
fn glued_row_offset(line: &str, line_start: usize) -> Option<usize> {
    let text = line.trim_end_matches(['\n', '\r']);
    let trimmed = text.trim_start_matches(' ');
    let value = trimmed.strip_prefix(GUIDANCE_LEAD)?;
    let value_start = line_start + (text.len() - trimmed.len()) + GUIDANCE_LEAD.len();
    let mut best: Option<usize> = None;
    for ending in PROVABLE_GUIDANCE_ENDINGS {
        for (at, _) in value.match_indices(ending) {
            let split = at + ending.len();
            let rest = &value[split..];
            let bullet = rest.trim_start_matches(' ');
            let Some(row) = bullet.strip_prefix("- ") else {
                continue;
            };
            if row.trim().is_empty()
                || crate::response_tree_sync::generated_completed_guidance(&value[..split])
                    .is_none()
            {
                continue;
            }
            if best.is_none_or(|best| split < best) {
                best = Some(split);
            }
        }
    }
    best.map(|split| value_start + split)
}

fn split_glued_rows(body: &str, repairs: &mut Vec<SelfRepair>) -> String {
    let mut current = body.to_string();
    for _ in 0..256 {
        let rows = rows_of(&current);
        let mut found = None;
        let mut cursor = 0;
        for (row, line) in rows.iter().zip(current.split_inclusive('\n')) {
            cursor += line.len();
            if row.fenced || row.tabbed {
                continue;
            }
            if let Some(at) = glued_row_offset(line, row.start) {
                found = Some((at, cursor));
                break;
            }
        }
        let Some((at, line_end)) = found else {
            break;
        };
        let glued = current[at..line_end].trim_end_matches(['\n', '\r']);
        repairs.push(SelfRepair {
            kind: SelfRepairKind::GluedGuidanceRow,
            detail: glued
                .trim_start_matches(' ')
                .trim_start_matches("- ")
                .to_string(),
            rewritten: None,
        });
        current.insert(at, '\n');
    }
    current
}

fn is_response_row(item: &str) -> bool {
    let item = item.trim();
    item.starts_with("**Response [#r-") && item.ends_with("]**")
}

fn is_comment_row(item: &str) -> bool {
    let item = item.trim();
    item.len() >= 4 && item.starts_with("%%") && item.ends_with("%%")
}

/// One past the last line under the row at `index`: the following lines
/// indented deeper, up to the first blank or shallower-or-equal line.
fn subtree_end(rows: &[Row<'_>], index: usize) -> usize {
    let indent = rows[index].indent;
    (index + 1..rows.len())
        .find(|at| rows[*at].blank || rows[*at].indent <= indent)
        .unwrap_or(rows.len())
}

/// One planned move: the response tree `[from, to)` goes before line `dest`,
/// each of its lines losing `cut` leading spaces.
struct Move {
    from: usize,
    to: usize,
    dest: usize,
    cut: usize,
}

fn plan_move(rows: &[Row<'_>], parents: &[Option<usize>], index: usize) -> Option<Move> {
    let row = &rows[index];
    if row.fenced || row.tabbed || !row.item.is_some_and(is_response_row) {
        return None;
    }
    let parent = parents[index]?;
    if rows[parent]
        .item
        .is_some_and(is_generated_response_structure_row)
    {
        return None;
    }
    let target = child_indent(rows, parents, parent);
    let to = subtree_end(rows, index);
    // A blank line inside the tree hides where it ends.
    if rows[to..]
        .iter()
        .find(|line| !line.blank)
        .is_some_and(|line| rows.get(to).is_some_and(|at| at.blank) && line.indent > row.indent)
    {
        return None;
    }
    let parent_end = subtree_end(rows, parent);
    if rows[parent..parent_end]
        .iter()
        .any(|line| line.tabbed || line.fenced)
    {
        return None;
    }
    if row.indent > target {
        if may_be_torn_off_the_tree_above(rows, parent, index) {
            return None;
        }
        return Some(Move {
            from: index,
            to,
            dest: parent_end,
            cut: row.indent - target,
        });
    }
    if row.indent != target {
        return None;
    }
    let mut dest = to;
    while dest < parent_end
        && rows[dest].indent == target
        && rows[dest].item.is_some_and(is_comment_row)
        && subtree_end(rows, dest) == dest + 1
    {
        dest += 1;
    }
    (dest > to).then_some(Move {
        from: index,
        to,
        dest,
        cut: 0,
    })
}

fn slot_in_body(body: &str, repairs: &mut Vec<SelfRepair>) -> String {
    let mut current = split_glued_rows(body, repairs);
    // One tree per pass keeps every offset valid; a moved tree lands in its
    // slot, so it is never planned again.
    for _ in 0..256 {
        let rows = rows_of(&current);
        let parents = parents(&rows);
        let Some(step) = (0..rows.len()).find_map(|index| plan_move(&rows, &parents, index)) else {
            break;
        };
        let offset = |line: usize| rows.get(line).map_or(current.len(), |row| row.start);
        let mut tree = String::new();
        for line in current[offset(step.from)..offset(step.to)].split_inclusive('\n') {
            let lead = line.len() - line.trim_start_matches(' ').len();
            tree.push_str(&line[step.cut.min(lead)..]);
        }
        let mut between = current[offset(step.to)..offset(step.dest)].to_string();
        if !between.is_empty() && !between.ends_with('\n') {
            between.push('\n');
            if tree.ends_with('\n') {
                tree.pop();
            }
        }
        repairs.push(SelfRepair {
            kind: SelfRepairKind::ResponseTreeSlot,
            detail: rows[step.from].item.unwrap_or_default().to_string(),
            rewritten: None,
        });
        let mut out = String::with_capacity(current.len());
        out.push_str(&current[..offset(step.from)]);
        out.push_str(&between);
        out.push_str(&tree);
        out.push_str(&current[offset(step.dest)..]);
        current = out;
    }
    current
}

/// Put every queue row's response tree in its slot: one level below the row,
/// after the row's comment and child rows, restoring a row glued onto its
/// generated `**Guidance:**` line first (see the module docs). A second run
/// changes nothing.
pub fn slot_response_trees(document: &str) -> (String, Vec<SelfRepair>) {
    if !document.contains("**Response [#r-") {
        return (document.to_string(), Vec::new());
    }
    let Ok(components) = element::parse(document) else {
        return (document.to_string(), Vec::new());
    };
    let mut repairs = Vec::new();
    let mut edits = Vec::new();
    for queue in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let body = queue.content(document);
        let repaired = slot_in_body(body, &mut repairs);
        if repaired != body {
            edits.push((queue.open_end, queue.close_start, repaired));
        }
    }
    let mut out = document.to_string();
    for (start, end, body) in edits.into_iter().rev() {
        out.replace_range(start..end, &body);
    }
    (out, repairs)
}

#[cfg(test)]
#[path = "response_tree_slot_tests.rs"]
mod tests;

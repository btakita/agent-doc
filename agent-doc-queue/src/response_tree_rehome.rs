//! `#e107moved`: a top-level queue row that a torn merge left inside another
//! row's response tree is moved back out.
//!
//! On 2026-11-22 the struck top-level row `[#q-e107b04c1cd5]` landed between
//! the `**Remaining:**` list and the `**State:**` row of the
//! `[#r-ed4566860530]` response under the row above it. A column-0 row ends
//! the tree above it, so the response's State, Guidance and follow-up rows
//! became the moved row's children: the structural projection then read the
//! moved row as a parent with live leaves and un-struck it, and closed the
//! tree above as if its work were done. The response stopped re-rendering.
//!
//! The shape is never authored. A response's rows sit one level below its
//! `**Response [#r-…]**` row, so a column-0 row followed directly by a
//! deeper generated response-structure row (`**State:**`, `**Guidance:**`,
//! a response-detail row, …) at exactly that level, while the last row at
//! the response row's depth in the tree above is that `**Response**` row,
//! has split that response. [`rehome_rows_inside_response_trees`] moves the
//! row's line to just after the run of rows it split off, so the run rejoins
//! its response and the row is a top-level row again. Only the row's line
//! moves; its text is kept byte for byte, except that a strike lost on the
//! way is restored when `struck_provenance` proves the row was struck (a
//! committed or baseline copy of the document holds it struck at column 0,
//! outside any response tree) and the row has no live row of its own.
//!
//! A row whose response tree is gone (the operator deleted it) is not
//! moved: nothing proves where the split-off rows belong.

use agent_doc_element::element;

use crate::self_repair::{SelfRepair, SelfRepairKind};

struct Row<'a> {
    start: usize,
    end: usize,
    indent: usize,
    item: Option<&'a str>,
    blank: bool,
}

fn rows_of(body: &str) -> Vec<Row<'_>> {
    let mut out = Vec::new();
    let mut cursor = 0;
    for raw in body.split_inclusive('\n') {
        let start = cursor;
        cursor += raw.len();
        let text = raw.trim_end_matches(['\n', '\r']);
        let trimmed = text.trim_start_matches(' ');
        let item = trimmed.strip_prefix("- ").map(str::trim_end);
        out.push(Row {
            start,
            end: cursor,
            indent: text.len() - trimmed.len(),
            item,
            blank: trimmed.trim().is_empty(),
        });
    }
    out
}

fn is_response_row(item: &str) -> bool {
    let item = item.trim();
    item.starts_with("**Response [#r-") && item.ends_with("]**")
}

/// `~~…~~` around the whole row after an optional `### ` heading.
fn struck_inner(item: &str) -> Option<&str> {
    let item = item.strip_prefix("### ").unwrap_or(item).trim();
    item.strip_prefix("~~")
        .and_then(|rest| rest.strip_suffix("~~"))
        .filter(|inner| !inner.trim().is_empty())
}

/// The row's work text: no heading, strike, lifecycle marker or owner label.
pub fn row_work_text(item: &str) -> String {
    let item = item.trim();
    let item = item.strip_prefix("### ").unwrap_or(item);
    let inner = struck_inner(item).unwrap_or(item);
    agent_doc_element_queue::struck_work_text(inner)
        .trim()
        .to_string()
}

/// For the column-0 row at `index`: the end (exclusive row index) of the run
/// of response rows it split off the tree above, when the split is proven.
fn split_run_end(rows: &[Row<'_>], index: usize) -> Option<usize> {
    let row = &rows[index];
    if row.indent != 0 || row.item.is_none() {
        return None;
    }
    let first = rows.get(index + 1)?;
    let first_item = first.item?;
    if first.blank
        || first.indent < 4
        || !agent_doc_markdown_ast::overlay::is_generated_response_structure_row(first_item)
    {
        return None;
    }
    let response_indent = first.indent - 2;
    // The tree above: its last row at or above the response row's depth must
    // be that `**Response [#r-…]**` row, so the run continues its fields.
    let owner = rows[..index]
        .iter()
        .rev()
        .take_while(|above| above.blank || above.indent > 0 || above.item.is_none())
        .find(|above| above.item.is_some() && above.indent <= response_indent)?;
    if owner.indent != response_indent || !owner.item.is_some_and(is_response_row) {
        return None;
    }
    let end = (index + 1..rows.len())
        .find(|at| rows[*at].blank || rows[*at].indent <= response_indent)
        .unwrap_or(rows.len());
    Some(end)
}

/// Whether `document` holds a column-0 queue row with this work text struck,
/// outside any response tree.
pub fn top_level_row_struck_in(document: &str, work: &str) -> bool {
    let Ok(components) = element::parse(document) else {
        return false;
    };
    components
        .iter()
        .filter(|component| component.name == "queue")
        .any(|queue| {
            let rows = rows_of(queue.content(document));
            (0..rows.len()).any(|index| {
                rows[index].indent == 0
                    && rows[index].item.is_some_and(|item| {
                        struck_inner(item).is_some() && row_work_text(item) == work
                    })
                    && split_run_end(&rows, index).is_none()
            })
        })
}

fn rehome_in_body(
    body: &str,
    struck_provenance: &dyn Fn(&str) -> bool,
    repairs: &mut Vec<SelfRepair>,
) -> String {
    let mut current = body.to_string();
    // One move per pass keeps every offset valid; a body holds few such rows.
    for _ in 0..64 {
        let rows = rows_of(&current);
        let Some((index, end)) =
            (0..rows.len()).find_map(|index| split_run_end(&rows, index).map(|end| (index, end)))
        else {
            break;
        };
        let row = &rows[index];
        let item = row.item.expect("a split row is a list item");
        // The row's own subtree once it sits after the run.
        let own_end = (end..rows.len())
            .find(|at| !rows[*at].blank && rows[*at].indent == 0)
            .unwrap_or(rows.len());
        let live_child = rows[end..own_end].iter().any(|child| {
            child
                .item
                .is_some_and(|child| struck_inner(child).is_none())
        });
        let work = row_work_text(item);
        let restore_strike = struck_inner(item).is_none()
            && !live_child
            && !work.is_empty()
            && struck_provenance(&work);
        let line = &current[row.start..row.end];
        let moved = if restore_strike {
            let heading = if item.trim_start().starts_with("### ") {
                "### "
            } else {
                ""
            };
            format!("- {heading}~~{work}~~\n")
        } else if line.ends_with('\n') {
            line.to_string()
        } else {
            format!("{line}\n")
        };
        let insert_at = rows.get(end).map_or(current.len(), |next| next.start);
        let mut out = String::with_capacity(current.len() + 8);
        out.push_str(&current[..row.start]);
        out.push_str(&current[row.end..insert_at]);
        if !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&moved);
        out.push_str(&current[insert_at..]);
        let rewritten = restore_strike.then(|| {
            (
                item.to_string(),
                format!(
                    "{}~~{work}~~",
                    if item.trim_start().starts_with("### ") {
                        "### "
                    } else {
                        ""
                    }
                ),
            )
        });
        repairs.push(SelfRepair {
            kind: SelfRepairKind::RowInsideResponseTree,
            detail: work,
            rewritten,
        });
        if out == current {
            break;
        }
        current = out;
    }
    current
}

/// Move every column-0 queue row that split another row's response tree back
/// below that tree, restoring a lost strike when `struck_provenance(work)`
/// proves it (see the module docs). A second run changes nothing.
pub fn rehome_rows_inside_response_trees(
    document: &str,
    struck_provenance: &dyn Fn(&str) -> bool,
) -> (String, Vec<SelfRepair>) {
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
        let repaired = rehome_in_body(body, struck_provenance, &mut repairs);
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
#[path = "response_tree_rehome_tests.rs"]
mod tests;

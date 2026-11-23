//! `#selfrepaircleanup`: a queue row that sits more than one level below its
//! parent row moves up to sit exactly one level below it.
//!
//! Two live shapes on 2026-11-22 (anonymized in the tests):
//!
//! - An operator pasted two lines of a response tree inside one code span,
//!   so the second line (`      - agent: coordinator runs … `` `) sat 6
//!   spaces under their new column-0 row. It was answered, struck in place,
//!   and given its own `**Response [#r-…]**` tree beneath it. Markdown reads
//!   a line 4 or more columns past its parent's content as that parent's
//!   paragraph text, so the answer and its response rendered as part of the
//!   operator's sentence: "skipped 2 indentations from its parent".
//! - After a response tree was deleted, the two `🙋` follow-up rows it held
//!   stayed at 4 spaces under the column-0 row that had split the tree
//!   (`#e107moved`): one indentation level skipped.
//!
//! Every reader already takes the nearest shallower row as a row's parent;
//! only the bytes disagree. [`renest_child_rows`] moves each such row up by
//! the excess, with its whole subtree, so the subtree keeps its structure
//! relative to itself. A row "skips" when it sits deeper than its parent row
//! by more than one step, where a step is the parent's own depth below its
//! parent (2 spaces when unknown, as for a column-0 parent). Only leading
//! spaces change; the text after them is byte-for-byte the same.
//!
//! Fail closed: a subtree indented with tabs, or holding a fenced code line,
//! is left alone, and so is a run directly after a column-0 row that fits the
//! tree above it exactly one level below a `**Response [#r-…]**` row, a
//! response container (`**Remaining:**`, …) or a response entry. That run
//! may be torn off the tree above (the `#e107moved` family, which
//! [`crate::response_tree_rehome`] moves back when it can prove it); nothing
//! here proves which parent it belongs to. A second run changes nothing.

use agent_doc_element::element;
use agent_doc_markdown_ast::overlay::{
    GENERATED_RESPONSE_STRUCTURE_CONTAINER_LABELS, is_generated_response_structure_row,
};

use crate::self_repair::{SelfRepair, SelfRepairKind};

pub(crate) struct Row<'a> {
    pub(crate) start: usize,
    pub(crate) indent: usize,
    pub(crate) item: Option<&'a str>,
    pub(crate) blank: bool,
    pub(crate) tabbed: bool,
    pub(crate) fenced: bool,
}

pub(crate) fn rows_of(body: &str) -> Vec<Row<'_>> {
    let mut out = Vec::new();
    let mut cursor = 0;
    let mut in_fence = false;
    for raw in body.split_inclusive('\n') {
        let start = cursor;
        cursor += raw.len();
        let text = raw.trim_end_matches(['\n', '\r']);
        let trimmed = text.trim_start_matches([' ', '\t']);
        let lead = &text[..text.len() - trimmed.len()];
        let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        let fenced = in_fence || fence;
        if fence {
            in_fence = !in_fence;
        }
        out.push(Row {
            start,
            indent: lead.len(),
            item: (!fenced)
                .then(|| trimmed.strip_prefix("- ").map(str::trim_end))
                .flatten(),
            blank: trimmed.is_empty(),
            tabbed: lead.contains('\t'),
            fenced,
        });
    }
    out
}

/// The nearest shallower list row above each list row.
pub(crate) fn parents(rows: &[Row<'_>]) -> Vec<Option<usize>> {
    let mut parents = vec![None; rows.len()];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if row.item.is_none() {
            continue;
        }
        while stack
            .last()
            .is_some_and(|(indent, _)| *indent >= row.indent)
        {
            stack.pop();
        }
        parents[index] = stack.last().map(|(_, parent)| *parent);
        stack.push((row.indent, index));
    }
    parents
}

/// One past the last line of the run that moves with the row at `index`:
/// the row, the rows after it at its depth (its later siblings, which share
/// its parent and its excess) and their subtrees.
fn run_end(rows: &[Row<'_>], index: usize) -> usize {
    let indent = rows[index].indent;
    (index + 1..rows.len())
        .find(|at| rows[*at].blank || rows[*at].indent < indent)
        .unwrap_or(rows.len())
}

/// A row that takes the response rows of a tree as children: the
/// `**Response [#r-…]**` row, a response container, or a response entry
/// (a generated response-detail row that is not a `**Label:** value` field).
fn holds_response_rows(item: &str) -> bool {
    let item = item.trim();
    (item.starts_with("**Response [#r-") && item.ends_with("]**"))
        || GENERATED_RESPONSE_STRUCTURE_CONTAINER_LABELS.contains(&item)
        || (!item.starts_with("**") && is_generated_response_structure_row(item))
}

/// The row at `index` sits right after the column-0 row `parent`, and the
/// tree above that row ends, exactly one level above it, on a row that holds
/// response rows: the run may have been torn off that tree.
pub(crate) fn may_be_torn_off_the_tree_above(
    rows: &[Row<'_>],
    parent: usize,
    index: usize,
) -> bool {
    if rows[parent].indent != 0 {
        return false;
    }
    let home = rows[index].indent.saturating_sub(2);
    rows[..parent]
        .iter()
        .rev()
        .take_while(|above| above.blank || above.indent > 0 || above.item.is_none())
        .find(|above| above.item.is_some() && above.indent <= home)
        .is_some_and(|above| above.indent == home && above.item.is_some_and(holds_response_rows))
}

/// The indent of a row exactly one level below `parent`: the parent's
/// indent plus its own step below its parent (2 spaces when unknown, as for
/// a column-0 parent).
pub(crate) fn child_indent(rows: &[Row<'_>], parents: &[Option<usize>], parent: usize) -> usize {
    let step = parents[parent]
        .map(|grand| rows[parent].indent.saturating_sub(rows[grand].indent))
        .filter(|step| (2..=4).contains(step))
        .unwrap_or(2);
    rows[parent].indent + step
}

/// For the row at `index`: how many leading spaces to cut from it and its
/// subtree, when it skips levels under its parent.
fn excess(rows: &[Row<'_>], parents: &[Option<usize>], index: usize) -> Option<usize> {
    let row = &rows[index];
    row.item?;
    let parent = parents[index]?;
    let target = child_indent(rows, parents, parent);
    if row.indent <= target {
        return None;
    }
    let end = run_end(rows, index);
    if rows[index..end]
        .iter()
        .any(|line| line.tabbed || line.fenced)
        || may_be_torn_off_the_tree_above(rows, parent, index)
    {
        return None;
    }
    Some(row.indent - target)
}

fn renest_in_body(body: &str, repairs: &mut Vec<SelfRepair>) -> String {
    let mut current = body.to_string();
    // One subtree per pass keeps every offset valid; a body holds few such
    // rows, and a moved subtree's own rows are already one step apart.
    for _ in 0..256 {
        let rows = rows_of(&current);
        let parents = parents(&rows);
        let Some((index, cut)) = (0..rows.len())
            .find_map(|index| excess(&rows, &parents, index).map(|cut| (index, cut)))
        else {
            break;
        };
        let end = run_end(&rows, index);
        let mut out = String::with_capacity(current.len());
        let mut cursor = 0;
        for row in &rows[index..end] {
            if row.blank {
                continue;
            }
            out.push_str(&current[cursor..row.start]);
            cursor = row.start + cut.min(row.indent);
            if row.indent == rows[index].indent
                && let Some(item) = row.item
            {
                repairs.push(SelfRepair {
                    kind: SelfRepairKind::MisnestedChildRow,
                    detail: item.to_string(),
                    rewritten: None,
                });
            }
        }
        out.push_str(&current[cursor..]);
        current = out;
    }
    current
}

/// Move every queue row that sits more than one level below its parent row
/// up to sit exactly one level below it, with its subtree (see the module
/// docs). A second run changes nothing.
pub fn renest_child_rows(document: &str) -> (String, Vec<SelfRepair>) {
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
        let repaired = renest_in_body(body, &mut repairs);
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
#[path = "child_row_renest_tests.rs"]
mod tests;

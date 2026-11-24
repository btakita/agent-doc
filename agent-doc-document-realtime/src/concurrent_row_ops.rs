//! `#composefault`: two writes that change ONE queue row, composed by a
//! line-level 3-way merge.
//!
//! Measured on haiven-dev `tasks/sdk/sdk-dev.md`. `queue report-complete`
//! admits the origin row's `[#q-…]` and inserts its follow-up rows right after
//! the origin's subtree. The strike (or a response attach) changes the same
//! row and inserts the response tree right under it. When one of the two is a
//! retained intent the editor never acknowledged, the other is derived over a
//! cut without it, and the deferred-write composition (or the controller's
//! retained projection over the editor's cut) merges them line by line:
//!
//! - **Same row, two edits.** Both sides replaced the origin line, each with a
//!   different line of the same row (`☑️ … [#q-b55e5bde5b7a]` and `~~…~~`). A
//!   line merge keeps both, so the row exists twice: the id'd copy with the
//!   follow-ups, and the struck id-less copy with the response tree
//!   (2026-11-24T21:45:07Z-21:45:43Z, c5815096).
//! - **Same gap, two blocks.** Both sides inserted after the same base line:
//!   one the response tree (deeper than the origin), the other the follow-up
//!   rows (the origin's siblings). The merge orders the two blocks by op
//!   order, not by depth, so the response tree can land after the follow-ups
//!   and read as the LAST follow-up's child (`[#r-1c7433c63076]`, 38373132),
//!   and an operator reply typed under the origin lands under it too.
//!
//! [`reconcile_concurrent_queue_row_ops`] rewrites the merge base and the
//! older side so the merge sees one op per row: the newer side's row line is
//! taken as base and the older side carries the combined row; the newer
//! side's block is taken as base and placed in the older side by depth (a
//! deeper block first, since it belongs to the line above the gap). The newer
//! side is never changed.

use agent_doc_element::component_scope::top_level_component_bodies;
use similar::{Algorithm, DiffOp, capture_diff_slices};

/// The merge inputs [`reconcile_concurrent_queue_row_ops`] rewrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcurrentQueueRowOps {
    /// The merge base: each folded row at the newer side's line, and each
    /// placed block of the newer side present.
    pub base: String,
    /// The older side: each folded row at its combined line, and each placed
    /// block of the newer side beside its own block, ordered by depth.
    pub older: String,
    /// Rows both sides changed, folded to one line.
    pub folded: usize,
    /// Gaps where both sides inserted, ordered by depth.
    pub ordered: usize,
}

/// `#composefault`: rewrite `merge_base` and `older` so a 3-way merge with
/// `newer` keeps each queue row once and each inserted block under its own
/// parent. See the module docs. `None` when nothing changes or the three
/// texts do not share one component layout.
pub fn reconcile_concurrent_queue_row_ops(
    merge_base: &str,
    older: &str,
    newer: &str,
) -> Option<ConcurrentQueueRowOps> {
    let base_bodies = top_level_component_bodies(merge_base)?;
    let older_bodies = top_level_component_bodies(older)?;
    let newer_bodies = top_level_component_bodies(newer)?;
    if base_bodies.len() != older_bodies.len()
        || base_bodies.len() != newer_bodies.len()
        || base_bodies
            .iter()
            .zip(&older_bodies)
            .zip(&newer_bodies)
            .any(|((base, older), newer)| {
                base.component != older.component || base.component != newer.component
            })
    {
        return None;
    }
    let mut base_edits = Vec::new();
    let mut older_edits = Vec::new();
    let (mut folded, mut ordered) = (0, 0);
    for ((base, older_body), newer_body) in base_bodies.iter().zip(&older_bodies).zip(&newer_bodies)
    {
        if !matches!(base.component.name.as_str(), "queue" | "run" | "loop") {
            continue;
        }
        let base_text = &merge_base[base.body.clone()];
        let older_text = &older[older_body.body.clone()];
        let newer_text = &newer[newer_body.body.clone()];
        if base_text == older_text || base_text == newer_text {
            continue;
        }
        let Some(body) = reconcile_body(base_text, older_text, newer_text) else {
            continue;
        };
        folded += body.folded;
        ordered += body.ordered;
        if body.base != base_text {
            base_edits.push((base.body.clone(), body.base));
        }
        if body.older != older_text {
            older_edits.push((older_body.body.clone(), body.older));
        }
    }
    if base_edits.is_empty() && older_edits.is_empty() {
        return None;
    }
    Some(ConcurrentQueueRowOps {
        base: splice(merge_base, base_edits),
        older: splice(older, older_edits),
        folded,
        ordered,
    })
}

fn splice(text: &str, mut edits: Vec<(std::ops::Range<usize>, String)>) -> String {
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut out = text.to_string();
    for (range, replacement) in edits {
        out.replace_range(range, &replacement);
    }
    out
}

struct ReconciledBody {
    base: String,
    older: String,
    folded: usize,
    ordered: usize,
}

fn reconcile_body(base: &str, older: &str, newer: &str) -> Option<ReconciledBody> {
    let mut base_lines = owned_lines(base);
    let mut older_lines = owned_lines(older);
    let newer_lines = owned_lines(newer);

    // Same row, two edits: the newer line becomes base, the older side
    // carries the combined line.
    let older_alignment = Alignment::of(&base_lines, &older_lines);
    let newer_alignment = Alignment::of(&base_lines, &newer_lines);
    let mut folded = 0;
    for (index, base_line) in base_lines.clone().iter().enumerate() {
        let (Fate::Replaced(at_older), Fate::Replaced(at_newer)) =
            (older_alignment.fate[index], newer_alignment.fate[index])
        else {
            continue;
        };
        let older_line = &older_lines[at_older];
        let newer_line = &newer_lines[at_newer];
        if line_text(older_line) == line_text(newer_line) {
            continue;
        }
        let Some(combined) = combine_row_edits(base_line, older_line, newer_line) else {
            continue;
        };
        base_lines[index] = newer_line.clone();
        older_lines[at_older] = combined;
        folded += 1;
    }

    // Same gap, two blocks: the newer block becomes base and is placed in the
    // older side by depth.
    let older_alignment = Alignment::of(&base_lines, &older_lines);
    let newer_alignment = Alignment::of(&base_lines, &newer_lines);
    let mut ordered = 0;
    // Back to front, so earlier indices stay valid while splicing.
    for gap in (0..=base_lines.len()).rev() {
        let older_block = &older_alignment.gaps[gap];
        let newer_block = &newer_alignment.gaps[gap];
        let (Some(&older_first), Some(&older_last), Some(&newer_first)) =
            (older_block.first(), older_block.last(), newer_block.first())
        else {
            continue;
        };
        let older_depth = indent(&older_lines[older_first]);
        let newer_depth = indent(&newer_lines[newer_first]);
        if older_depth == newer_depth {
            continue;
        }
        let block: Vec<String> = newer_block
            .iter()
            .map(|&index| with_newline(&newer_lines[index]))
            .collect();
        // A block the older side already holds somewhere is not a second
        // insert to order; leave it to the merge.
        if block.iter().any(|line| {
            !line.trim().is_empty()
                && older_lines
                    .iter()
                    .any(|held| line_text(held) == line_text(line))
        }) {
            continue;
        }
        let at = if newer_depth > older_depth {
            older_first
        } else {
            older_last + 1
        };
        older_lines.splice(at..at, block.iter().cloned());
        base_lines.splice(gap..gap, block);
        ordered += 1;
    }
    if folded == 0 && ordered == 0 {
        return None;
    }
    Some(ReconciledBody {
        base: base_lines.concat(),
        older: older_lines.concat(),
        folded,
        ordered,
    })
}

fn owned_lines(text: &str) -> Vec<String> {
    text.split_inclusive('\n').map(str::to_string).collect()
}

fn line_text(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

fn with_newline(line: &str) -> String {
    if line.ends_with('\n') {
        line.to_string()
    } else {
        format!("{line}\n")
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Kept,
    /// Replaced by the side's line at this index, the same row.
    Replaced(usize),
    Deleted,
}

/// One side's line-level change over base: each base line's fate, and the
/// side's lines inserted in each gap (`gaps[g]` sits before base line `g`).
struct Alignment {
    fate: Vec<Fate>,
    gaps: Vec<Vec<usize>>,
}

impl Alignment {
    fn of(base: &[String], side: &[String]) -> Self {
        let base_keys: Vec<&str> = base.iter().map(|line| line_text(line)).collect();
        let side_keys: Vec<&str> = side.iter().map(|line| line_text(line)).collect();
        let mut fate = vec![Fate::Kept; base.len()];
        let mut gaps = vec![Vec::new(); base.len() + 1];
        for op in capture_diff_slices(Algorithm::Myers, &base_keys, &side_keys) {
            match op {
                DiffOp::Equal { .. } => {}
                DiffOp::Delete {
                    old_index, old_len, ..
                } => {
                    for slot in &mut fate[old_index..old_index + old_len] {
                        *slot = Fate::Deleted;
                    }
                }
                DiffOp::Insert {
                    old_index,
                    new_index,
                    new_len,
                } => gaps[old_index].extend(new_index..new_index + new_len),
                DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => {
                    // Pair each side line with the next base line of the same
                    // row, in order; what pairs with nothing is inserted after
                    // the last paired base line.
                    let end = old_index + old_len;
                    let mut next_old = old_index;
                    let mut unpaired = Vec::new();
                    for (side_index, side_line) in
                        side.iter().enumerate().skip(new_index).take(new_len)
                    {
                        let paired =
                            (next_old..end).find(|&old| same_row_line(&base[old], side_line));
                        match paired {
                            Some(old) => {
                                for slot in &mut fate[next_old..old] {
                                    *slot = Fate::Deleted;
                                }
                                gaps[next_old].append(&mut unpaired);
                                fate[old] = Fate::Replaced(side_index);
                                next_old = old + 1;
                            }
                            None => unpaired.push(side_index),
                        }
                    }
                    for slot in &mut fate[next_old..end] {
                        *slot = Fate::Deleted;
                    }
                    gaps[next_old].append(&mut unpaired);
                }
            }
        }
        Self { fate, gaps }
    }
}

/// The parts of one queue list line.
struct RowLine<'a> {
    /// Indent and list marker (`  - `, `1. `).
    lead: &'a str,
    /// `### ` heading prefixes.
    heading: &'a str,
    /// The rest of the line, without its newline.
    body: &'a str,
    newline: &'a str,
}

fn row_line(line: &str) -> Option<RowLine<'_>> {
    let text = line_text(line);
    let newline = &line[text.len()..];
    let trimmed = text.trim_start();
    let marker_len = if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        2
    } else {
        agent_doc_element_queue::ordered_list_marker_len(trimmed)? + 1
    };
    let lead_len = text.len() - trimmed.len() + marker_len;
    let rest = &text[lead_len..];
    let mut body = rest;
    while let Some(after) = body.strip_prefix("### ") {
        body = after;
    }
    let heading = &rest[..rest.len() - body.len()];
    Some(RowLine {
        lead: &text[..lead_len],
        heading,
        body,
        newline,
    })
}

/// Whether `body` is one single-line strike (`~~work~~`), past any
/// projection prefix.
fn struck(body: &str) -> bool {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(body).trim();
    work.len() > 4 && work.starts_with("~~") && work.ends_with("~~")
}

/// The binary-generated `[#q-…]` a row carries, inside its strike or at its
/// tail.
fn generated_id(body: &str) -> Option<String> {
    if struck(body) {
        let work = agent_doc_element_queue::strip_attempt_projection_prefix(body).trim();
        return agent_doc_element_queue::struck_generated_prompt_id(work)
            .map(str::to_ascii_lowercase);
    }
    agent_doc_element_queue::generated_queue_prompt_id(body)
}

/// A row's work text: projection prefix, strike and generated id set aside,
/// whitespace collapsed. Two lines of one row (claimed, reported, struck,
/// id-admitted) share it.
fn row_key(body: &str) -> String {
    let mut work = agent_doc_element_queue::strip_attempt_projection_prefix(body).trim();
    if struck(body) {
        work = &work[2..work.len() - 2];
    }
    agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(work)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn same_row_line(a: &str, b: &str) -> bool {
    let (Some(left), Some(right)) = (row_line(a), row_line(b)) else {
        return false;
    };
    indent(a) == indent(b) && {
        let key = row_key(left.body);
        !key.is_empty() && key == row_key(right.body)
    }
}

/// The one line for a row `older` and `newer` each changed from `base`: the
/// newer line, struck when the older line struck it, carrying the older
/// line's generated id when the newer line has none. `None` when the edits
/// are not two projections of one row (a different work text or id, or a
/// strike one side undid).
fn combine_row_edits(base: &str, older: &str, newer: &str) -> Option<String> {
    let base_row = row_line(base)?;
    let older_row = row_line(older)?;
    let newer_row = row_line(newer)?;
    if !(same_row_line(base, older) && same_row_line(base, newer)) {
        return None;
    }
    let base_struck = struck(base_row.body);
    let older_struck = struck(older_row.body);
    let newer_struck = struck(newer_row.body);
    if base_struck && !(older_struck && newer_struck) {
        return None;
    }
    let older_id = generated_id(older_row.body);
    let newer_id = generated_id(newer_row.body);
    if let (Some(older_id), Some(newer_id)) = (&older_id, &newer_id)
        && older_id != newer_id
    {
        return None;
    }
    let mut body = if older_struck && !newer_struck {
        agent_doc_element_queue::render_struck_row_body(newer_row.body)
    } else {
        newer_row.body.to_string()
    };
    if let (Some(id), None) = (&older_id, &newer_id) {
        body = if struck(&body) {
            let inner = body.trim_end().strip_suffix("~~")?;
            format!("{} [#{id}]~~", inner.trim_end())
        } else {
            format!("{} [#{id}]", body.trim_end())
        };
    }
    Some(format!(
        "{}{}{}{}",
        newer_row.lead, newer_row.heading, body, newer_row.newline
    ))
}

#[cfg(test)]
#[path = "concurrent_row_ops_tests.rs"]
mod tests;

//! `#dupwrite`: self-repair of stale copies of the operator's typed rows.
//!
//! A retained write composed over an older cut of a row the operator was
//! still typing could merge that older cut back beside the operator's newer
//! one (measured on a session document 2026-11-24T17:17-17:19Z: one row read
//! four times, and four typing snapshots of another row, one before its `- `
//! list marker, sat above the finished row). The merge no longer does that
//! (`agent_doc_document_realtime::write_policy::neutralize_stale_queue_row_copies`);
//! this pass removes the copies an older binary left.
//!
//! Only a **free-text leaf row** is ever removed: a live (not struck) row in
//! a queue component with no claim projection, no authored `[#id]` (a
//! generated `[#q-…]` occurrence id and `⟦…⟧` markers are the binary's and
//! are ignored), no child rows and no generated response structure, or a
//! bare line at column 0 (a row typed before its `- `). It is a copy of a **newer row** at the same depth under
//! the same parent when, comparing work text (list marker, `###` heading and
//! lifecycle/intake projection removed):
//!
//! - **exact duplicate**: the work texts are equal, and some recorded version
//!   of the document held that row exactly once (the doubling came later).
//!   Only prose rows (with a space) qualify; a repeated bare link or `#tag`
//!   is list structure. A copy with child rows stays, else the last copy
//!   without a `⟦effect:N⟧` duplicate ordinal, else the last copy;
//! - **typing snapshot**: the copy's work text (at least
//!   [`STALE_PREFIX_MIN_CHARS`]) is a strict prefix of the newer row's, and a
//!   recorded version held the copy while the newer row did not exist yet:
//!   the operator typed the newer row on from it.
//!
//! The recorded versions are the document's content-bearing history (merge
//! baselines, retained-write cuts and targets, the committed copy). Without
//! that proof nothing is removed. A removed copy's text is always contained
//! in the row that stays, so no operator text is lost.

use std::collections::HashMap;

use agent_doc_element::element;

use crate::self_repair::{SelfRepair, SelfRepairKind};

/// Shortest work text a typing-snapshot copy may have.
pub const STALE_PREFIX_MIN_CHARS: usize = 8;

/// Work text of one queue line: list marker, `###` heading, lifecycle and
/// intake projection removed, whitespace collapsed.
pub fn queue_line_work(line: &str) -> String {
    let trimmed = line.trim();
    let body = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .unwrap_or(trimmed);
    let mut work = body.trim();
    loop {
        let before = work;
        while let Some(rest) = work.strip_prefix("### ") {
            work = rest.trim_start();
        }
        work = agent_doc_element_queue::strip_attempt_projection_prefix(work).trim_start();
        if work == before {
            break;
        }
    }
    // The binary's own annotations: a generated `[#q-…]` occurrence id and
    // trailing `⟦…⟧` markers (`⟦effect:N⟧` duplicate ordinals, publication).
    let mut work = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(work);
    while let Some(open) = work
        .trim_end()
        .strip_suffix('⟧')
        .and_then(|rest| rest.rfind(" ⟦"))
    {
        work.truncate(open);
    }
    let work = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(&work);
    work.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The work texts of every queue-component line in recorded versions of one
/// document, oldest first.
#[derive(Debug, Clone, Default)]
pub struct TypingHistory {
    versions: Vec<HashMap<String, usize>>,
}

impl TypingHistory {
    /// Index `documents` (oldest first). A version that does not parse is
    /// skipped.
    pub fn from_documents<S: AsRef<str>>(documents: &[S]) -> Self {
        let versions = documents
            .iter()
            .filter_map(|document| {
                let document = document.as_ref();
                let components = element::parse(document).ok()?;
                let mut rows: HashMap<String, usize> = HashMap::new();
                for queue in components.iter().filter(|c| c.name == "queue") {
                    for line in queue.content(document).lines() {
                        let work = queue_line_work(line);
                        if !work.is_empty() {
                            *rows.entry(work).or_insert(0) += 1;
                        }
                    }
                }
                Some(rows)
            })
            .collect();
        Self { versions }
    }

    /// Some version held `work` exactly once.
    fn held_once(&self, work: &str) -> bool {
        self.versions
            .iter()
            .any(|rows| rows.get(work).copied() == Some(1))
    }

    /// Some version held `copy` while no row read `newer` yet.
    fn typed_before(&self, copy: &str, newer: &str) -> bool {
        self.versions
            .iter()
            .any(|rows| rows.contains_key(copy) && !rows.contains_key(newer))
    }
}

struct Row {
    start: usize,
    end: usize,
    indent: usize,
    parent: Option<usize>,
    work: String,
    /// A free-text row (see the module docs); only these are compared.
    free_text: bool,
    /// Carries a `⟦effect:N⟧` duplicate ordinal: a copy, not the original.
    effect_ordinal: bool,
    has_children: bool,
}

fn is_struck(work: &str) -> bool {
    work.starts_with("~~") && work.len() > 4 && work[2..].contains("~~")
}

fn rows_of(body: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut cursor = 0;
    for raw in body.split_inclusive('\n') {
        let start = cursor;
        cursor += raw.len();
        let text = raw.trim_end_matches(['\n', '\r']);
        let trimmed = text.trim_start_matches([' ', '\t']);
        if trimmed.is_empty() {
            stack.clear();
            continue;
        }
        let indent = text.len() - trimmed.len();
        let list = ["- ", "* ", "+ "]
            .iter()
            .any(|bullet| trimmed.starts_with(bullet));
        // A bare line counts only at column 0 (a row typed before its `- `);
        // deeper bare lines are a row's continuation prose.
        if !list && indent > 0 {
            continue;
        }
        let bare_markup = !list
            && ["<!--", "%%", ">", "`", "#", "|"]
                .iter()
                .any(|prefix| trimmed.starts_with(prefix));
        while stack.last().is_some_and(|&top| rows[top].indent >= indent) {
            stack.pop();
        }
        let parent = if list { stack.last().copied() } else { None };
        if let Some(parent) = parent {
            rows[parent].has_children = true;
        }
        let work = queue_line_work(trimmed);
        let item = if list { &trimmed[2..] } else { trimmed };
        let claim = agent_doc_document::queue_projection::owner_projection(
            item.strip_prefix("### ").unwrap_or(item),
        )
        .is_some();
        let free_text = !work.is_empty()
            && !bare_markup
            && !is_struck(&work)
            && !claim
            && !work.contains("[#")
            && !work.starts_with("**")
            && !trimmed.contains(agent_doc_markdown_ast::overlay::RESPONSE_DETAIL_MARKER);
        rows.push(Row {
            start,
            end: cursor,
            indent,
            parent,
            work,
            free_text,
            effect_ordinal: trimmed.contains(" ⟦effect:"),
            has_children: false,
        });
        if list {
            stack.push(rows.len() - 1);
        }
    }
    rows
}

/// Remove proven stale copies of the operator's free-text rows from every
/// queue component (see the module docs). `history` is called at most once,
/// and only when some row has a candidate copy.
pub fn drop_stale_typing_copies(
    document: &str,
    history: &mut dyn FnMut() -> TypingHistory,
) -> (String, Vec<SelfRepair>) {
    let Ok(components) = element::parse(document) else {
        return (document.to_string(), Vec::new());
    };
    let mut loaded: Option<TypingHistory> = None;
    let mut repairs = Vec::new();
    let mut edits: Vec<(usize, usize)> = Vec::new();
    for queue in components.iter().filter(|c| c.name == "queue") {
        let body = queue.content(document);
        let rows = rows_of(body);
        let parent_work = |row: &Row| row.parent.map(|parent| rows[parent].work.as_str());
        let same_place =
            |a: &Row, b: &Row| a.indent == b.indent && parent_work(a) == parent_work(b);
        let mut removed = vec![false; rows.len()];
        for (index, row) in rows.iter().enumerate() {
            if !row.free_text || row.has_children {
                continue;
            }
            // Exact duplicates: keep the last copy, or a copy with children.
            let copies: Vec<usize> = rows
                .iter()
                .enumerate()
                .filter(|(_, other)| {
                    other.free_text && other.work == row.work && same_place(row, other)
                })
                .map(|(at, _)| at)
                .collect();
            let keeper = copies
                .iter()
                .rev()
                .find(|&&at| rows[at].has_children)
                .or_else(|| copies.iter().rev().find(|&&at| !rows[at].effect_ordinal))
                .or_else(|| copies.last())
                .copied();
            // Only prose: a repeated bare link or `#tag` is the operator's
            // own list structure.
            let duplicate = copies.len() > 1 && keeper != Some(index) && row.work.contains(' ');
            let newer_prefix_of = rows.iter().find(|other| {
                other.free_text
                    && same_place(row, other)
                    && other.work.len() > row.work.len()
                    && row.work.chars().count() >= STALE_PREFIX_MIN_CHARS
                    && other.work.starts_with(row.work.as_str())
            });
            if !duplicate && newer_prefix_of.is_none() {
                continue;
            }
            let history = loaded.get_or_insert_with(&mut *history);
            let proven = (duplicate && history.held_once(&row.work))
                || newer_prefix_of
                    .is_some_and(|newer| history.typed_before(&row.work, &newer.work));
            if !proven {
                continue;
            }
            removed[index] = true;
            repairs.push(SelfRepair {
                kind: SelfRepairKind::StaleTypingCopy,
                detail: row.work.clone(),
                rewritten: None,
            });
        }
        let base = queue.open_end;
        edits.extend(
            rows.iter()
                .zip(&removed)
                .filter(|(_, removed)| **removed)
                .map(|(row, _)| (base + row.start, base + row.end)),
        );
    }
    if edits.is_empty() {
        return (document.to_string(), repairs);
    }
    let mut out = document.to_string();
    edits.sort_unstable();
    for (start, end) in edits.into_iter().rev() {
        out.replace_range(start..end, "");
    }
    (out, repairs)
}

#[cfg(test)]
#[path = "stale_typing_copies_tests.rs"]
mod tests;

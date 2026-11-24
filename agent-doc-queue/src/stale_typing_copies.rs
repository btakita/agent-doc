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
//! bare line at column 0 (a row typed before its `- `). It is a copy of a
//! **newer row** at the same depth under the same parent when, comparing
//! work text (list marker, `###` heading and lifecycle/intake projection
//! removed):
//!
//! - **exact duplicate**: the work texts are equal, and some recorded version
//!   of the document held that row exactly once, as itself or as one typing
//!   cut of it (`#dupwrite2`: a committed copy from while the operator was
//!   typing `- I do like the pipe` proves the finished row was one row): the
//!   doubling came later. Only prose rows (with a space) qualify; a repeated
//!   bare link or `#tag` is list structure. A claimed copy stays, else a copy
//!   with child rows, else the last copy without a `⟦effect:N⟧` duplicate
//!   ordinal, else the last copy;
//! - **typing snapshot**: the copy's work text (at least
//!   [`STALE_PREFIX_MIN_CHARS`]) is a strict prefix of the newer row's, a
//!   recorded version held the copy, and a LATER version held a longer cut on
//!   the way to the newer row with the copy gone: the operator typed on from
//!   it (`#dupwrite2`). Holding the copy before the newer row existed is not
//!   enough: a row the operator kept beside a longer one added later was
//!   never replaced by it.
//!
//! A bare copy that a queue rewrite fenced as a `---` prompt takes both
//! fence lines with it. A copy a ledger claim holds is never removed (no
//! claim may point at a removed row); the generated `[#q-…]` and
//! `⟦effect:N⟧` a copy carries go with its line.
//!
//! The recorded versions are the document's content-bearing history (merge
//! baselines, retained-write cuts and targets, the committed copy, and for
//! `queue sync` the recent committed history). Without that proof nothing is
//! removed, and the candidates are reported as unproven. A removed copy's
//! text is always contained in the row that stays, so no operator text is
//! lost.

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
                        if !work.is_empty() && !is_dash_rule_line(line) {
                            *rows.entry(work).or_insert(0) += 1;
                        }
                    }
                }
                Some(rows)
            })
            .collect();
        Self { versions }
    }

    /// [`Self::from_documents`] over only the versions that can bear on
    /// `candidates` (work texts of candidate copies): a version none of
    /// whose text starts a candidate's first [`STALE_PREFIX_MIN_CHARS`]
    /// characters holds no row of any candidate's lineage, so skipping it
    /// changes no proof; it only spares parsing it.
    pub fn from_relevant_documents<S: AsRef<str>>(documents: &[S], candidates: &[String]) -> Self {
        let keys: Vec<String> = candidates
            .iter()
            .map(|work| {
                work.chars()
                    .take(STALE_PREFIX_MIN_CHARS)
                    .collect::<String>()
            })
            .filter(|key| !key.is_empty())
            .collect();
        let relevant: Vec<&str> = documents
            .iter()
            .map(AsRef::as_ref)
            .filter(|document| {
                let collapsed = document.split_whitespace().collect::<Vec<_>>().join(" ");
                keys.iter().any(|key| collapsed.contains(key.as_str()))
            })
            .collect();
        Self::from_documents(&relevant)
    }

    /// Number of recorded versions.
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// No version was recorded.
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    /// Some version held `work` exactly once: as itself, or as one typing cut
    /// of it (a strict prefix of at least [`STALE_PREFIX_MIN_CHARS`]) the
    /// operator was still typing, and no other row of that lineage. The row
    /// was one row; the doubling came later.
    fn held_once(&self, work: &str) -> bool {
        self.versions.iter().any(|rows| {
            rows.iter()
                .filter(|(row, _)| {
                    row.as_str() == work
                        || (row.chars().count() >= STALE_PREFIX_MIN_CHARS
                            && work.len() > row.len()
                            && work.starts_with(row.as_str()))
                })
                .map(|(_, count)| *count)
                .sum::<usize>()
                == 1
        })
    }

    /// The operator typed on from `copy` toward `newer`: some version held
    /// `copy`, and a later version held a longer cut on the way to `newer`
    /// (`copy` ⊊ cut ⊑ `newer`) with `copy` gone. Holding `copy` before
    /// `newer` existed is not enough: a row the operator kept beside a
    /// longer one added later was never replaced by it.
    fn typed_on(&self, copy: &str, newer: &str) -> bool {
        let Some(first) = self
            .versions
            .iter()
            .position(|rows| rows.contains_key(copy))
        else {
            return false;
        };
        self.versions[first + 1..].iter().any(|rows| {
            !rows.contains_key(copy)
                && rows.keys().any(|cut| {
                    cut.len() > copy.len()
                        && cut.starts_with(copy)
                        && newer.starts_with(cut.as_str())
                })
        })
    }
}

/// A bare `---` line: a thematic break or a prompt-fence line.
fn is_dash_rule_line(line: &str) -> bool {
    line.trim() == "---"
}

struct Row {
    start: usize,
    end: usize,
    indent: usize,
    parent: Option<usize>,
    work: String,
    /// The row's queue item text (list marker and `### ` removed), for the
    /// claim check.
    item: String,
    /// A free-text row (see the module docs); only these are compared.
    free_text: bool,
    /// Carries a `⟦effect:N⟧` duplicate ordinal: a copy, not the original.
    effect_ordinal: bool,
    has_children: bool,
    /// A bare `---` line.
    dash_rule: bool,
    /// A list row (`- `), not a bare line.
    list: bool,
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
        let dash_rule = !list && is_dash_rule_line(trimmed);
        let bare_markup = !list
            && (dash_rule
                || ["<!--", "%%", ">", "`", "#", "|", "~~~"]
                    .iter()
                    .any(|prefix| trimmed.starts_with(prefix)));
        while stack.last().is_some_and(|&top| rows[top].indent >= indent) {
            stack.pop();
        }
        let parent = if list { stack.last().copied() } else { None };
        if let Some(parent) = parent {
            rows[parent].has_children = true;
        }
        let work = queue_line_work(trimmed);
        let item = if list { &trimmed[2..] } else { trimmed };
        let item = item.strip_prefix("### ").unwrap_or(item);
        let claim = agent_doc_document::queue_projection::owner_projection(item).is_some();
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
            item: item.to_string(),
            free_text,
            effect_ordinal: trimmed.contains(" ⟦effect:"),
            has_children: false,
            dash_rule,
            list,
        });
        if list {
            stack.push(rows.len() - 1);
        }
    }
    rows
}

/// The byte range a removed row takes with it: a bare row that a queue
/// rewrite fenced as a `---` prompt (`---`, the row, `---`, adjacent) takes
/// both fence lines, so no orphan `---` pair is left to parse as an empty
/// prompt fence or a setext underline.
fn removal_range(rows: &[Row], index: usize) -> (usize, usize) {
    let row = &rows[index];
    if !row.list
        && index > 0
        && index + 1 < rows.len()
        && rows[index - 1].dash_rule
        && rows[index - 1].end == row.start
        && rows[index + 1].dash_rule
        && rows[index + 1].start == row.end
    {
        return (rows[index - 1].start, rows[index + 1].end);
    }
    (row.start, row.end)
}

/// `item`'s generated `[#q-…]` id occurs in `body` outside `own` (the row's
/// own bytes): another queue row refers to it.
pub(crate) fn generated_id_referenced_elsewhere(
    body: &str,
    item: &str,
    own: std::ops::Range<usize>,
) -> bool {
    let Some(id) = agent_doc_element_queue::generated_queue_prompt_id(item) else {
        return false;
    };
    let lower = body.to_ascii_lowercase();
    lower
        .match_indices(&format!("#{id}"))
        .any(|(at, _)| !own.contains(&at))
}

/// What [`drop_stale_typing_copies_with`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaleTypingOutcome {
    /// The document with every proven copy removed.
    pub document: String,
    /// One [`SelfRepairKind::StaleTypingCopy`] per removed copy.
    pub repairs: Vec<SelfRepair>,
    /// Work text of every candidate copy kept for want of recorded proof
    /// (one entry per row), so the caller can say why nothing was removed.
    pub unproven: Vec<String>,
}

/// Remove proven stale copies of the operator's free-text rows from every
/// queue component (see the module docs). `history` is called at most once,
/// and only when some row has a candidate copy.
pub fn drop_stale_typing_copies(
    document: &str,
    history: &mut dyn FnMut() -> TypingHistory,
) -> (String, Vec<SelfRepair>) {
    let outcome = drop_stale_typing_copies_with(document, &mut |_| history(), &|_| false);
    (outcome.document, outcome.repairs)
}

/// One candidate copy: a row with an exact duplicate it is not the keeper
/// of, or a newer row it is a prefix of.
struct Candidate {
    /// Byte offset of the queue body in the document.
    base: usize,
    start: usize,
    end: usize,
    work: String,
    duplicate: bool,
    newer: Vec<String>,
}

/// [`drop_stale_typing_copies`] that never removes a row `is_claimed` holds
/// (its queue item text: a worker's row, which a ledger claim points at),
/// and reports the candidates it kept without proof. `history` receives the
/// candidates' work texts (see [`TypingHistory::from_relevant_documents`]);
/// it is called at most once, and only when some row is a candidate.
pub fn drop_stale_typing_copies_with(
    document: &str,
    history: &mut dyn FnMut(&[String]) -> TypingHistory,
    is_claimed: &dyn Fn(&str) -> bool,
) -> StaleTypingOutcome {
    let unchanged = || StaleTypingOutcome {
        document: document.to_string(),
        ..StaleTypingOutcome::default()
    };
    let Ok(components) = element::parse(document) else {
        return unchanged();
    };
    let mut candidates: Vec<Candidate> = Vec::new();
    for queue in components.iter().filter(|c| c.name == "queue") {
        let body = queue.content(document);
        let rows = rows_of(body);
        let parent_work = |row: &Row| row.parent.map(|parent| rows[parent].work.as_str());
        let same_place =
            |a: &Row, b: &Row| a.indent == b.indent && parent_work(a) == parent_work(b);
        // A claimed copy, and one whose generated id another queue row points
        // at (a reply, follow-up or relation), are never removed: nothing may
        // point at a removed row. Either is the keeper of its duplicates.
        let held: Vec<bool> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let (start, end) = removal_range(&rows, index);
                row.free_text
                    && (is_claimed(&row.item)
                        || generated_id_referenced_elsewhere(body, &row.item, start..end))
            })
            .collect();
        for (index, row) in rows.iter().enumerate() {
            if !row.free_text || row.has_children || held[index] {
                continue;
            }
            // Exact duplicates: keep a held copy, else a copy with
            // children, else the last copy without a duplicate ordinal, else
            // the last copy.
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
                .find(|&&at| held[at])
                .or_else(|| copies.iter().rev().find(|&&at| rows[at].has_children))
                .or_else(|| copies.iter().rev().find(|&&at| !rows[at].effect_ordinal))
                .or_else(|| copies.last())
                .copied();
            // Only prose: a repeated bare link or `#tag` is the operator's
            // own list structure.
            let duplicate = copies.len() > 1 && keeper != Some(index) && row.work.contains(' ');
            let newer: Vec<String> = rows
                .iter()
                .filter(|other| {
                    other.free_text
                        && same_place(row, other)
                        && other.work.len() > row.work.len()
                        && row.work.chars().count() >= STALE_PREFIX_MIN_CHARS
                        && other.work.starts_with(row.work.as_str())
                })
                .map(|other| other.work.clone())
                .collect();
            if !duplicate && newer.is_empty() {
                continue;
            }
            let (start, end) = removal_range(&rows, index);
            candidates.push(Candidate {
                base: queue.open_end,
                start,
                end,
                work: row.work.clone(),
                duplicate,
                newer,
            });
        }
    }
    if candidates.is_empty() {
        return unchanged();
    }
    let works: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate.work.clone())
        .collect();
    let history = history(&works);
    let mut repairs = Vec::new();
    let mut unproven = Vec::new();
    let mut edits: Vec<(usize, usize)> = Vec::new();
    for candidate in candidates {
        let proven = (candidate.duplicate && history.held_once(&candidate.work))
            || candidate
                .newer
                .iter()
                .any(|newer| history.typed_on(&candidate.work, newer));
        if !proven {
            unproven.push(candidate.work);
            continue;
        }
        edits.push((
            candidate.base + candidate.start,
            candidate.base + candidate.end,
        ));
        repairs.push(SelfRepair {
            kind: SelfRepairKind::StaleTypingCopy,
            detail: candidate.work,
            rewritten: None,
        });
    }
    // Two adjacent fenced copies can share a fence line: coalesce.
    edits.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in edits {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let mut out = document.to_string();
    for (start, end) in merged.into_iter().rev() {
        out.replace_range(start..end, "");
    }
    StaleTypingOutcome {
        document: out,
        repairs,
        unproven,
    }
}

#[cfg(test)]
#[path = "stale_typing_copies_tests.rs"]
mod tests;

//! `#selfrepaircleanup`: agent-doc repairs its own document artifacts
//! instead of asking the operator to.
//!
//! Some agent-doc bugs left artifacts in a session document's queue: extra
//! struck copies of one row, a response tree copied under a second row, and
//! a row's own claim label (`[#subagent:x#1]`) written into its text and then
//! quoted by every follow-up title. Workers reported each one as an
//! `operator: delete …` / `operator: clean …` action item, so the queue grew
//! `🙋` rows asking the operator to tidy up after agent-doc. The repairs are
//! mechanical, so the binary does them itself on every sync (preflight and
//! `queue sync`), with transforms that are provably safe:
//!
//! - **Duplicate struck rows.** Sibling rows (same parent, same indent) with
//!   byte-identical struck text (`~~…~~`) are one row. Every copy whose
//!   subtree is empty, or byte-identical (relative to its own indent) to a
//!   kept copy's, is removed; the first copy holding each distinct subtree
//!   stays, else the first copy. A copy with children no other copy has is
//!   never removed, and neither is a live row or a row that merely resembles
//!   another.
//! - **Duplicate response trees.** A `**Response [#r-…]**` tree whose id
//!   already owns a tree elsewhere in the queue collapses through
//!   [`crate::queue_consume::canonicalize_duplicate_queue_response_projections`]
//!   (`#ackrespdup`), which keeps operator descendants byte-for-byte.
//! - **Misplaced response trees.** A `**Response [#r-…]**` tree hanging
//!   directly under a generated response-structure row (`**Guidance:**`,
//!   `**State:**`, a response-detail row; none of them ever holds a
//!   response) is a copy a torn write put there. When its id's original
//!   lives in `agent:exchange` or the response index, its generated rows are
//!   removed; operator descendants (an answer row, a row with its own
//!   response) stay byte-for-byte under an inert `**Closed by:**` backlink
//!   ([`crate::queue_consume::retire_misplaced_queue_response_projections`]).
//! - **Stale claim labels.** An exact-attempt owner annotation
//!   (`[#subagent:x#1]`, with a lifecycle state word written before it such
//!   as `running`) inside a struck row, or in the text of a live row that
//!   carries a stable id (`[#q-…]`, `[#slug]`) and no attempt owner (a
//!   follow-up title quoting the labelled row), is removed once the claim
//!   ledger no longer holds that attempt. A live row's leading projection
//!   belongs to the lifecycle repaint; an id-less live row's text is its work
//!   identity, and a row a worker holds keeps its text, so neither is
//!   touched; a label inside inline code is evidence prose and is never
//!   touched; an attempt the ledger still holds keeps its label.
//!
//! Generated response structure (`**Label:**` rows and response-detail rows)
//! is never rewritten by the label pass. Every repair is returned so the
//! caller logs it in ops.log.
//!
//! [`is_self_repair_cleanup_item`] recognizes a hand-back action item that
//! asks the operator to clean such an artifact; `queue report-complete` runs
//! the repair instead of queuing it as a `🙋` row, and the response's
//! Remaining list treats it as resolved.

use std::collections::HashMap;

use agent_doc_element::element;
use agent_doc_markdown_ast::overlay::RESPONSE_DETAIL_MARKER;

/// What a repair did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfRepairKind {
    /// Removed an extra struck copy of a row.
    DuplicateStruckRow,
    /// Collapsed a response tree whose id already owns a tree.
    DuplicateResponseTree,
    /// Retired a response tree copied under a response-structure row whose
    /// original lives in `agent:exchange` or the response index.
    MisplacedResponseTree,
    /// Removed a retired attempt's owner label from a row's text.
    StaleClaimLabel,
}

impl SelfRepairKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateStruckRow => "duplicate_struck_row",
            Self::DuplicateResponseTree => "duplicate_response_tree",
            Self::MisplacedResponseTree => "misplaced_response_tree",
            Self::StaleClaimLabel => "stale_claim_label",
        }
    }
}

/// One repair, for ops.log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfRepair {
    pub kind: SelfRepairKind,
    /// The row text (or label) the repair removed or rewrote.
    pub detail: String,
}

impl SelfRepair {
    /// One ops.log token list: `kind=… detail="…"`.
    pub fn log_fields(&self) -> String {
        let detail: String = self
            .detail
            .chars()
            .take(160)
            .map(|c| if c == '"' || c == '\n' { '\'' } else { c })
            .collect();
        format!("kind={} detail=\"{detail}\"", self.kind.as_str())
    }
}

struct Line<'a> {
    start: usize,
    end: usize,
    indent: usize,
    /// List item content after `- `; `None` for a non-list line.
    item: Option<&'a str>,
    blank: bool,
}

fn lines_of(body: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut cursor = 0;
    for raw in body.split_inclusive('\n') {
        let start = cursor;
        cursor += raw.len();
        let text = raw.trim_end_matches(['\n', '\r']);
        let trimmed = text.trim_start_matches([' ', '\t']);
        let indent = text.len() - trimmed.len();
        let item = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
            .map(str::trim_end);
        out.push(Line {
            start,
            end: cursor,
            indent,
            item,
            blank: trimmed.is_empty(),
        });
    }
    out
}

/// Index one past the last line of the subtree under `index`: following
/// lines indented deeper, up to the first blank or shallower line.
fn subtree_end(lines: &[Line<'_>], index: usize) -> usize {
    let indent = lines[index].indent;
    (index + 1..lines.len())
        .find(|at| lines[*at].blank || lines[*at].indent <= indent)
        .unwrap_or(lines.len())
}

fn parents(lines: &[Line<'_>]) -> Vec<Option<usize>> {
    let mut parents = vec![None; lines.len()];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.item.is_none() {
            continue;
        }
        while stack
            .last()
            .is_some_and(|(indent, _)| *indent >= line.indent)
        {
            stack.pop();
        }
        parents[index] = stack.last().map(|(_, parent)| *parent);
        stack.push((line.indent, index));
    }
    parents
}

/// A struck row's text: `~~…~~` after an optional `### ` heading prefix.
fn is_struck_row(item: &str) -> bool {
    let item = item.strip_prefix("### ").unwrap_or(item);
    item.starts_with("~~") && item.len() > 4 && item[2..].contains("~~")
}

/// Apply `edit` to every queue component body of `document`.
fn edit_queue_bodies(
    document: &str,
    mut edit: impl FnMut(&str, &mut Vec<SelfRepair>) -> String,
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
        let repaired = edit(body, &mut repairs);
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

fn dedupe_struck_rows_in_body(body: &str, repairs: &mut Vec<SelfRepair>) -> String {
    let lines = lines_of(body);
    let parents = parents(&lines);
    // (parent, indent, exact row text) -> copies in document order.
    let mut groups: HashMap<(Option<usize>, usize, &str), Vec<usize>> = HashMap::new();
    let mut order: Vec<(Option<usize>, usize, &str)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(item) = line.item else {
            continue;
        };
        if !is_struck_row(item) {
            continue;
        }
        let key = (parents[index], line.indent, item);
        let copies = groups.entry(key).or_default();
        if copies.is_empty() {
            order.push(key);
        }
        copies.push(index);
    }
    let subtree_text = |index: usize| -> String {
        let end = subtree_end(&lines, index);
        let indent = lines[index].indent;
        (index + 1..end)
            .map(|at| {
                let raw = &body[lines[at].start..lines[at].end];
                raw.get(indent..).unwrap_or(raw).to_string()
            })
            .collect()
    };
    let mut removed: Vec<(usize, usize)> = Vec::new();
    for key in order {
        let copies = &groups[&key];
        if copies.len() < 2 {
            continue;
        }
        let subtrees = copies
            .iter()
            .map(|index| subtree_text(*index))
            .collect::<Vec<_>>();
        let mut kept_subtrees: Vec<&str> = Vec::new();
        let mut keep = vec![false; copies.len()];
        for (at, subtree) in subtrees.iter().enumerate() {
            if !subtree.is_empty() && !kept_subtrees.contains(&subtree.as_str()) {
                kept_subtrees.push(subtree);
                keep[at] = true;
            }
        }
        if kept_subtrees.is_empty() {
            keep[0] = true;
        }
        for (at, index) in copies.iter().enumerate() {
            if keep[at] {
                continue;
            }
            let start = lines[*index].start;
            let end_line = subtree_end(&lines, *index);
            let end = lines
                .get(end_line.saturating_sub(1))
                .map_or(body.len(), |line| line.end);
            if removed
                .iter()
                .any(|(outer_start, outer_end)| *outer_start <= start && end <= *outer_end)
            {
                continue;
            }
            removed.push((start, end));
            repairs.push(SelfRepair {
                kind: SelfRepairKind::DuplicateStruckRow,
                detail: key.2.to_string(),
            });
        }
    }
    if removed.is_empty() {
        return body.to_string();
    }
    removed.sort_unstable();
    let mut out = body.to_string();
    for (start, end) in removed.into_iter().rev() {
        out.replace_range(start..end, "");
    }
    out
}

/// Remove extra struck copies of a row (see the module docs).
pub fn dedupe_struck_rows(document: &str) -> (String, Vec<SelfRepair>) {
    edit_queue_bodies(document, dedupe_struck_rows_in_body)
}

/// Byte ranges of `text` outside inline code spans.
fn outside_code(text: &str) -> Vec<bool> {
    let mut in_code = false;
    let mut out = vec![false; text.len() + 1];
    for (at, ch) in text.char_indices() {
        if ch == '`' {
            in_code = !in_code;
            continue;
        }
        for offset in 0..ch.len_utf8() {
            out[at + offset] = !in_code;
        }
    }
    out[text.len()] = true;
    out
}

/// `(start, end, owner, generation)` of every owner annotation in `text`
/// outside inline code, from byte `from` on.
fn owner_labels(text: &str, from: usize) -> Vec<(usize, usize, String, u64)> {
    let outside = outside_code(text);
    let mut out = Vec::new();
    let mut cursor = from;
    while let Some(found) = text[cursor..].find("[#") {
        let at = cursor + found;
        cursor = at + 2;
        if !outside[at] {
            continue;
        }
        let Some(close) = text[at..].find(']') else {
            break;
        };
        let payload = &text[at + 2..at + close];
        let Some((owner, generation)) = payload.rsplit_once('#') else {
            continue;
        };
        if !owner.contains(':')
            || owner.starts_with(':')
            || owner.chars().any(|c| c.is_whitespace() || c == '[')
        {
            continue;
        }
        let Ok(generation) = generation.parse::<u64>() else {
            continue;
        };
        out.push((at, at + close + 1, owner.to_string(), generation));
        cursor = at + close + 1;
    }
    out
}

/// Byte offset in a live row's item where its leading lifecycle/owner
/// projection ends (the repaint owns everything before it).
fn projection_end(item: &str) -> usize {
    let text = item.strip_prefix("### ").unwrap_or(item);
    let heading = item.len() - text.len();
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    heading + (text.len() - work.len())
}

fn strip_stale_labels_in_body(
    body: &str,
    repairs: &mut Vec<SelfRepair>,
    is_live: &dyn Fn(&str, u64) -> bool,
) -> String {
    let lines = lines_of(body);
    let mut edits: Vec<(usize, usize)> = Vec::new();
    for line in &lines {
        let Some(item) = line.item else {
            continue;
        };
        if item.starts_with("**") || item.contains(RESPONSE_DETAIL_MARKER) {
            continue;
        }
        let item_start = body[line.start..line.end]
            .find(item)
            .map_or(line.start, |offset| line.start + offset);
        let from = if is_struck_row(item) {
            0
        } else {
            // A live row's text is its work identity unless it carries a
            // stable id; an id-less row, or one a worker holds, keeps its
            // text so no claim or lineage record loses its row.
            let heading = item.strip_prefix("### ").unwrap_or(item);
            let work = agent_doc_element_queue::strip_attempt_projection_prefix(heading);
            if agent_doc_element_queue::queue_prompt_stable_id(work).is_none()
                || agent_doc_document::queue_projection::owner_projection(heading).is_some()
            {
                continue;
            }
            projection_end(item)
        };
        for (start, end, owner, generation) in owner_labels(item, from) {
            if is_live(&owner, generation) {
                continue;
            }
            // A lifecycle state word written before the label (`running
            // [#subagent:x#1]`) is part of the same torn token.
            let before = item[..start].trim_end_matches(' ');
            let word_start = before
                .rfind(|c: char| c.is_whitespace() || matches!(c, '“' | '\'' | '"' | '~' | '('))
                .map_or(0, |at| {
                    at + before[at..].chars().next().map_or(1, char::len_utf8)
                });
            let word = &before[word_start..];
            let mut cut_start = start;
            if !word.is_empty()
                && word_start >= from
                && agent_doc_frontmatter::frontmatter::lifecycle_word_before_owner_annotation(
                    &format!("{word} {}", &item[start..end]),
                )
                .is_some()
            {
                cut_start = word_start;
            }
            // Take the following space too, or else a preceding one.
            let mut cut_end = end;
            if item[cut_end..].starts_with(' ') {
                cut_end += 1;
            } else if cut_start > 0 && item[..cut_start].ends_with(' ') {
                cut_start -= 1;
            }
            edits.push((item_start + cut_start, item_start + cut_end));
            repairs.push(SelfRepair {
                kind: SelfRepairKind::StaleClaimLabel,
                detail: format!("[#{owner}#{generation}] in {item}"),
            });
        }
    }
    if edits.is_empty() {
        return body.to_string();
    }
    let mut out = body.to_string();
    for (start, end) in edits.into_iter().rev() {
        out.replace_range(start..end, "");
    }
    out
}

/// Remove retired attempts' owner labels from row text (see the module
/// docs). `is_live(owner, generation)` says whether the claim ledger still
/// holds that attempt.
pub fn strip_stale_claim_labels(
    document: &str,
    is_live: &dyn Fn(&str, u64) -> bool,
) -> (String, Vec<SelfRepair>) {
    edit_queue_bodies(document, |body, repairs| {
        strip_stale_labels_in_body(body, repairs, is_live)
    })
}

/// Run every self-repair over `document`: misplaced response trees
/// (`response_proven(id)` says the `[#r-…]` id's original lives in
/// `agent:exchange` or the response index), duplicate response trees, stale
/// claim labels, then duplicate struck rows (a label removal can make two
/// struck copies identical). `None` when nothing changed; a second run is a
/// no-op.
pub fn self_repair_document(
    document: &str,
    is_live: &dyn Fn(&str, u64) -> bool,
    response_proven: &dyn Fn(&str) -> bool,
) -> Option<(String, Vec<SelfRepair>)> {
    let mut repairs = Vec::new();
    let mut current = document.to_string();
    if let Ok((retired, ids)) =
        crate::queue_consume::retire_misplaced_queue_response_projections(&current, response_proven)
        && !ids.is_empty()
    {
        repairs.extend(ids.into_iter().map(|id| SelfRepair {
            kind: SelfRepairKind::MisplacedResponseTree,
            detail: id,
        }));
        current = retired;
    }
    if let Ok(duplicates) = crate::queue_consume::duplicate_queue_response_projection_ids(&current)
        && !duplicates.is_empty()
        && let Ok(collapsed) =
            crate::queue_consume::canonicalize_duplicate_queue_response_projections(&current)
        && collapsed != current
    {
        repairs.extend(duplicates.into_iter().map(|id| SelfRepair {
            kind: SelfRepairKind::DuplicateResponseTree,
            detail: id,
        }));
        current = collapsed;
    }
    let (labelled, label_repairs) = strip_stale_claim_labels(&current, is_live);
    current = labelled;
    repairs.extend(label_repairs);
    let (deduped, dedupe_repairs) = dedupe_struck_rows(&current);
    current = deduped;
    repairs.extend(dedupe_repairs);
    (current != document).then_some((current, repairs))
}

/// Whether a hand-back action item asks the operator to clean up one of
/// agent-doc's own document artifacts: an operator-owned item whose action
/// is to delete, remove, clean, collapse or deduplicate, and that names a
/// duplicate or misplaced copy, a doubled row, or a claim label, in the
/// session document (a row, copy, line, block, response, or title). An item
/// about anything else (a token, a branch, a worktree, a release) is not one.
pub fn is_self_repair_cleanup_item(item: &str) -> bool {
    use crate::followup_owner::{ActionItemOwner, classify_action_item};
    if classify_action_item(item).0 != ActionItemOwner::Human {
        return false;
    }
    let lower = item.to_ascii_lowercase();
    let words = |list: &[&str]| list.iter().any(|word| lower.contains(word));
    let acts = words(&[
        "delete",
        "remove",
        "clean",
        "collapse",
        "dedupe",
        "deduplicate",
        "strip ",
    ]);
    let artifact = words(&[
        "duplicate",
        "doubled",
        "extra struck",
        "extra cop",
        "struck cop",
        "misplaced cop",
        "stray cop",
        "[#subagent:",
        "[#worker:",
        "claim label",
        "quoting the label",
        "quote the label",
    ]);
    let in_document = words(&[
        " row", " cop", " line", " block", "response", " title", "~~", " tree",
    ]);
    acts && artifact && in_document
}

/// `#treefinish`: whether none of the artifacts a self-repair cleanup item
/// ([`is_self_repair_cleanup_item`]) asks about is left in the queue, so
/// the item is already done. Conservative: every artifact kind the repairs
/// handle is checked, not only the one the item names.
///
/// - no response tree hangs under a response-structure row (misplaced);
/// - no `[#r-…]` id owns two response trees in the queue, and every
///   `[#r-…]` id the item names owns at most one;
/// - no two sibling rows carry byte-identical struck text, whatever their
///   subtrees (self-repair keeps copies whose subtrees differ, and those
///   still need a person);
/// - when the item is about a claim label, no row carries one the label
///   repair would remove once its claim retires.
pub fn self_repair_artifacts_absent(document: &str, item: &str) -> bool {
    let Ok((_, misplaced)) =
        crate::queue_consume::retire_misplaced_queue_response_projections(document, &|_| true)
    else {
        return false;
    };
    if !misplaced.is_empty() {
        return false;
    }
    let Ok(duplicates) = crate::queue_consume::duplicate_queue_response_projection_ids(document)
    else {
        return false;
    };
    if !duplicates.is_empty() {
        return false;
    }
    let Ok(components) = element::parse(document) else {
        return false;
    };
    let queues = components
        .iter()
        .filter(|component| component.name == "queue")
        .map(|component| component.content(document))
        .collect::<Vec<_>>();
    for id in response_ids_named(item) {
        let header = format!("**Response [#{id}]**");
        let held = queues
            .iter()
            .map(|body| {
                body.lines()
                    .filter(|line| {
                        line.trim_start()
                            .trim_start_matches("- ")
                            .starts_with(&header)
                    })
                    .count()
            })
            .sum::<usize>();
        if held > 1 {
            return false;
        }
    }
    let lower = item.to_ascii_lowercase();
    let about_label = ["[#subagent:", "[#worker:", "claim label", "the label"]
        .iter()
        .any(|word| lower.contains(word));
    if about_label
        && !strip_stale_claim_labels(document, &|_, _| false)
            .1
            .is_empty()
    {
        return false;
    }
    queues.iter().all(|body| !struck_duplicate_siblings(body))
}

/// `r-…` ids an item names (`[#r-abc123]`, `#r-abc123`), lowercase.
fn response_ids_named(item: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = item;
    while let Some(at) = rest.find("#r-") {
        let tail = &rest[at + 1..];
        let hex = tail[2..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect::<String>();
        if hex.len() >= 8 {
            let id = format!("r-{}", hex.to_ascii_lowercase());
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        rest = &tail[2..];
    }
    ids
}

/// Any two sibling rows (same parent, same indent) with byte-identical
/// struck text.
fn struck_duplicate_siblings(body: &str) -> bool {
    let lines = lines_of(body);
    let parents = parents(&lines);
    let mut seen = std::collections::HashSet::new();
    lines.iter().enumerate().any(|(index, line)| {
        line.item.is_some_and(|item| {
            is_struck_row(item) && !seen.insert((parents[index], line.indent, item))
        })
    })
}

#[cfg(test)]
#[path = "self_repair_tests.rs"]
mod tests;

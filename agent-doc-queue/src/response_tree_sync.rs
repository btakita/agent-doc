//! A struck row's response tree stays short and in sync with its queue tree
//! (`#guidancescope`, `#handbackrowstructure`).
//!
//! Two things made a finished nested row's response tree mislead the
//! operator:
//!
//! - **Stale, parent-blind guidance.** A struck row's `**Guidance:**` was
//!   rendered once, when the row was struck, from the queue tree of that
//!   moment. Later the operator deleted a sibling, struck another row, or an
//!   agent placed follow-up rows, and the Guidance still named rows that were
//!   gone or closed. A finished second-level row also led with "Safe to delete
//!   this completed row" while its root item was still open, so it read as if
//!   the whole item were done.
//!   [`crate::queue_consume::completed_row_deletion_guidance`] now says first
//!   that the row is done but its root item is still open (and how many rows
//!   keep it open), and [`reconcile_tree_scoped_guidance`] re-renders every
//!   struck row's generated deletion guidance from the current tree. Preflight
//!   queue maintenance, `queue sync`, and every strike run it, so the
//!   guidance follows the tree as it changes.
//! - **Verbose hand-back copies.** A strike copied the whole worker hand-back
//!   (What changed, Verification, Notes, Next actions) under the struck row,
//!   often dozens of lines per row. The row now carries only the salient part
//!   (Summary lead, State, Guidance, the action items that live only there)
//!   and a `**Details:**` row naming the sections that stay in
//!   `agent:exchange`. [`slim_handback_response_details`] slims trees written
//!   before this rule, only when the hand-back's completion cell is still in
//!   the document and only for sections whose bytes are exactly what the
//!   strike rendered from that hand-back, so operator text is never removed.
//!
//! Both passes are idempotent and touch only binary-owned response rows of
//! struck queue rows.

use agent_doc_document::worker_handback::SectionKind;
use agent_doc_markdown_ast::overlay::ItemSurface;

use crate::followup_moved_state::{FOLLOW_UP_MOVED_GUIDANCE_LEAD, moved_followup_guidance};
use crate::queue_consume::{RESPONSE_STATE_COMPLETE, completed_row_deletion_guidance};

/// The response-structure label of the row that points at the hand-back
/// sections left in `agent:exchange`.
pub const DETAILS_LABEL: &str = "**Details:**";

/// The longest Summary lead a slim tree carries before it is cut at a
/// sentence end.
const SUMMARY_LEAD_LIMIT: usize = 400;

/// The generated deletion-guidance openings
/// ([`completed_row_deletion_guidance`]), current and earlier wordings.
const GENERATED_GUIDANCE_OPENINGS: [&str; 6] = [
    "Safe to delete this completed row; ",
    "Safe to delete this row and its ",
    "Safe to delete the whole tree under ",
    "More work remains beneath this row. This item is complete, but keep this row and its ",
    "This row is complete; the item is complete and has no remaining tracked or unpublished work.",
    "This row is done, but its root item ",
];

/// The text a `**Details:**` row carries for `owner`'s hand-back; retiring
/// that hand-back's completion cell keeps the cell as a record while a queue
/// row still shows this token
/// ([`crate::queue_consume::retire_consumed_completion_feedback`]).
pub fn handback_details_token(owner: &str) -> String {
    format!(
        "`{}` hand-back in agent:exchange",
        owner.replace('`', "'").replace('—', "-")
    )
}

/// The `**Details:**` value naming the hand-back sections a slim tree leaves
/// in `agent:exchange`, and the markdown files What changed names (a created
/// plan file stays visible on the row). `None` when nothing was left out.
pub(crate) fn details_value(
    owner: &str,
    summary_trimmed: bool,
    omitted: &[&str],
    what_changed: &str,
) -> Option<String> {
    let mut sections = Vec::new();
    if summary_trimmed {
        sections.push("The full Summary");
    }
    sections.extend_from_slice(omitted);
    if sections.is_empty() {
        return None;
    }
    let list = match sections.as_slice() {
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => unreachable!("sections is not empty"),
    };
    let verb = if sections.len() == 1 { "is" } else { "are" };
    let mut value = format!("{list} {verb} in the {}.", handback_details_token(owner));
    let files = markdown_files(what_changed);
    if !files.is_empty() {
        value.push_str(" What changed names ");
        value.push_str(&files.join(", "));
        value.push('.');
    }
    Some(value)
}

/// Code spans in `markdown` that name a markdown file, in order, once each.
fn markdown_files(markdown: &str) -> Vec<String> {
    let mut files = Vec::new();
    for (index, span) in markdown.split('`').enumerate() {
        let span = span.trim();
        if index % 2 == 1
            && span.ends_with(".md")
            && !span.contains(char::is_whitespace)
            && !files
                .iter()
                .any(|have: &String| have.trim_matches('`') == span)
        {
            files.push(format!("`{span}`"));
        }
    }
    files
}

/// The lead of a hand-back Summary for a slim tree: its first paragraph,
/// whitespace collapsed, cut after the last whole sentence that fits
/// [`SUMMARY_LEAD_LIMIT`]. The flag says whether anything was left out. `None`
/// when the first paragraph is a list or code, which has no lead sentence; the
/// caller then renders the Summary whole.
pub(crate) fn summary_lead(markdown: &str) -> Option<(String, bool)> {
    let mut paragraphs = markdown
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty());
    let first = paragraphs.next()?;
    let more = paragraphs.next().is_some();
    let opener = first.trim_start();
    if ["- ", "* ", "+ ", "```", "~~~", "> ", "|", "#"]
        .iter()
        .any(|lead| opener.starts_with(lead))
        || opener
            .split_once(". ")
            .is_some_and(|(digits, _)| digits.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let collapsed = first.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= SUMMARY_LEAD_LIMIT {
        return Some((collapsed, more));
    }
    match last_sentence_end_within(&collapsed, SUMMARY_LEAD_LIMIT) {
        Some(end) => Some((collapsed[..end].to_string(), true)),
        None => Some((collapsed, more)),
    }
}

/// Byte offset just past the last sentence end (`.`, `?`, `!` followed by a
/// space and a capital, digit, `*` or backtick) within the first `limit`
/// characters, outside code spans.
fn last_sentence_end_within(text: &str, limit: usize) -> Option<usize> {
    let mut in_code = false;
    let mut best = None;
    let mut count = 0;
    for (at, ch) in text.char_indices() {
        count += 1;
        if count > limit {
            break;
        }
        if ch == '`' {
            in_code = !in_code;
            continue;
        }
        if in_code || !matches!(ch, '.' | '?' | '!') {
            continue;
        }
        let rest = &text[at + ch.len_utf8()..];
        let Some(after) = rest.strip_prefix(' ') else {
            continue;
        };
        if after
            .chars()
            .next()
            .is_some_and(|next| next.is_uppercase() || next.is_ascii_digit() || "*`".contains(next))
        {
            best = Some(at + ch.len_utf8());
        }
    }
    best
}

/// Whether a Guidance value is one the binary rendered as completed-row
/// deletion guidance; `Some(true)` when it leads with the moved-action-items
/// sentence ([`moved_followup_guidance`]).
fn generated_completed_guidance(value: &str) -> Option<bool> {
    let (moved, rest) = match value.strip_prefix(FOLLOW_UP_MOVED_GUIDANCE_LEAD) {
        Some(rest) => (true, rest.trim_start()),
        None => (false, value),
    };
    GENERATED_GUIDANCE_OPENINGS
        .iter()
        .any(|opening| rest.starts_with(opening))
        .then_some(moved)
}

/// One document line: its start offset, its indent, and its trimmed text.
struct Line<'a> {
    start: usize,
    end: usize,
    indent: usize,
    text: &'a str,
}

fn lines_from(document: &str, from: usize) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut cursor = from;
    for raw in document[from..].split_inclusive('\n') {
        let start = cursor;
        cursor += raw.len();
        let body = raw.trim_end_matches(['\n', '\r']);
        let text = body.trim_start_matches([' ', '\t']);
        out.push(Line {
            start,
            end: cursor,
            indent: body.len() - text.len(),
            text: text.trim_end(),
        });
    }
    out
}

/// The struck row's own `**Response [#r-…]**` child: the lines beneath it
/// and the indent of its direct children. A response under a nested row
/// belongs to that row.
struct OwnResponse<'a> {
    /// Every line beneath the Response row, blank lines included.
    block: Vec<Line<'a>>,
    detail_indent: usize,
}

impl OwnResponse<'_> {
    /// The byte range and text of a direct child's value (`- <label> value`).
    fn value(&self, label: &str) -> Option<(std::ops::Range<usize>, &str)> {
        let lead = format!("- {label} ");
        self.block.iter().find_map(|line| {
            if line.indent != self.detail_indent {
                return None;
            }
            let value = line.text.strip_prefix(lead.as_str())?;
            let offset = line.start + line.indent + lead.len();
            Some((offset..offset + value.len(), value))
        })
    }

    fn has_label(&self, label: &str) -> bool {
        self.block.iter().any(|line| {
            line.indent == self.detail_indent
                && line
                    .text
                    .strip_prefix("- ")
                    .is_some_and(|text| text.starts_with(label))
        })
    }
}

fn own_response(document: &str, item_start: usize, item_end: usize) -> Option<OwnResponse<'_>> {
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
        if !line.text.is_empty() && line.indent <= item_indent {
            break;
        }
        scope.push(line);
    }
    let child_indent = scope
        .iter()
        .filter(|line| !line.text.is_empty())
        .map(|line| line.indent)
        .min()?;
    let response_at = scope.iter().position(|line| {
        line.indent == child_indent && line.text.starts_with("- **Response [#r-")
    })?;
    let response_indent = scope[response_at].indent;
    let mut block: Vec<Line<'_>> = scope
        .into_iter()
        .skip(response_at + 1)
        .take_while(|line| line.text.is_empty() || line.indent > response_indent)
        .collect();
    while block.last().is_some_and(|line| line.text.is_empty()) {
        block.pop();
    }
    let detail_indent = block
        .iter()
        .filter(|line| !line.text.is_empty())
        .map(|line| line.indent)
        .min()?;
    Some(OwnResponse {
        block,
        detail_indent,
    })
}

fn struck_rows(document: &str) -> Vec<agent_doc_markdown_ast::mutations::MutationItemNode> {
    agent_doc_markdown_ast::mutations::item_nodes_all(document, "queue")
        .unwrap_or_default()
        .into_iter()
        .filter(|node| node.item.struck && node.item.surface != ItemSurface::MultilinePrompt)
        .collect()
}

fn apply_edits(document: &str, mut edits: Vec<(std::ops::Range<usize>, String)>) -> String {
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut out = document.to_string();
    for (range, value) in edits {
        out.replace_range(range, &value);
    }
    out
}

/// Re-render the generated deletion guidance of every struck row whose own
/// response is complete, from the current queue tree (`#guidancescope`).
///
/// Only a Guidance value the binary rendered
/// ([`completed_row_deletion_guidance`], optionally led by the
/// moved-action-items sentence) under `**State:** Complete.` is rewritten, so
/// operator text and other states are never touched. Returns the updated
/// document and how many rows changed, or `None` when every guidance already
/// matches its tree. A second pass changes nothing.
pub fn reconcile_tree_scoped_guidance(document: &str) -> Option<(String, usize)> {
    let mut tree = None;
    let mut edits = Vec::new();
    for node in struck_rows(document) {
        let Some(response) = own_response(document, node.item.start_byte, node.item.end_byte)
        else {
            continue;
        };
        if response.value("**State:**").map(|(_, value)| value) != Some(RESPONSE_STATE_COMPLETE) {
            continue;
        }
        let Some((range, value)) = response.value("**Guidance:**") else {
            continue;
        };
        let Some(moved) = generated_completed_guidance(value) else {
            continue;
        };
        let tree = tree.get_or_insert_with(|| {
            crate::queue_tree::queue_tree(document, &crate::queue_tree::QueueTreeOptions::default())
                .ok()
        });
        let Some(closure) = tree.as_ref().and_then(|tree| {
            crate::queue_tree::row_closure_at_offset(tree, document, node.item.start_byte)
        }) else {
            continue;
        };
        let guidance = if moved {
            moved_followup_guidance(Some(&closure))
        } else {
            completed_row_deletion_guidance(Some(&closure))
        };
        if guidance != value {
            edits.push((range, guidance));
        }
    }
    let rows = edits.len();
    (rows > 0).then(|| (apply_edits(document, edits), rows))
}

/// Slim the copied hand-back sections out of struck rows' response trees
/// written before the slim shape (`#handbackrowstructure`).
///
/// For a struck row whose hand-back completion cell is still in the document,
/// each What changed / Verification / Notes / Next actions section whose
/// bytes are exactly what the strike rendered from that hand-back is removed,
/// and one `**Details:**` row naming them takes the first one's place. Under
/// `**State:** Complete.`, an Action items section whose items all have their
/// own follow-up rows is removed too; those rows hold the work. A section an
/// operator edited, or one under a row with no matching cell, stays. The
/// Summary is never changed: it is the projection's response evidence. A
/// second pass changes nothing.
pub fn slim_handback_response_details(document: &str) -> Option<(String, usize)> {
    const DETAIL_KINDS: [SectionKind; 4] = [
        SectionKind::WhatChanged,
        SectionKind::Verification,
        SectionKind::Notes,
        SectionKind::NextActions,
    ];
    let mut edits = Vec::new();
    let mut rows = 0;
    let mut handbacks = None;
    for node in struck_rows(document) {
        let Some(response) = own_response(document, node.item.start_byte, node.item.end_byte)
        else {
            continue;
        };
        let copied = DETAIL_KINDS
            .iter()
            .chain([&SectionKind::ActionItems])
            .any(|kind| response.has_label(&format!("**{}:**", kind.heading())));
        if !copied {
            continue;
        }
        let handbacks =
            handbacks.get_or_insert_with(|| crate::queue_consume::DocumentHandbacks::new(document));
        let Some(found) = handbacks.for_row(&node.item.text, node.item.start_byte) else {
            continue;
        };
        let complete =
            response.value("**State:**").map(|(_, value)| value) == Some(RESPONSE_STATE_COMPLETE);
        let action_items = found.handback.action_items();
        let action_items_moved = complete
            && crate::followup_moved_state::action_items_held_as_rows(
                document,
                &node.item.text,
                &action_items,
            );
        let indent = " ".repeat(response.detail_indent);
        let mut removed = Vec::new();
        let mut omitted = Vec::new();
        let mut what_changed = String::new();
        for kind in DETAIL_KINDS
            .iter()
            .copied()
            .chain(action_items_moved.then_some(SectionKind::ActionItems))
        {
            let markdown = crate::queue_consume::handback_section_markdown(&found.handback, kind);
            if markdown.is_empty() {
                continue;
            }
            let mut expected = String::new();
            crate::queue_consume::render_handback_section(&mut expected, &indent, kind, &markdown);
            let Some(range) = exact_section(&response, kind, &expected) else {
                continue;
            };
            if kind != SectionKind::ActionItems {
                omitted.push(kind.heading());
                if kind == SectionKind::WhatChanged {
                    what_changed = markdown;
                }
            }
            removed.push(range);
        }
        if removed.is_empty() {
            continue;
        }
        removed.sort_by_key(|range| range.start);
        let details = (!response.has_label(DETAILS_LABEL))
            .then(|| details_value(&found.owner, false, &omitted, &what_changed))
            .flatten();
        let action_items_removed = removed.iter().any(|range| {
            document[range.clone()]
                .trim_start()
                .starts_with("- **Action items:**")
        });
        // `#followupmovedstate`: the removed action items live in their own
        // follow-up rows; the Guidance says so, as a fresh strike's would.
        if action_items_removed
            && let Some((guidance, value)) = response.value("**Guidance:**")
            && generated_completed_guidance(value) == Some(false)
        {
            edits.push((
                guidance.start..guidance.start,
                format!("{FOLLOW_UP_MOVED_GUIDANCE_LEAD} "),
            ));
        }
        let first_detail = removed
            .iter()
            .filter(|range| {
                !document[(*range).clone()]
                    .trim_start()
                    .starts_with("- **Action items:**")
            })
            .map(|range| range.start)
            .next();
        for range in removed {
            let replacement = match (&details, first_detail) {
                (Some(details), Some(at)) if at == range.start => {
                    format!("{indent}- {DETAILS_LABEL} {details}\n")
                }
                _ => String::new(),
            };
            edits.push((range, replacement));
        }
        rows += 1;
    }
    (rows > 0).then(|| (apply_edits(document, edits), rows))
}

/// The byte range of `kind`'s section among the response's direct children,
/// when its lines are exactly `expected` (trailing whitespace aside: a strike
/// indents a blank line inside a section) and nothing more is nested under it.
fn exact_section(
    response: &OwnResponse<'_>,
    kind: SectionKind,
    expected: &str,
) -> Option<std::ops::Range<usize>> {
    let label = format!("- **{}:**", kind.heading());
    let at = response
        .block
        .iter()
        .position(|line| line.indent == response.detail_indent && line.text.starts_with(&label))?;
    let expected = expected.strip_suffix('\n').unwrap_or(expected);
    let mut end = response.block[at].start;
    let mut count = 0;
    for (offset, want) in expected.split('\n').enumerate() {
        let line = response.block.get(at + offset)?;
        let have = format!("{}{}", " ".repeat(line.indent), line.text);
        if have.trim_end() != want.trim_end() {
            return None;
        }
        end = line.end;
        count = offset + 1;
    }
    // The section ends where `expected` ends: the next non-blank line is a
    // sibling (or the end of the response), never a child left behind.
    let next = response.block[at + count..]
        .iter()
        .find(|line| !line.text.is_empty());
    if next.is_some_and(|line| line.indent > response.detail_indent) {
        return None;
    }
    Some(response.block[at].start..end)
}

/// Both passes, slim then guidance, as one successor
/// (`#guidancescope`, `#handbackrowstructure`). Returns the document, how many
/// trees were slimmed, and how many guidance values changed.
pub fn sync_response_trees(document: &str) -> Option<(String, usize, usize)> {
    let (slimmed, slim_rows) =
        slim_handback_response_details(document).unwrap_or_else(|| (document.to_string(), 0));
    let (synced, guidance_rows) = reconcile_tree_scoped_guidance(&slimmed).unwrap_or((slimmed, 0));
    (slim_rows + guidance_rows > 0).then_some((synced, slim_rows, guidance_rows))
}

#[cfg(test)]
#[path = "response_tree_sync_tests.rs"]
mod tests;

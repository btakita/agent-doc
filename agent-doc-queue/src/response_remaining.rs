//! A response tree says what is left, as a short list (`#guidancescope`).
//!
//! A worker hand-back with action items renders its row's response as
//! `**State:** Follow-up work remains.` / `**Guidance:** More work remains.
//! Keep this tree and follow the action items in this response.`, once, when
//! it is written. Nothing re-read it: after every action item was answered
//! (a struck answer row under it) or done (a struck follow-up row, a struck
//! `Do the next actions.` row), the tree still said more work remained, a
//! nested response repeated the same sentence, and the operator had to read
//! the whole tree to find out that nothing, or one thing, was left.
//!
//! [`reconcile_response_remaining`] recomputes that from the live document
//! on every sync and renders it in the response itself:
//!
//! ```text
//! - **Remaining:**
//!   - 🙋 operator: unlock the `login` keyring once <!-- agent-doc:response-detail -->
//! - **State:** Waiting on operator input.
//! - **Guidance:** 🙋 5/6 done, 1 yours. Only you can finish this: unlock the `login` keyring once. Strike or answer its row when done.
//! ```
//!
//! `#donetrack`: the Guidance opens with a tally of the tree (every action
//! item once, and every queue row, in the response's scope): `✅ N/N done.`
//! when nothing is left, `🙋 d/t done, k yours.` when only operator work is
//! left, `⏳ d/t done, k yours.` while agent work is open. Each Remaining
//! entry carries `⏳` (agent) or `🙋` (operator). With only operator work
//! left the State is `Waiting on operator input.` and the Guidance names the
//! one thing to do (or how many); with agent work left the State is
//! `Follow-up work remains.`; once nothing is left, `**State:** Complete.`
//! and the Guidance says `🗑️ Safe to delete this tree.` under a top-level
//! row (`#treefinish`: the root carries the tree's indicator), only the
//! tally under a nested row, and `🗑️ Safe to delete this response.` under a
//! response. The wordings this pass wrote earlier are still its own and are
//! rewritten.
//!
//! # What remains
//!
//! The scope of a response is its parent's subtree: the queue row it hangs
//! under (or the response row it is nested in) and everything beneath. Its
//! Remaining list is, in document order:
//!
//! - every **unresolved action item** of a response in that scope (its own
//!   and nested ones), except a response that itself says `Complete.`;
//! - every **open queue row** in that scope (the queue tree's per-row rule,
//!   [`crate::queue_tree::row_keeps_tree_alive`]: a live leaf, a row with
//!   projected state; a plain group parent is only a label, and a delegated
//!   or self-repaired row is resolved and counts as done), unless it sits
//!   under an unresolved action item or is that item's own follow-up row,
//!   which the item's entry already stands for;
//! - the parent row itself when it is live work of its own.
//!
//! An action item (a `… <!-- agent-doc:response-detail -->` entry under
//! `**Action items:**`) is **resolved** when any of these holds:
//!
//! 1. the item itself is struck, or a struck row is nested under it (the
//!    operator's answer, such as `~~keep after unlocking.~~`);
//! 2. its follow-up row (`[#<origin>] follow-up: <item>`, or the legacy
//!    `follow up on an action item from …: <item>`, any
//!    origin) is struck in a queue, or is gone from every queue but quoted as
//!    a dispatched `> ` queue prompt in `agent:exchange` (finished, then
//!    deleted). A live follow-up row keeps it open; an item that never had a
//!    row stays open;
//! 3. for an `agent:` item only, a later completion covers it: a struck row
//!    in the same response, after the item and not under any action item
//!    (the operator's `~~Do the next actions.~~`). Agent items are finished in
//!    the turn that closes such a row (`#finishactionable`);
//! 4. `#treefinish`: its follow-up row is live in another top-level tree.
//!    The work was delegated to that row, so this tree no longer holds it
//!    (a live follow-up row inside the same tree still keeps it open).
//!
//! Nothing else resolves an item: prose that says "done" does not, and an
//! operator-owned item is never covered by agent work.
//!
//! # Safeguards
//!
//! Only binary-owned values change: a State/Guidance pair in the follow-up
//! form above, or in a form this pass rendered, and a Remaining section whose
//! every entry is a generated detail row. A response whose Remaining section
//! an operator edited (an unmarked line in it) is left alone, and so is a
//! struck row's response whose action items all have follow-up rows while it
//! is still in the plain follow-up form: `#followupmovedstate` owns that
//! case. Entries are reserved response structure, never queue rows, so a
//! second pass changes nothing.

use agent_doc_markdown_ast::overlay::RESPONSE_DETAIL_MARKER;

use crate::followup_moved_state::FOLLOW_UP_ACTION_ITEMS_GUIDANCE;
use crate::followup_owner::{
    ActionItemOwner, OPERATOR_OWNED_TAG, classify_action_item, explicit_owner_label,
    followup_row_action_item,
};
use crate::queue_consume::{
    RESPONSE_STATE_COMPLETE, RESPONSE_STATE_FOLLOW_UP, RESPONSE_STATE_WAITING_ON_OPERATOR,
};
use crate::queue_tree::{QueueNodeState, QueueTree};

/// The response-structure label of the short list of what is left.
pub const REMAINING_LABEL: &str = "**Remaining:**";

/// Guidance once nothing remains under a response that hangs under a
/// top-level row (after the tally, `✅ N/N done.`): the tree's keep/delete
/// indicator (`#treefinish`). A response under a nested row says only the
/// tally; the root carries the indicator.
pub const NOTHING_REMAINS_ROW_GUIDANCE: &str = crate::queue_consume::TREE_DELETE_GUIDANCE;

/// Guidance once nothing remains in a response nested under another response
/// (after the tally).
pub const NOTHING_REMAINS_RESPONSE_GUIDANCE: &str = "🗑️ Safe to delete this response.";

/// The earlier nothing-remains wordings, still recognized as this pass's own.
const LEGACY_NOTHING_REMAINS_GUIDANCE: [&str; 2] = [
    "Nothing remains: every action item and row under this row is resolved. Safe to delete this row and everything under it.",
    "Nothing remains in this response: every action item and row in it is resolved. Safe to delete this response.",
];

/// The nothing-remains tails after the tally before `#treefinish`.
const LEGACY_NOTHING_REMAINS_TAILS: [&str; 2] = [
    "Complete. Safe to delete this tree.",
    "Complete. Safe to delete this response.",
];

/// Opens the guidance when only operator work is left.
pub const ONLY_YOU_GUIDANCE_LEAD: &str = "Only you can finish this: ";

/// The most entries a Remaining list shows; the rest are counted.
const REMAINING_LIMIT: usize = 8;

/// The longest entry text after its owner label.
const ENTRY_TEXT_LIMIT: usize = 100;

/// Status glyphs (`#donetrack`): done, agent work open, operator work open.
pub const DONE_GLYPH: &str = "✅";
pub const AGENT_OPEN_GLYPH: &str = "⏳";
pub const OPERATOR_OPEN_GLYPH: &str = "🙋";

/// `N item(s) remain, listed under Remaining. Keep this tree until …`.
pub fn remaining_guidance(count: usize) -> String {
    if count == 1 {
        "1 item remains, listed under Remaining. Keep this tree until it is done.".to_string()
    } else {
        format!("{count} items remain, listed under Remaining. Keep this tree until they are done.")
    }
}

/// `#donetrack`: what is done in a response's tree, as one leading token:
/// `✅ 11/11 done.`, `🙋 3/4 done, 1 yours.`, or `⏳ 5/9 done, 1 yours.`
/// (`yours` counts operator-owned entries still open).
pub fn tally(done: usize, total: usize, agent_open: usize, yours: usize) -> String {
    let glyph = if agent_open > 0 {
        AGENT_OPEN_GLYPH
    } else if yours > 0 {
        OPERATOR_OPEN_GLYPH
    } else {
        DONE_GLYPH
    };
    if yours > 0 {
        format!("{glyph} {done}/{total} done, {yours} yours.")
    } else {
        format!("{glyph} {done}/{total} done.")
    }
}

/// The tally a guidance opens with, and the rest of it (empty when the
/// guidance is only the tally).
fn split_tally(guidance: &str) -> Option<&str> {
    let rest = [DONE_GLYPH, AGENT_OPEN_GLYPH, OPERATOR_OPEN_GLYPH]
        .iter()
        .find_map(|glyph| guidance.strip_prefix(glyph))?
        .strip_prefix(' ')?;
    let (fraction, rest) = rest.split_once(" done")?;
    let (done, total) = fraction.split_once('/')?;
    if done.parse::<usize>().is_err() || total.parse::<usize>().is_err() {
        return None;
    }
    let rest = match rest.strip_prefix(", ") {
        Some(yours) => {
            let digits = yours.chars().take_while(char::is_ascii_digit).count();
            yours[digits..].strip_prefix(" yours.")?
        }
        None => rest.strip_prefix('.')?,
    };
    if rest.is_empty() {
        return Some(rest);
    }
    rest.strip_prefix(' ')
}

fn is_remaining_guidance(value: &str) -> bool {
    let digits = value.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && value[..digits]
            .parse::<usize>()
            .is_ok_and(|count| remaining_guidance(count) == value)
}

/// Whether a State/Guidance pair is one this pass manages.
fn managed_form(state: &str, guidance: &str) -> bool {
    if let Some(tail) = split_tally(guidance) {
        return (state == RESPONSE_STATE_FOLLOW_UP && is_remaining_guidance(tail))
            || (state == RESPONSE_STATE_WAITING_ON_OPERATOR
                && tail.starts_with(ONLY_YOU_GUIDANCE_LEAD))
            || (state == RESPONSE_STATE_COMPLETE
                && (tail.is_empty()
                    || tail == NOTHING_REMAINS_ROW_GUIDANCE
                    || tail == NOTHING_REMAINS_RESPONSE_GUIDANCE
                    || LEGACY_NOTHING_REMAINS_TAILS.contains(&tail)));
    }
    (state == RESPONSE_STATE_FOLLOW_UP
        && (guidance == FOLLOW_UP_ACTION_ITEMS_GUIDANCE || is_remaining_guidance(guidance)))
        || (state == RESPONSE_STATE_COMPLETE && LEGACY_NOTHING_REMAINS_GUIDANCE.contains(&guidance))
}

// ---------------------------------------------------------------------------
// Short readable text
// ---------------------------------------------------------------------------

/// A row's or item's text without projection and binary annotations: the
/// heading, lifecycle and `🙋` prefix, a strike wrapper, `[operator-verify]`,
/// the generated `[#q-…]` id, report-state and effect annotations, and the
/// response-detail marker. Whitespace collapsed.
pub(crate) fn plain_text(text: &str) -> String {
    let mut text = text.trim();
    text = text
        .strip_suffix(RESPONSE_DETAIL_MARKER)
        .unwrap_or(text)
        .trim();
    text = text.strip_prefix("### ").unwrap_or(text);
    text = agent_doc_element_queue::strip_attempt_projection_prefix(text).trim();
    text = text.strip_prefix("### ").unwrap_or(text);
    text = text
        .strip_prefix("~~")
        .and_then(|inner| inner.strip_suffix("~~"))
        .unwrap_or(text)
        .trim();
    let collapsed = text
        .split_whitespace()
        .filter(|token| !token.eq_ignore_ascii_case(OPERATOR_OWNED_TAG))
        .collect::<Vec<_>>()
        .join(" ");
    let stripped = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(&collapsed);
    let stripped = agent_doc_element_queue::strip_queue_report_state_annotation(stripped.trim());
    // A strike wrapper and `⟦…⟧` annotations are projection outside code; a
    // code span quoting a struck row keeps them.
    let mut out = String::with_capacity(stripped.len());
    for (index, segment) in stripped.split('`').enumerate() {
        if index > 0 {
            out.push('`');
        }
        if index % 2 == 1 {
            out.push_str(segment);
            continue;
        }
        let mut rest = segment;
        while let Some(at) = rest.find('⟦') {
            out.push_str(&rest[..at].replace("~~", ""));
            match rest[at..].find('⟧') {
                Some(end) => rest = &rest[at + end + '⟧'.len_utf8()..],
                None => {
                    out.push_str(&rest[at..].replace("~~", ""));
                    rest = "";
                }
            }
        }
        out.push_str(&rest.replace("~~", ""));
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The owner label a worker wrote (`agent`, `operator`, `human (who)`) and
/// the text after it.
fn split_owner_label(text: &str) -> Option<(String, &str)> {
    explicit_owner_label(text)?;
    let colon = text.find(':')?;
    let label = text[..colon]
        .trim_matches(|c: char| c == '*' || c == '_' || c.is_whitespace())
        .to_string();
    let rest = text[colon + 1..].trim_start_matches(['*', '_']).trim();
    let label = match label.to_ascii_lowercase().as_str() {
        "agent" | "operator" | "human" => label.to_ascii_lowercase(),
        _ => label,
    };
    Some((label, rest))
}

/// Byte offsets of `text` outside inline code spans.
fn outside_code(text: &str) -> Vec<bool> {
    let mut in_code = false;
    let mut out = vec![false; text.len() + 1];
    for (at, ch) in text.char_indices() {
        if ch == '`' {
            in_code = !in_code;
            continue;
        }
        out[at] = !in_code;
    }
    out[text.len()] = true;
    out
}

/// The first clause of `text` as a short imperative: cut at the first
/// sentence or clause break outside code (`. `, `; `, `: `, ` (`, ` — `),
/// then at a word boundary within `limit` characters (`…` marks a cut), never
/// leaving a code span open.
pub(crate) fn short_imperative(text: &str, limit: usize) -> String {
    let text = text.trim();
    let outside = outside_code(text);
    let mut cut = text.len();
    for brk in [". ", "; ", ": ", " (", " — "] {
        let mut from = 0;
        while let Some(found) = text[from..].find(brk) {
            let at = from + found;
            if outside[at] && text[..at].chars().count() >= 12 {
                cut = cut.min(at);
                break;
            }
            from = at + brk.len();
        }
    }
    let mut short = text[..cut]
        .trim_end_matches(['.', ',', ':', ';', ' '])
        .to_string();
    if short.chars().count() > limit {
        let byte_limit = short
            .char_indices()
            .nth(limit.saturating_sub(1))
            .map_or(short.len(), |(at, _)| at);
        let outside = outside_code(&short);
        let space = short[..byte_limit]
            .rmatch_indices(' ')
            .map(|(at, _)| at)
            .find(|at| outside[*at])
            .unwrap_or(byte_limit);
        short.truncate(space);
        if short.matches('`').count() % 2 == 1
            && let Some(open) = short.rfind('`')
        {
            short.truncate(open);
        }
        let trimmed = short.trim_end_matches(['.', ',', ':', ';', ' ']).len();
        short.truncate(trimmed);
        short.push('…');
    }
    short
}

/// Short readable text for a queue row's work text: a follow-up row reads as
/// its action item (`operator: decide whether to keep …`), any other row as
/// its first clause. Used wherever guidance names a row
/// ([`crate::queue_tree::RowRef::label`]).
pub fn short_row_text(work: &str, limit: usize) -> String {
    let plain = plain_text(work);
    let body = followup_row_action_item(&plain).unwrap_or(&plain);
    match split_owner_label(body) {
        Some((label, rest)) => {
            let rest_limit = limit.saturating_sub(label.chars().count() + 2).max(16);
            format!("{label}: {}", short_imperative(rest, rest_limit))
        }
        None => short_imperative(body, limit),
    }
}

// ---------------------------------------------------------------------------
// Document scan
// ---------------------------------------------------------------------------

struct Line<'a> {
    start: usize,
    end: usize,
    indent: usize,
    /// Trimmed text (no indent, no trailing whitespace).
    text: &'a str,
}

fn document_lines(document: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut cursor = 0;
    for raw in document.split_inclusive('\n') {
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

/// Line indexes `[from, to)` nested under the line at `parent` (indent
/// greater than its own), blank lines inside included, trailing blanks not.
fn subtree(lines: &[Line<'_>], parent: usize, limit: usize) -> std::ops::Range<usize> {
    let indent = lines[parent].indent;
    let mut end = parent + 1;
    let mut last = parent + 1;
    while end < limit {
        let line = &lines[end];
        if !line.text.is_empty() && line.indent <= indent {
            break;
        }
        end += 1;
        if !line.text.is_empty() {
            last = end;
        }
    }
    parent + 1..last
}

struct ActionItem {
    /// Line index of the entry.
    line: usize,
    /// Line indexes nested under the entry.
    span: std::ops::Range<usize>,
    owner: String,
    agent: bool,
    /// The item text without its owner label.
    text: String,
    /// The rendered entry, marker stripped, whitespace collapsed: the text
    /// report-complete quoted into its follow-up row.
    raw: String,
    /// Normalized key for follow-up row matching.
    key: String,
    struck: bool,
}

struct ResponseBlock {
    /// Line index of the `**Response [#r-…]**` row.
    line: usize,
    /// The response's parent line, when there is one.
    parent: Option<usize>,
    /// Line indexes beneath the Response row.
    block: std::ops::Range<usize>,
    /// The parent's subtree (the scope of what remains).
    scope: std::ops::Range<usize>,
    detail_indent: usize,
    state: Option<(std::ops::Range<usize>, String)>,
    state_line: Option<usize>,
    guidance: Option<(std::ops::Range<usize>, String)>,
    /// The existing Remaining section: its byte range, and whether every
    /// line in it is generated.
    remaining: Option<(std::ops::Range<usize>, bool)>,
    items: Vec<ActionItem>,
}

fn value_of(
    document: &str,
    line: &Line<'_>,
    label: &str,
) -> Option<(std::ops::Range<usize>, String)> {
    let lead = format!("- {label} ");
    let value = line.text.strip_prefix(lead.as_str())?;
    let offset = line.start + line.indent + lead.len();
    debug_assert_eq!(&document[offset..offset + value.len()], value);
    Some((offset..offset + value.len(), value.to_string()))
}

pub(crate) fn item_key(text: &str) -> String {
    plain_text(text)
}

fn parse_response(
    document: &str,
    lines: &[Line<'_>],
    at: usize,
    limit: usize,
) -> Option<ResponseBlock> {
    let block = subtree(lines, at, limit);
    let detail_indent = lines[block.clone()]
        .iter()
        .filter(|line| !line.text.is_empty())
        .map(|line| line.indent)
        .min()?;
    let parent = (0..at)
        .rev()
        .find(|&index| !lines[index].text.is_empty() && lines[index].indent < lines[at].indent)
        .filter(|&index| lines[index].text.starts_with("- "));
    let scope = match parent {
        Some(parent) => subtree(lines, parent, limit),
        None => block.clone(),
    };
    let mut response = ResponseBlock {
        line: at,
        parent,
        block: block.clone(),
        scope,
        detail_indent,
        state: None,
        state_line: None,
        guidance: None,
        remaining: None,
        items: Vec::new(),
    };
    let mut index = block.start;
    while index < block.end {
        let line = &lines[index];
        index += 1;
        if line.text.is_empty() || line.indent != detail_indent {
            continue;
        }
        if response.state.is_none()
            && let Some(value) = value_of(document, line, "**State:**")
        {
            response.state = Some(value);
            response.state_line = Some(index - 1);
        } else if response.guidance.is_none()
            && let Some(value) = value_of(document, line, "**Guidance:**")
        {
            response.guidance = Some(value);
        } else if line.text == format!("- {REMAINING_LABEL}") {
            let start = index - 1;
            let mut generated = true;
            let mut end = index;
            while end < block.end
                && (lines[end].text.is_empty() || lines[end].indent > detail_indent)
            {
                let entry = &lines[end];
                if !entry.text.is_empty()
                    && (entry.indent != detail_indent + 2
                        || !entry.text.starts_with("- ")
                        || !entry.text.ends_with(RESPONSE_DETAIL_MARKER))
                {
                    generated = false;
                }
                end += 1;
            }
            index = end;
            let mut last = end;
            while last > start + 1 && lines[last - 1].text.is_empty() {
                last -= 1;
            }
            response.remaining = Some((lines[start].start..lines[last - 1].end, generated));
        } else if line.text.starts_with("- **Action items:**") {
            let mut entry_indent = None;
            while index < block.end
                && (lines[index].text.is_empty() || lines[index].indent > detail_indent)
            {
                let entry = &lines[index];
                if entry.text.is_empty() {
                    index += 1;
                    continue;
                }
                let indent = *entry_indent.get_or_insert(entry.indent);
                if entry.indent == indent
                    && let Some(body) = entry.text.strip_prefix("- ")
                    && body.ends_with(RESPONSE_DETAIL_MARKER)
                {
                    let span = subtree(lines, index, block.end);
                    let struck = body.trim_start().starts_with("~~");
                    let plain = plain_text(body);
                    let (owner, text, agent) = match split_owner_label(&plain) {
                        Some((label, rest)) => {
                            let agent =
                                explicit_owner_label(&plain) == Some(ActionItemOwner::Agent);
                            (label, rest.to_string(), agent)
                        }
                        None => {
                            let agent = classify_action_item(&plain).0 == ActionItemOwner::Agent;
                            let owner = if agent { "agent" } else { "operator" };
                            (owner.to_string(), plain.clone(), agent)
                        }
                    };
                    let raw = body
                        .strip_suffix(RESPONSE_DETAIL_MARKER)
                        .unwrap_or(body)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    response.items.push(ActionItem {
                        line: index,
                        span: span.clone(),
                        owner,
                        agent,
                        text,
                        raw,
                        key: item_key(&plain),
                        struck,
                    });
                    index = span.end.max(index + 1);
                    continue;
                }
                index += 1;
            }
        }
    }
    Some(response)
}

/// Follow-up rows in the document: each one's action-item key, and whether
/// it is live in a queue, struck in a queue, or only quoted in
/// `agent:exchange`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FollowupRow {
    Live,
    Struck,
    Quoted,
}

/// Each row's action-item key, its state, and its 1-based queue line (none
/// for a quote in `agent:exchange`).
fn followup_rows(document: &str, tree: &QueueTree) -> Vec<(String, FollowupRow, Option<usize>)> {
    let mut rows = Vec::new();
    for node in &tree.nodes {
        let plain = plain_text(&node.full_text);
        if let Some(item) = followup_row_action_item(&plain) {
            let state = if node.closed {
                FollowupRow::Struck
            } else {
                FollowupRow::Live
            };
            rows.push((item_key(item), state, Some(node.line)));
        }
    }
    if let Ok(components) = agent_doc_element::element::parse(document) {
        for component in components.iter().filter(|c| c.name == "exchange") {
            for line in component.content(document).lines() {
                let Some(quoted) = line.trim().strip_prefix('>') else {
                    continue;
                };
                let quoted = quoted.trim_start();
                let quoted = quoted
                    .strip_prefix("**Queue prompt:**")
                    .unwrap_or(quoted)
                    .trim_start();
                let plain = plain_text(quoted);
                if let Some(item) = followup_row_action_item(&plain) {
                    rows.push((item_key(item), FollowupRow::Quoted, None));
                }
            }
        }
    }
    rows
}

/// One Remaining entry.
struct Entry {
    line: usize,
    owner: String,
    text: String,
}

impl Entry {
    fn render(&self, indent: usize) -> String {
        let text = short_imperative(&self.text, ENTRY_TEXT_LIMIT);
        let glyph = if self.owner == "agent" {
            AGENT_OPEN_GLYPH
        } else {
            OPERATOR_OPEN_GLYPH
        };
        format!(
            "{}- {glyph} {}: {} {RESPONSE_DETAIL_MARKER}\n",
            " ".repeat(indent),
            self.owner,
            text
        )
    }
}

fn render_remaining(entries: &[Entry], detail_indent: usize) -> String {
    let mut out = format!("{}- {REMAINING_LABEL}\n", " ".repeat(detail_indent));
    for entry in entries.iter().take(REMAINING_LIMIT) {
        out.push_str(&entry.render(detail_indent + 2));
    }
    if entries.len() > REMAINING_LIMIT {
        out.push_str(&format!(
            "{}- and {} more in this tree {RESPONSE_DETAIL_MARKER}\n",
            " ".repeat(detail_indent + 2),
            entries.len() - REMAINING_LIMIT
        ));
    }
    out
}

/// The owner and text of an open queue row as a Remaining entry.
fn open_row_entry(node: &crate::queue_tree::QueueTreeNode) -> (String, String) {
    let plain = plain_text(&node.full_text);
    let operator = node.state == QueueNodeState::WaitingOnYou
        || crate::followup_owner::carries_operator_owned_tag(&node.full_text)
        || agent_doc_document::queue_projection::operator_action_marked(&node.full_text);
    let body = followup_row_action_item(&plain).unwrap_or(&plain);
    match split_owner_label(body) {
        Some((label, rest)) => (label, rest.to_string()),
        None if operator => ("operator".to_string(), body.to_string()),
        None => ("agent".to_string(), body.to_string()),
    }
}

/// Recompute every response tree's Remaining list, State and Guidance from
/// the live document (`#guidancescope`).
///
/// Returns the updated document and how many responses changed, or `None`
/// when every managed response already says what is left. A second pass
/// changes nothing.
pub fn reconcile_response_remaining(document: &str) -> Option<(String, usize)> {
    if !document.contains("**Response [#r-") {
        return None;
    }
    let components = agent_doc_element::element::parse(document).ok()?;
    let lines = document_lines(document);
    let line_of_offset = |offset: usize| lines.partition_point(|line| line.end <= offset);
    let mut responses = Vec::new();
    for component in components.iter().filter(|c| c.name == "queue") {
        let first = line_of_offset(component.open_end);
        let limit = line_of_offset(component.close_start).min(lines.len());
        for at in first..limit {
            if lines[at].text.starts_with("- **Response [#r-")
                && let Some(response) = parse_response(document, &lines, at, limit)
            {
                responses.push(response);
            }
        }
    }
    let mut managed = responses
        .iter()
        .enumerate()
        .filter(|(_, response)| {
            matches!(
                (&response.state, &response.guidance),
                (Some((_, state)), Some((_, guidance))) if managed_form(state, guidance)
            ) && response
                .remaining
                .as_ref()
                .is_none_or(|(_, generated)| *generated)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if managed.is_empty() {
        return None;
    }
    let tree =
        crate::queue_tree::queue_tree(document, &crate::queue_tree::QueueTreeOptions::default())
            .ok()?;
    let node_at = |line: usize| tree.nodes.iter().position(|node| node.line == line + 1);
    // `#followupmovedstate` owns a struck row's plain follow-up form once
    // every action item has its own follow-up row.
    managed.retain(|&index| {
        let response = &responses[index];
        let legacy = response
            .guidance
            .as_ref()
            .is_some_and(|(_, guidance)| guidance == FOLLOW_UP_ACTION_ITEMS_GUIDANCE);
        let struck_parent = response
            .parent
            .and_then(node_at)
            .filter(|&node| tree.nodes[node].closed);
        !(legacy
            && struck_parent.is_some_and(|node| {
                let items = response
                    .items
                    .iter()
                    .map(|item| item.raw.clone())
                    .collect::<Vec<_>>();
                crate::followup_moved_state::action_items_held_as_rows(
                    document,
                    &tree.nodes[node].full_text,
                    &items,
                )
            }))
    });
    let followups = followup_rows(document, &tree);

    // Every action-item line and the rows nested under it, for rule 3.
    let in_item_span = |line: usize| {
        responses.iter().any(|response| {
            response
                .items
                .iter()
                .any(|item| item.line == line || item.span.contains(&line))
        })
    };
    // The 1-based line range of the top-level tree that holds a 0-based
    // document line (`#treefinish`).
    let tree_lines_of = |line: usize| {
        tree.nodes
            .iter()
            .find(|node| node.depth == 0 && node.line <= line + 1 && line < node.end_line)
            .map(|node| node.line..=node.end_line)
    };
    // Rule 1–4 for each item of each response.
    let resolved: Vec<Vec<bool>> = responses
        .iter()
        .map(|response| {
            let own_tree = tree_lines_of(response.line);
            response
                .items
                .iter()
                .map(|item| {
                    if item.struck
                        || item
                            .span
                            .clone()
                            .any(|line| node_at(line).is_some_and(|index| tree.nodes[index].closed))
                    {
                        return true;
                    }
                    let matches = followups
                        .iter()
                        .filter(|(key, _, _)| *key == item.key)
                        .map(|(_, state, _)| *state)
                        .collect::<Vec<_>>();
                    if !matches.contains(&FollowupRow::Live)
                        && (matches.contains(&FollowupRow::Struck)
                            || matches.contains(&FollowupRow::Quoted))
                    {
                        return true;
                    }
                    // `#treefinish`: rule 4, delegation. A live follow-up row
                    // in another top-level tree carries the item; this tree
                    // no longer holds it.
                    if followups.iter().any(|(key, state, line)| {
                        *key == item.key
                            && *state == FollowupRow::Live
                            && line.is_some_and(|line| {
                                own_tree.as_ref().is_none_or(|own| !own.contains(&line))
                            })
                    }) {
                        return true;
                    }
                    // `#selfrepaircleanup`: an operator ask to clean up
                    // agent-doc's own artifacts is agent-doc's repair, not
                    // operator work, unless a live row still tracks it.
                    if !matches.contains(&FollowupRow::Live)
                        && crate::self_repair::is_self_repair_cleanup_item(&item.raw)
                    {
                        return true;
                    }
                    item.agent
                        && (item.span.end.max(item.line + 1)..response.block.end).any(|line| {
                            !in_item_span(line)
                                && node_at(line).is_some_and(|index| tree.nodes[index].closed)
                        })
                })
                .collect()
        })
        .collect();

    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut changed = 0;
    for index in managed {
        let response = &responses[index];
        let scope = response.scope.clone();
        let mut entries: Vec<Entry> = Vec::new();
        let mut listed_keys: Vec<String> = Vec::new();
        let mut covered: Vec<std::ops::Range<usize>> = Vec::new();
        for (other_index, other) in responses.iter().enumerate() {
            if other_index != index && !scope.contains(&other.line) {
                continue;
            }
            if other_index != index
                && other
                    .state
                    .as_ref()
                    .is_some_and(|(_, state)| state == RESPONSE_STATE_COMPLETE)
                && !other
                    .guidance
                    .as_ref()
                    .is_some_and(|(_, guidance)| managed_form(RESPONSE_STATE_COMPLETE, guidance))
            {
                // A response that says it is complete by another rule keeps
                // its word; only this pass's own Complete is recomputed.
                continue;
            }
            for (item, done) in other.items.iter().zip(&resolved[other_index]) {
                if *done || listed_keys.contains(&item.key) {
                    continue;
                }
                listed_keys.push(item.key.clone());
                covered.push(item.span.clone());
                entries.push(Entry {
                    line: item.line,
                    owner: item.owner.clone(),
                    text: item.text.clone(),
                });
            }
        }
        let mut open_lines: Vec<usize> = scope.clone().collect();
        if let Some(parent) = response.parent {
            open_lines.insert(0, parent);
        }
        for line in open_lines {
            let Some(node) = node_at(line) else {
                continue;
            };
            if !crate::queue_tree::row_keeps_tree_alive(&tree, node)
                || covered.iter().any(|span| span.contains(&line))
            {
                continue;
            }
            let row = &tree.nodes[node];
            let plain = plain_text(&row.full_text);
            if followup_row_action_item(&plain)
                .is_some_and(|item| listed_keys.contains(&item_key(item)))
            {
                continue;
            }
            let (owner, text) = open_row_entry(row);
            entries.push(Entry { line, owner, text });
        }
        entries.sort_by_key(|entry| entry.line);

        // `#donetrack`: every action item (once per key) and queue row in
        // scope, done or not, for the tally.
        let mut tally_keys: Vec<String> = Vec::new();
        let mut item_spans: Vec<std::ops::Range<usize>> = Vec::new();
        let mut total = 0usize;
        let mut done = 0usize;
        for (other_index, other) in responses.iter().enumerate() {
            if other_index != index && !scope.contains(&other.line) {
                continue;
            }
            for (item, resolved_item) in other.items.iter().zip(&resolved[other_index]) {
                item_spans.push(item.line..item.span.end.max(item.line + 1));
                if tally_keys.contains(&item.key) {
                    continue;
                }
                tally_keys.push(item.key.clone());
                total += 1;
                done += usize::from(*resolved_item);
            }
        }
        let mut row_lines: Vec<usize> = scope.clone().collect();
        if let Some(parent) = response.parent {
            row_lines.insert(0, parent);
        }
        for line in row_lines {
            let Some(node) = node_at(line) else {
                continue;
            };
            let row = &tree.nodes[node];
            if row.state == QueueNodeState::Group
                || item_spans.iter().any(|span| span.contains(&line))
                || followup_row_action_item(&plain_text(&row.full_text))
                    .is_some_and(|item| tally_keys.contains(&item_key(item)))
            {
                continue;
            }
            if row.closed || row.resolution.is_some() {
                total += 1;
                done += 1;
            } else if crate::queue_tree::row_keeps_tree_alive(&tree, node) {
                total += 1;
            }
        }
        let yours = entries
            .iter()
            .filter(|entry| entry.owner != "agent")
            .count();
        let agent_open = entries.len() - yours;
        // An entry the tally did not see (an open parent row folded into an
        // item) still counts as open work.
        let total = total.max(done + entries.len());
        let tally = tally(done, total, agent_open, yours);

        let (state_range, state) = response.state.clone().expect("managed has a state");
        let (guidance_range, guidance) = response.guidance.clone().expect("managed has guidance");
        let (want_state, want_guidance, want_remaining) = if entries.is_empty() {
            // `#treefinish`: the root's response carries the tree's
            // indicator; a nested row's response is only its tally.
            let guidance = match response.parent.and_then(node_at) {
                Some(node)
                    if tree.nodes[node].depth == 0
                        && crate::queue_tree::row_closure(&tree, node)
                            .is_some_and(|closure| closure.subtree_closed()) =>
                {
                    format!("{tally} {NOTHING_REMAINS_ROW_GUIDANCE}")
                }
                Some(_) => tally.clone(),
                None => format!("{tally} {NOTHING_REMAINS_RESPONSE_GUIDANCE}"),
            };
            (RESPONSE_STATE_COMPLETE, guidance, None)
        } else if agent_open == 0 {
            // Only the operator can finish it: say exactly what to do.
            let what = if yours == 1 {
                let text = short_imperative(&entries[0].text, ENTRY_TEXT_LIMIT);
                let text = text.trim_end_matches(['.', ' ']);
                format!("{ONLY_YOU_GUIDANCE_LEAD}{text}. Strike or answer its row when done.")
            } else {
                format!(
                    "{ONLY_YOU_GUIDANCE_LEAD}{yours} items for you, listed under Remaining. Strike or answer each row when done."
                )
            };
            (
                RESPONSE_STATE_WAITING_ON_OPERATOR,
                format!("{tally} {what}"),
                Some(render_remaining(&entries, response.detail_indent)),
            )
        } else {
            (
                RESPONSE_STATE_FOLLOW_UP,
                format!("{tally} {}", remaining_guidance(entries.len())),
                Some(render_remaining(&entries, response.detail_indent)),
            )
        };
        let mut response_edits = Vec::new();
        if state != want_state {
            response_edits.push((state_range, want_state.to_string()));
        }
        if guidance != want_guidance {
            response_edits.push((guidance_range, want_guidance));
        }
        match (&response.remaining, want_remaining) {
            (Some((range, _)), Some(want)) => {
                if document[range.clone()] != *want {
                    response_edits.push((range.clone(), want));
                }
            }
            (Some((range, _)), None) => response_edits.push((range.clone(), String::new())),
            (None, Some(want)) => {
                let at = lines[response.state_line.expect("managed has a state line")].start;
                response_edits.push((at..at, want));
            }
            (None, None) => {}
        }
        if !response_edits.is_empty() {
            changed += 1;
            edits.extend(response_edits);
        }
    }
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse((range.start, range.end)));
    let mut out = document.to_string();
    for (range, value) in edits {
        out.replace_range(range, &value);
    }
    Some((out, changed))
}

#[cfg(test)]
#[path = "response_remaining_tests.rs"]
mod tests;

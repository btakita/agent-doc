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
//!   touched; an attempt the ledger still holds keeps its label. A
//!   follow-up row's quoted origin and its action item are scanned for
//!   code spans separately: the origin is a truncated label that can end
//!   inside a span (`“… prompt ` - …”`), and its stray backtick used to
//!   flip every later span of the action item, so a label quoted in code
//!   was stripped while its lifecycle word stayed (`` `running amend
//!   core-v1` ``).
//! - **Claim-label residue.** Rows that still carry a
//!   retired label's leftovers are restored only when `agent:exchange`
//!   proves the text they came from:
//!   - a follow-up row (stable id, no worker holding it, or struck) whose
//!     whole action item is a hand-back bullet the exchange records with a
//!     retired `<word> [#owner#N]` in it, either still labelled (quoted in
//!     code) or with only the lifecycle word left behind, takes that bullet
//!     with each label and its word removed;
//!   - a struck row whose whole text is a labelled leaf the exchange quotes
//!     (`` `🟡 running [#subagent:x#1] amend core-v1` ``) with only the word
//!     left behind (`~~running amend core-v1~~`), beside a struck sibling
//!     holding the clean leaf (`~~amend core-v1~~`), takes the leaf's text,
//!     and the duplicate pass collapses the two.
//!
//!   Without such a record nothing is rewritten: a row that merely reads
//!   like residue (`stop the running amend job`), an id-less live row, a
//!   row a worker holds, and generated response structure keep their text.
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
    /// Restored a row whose text holds a retired claim label's residue (the
    /// lifecycle word left behind, or the label quoted in a follow-up title)
    /// to the clean text its `agent:exchange` provenance proves.
    ClaimLabelResidue,
}

impl SelfRepairKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateStruckRow => "duplicate_struck_row",
            Self::DuplicateResponseTree => "duplicate_response_tree",
            Self::MisplacedResponseTree => "misplaced_response_tree",
            Self::StaleClaimLabel => "stale_claim_label",
            Self::ClaimLabelResidue => "claim_label_residue",
        }
    }
}

/// One repair, for ops.log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfRepair {
    pub kind: SelfRepairKind,
    /// The row text (or label) the repair removed or rewrote.
    pub detail: String,
    /// A row whose text the repair rewrote: `(before, after)`, each the row
    /// after its `- ` bullet. Ledgers keyed on row text carry their record
    /// from `before` to `after`.
    pub rewritten: Option<(String, String)>,
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
                rewritten: None,
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

/// `inner` as a byte range of `outer`, when it is a subslice of it.
fn offset_in(outer: &str, inner: &str) -> Option<usize> {
    let base = outer.as_ptr() as usize;
    let at = inner.as_ptr() as usize;
    (at >= base && at + inner.len() <= base + outer.len()).then(|| at - base)
}

/// Byte offset in a row's item (live or struck, any projection) where the
/// action item of a follow-up row starts (`follow up on an action item
/// from <origin>: <item>`); `None` for any other row.
fn followup_action_offset(item: &str) -> Option<usize> {
    let text = item.strip_prefix("### ").unwrap_or(item);
    let text = text.strip_prefix("~~").unwrap_or(text);
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    let action = crate::followup_owner::followup_row_action_item(work)?;
    offset_in(item, action)
}

/// [`outside_code`] for a queue row. A follow-up row's quoted origin is a
/// truncated label that can end inside a code span (`“… prompt ` - …”`), so
/// its stray backtick must not flip every later span of the action item:
/// the origin and the action item are scanned separately.
fn row_outside_code(item: &str) -> Vec<bool> {
    match followup_action_offset(item) {
        Some(split) if split > 0 => {
            let mut out = outside_code(&item[..split]);
            out.pop();
            out.extend(outside_code(&item[split..]));
            out
        }
        _ => outside_code(item),
    }
}

/// `(start, end, owner, generation)` of every owner annotation in `text`
/// from byte `from` on whose start `outside` marks (all of them when
/// `outside` is `None`).
fn owner_labels(
    text: &str,
    from: usize,
    outside: Option<&[bool]>,
) -> Vec<(usize, usize, String, u64)> {
    let mut out = Vec::new();
    let mut cursor = from;
    while let Some(found) = text[cursor..].find("[#") {
        let at = cursor + found;
        cursor = at + 2;
        if outside.is_some_and(|outside| !outside[at]) {
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

/// Start of the lifecycle state word written directly before the owner
/// annotation at `label_start` (`running [#subagent:x#1]`), if there is one.
/// The word starts after whitespace, an opening quote, a strike, a
/// parenthesis, or a backtick.
fn lifecycle_word_start(text: &str, label_start: usize, label_end: usize) -> Option<usize> {
    let before = text[..label_start].trim_end_matches(' ');
    if before.len() == label_start {
        return None;
    }
    let word_start = before
        .rfind(|c: char| c.is_whitespace() || matches!(c, '“' | '\'' | '"' | '~' | '(' | '`'))
        .map_or(0, |at| {
            at + before[at..].chars().next().map_or(1, char::len_utf8)
        });
    let word = &before[word_start..];
    (!word.is_empty()
        && agent_doc_frontmatter::frontmatter::lifecycle_word_before_owner_annotation(&format!(
            "{word} {}",
            &text[label_start..label_end]
        ))
        .is_some())
    .then_some(word_start)
}

/// Byte offset in a live row's item where its leading lifecycle/owner
/// projection ends (the repaint owns everything before it).
fn projection_end(item: &str) -> usize {
    let text = item.strip_prefix("### ").unwrap_or(item);
    let heading = item.len() - text.len();
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    heading + (text.len() - work.len())
}

/// Whether the label pass may rewrite this live row's text: it carries a
/// stable id and no worker holds it. A live row's text is its work identity
/// unless it carries a stable id; an id-less row, or one a worker holds,
/// keeps its text so no claim or lineage record loses its row.
fn live_row_text_is_rewritable(item: &str) -> bool {
    let heading = item.strip_prefix("### ").unwrap_or(item);
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(heading);
    agent_doc_element_queue::queue_prompt_stable_id(work).is_some()
        && agent_doc_document::queue_projection::owner_projection(heading).is_none()
}

/// Remove byte ranges `cuts` (sorted, disjoint) from `text`.
fn without_ranges(text: &str, cuts: &[(usize, usize)]) -> String {
    let mut out = text.to_string();
    for (start, end) in cuts.iter().rev() {
        out.replace_range(*start..*end, "");
    }
    out
}

/// The cut for a label at `start..end` (with `cut_start` at the label or
/// its lifecycle word): the following space too, or else a preceding one.
fn label_cut(text: &str, mut cut_start: usize, end: usize) -> (usize, usize) {
    let mut cut_end = end;
    if text[cut_end..].starts_with(' ') {
        cut_end += 1;
    } else if cut_start > 0 && text[..cut_start].ends_with(' ') {
        cut_start -= 1;
    }
    (cut_start, cut_end)
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
            if !live_row_text_is_rewritable(item) {
                continue;
            }
            projection_end(item)
        };
        let outside = row_outside_code(item);
        let mut cuts = Vec::new();
        let mut labels = Vec::new();
        for (start, end, owner, generation) in owner_labels(item, from, Some(&outside)) {
            if is_live(&owner, generation) {
                continue;
            }
            // A lifecycle state word written before the label (`running
            // [#subagent:x#1]`) is part of the same torn token.
            let cut_start = lifecycle_word_start(item, start, end)
                .filter(|word_start| *word_start >= from)
                .unwrap_or(start);
            cuts.push(label_cut(item, cut_start, end));
            labels.push(format!("[#{owner}#{generation}]"));
        }
        if cuts.is_empty() {
            continue;
        }
        let after = without_ranges(item, &cuts);
        for (at, label) in labels.into_iter().enumerate() {
            repairs.push(SelfRepair {
                kind: SelfRepairKind::StaleClaimLabel,
                detail: format!("{label} in {item}"),
                rewritten: (at == 0).then(|| (item.to_string(), after.clone())),
            });
        }
        edits.extend(
            cuts.into_iter()
                .map(|(start, end)| (item_start + start, item_start + end)),
        );
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

/// One text `agent:exchange` records with a retired attempt's label in it,
/// in the three forms a queue row can hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LabelEvidence {
    /// As recorded: `… running [#subagent:x#1] amend core-v1 …`.
    labelled: String,
    /// With only the labels removed, the lifecycle words left behind (the
    /// text an earlier label strip wrote): `… running amend core-v1 …`.
    residue: String,
    /// With each label and its lifecycle word removed: `… amend core-v1 …`.
    clean: String,
}

/// [`LabelEvidence`] for `text` when it holds a retired attempt's label
/// with a lifecycle word written before it; `None` otherwise.
fn label_evidence(text: &str, is_live: &dyn Fn(&str, u64) -> bool) -> Option<LabelEvidence> {
    let mut label_cuts = Vec::new();
    let mut word_cuts = Vec::new();
    for (start, end, owner, generation) in owner_labels(text, 0, None) {
        if is_live(&owner, generation) {
            continue;
        }
        let Some(word_start) = lifecycle_word_start(text, start, end) else {
            continue;
        };
        label_cuts.push(label_cut(text, start, end));
        word_cuts.push(label_cut(text, word_start, end));
    }
    if label_cuts.is_empty() {
        return None;
    }
    Some(LabelEvidence {
        labelled: text.to_string(),
        residue: without_ranges(text, &label_cuts),
        clean: without_ranges(text, &word_cuts),
    })
}

/// Inline code span contents of `line` (single-backtick spans).
fn code_spans(line: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else {
            break;
        };
        spans.push(&after[..close]);
        rest = &after[close + 1..];
    }
    spans
}

/// Everything `agent:exchange` proves about retired claim labels: each
/// hand-back bullet (`> - operator: …`) holding one becomes bullet
/// evidence, and each inline code span quoting a labelled row
/// (`` `🟡 running [#subagent:x#1] amend core-v1` ``) becomes span
/// evidence, cut to start at its lifecycle word.
fn exchange_label_evidence(
    document: &str,
    is_live: &dyn Fn(&str, u64) -> bool,
) -> (Vec<LabelEvidence>, Vec<LabelEvidence>) {
    let mut bullets = Vec::new();
    let mut spans = Vec::new();
    let Ok(components) = element::parse(document) else {
        return (bullets, spans);
    };
    for exchange in components
        .iter()
        .filter(|component| component.name == "exchange")
    {
        for line in exchange.content(document).lines() {
            if !line.contains("[#") {
                continue;
            }
            let mut text = line.trim();
            while let Some(rest) = text.strip_prefix('>') {
                text = rest.trim_start();
            }
            if let Some(bullet) = text
                .strip_prefix("- ")
                .or_else(|| text.strip_prefix("* "))
                .map(str::trim_end)
                && let Some(evidence) = label_evidence(bullet, is_live)
                && !bullets.contains(&evidence)
            {
                bullets.push(evidence);
            }
            for span in code_spans(line) {
                // The span cut to start at its first labelled lifecycle
                // word: the leaf text a labelled row held after its
                // projection (`🟡 running [#subagent:x#1] amend core-v1`).
                let Some(word_start) = owner_labels(span, 0, None).into_iter().find_map(
                    |(start, end, owner, generation)| {
                        (!is_live(&owner, generation))
                            .then(|| lifecycle_word_start(span, start, end))
                            .flatten()
                    },
                ) else {
                    continue;
                };
                if let Some(evidence) = label_evidence(&span[word_start..], is_live)
                    && !evidence.clean.trim().is_empty()
                    && !spans.contains(&evidence)
                {
                    spans.push(evidence);
                }
            }
        }
    }
    (bullets, spans)
}

/// Whether `rest`, the text after an action item in its row, holds only
/// trailing tags and annotations (`[operator-verify]`, `[#q-…]`, `⟦…⟧`) or
/// a strike's close.
fn only_trailing_tags(rest: &str) -> bool {
    let mut rest = rest.trim_start();
    loop {
        if rest.is_empty() {
            return true;
        }
        if let Some(after) = rest.strip_prefix("~~") {
            rest = after.trim_start();
            continue;
        }
        let close = match rest.chars().next() {
            Some('[') => ']',
            Some('⟦') => '⟧',
            _ => return false,
        };
        let Some(end) = rest.find(close) else {
            return false;
        };
        rest = rest[end + close.len_utf8()..].trim_start();
    }
}

/// `(offset, text)` of a struck row's `~~…~~` content.
fn struck_inner(item: &str) -> Option<(usize, &str)> {
    let text = item.strip_prefix("### ").unwrap_or(item);
    let rest = text.strip_prefix("~~")?;
    let inner = &rest[..rest.find("~~")?];
    Some((offset_in(item, inner)?, inner))
}

fn restore_label_residue_in_body(
    body: &str,
    repairs: &mut Vec<SelfRepair>,
    bullets: &[LabelEvidence],
    spans: &[LabelEvidence],
) -> String {
    let lines = lines_of(body);
    let parents = parents(&lines);
    let mut struck_siblings: HashMap<(Option<usize>, usize), Vec<&str>> = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some((_, inner)) = line.item.and_then(struck_inner) {
            struck_siblings
                .entry((parents[index], line.indent))
                .or_default()
                .push(inner);
        }
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(item) = line.item else {
            continue;
        };
        if item.starts_with("**") || item.contains(RESPONSE_DETAIL_MARKER) {
            continue;
        }
        let struck = is_struck_row(item);
        if !struck && !live_row_text_is_rewritable(item) {
            continue;
        }
        // A follow-up row whose whole action item is a hand-back bullet the
        // exchange records with the label, either still labelled (quoted in
        // code) or with only the lifecycle word left behind.
        let mut rewrite = followup_action_offset(item).and_then(|at| {
            let action = &item[at..];
            bullets.iter().find_map(|evidence| {
                [&evidence.residue, &evidence.labelled]
                    .into_iter()
                    .find(|form| {
                        action.starts_with(form.as_str())
                            && only_trailing_tags(&action[form.len()..])
                    })
                    .map(|form| (at, at + form.len(), evidence.clean.as_str()))
            })
        });
        // A struck copy whose whole text is a labelled leaf's residue, beside
        // a struck sibling holding the clean leaf: the two differ only by
        // the residue, so the copy takes the leaf's text (and the dedupe
        // pass collapses it).
        if rewrite.is_none()
            && let Some((at, inner)) = struck_inner(item)
        {
            let siblings = struck_siblings
                .get(&(parents[index], line.indent))
                .map(Vec::as_slice)
                .unwrap_or_default();
            rewrite = spans
                .iter()
                .find(|evidence| {
                    inner == evidence.residue && siblings.contains(&evidence.clean.as_str())
                })
                .map(|evidence| (at, at + inner.len(), evidence.clean.as_str()));
        }
        let Some((start, end, clean)) = rewrite else {
            continue;
        };
        let item_start = body[line.start..line.end]
            .find(item)
            .map_or(line.start, |offset| line.start + offset);
        let after = format!("{}{clean}{}", &item[..start], &item[end..]);
        repairs.push(SelfRepair {
            kind: SelfRepairKind::ClaimLabelResidue,
            detail: item.to_string(),
            rewritten: Some((item.to_string(), after)),
        });
        edits.push((item_start + start, item_start + end, clean.to_string()));
    }
    if edits.is_empty() {
        return body.to_string();
    }
    let mut out = body.to_string();
    for (start, end, clean) in edits.into_iter().rev() {
        out.replace_range(start..end, &clean);
    }
    out
}

/// Restore rows holding a retired claim label's residue (see the module
/// docs) to the clean text `agent:exchange` proves. `is_live(owner,
/// generation)` says whether the claim ledger still holds that attempt.
pub fn restore_claim_label_residue(
    document: &str,
    is_live: &dyn Fn(&str, u64) -> bool,
) -> (String, Vec<SelfRepair>) {
    let (bullets, spans) = exchange_label_evidence(document, is_live);
    if bullets.is_empty() && spans.is_empty() {
        return (document.to_string(), Vec::new());
    }
    edit_queue_bodies(document, |body, repairs| {
        restore_label_residue_in_body(body, repairs, &bullets, &spans)
    })
}

/// Run every self-repair over `document`: misplaced response trees
/// (`response_proven(id)` says the `[#r-…]` id's original lives in
/// `agent:exchange` or the response index), duplicate response trees, stale
/// claim labels, claim-label residue, then duplicate struck rows (a label
/// removal or residue restore can make two struck copies identical). `None` when nothing changed; a second run is a
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
            rewritten: None,
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
            rewritten: None,
        }));
        current = collapsed;
    }
    let (labelled, label_repairs) = strip_stale_claim_labels(&current, is_live);
    current = labelled;
    repairs.extend(label_repairs);
    let (restored, residue_repairs) = restore_claim_label_residue(&current, is_live);
    current = restored;
    repairs.extend(residue_repairs);
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

//! Waiting-on-you packet ownership (`#waitingpacketowner`).
//!
//! A row marked `<!-- agent-doc:waiting-item -->` is the binary-owned view of
//! one open `agent:todo` item. Every row beneath it, up to the next nested
//! waiting item, belongs to that item's packet: generated Response metadata,
//! offered Choices, the operator's replies, and any legacy worker attempt rows.
//! None of them is queue work of its own. They never receive a `📥`
//! projection, a stable `q-` id, a claim, or a dispatch; an operator reply is
//! decision evidence that selects the waiting item itself.
//!
//! This module is pure: it classifies parsed queue entries and source lines and
//! performs the closeout normalization of packets that older binaries polluted.

use std::ops::Range;

use agent_doc_document::queue_projection::{
    QueueLifecycleMarker, lifecycle_marker, owner_projection,
};

use agent_doc_markdown_ast::reply_row::{ReplyRow, ReplyRows};

use crate::document_queue::{QueueEntry, QueuePrompt};
use crate::queue_consume::WAITING_ON_YOU_ITEM_MARKER;

/// Follow-up rows that older lifecycle settling synthesized beneath a waiting
/// item's inert packet (`settle_inert_group_parents`). They are binary text,
/// never operator text, and the scheduler can no longer dispatch them.
const BINARY_FOLLOW_UP_TEXTS: [&str; 8] = [
    "Resolve the open operator action to complete the parent item.",
    "Resolve the blockers and complete the remaining work for the parent item.",
    "Retry the lost work and complete the parent item.",
    "Complete the in-progress work for the parent item.",
    "Reconcile the reported work, then complete the parent item.",
    "Diagnose the conflicting work state, then complete the parent item.",
    "Resume or explicitly close the skipped work for the parent item.",
    "Complete the remaining work for the parent item.",
];

/// What one live row inside a waiting item's packet is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitingMember {
    /// Operator-authored text: a reply, answer, or note. Decision evidence.
    Operator,
    /// A worker attempt projected onto the row (`🟡`, `🚧`, `☑️`, `⚠️`, ...).
    Attempt(QueueLifecycleMarker),
    /// A legacy binary follow-up row (`Complete the remaining work ...`).
    BinaryFollowUp,
    /// An offered option (`**label**: effect`) that drifted out of its inert
    /// `**Choices:**` block. It is an option for the operator, not an answer.
    OfferedChoice,
}

fn entry_reply_row(entry: &QueueEntry) -> ReplyRow<'_> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => ReplyRow::Item {
            indent: prompt.indent,
            body: &prompt.text,
        },
        QueueEntry::Freeform(line) => agent_doc_markdown_ast::reply_row::reply_row_of_line(
            line.lines().next().unwrap_or_default(),
        ),
        _ => ReplyRow::Other { indent: 0 },
    }
}

/// Parsed queue entries viewed as list rows for the shared reply-row predicate.
struct EntryRows<'a>(&'a [QueueEntry]);

impl ReplyRows for EntryRows<'_> {
    fn row_count(&self) -> usize {
        self.0.len()
    }

    fn row(&self, index: usize) -> ReplyRow<'_> {
        self.0.get(index).map_or(ReplyRow::Blank, entry_reply_row)
    }
}

/// `parse_spans` output viewed as list rows.
struct SpannedEntryRows<'a, S>(&'a [(QueueEntry, S)]);

impl<S> ReplyRows for SpannedEntryRows<'_, S> {
    fn row_count(&self) -> usize {
        self.0.len()
    }

    fn row(&self, index: usize) -> ReplyRow<'_> {
        self.0
            .get(index)
            .map_or(ReplyRow::Blank, |(entry, _)| entry_reply_row(entry))
    }
}

/// Whether `entries[index]` is a binary-owned reply row (`#barereplymarker`):
/// the shared [`agent_doc_markdown_ast::reply_row::is_agent_doc_reply_row_in`]
/// predicate over parsed queue entries.
pub fn is_agent_doc_reply_entry(entries: &[QueueEntry], index: usize) -> bool {
    agent_doc_markdown_ast::reply_row::is_agent_doc_reply_row_in(&EntryRows(entries), index)
}

/// [`is_agent_doc_reply_entry`] over `parse_spans` output.
pub fn is_agent_doc_reply_spanned_entry<S>(entries: &[(QueueEntry, S)], index: usize) -> bool {
    agent_doc_markdown_ast::reply_row::is_agent_doc_reply_row_in(&SpannedEntryRows(entries), index)
}

/// Whether `entry` is a waiting-item head row, live or struck.
pub fn is_waiting_item_head(entry: &QueueEntry) -> bool {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
            prompt.text.contains(WAITING_ON_YOU_ITEM_MARKER)
        }
        _ => false,
    }
}

/// List indentation of any entry, including an inert (`Freeform`) list row.
pub fn entry_list_indent(entry: &QueueEntry) -> Option<usize> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => Some(prompt.indent),
        QueueEntry::Freeform(line) => {
            let first = line.lines().next().unwrap_or_default();
            let trimmed = first.trim_start_matches([' ', '\t']);
            (trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ "))
                .then(|| first.len() - trimmed.len())
        }
        _ => None,
    }
}

/// Exclusive end of the subtree rooted at `index` (every later entry indented
/// deeper than it, including inert rows).
pub fn subtree_end(entries: &[QueueEntry], index: usize) -> usize {
    let Some(own) = entries.get(index).and_then(entry_list_indent) else {
        return index + 1;
    };
    entries[index + 1..]
        .iter()
        .position(|entry| entry_list_indent(entry).is_some_and(|indent| indent <= own))
        .map_or(entries.len(), |offset| index + 1 + offset)
}

/// The waiting item that owns the entry at `index`: the nearest enclosing
/// waiting-item head, or the entry itself when it is a head.
pub fn waiting_owner(entries: &[QueueEntry], index: usize) -> Option<usize> {
    if entries.get(index).is_some_and(is_waiting_item_head) {
        return Some(index);
    }
    let mut depth = entries.get(index).and_then(entry_list_indent)?;
    for candidate in (0..index).rev() {
        let Some(indent) = entry_list_indent(&entries[candidate]) else {
            continue;
        };
        if indent >= depth {
            continue;
        }
        if is_waiting_item_head(&entries[candidate]) {
            return Some(candidate);
        }
        if indent == 0 {
            return None;
        }
        depth = indent;
    }
    None
}

/// Whether the entry at `index` is a packet row: owned by a waiting item
/// without being that item's head.
pub fn is_packet_row(entries: &[QueueEntry], index: usize) -> bool {
    waiting_owner(entries, index).is_some_and(|owner| owner != index)
}

/// Indexes of the rows the waiting item at `head` owns (not nested items, and
/// not the rows nested items own).
pub fn packet_member_indexes(entries: &[QueueEntry], head: usize) -> Vec<usize> {
    (head + 1..subtree_end(entries, head))
        .filter(|index| waiting_owner(entries, *index) == Some(head))
        .collect()
}

/// Classify one live packet row.
pub fn classify_member(prompt: &QueuePrompt) -> WaitingMember {
    if let Some(
        marker @ (QueueLifecycleMarker::Claimed
        | QueueLifecycleMarker::Running
        | QueueLifecycleMarker::Reported
        | QueueLifecycleMarker::Lost
        | QueueLifecycleMarker::LostClaimed
        | QueueLifecycleMarker::LostRunning
        | QueueLifecycleMarker::LostReported
        | QueueLifecycleMarker::Reconciling
        | QueueLifecycleMarker::Unknown),
    ) = lifecycle_marker(&prompt.text)
    {
        return WaitingMember::Attempt(marker);
    }
    if owner_projection(&prompt.text).is_some() {
        return WaitingMember::Attempt(QueueLifecycleMarker::Claimed);
    }
    if is_binary_follow_up(&prompt.text) {
        return WaitingMember::BinaryFollowUp;
    }
    if is_offered_choice(&prompt.text) {
        return WaitingMember::OfferedChoice;
    }
    WaitingMember::Operator
}

/// Whether `text` has the generated offered-choice shape `**label**: effect`
/// (optionally followed by binary `[#id]` / `⟦...⟧` annotations).
pub fn is_offered_choice(text: &str) -> bool {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text).trim();
    let Some(rest) = work.strip_prefix("**") else {
        return false;
    };
    let Some((label, effect)) = rest.split_once("**: ") else {
        return false;
    };
    !label.trim().is_empty() && !label.contains("**") && !effect.trim().is_empty()
}

/// Whether `text` is exactly a legacy binary follow-up sentence plus only
/// binary annotations (`[#id]`, `⟦effect:N⟧`).
pub fn is_binary_follow_up(text: &str) -> bool {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text).trim();
    BINARY_FOLLOW_UP_TEXTS.iter().any(|sentence| {
        work.strip_prefix(sentence)
            .is_some_and(only_binary_annotations)
    })
}

fn only_binary_annotations(mut rest: &str) -> bool {
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return true;
        }
        let close = if rest.starts_with("[#") {
            "]"
        } else if rest.starts_with('⟦') {
            "⟧"
        } else {
            return false;
        };
        let Some(end) = rest.find(close) else {
            return false;
        };
        let token = &rest[..end];
        if token.contains(char::is_whitespace) {
            return false;
        }
        rest = &rest[end + close.len()..];
    }
}

/// The operator's text when an inert (`Freeform`) packet row at
/// `entries[index]` is an operator answer: a live list row typed beneath
/// generated response metadata, which parses inert because of where it sits.
/// Generated rows are reply rows (trailing `▫️` in the rendered position, or a
/// legacy reply comment) or have a bold `**label**` shape; comments, quotes,
/// HTML and struck rows are never answers.
pub fn operator_freeform_text(entries: &[QueueEntry], index: usize) -> Option<&str> {
    let QueueEntry::Freeform(line) = entries.get(index)? else {
        return None;
    };
    let first = line.lines().next()?;
    let body = first.trim_start().strip_prefix("- ")?.trim();
    if body.is_empty()
        || is_agent_doc_reply_entry(entries, index)
        || body.starts_with("**")
        || body.starts_with("%%")
        || body.starts_with('<')
        || body.starts_with('>')
        || body.starts_with("~~")
        || body.starts_with("&gt;")
    {
        return None;
    }
    Some(body)
}

/// Whether the waiting item at `head` has live operator-authored evidence.
pub fn has_operator_reply(entries: &[QueueEntry], head: usize) -> bool {
    packet_member_indexes(entries, head)
        .into_iter()
        .any(|index| match &entries[index] {
            QueueEntry::Prompt(prompt) => {
                classify_member(prompt) == WaitingMember::Operator
                    && !is_agent_doc_reply_entry(entries, index)
                    && !agent_doc_element_queue::struck_work_text(&prompt.text)
                        .trim()
                        .is_empty()
            }
            QueueEntry::Freeform(_) => operator_freeform_text(entries, index).is_some(),
            _ => false,
        })
}

/// Whether `prompt` is the waiting item's own attempt projected onto a
/// synthesized child row (`☑️ [#subagent:x#1] Resolve the open operator action
/// to complete the parent item.`): the claim belongs to the item itself.
pub fn is_item_attempt_projection(prompt: &QueuePrompt) -> bool {
    matches!(classify_member(prompt), WaitingMember::Attempt(_))
        && is_binary_follow_up(&prompt.text)
}

/// Whether the waiting item at `head` is selected: an operator reply selects
/// it, and the item's own attempt projected onto a child keeps it selected so
/// that attempt's claim stays live.
pub fn is_selected(entries: &[QueueEntry], head: usize) -> bool {
    has_operator_reply(entries, head)
        || packet_member_indexes(entries, head)
            .into_iter()
            .any(|index| {
                matches!(&entries[index], QueueEntry::Prompt(prompt)
                if is_item_attempt_projection(prompt))
            })
}

/// Whether a row the waiting item owns carries a worker attempt of its own
/// (an older binary dispatched an offered choice or a reply as separate
/// work). Until those attempts are reconciled the item is not selected again:
/// a new attempt on the item would duplicate work already reported.
pub fn awaits_reconciliation(entries: &[QueueEntry], head: usize) -> bool {
    packet_member_indexes(entries, head)
        .into_iter()
        .any(|index| {
            matches!(&entries[index], QueueEntry::Prompt(prompt)
            if matches!(classify_member(prompt), WaitingMember::Attempt(_))
                && !is_item_attempt_projection(prompt))
        })
}

/// Byte ranges of every waiting-item head line and its descendant lines in a
/// whole document. Stable-id admission skips queue nodes inside them.
pub fn waiting_tree_byte_ranges(content: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        lines.push((offset, line));
        offset += line.len();
    }
    let list_indent = |line: &str| {
        let trimmed = line.trim_start_matches([' ', '\t']);
        (trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ "))
            .then(|| line.len() - trimmed.len())
    };
    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let (start, line) = lines[index];
        let Some(indent) = list_indent(line).filter(|_| line.contains(WAITING_ON_YOU_ITEM_MARKER))
        else {
            index += 1;
            continue;
        };
        let mut end_index = index + 1;
        while end_index < lines.len() {
            let candidate = lines[end_index].1;
            let trimmed = candidate.trim_start();
            if trimmed.starts_with("<!-- agent:") || trimmed.starts_with("<!-- /agent:") {
                break;
            }
            if list_indent(candidate).is_some_and(|other| other <= indent) {
                break;
            }
            end_index += 1;
        }
        let end = lines.get(end_index).map_or(content.len(), |(at, _)| *at);
        ranges.push(start..end);
        index += 1;
    }
    ranges
}

/// Normalize the packets of every waiting item in one queue body.
///
/// Older binaries treated packet rows as ordinary queue leaves. This removes
/// only binary projection from them: a `📥` pending prefix (no claim exists
/// behind it) and an unclaimed, childless legacy follow-up row. Operator text,
/// worker attempts, and every inert row are left byte-for-byte.
pub fn normalize_waiting_packets(entries: &[QueueEntry]) -> Option<Vec<QueueEntry>> {
    let mut out = Vec::with_capacity(entries.len());
    let mut changed = false;
    for (index, entry) in entries.iter().enumerate() {
        if !is_packet_row(entries, index) {
            out.push(entry.clone());
            continue;
        }
        let QueueEntry::Prompt(prompt) = entry else {
            out.push(entry.clone());
            continue;
        };
        let childless = subtree_end(entries, index) == index + 1;
        if childless
            && is_binary_follow_up(&prompt.text)
            && owner_projection(&prompt.text).is_none()
            && matches!(
                lifecycle_marker(&prompt.text),
                None | Some(QueueLifecycleMarker::Pending)
            )
        {
            changed = true;
            continue;
        }
        if lifecycle_marker(&prompt.text) == Some(QueueLifecycleMarker::Pending)
            && owner_projection(&prompt.text).is_none()
            && let Some(stripped) =
                agent_doc_document::queue_projection::strip_in_progress_marker_for_display(
                    &prompt.text,
                )
        {
            let mut next = prompt.clone();
            next.text = stripped;
            out.push(QueueEntry::Prompt(next));
            changed = true;
            continue;
        }
        out.push(entry.clone());
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_queue;

    fn entries(body: &str) -> Vec<QueueEntry> {
        document_queue::parse(body).unwrap()
    }

    const TREE: &str = concat!(
        "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
        "  - [#alpha] Choose the alpha owner. <!-- agent-doc:waiting-item -->\n",
        "    - **Response:** Choose the alpha owner. ▫️\n",
        "      - **Next actions:** ▫️\n",
        "        - **Action:** Choose the alpha owner. ▫️\n",
        "          - 📥 commit + integrate + push [#q-111111111111]\n",
        "    - 📥 Complete the remaining work for the parent item. ⟦effect:3⟧ [#q-222222222222]\n",
        "    - [#beta] Nested sibling item. <!-- agent-doc:waiting-item -->\n",
        "      - Is this moot?\n",
        "  - [#gamma] Approve the release. <!-- agent-doc:waiting-item -->\n",
        "    - ☑️ [#subagent:gamma#1] Resolve the open operator action to complete the parent item. [#gamma]\n",
    );

    #[test]
    fn packet_rows_belong_to_their_nearest_waiting_item() {
        let parsed = entries(TREE);
        let index_of = |needle: &str| {
            parsed
                .iter()
                .position(|entry| match entry {
                    QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
                        prompt.text.contains(needle)
                    }
                    QueueEntry::Freeform(line) => line.contains(needle),
                    _ => false,
                })
                .unwrap()
        };
        let alpha = index_of("[#alpha]");
        let beta = index_of("[#beta]");
        let reply = index_of("commit + integrate");
        let moot = index_of("Is this moot?");
        assert_eq!(waiting_owner(&parsed, reply), Some(alpha));
        assert_eq!(waiting_owner(&parsed, moot), Some(beta));
        assert_eq!(waiting_owner(&parsed, beta), Some(beta));
        assert!(is_packet_row(&parsed, reply));
        assert!(!is_packet_row(&parsed, alpha));
        assert!(
            !packet_member_indexes(&parsed, alpha).contains(&moot),
            "a nested item's packet is never grafted onto its parent"
        );
        assert!(has_operator_reply(&parsed, alpha));
        assert!(has_operator_reply(&parsed, beta));
        let gamma = index_of("[#gamma] Approve");
        assert!(
            !has_operator_reply(&parsed, gamma),
            "a worker attempt row is not an operator answer"
        );
        assert!(
            is_selected(&parsed, gamma),
            "the item's own projected attempt keeps its claim live"
        );
        assert!(!awaits_reconciliation(&parsed, gamma));
    }

    #[test]
    fn normalization_strips_pending_projection_and_drops_phantom_follow_ups() {
        let parsed = entries(TREE);
        let normalized = normalize_waiting_packets(&parsed).expect("packet rows were polluted");
        let body = document_queue::render(&normalized);
        assert!(
            body.contains("          - commit + integrate + push [#q-111111111111]\n"),
            "{body}"
        );
        assert!(!body.contains("Complete the remaining work"), "{body}");
        assert!(
            body.contains("Resolve the open operator action to complete the parent item. [#gamma]"),
            "a claimed attempt row is never removed: {body}"
        );
        assert!(normalize_waiting_packets(&normalized).is_none());
    }

    #[test]
    fn byte_ranges_cover_waiting_heads_and_their_descendants() {
        let doc = format!("<!-- agent:queue -->\n{TREE}- ordinary work\n<!-- /agent:queue -->\n");
        let ranges = waiting_tree_byte_ranges(&doc);
        let covered = |needle: &str| {
            let at = doc.find(needle).unwrap();
            ranges.iter().any(|range| range.contains(&at))
        };
        assert!(covered("commit + integrate"));
        assert!(covered("[#alpha]"));
        assert!(covered("Is this moot?"));
        assert!(!covered("ordinary work"));
        assert!(!covered("Waiting on you"));
    }

    #[test]
    fn stable_id_admission_never_reaches_a_waiting_packet() {
        let doc = concat!(
            "<!-- agent:queue id=\"main\" -->\n",
            "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
            "  - [#alpha] Choose the alpha owner. <!-- agent-doc:waiting-item -->\n",
            "    - **Response:** Choose the alpha owner. ▫️\n",
            "      - commit + integrate + push\n",
            "- ordinary work\n",
            "<!-- /agent:queue -->\n",
        );
        let projected = crate::prompt_identity::project(doc, "fixture");
        let admitted = projected
            .assignments
            .iter()
            .map(|assignment| assignment.source_text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(admitted, vec!["ordinary work"]);
        assert!(
            projected
                .content
                .contains("      - commit + integrate + push\n")
        );
    }

    #[test]
    fn settling_never_strikes_or_extends_a_waiting_item() {
        let parsed = entries(concat!(
            "- [#alpha] Choose the alpha owner. <!-- agent-doc:waiting-item -->\n",
            "  - ~~approved~~\n",
            "- 🙋 [#beta] Approve the release. <!-- agent-doc:waiting-item -->\n",
            "  - ~~looks good~~\n",
        ));
        let open = std::collections::HashSet::from(["alpha".to_string(), "beta".to_string()]);
        assert!(
            document_queue::settle_inert_group_parents(&parsed, &open).is_none(),
            "a waiting item is closed by its Todo, never by settling its packet"
        );
    }

    #[test]
    fn the_open_todo_gate_on_a_waiting_head_is_a_fixed_point_across_passes() {
        let body = concat!(
            "- [#yeja] Choose the project-root policy. <!-- agent-doc:waiting-item -->\n",
            "  - **Response:** Choose the project-root policy. ▫️\n",
        );
        let open = std::collections::HashSet::from(["yeja".to_string()]);
        let gate = |body: &str| {
            document_queue::project_prompt_operator_actions(body, &[], &open)
                .unwrap()
                .map_or_else(|| body.to_string(), |(projected, _)| projected)
        };
        let lifecycle = |body: &str| {
            let parsed = entries(body);
            let targets = parsed
                .iter()
                .filter_map(|entry| match entry {
                    QueueEntry::Prompt(prompt) => Some(document_queue::QueueRowProjection {
                        text: prompt.text.clone(),
                        marker: QueueLifecycleMarker::Pending,
                        owner: None,
                        blockers: Vec::new(),
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>();
            document_queue::set_prompt_runtime_projections(&parsed, &targets).map_or_else(
                || body.to_string(),
                |marked| document_queue::render(&marked),
            )
        };
        let once = lifecycle(&gate(body));
        assert!(
            once.contains(
                "🙋 [#yeja] Choose the project-root policy. <!-- agent-doc:waiting-item -->\n"
            ),
            "{once}"
        );
        assert_eq!(
            lifecycle(&gate(&once)),
            once,
            "the Todo id never moves to the tail"
        );
    }
}

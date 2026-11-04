//! Read-only queue tree view (`#queuetreeview`).
//!
//! `agent-doc queue tree FILE` and the editor plugins' gutter icons read one
//! projection of every queue component: one node per queue row (and per live
//! operator answer typed inside a waiting item's packet), each with one derived
//! state, plus a summary that counts the states.
//!
//! The states come from the same binary-owned projections the closeout renders
//! into the document: a struck row is done, a lifecycle prefix names the attempt
//! phase, and a Waiting-on-you head carries the state and roll-up that
//! [`crate::waiting_status`] derives from its packet. Generated packet metadata
//! (Response, Next, Choices labels) is not a node; it is folded under its item.
//!
//! This module is pure: it reads a document string and never writes one.

use std::collections::HashSet;

use agent_doc_document::queue_projection::{QueueLifecycleMarker, lifecycle_marker};
use agent_doc_element::element;
use serde::Serialize;

use crate::document_queue::{self, QueueEntry};
use crate::waiting_packet::{self, WaitingMember};
use crate::waiting_status::{self, WaitingState};

/// The derived state of one queue tree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueueNodeState {
    /// The operator owes the next step (an open ask, a `🙋` gate, or a
    /// waiting item whose packet is fully closed and only its Todo is open).
    WaitingOnYou,
    /// The operator answered a waiting item; no attempt picked it up yet.
    Answered,
    /// A worker attempt is claimed or running.
    Running,
    /// A worker reported; the coordinator still has to integrate and close it.
    NeedsIntegration,
    /// The attempt was lost before it reported.
    Lost,
    /// Completion evidence exists but is not yet reconciled to an attempt.
    Reconciling,
    /// Held behind open prerequisite work.
    Blocked,
    /// Queued for pickup (`📥`).
    Pending,
    /// A live row with no projected state yet.
    Open,
    Skipped,
    Unknown,
    /// An operator answer typed inside a waiting item's packet.
    Reply,
    /// An offered choice that drifted out of its inert Choices block.
    Choice,
    /// A legacy binary follow-up row inside a waiting packet.
    FollowUp,
    /// Struck: closed.
    Done,
    /// The binary-owned Waiting-on-you root. Not counted as an item.
    Group,
}

/// Summary order: what the operator acts on first.
const SUMMARY_ORDER: [QueueNodeState; 15] = [
    QueueNodeState::Done,
    QueueNodeState::WaitingOnYou,
    QueueNodeState::Answered,
    QueueNodeState::Running,
    QueueNodeState::NeedsIntegration,
    QueueNodeState::Lost,
    QueueNodeState::Reconciling,
    QueueNodeState::Blocked,
    QueueNodeState::Pending,
    QueueNodeState::Open,
    QueueNodeState::Skipped,
    QueueNodeState::Unknown,
    QueueNodeState::Reply,
    QueueNodeState::Choice,
    QueueNodeState::FollowUp,
];

impl QueueNodeState {
    /// Node label (`needs integration`).
    pub fn label(self) -> &'static str {
        match self {
            Self::WaitingOnYou => "waiting on you",
            Self::Answered => "answered",
            Self::Running => "running",
            Self::NeedsIntegration => "needs integration",
            Self::Lost => "lost",
            Self::Reconciling => "reconciling",
            Self::Blocked => "blocked",
            Self::Pending => "pending",
            Self::Open => "open",
            Self::Skipped => "skipped",
            Self::Unknown => "unknown",
            Self::Reply => "reply",
            Self::Choice => "choice",
            Self::FollowUp => "follow-up",
            Self::Done => "done",
            Self::Group => "group",
        }
    }

    /// Summary label for a count (`4 need integration`, `2 replies`).
    pub fn summary_label(self, count: usize) -> &'static str {
        let plural = count != 1;
        match self {
            Self::NeedsIntegration if plural => "need integration",
            Self::Reply if plural => "replies",
            Self::Choice if plural => "choices",
            Self::FollowUp if plural => "follow-ups",
            other => other.label(),
        }
    }

    /// Whether the node is closed (its subtree folds in an editor).
    pub fn is_closed(self) -> bool {
        self == Self::Done
    }

    fn of_marker(marker: QueueLifecycleMarker) -> Self {
        match marker {
            QueueLifecycleMarker::Pending => Self::Pending,
            QueueLifecycleMarker::Blocked => Self::Blocked,
            QueueLifecycleMarker::Claimed | QueueLifecycleMarker::Running => Self::Running,
            QueueLifecycleMarker::Reported | QueueLifecycleMarker::LostReported => {
                Self::NeedsIntegration
            }
            QueueLifecycleMarker::Lost
            | QueueLifecycleMarker::LostClaimed
            | QueueLifecycleMarker::LostRunning => Self::Lost,
            QueueLifecycleMarker::Reconciling => Self::Reconciling,
            QueueLifecycleMarker::Unknown => Self::Unknown,
            QueueLifecycleMarker::Skipped => Self::Skipped,
            QueueLifecycleMarker::AwaitingOperator => Self::WaitingOnYou,
        }
    }
}

/// A waiting item's roll-up (`you · agent · done`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WaitingRollup {
    pub you: usize,
    pub agent: usize,
    pub done: usize,
}

/// One node of the read-only queue tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueTreeNode {
    /// 1-based document line of the row.
    pub line: usize,
    /// 1-based last document line of the row's subtree (inclusive). An editor
    /// folds `line + 1 ..= end_line` for a closed node.
    pub end_line: usize,
    /// Nesting depth among nodes (0 for a top-level queue row).
    pub depth: usize,
    /// The queue component's `id` attribute, or its marker name.
    pub queue: String,
    pub state: QueueNodeState,
    /// The glyph an editor gutter shows for the state.
    pub glyph: String,
    /// One line of work text, with binary projection stripped.
    pub text: String,
    /// The row's own `[#id]`, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Whether the node is closed (`state == done`).
    pub closed: bool,
    /// Whether the node is a Waiting-on-you item head.
    pub waiting_item: bool,
    /// The waiting item's roll-up, for a live waiting head.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollup: Option<WaitingRollup>,
}

/// One state and how many nodes carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueTreeCount {
    pub state: QueueNodeState,
    pub count: usize,
}

/// Counts over the listed nodes (group nodes excluded).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueTreeSummary {
    pub items: usize,
    pub counts: Vec<QueueTreeCount>,
}

impl QueueTreeSummary {
    fn of(nodes: &[QueueTreeNode]) -> Self {
        let counted = nodes
            .iter()
            .filter(|node| node.state != QueueNodeState::Group)
            .collect::<Vec<_>>();
        let counts = SUMMARY_ORDER
            .iter()
            .filter_map(|state| {
                let count = counted.iter().filter(|node| node.state == *state).count();
                (count > 0).then_some(QueueTreeCount {
                    state: *state,
                    count,
                })
            })
            .collect();
        Self {
            items: counted.len(),
            counts,
        }
    }

    /// `24 items: 9 done · 6 waiting on you · 3 running · 4 need integration`.
    pub fn line(&self) -> String {
        let noun = if self.items == 1 { "item" } else { "items" };
        if self.counts.is_empty() {
            return format!("{} {noun}", self.items);
        }
        let parts = self
            .counts
            .iter()
            .map(|count| format!("{} {}", count.count, count.state.summary_label(count.count)))
            .collect::<Vec<_>>();
        format!("{} {noun}: {}", self.items, parts.join(" · "))
    }
}

/// The read-only queue tree of one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueTree {
    pub summary: QueueTreeSummary,
    pub nodes: Vec<QueueTreeNode>,
}

/// What to include.
#[derive(Debug, Clone, Default)]
pub struct QueueTreeOptions {
    /// Only the node matching this item (`#id`, `do [#id]`, `id`, or text) and
    /// its subtree.
    pub item: Option<String>,
    /// Drop closed nodes together with their subtrees.
    pub open_only: bool,
}

const DONE_GLYPH: &str = "✓";
const OPEN_GLYPH: &str = "·";
const REPLY_GLYPH: &str = "↳";
const CHOICE_GLYPH: &str = "◇";
const GROUP_GLYPH: &str = "▾";
const TEXT_LIMIT: usize = 100;

fn glyph(state: QueueNodeState, marker: Option<QueueLifecycleMarker>) -> String {
    if let Some(marker) = marker {
        return marker.prefix().to_string();
    }
    match state {
        QueueNodeState::Done => DONE_GLYPH.to_string(),
        QueueNodeState::WaitingOnYou => {
            agent_doc_document::queue_projection::operator_action_marker().to_string()
        }
        QueueNodeState::Reply | QueueNodeState::FollowUp => REPLY_GLYPH.to_string(),
        QueueNodeState::Choice => CHOICE_GLYPH.to_string(),
        QueueNodeState::Group => GROUP_GLYPH.to_string(),
        QueueNodeState::Answered => QueueLifecycleMarker::Pending.prefix().to_string(),
        _ => OPEN_GLYPH.to_string(),
    }
}

/// One line of display text: the work text without projection prefixes, the
/// waiting status token, hidden markers, or trailing effect annotations.
fn display_text(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or_default();
    let work = agent_doc_element_queue::struck_work_text(first);
    let work = waiting_status::without_status_token(work);
    let mut text = strip_html_comments(&work);
    text = strip_effect_annotations(&text);
    let text = text.trim();
    let text = text
        .strip_prefix("~~")
        .and_then(|inner| inner.strip_suffix("~~"))
        .unwrap_or(text)
        .trim();
    truncate(text, TEXT_LIMIT)
}

fn strip_html_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("<!--") {
        out.push_str(&rest[..at]);
        match rest[at..].find("-->") {
            Some(end) => rest = &rest[at + end + 3..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_effect_annotations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("⟦effect:") {
        out.push_str(&rest[..at]);
        match rest[at..].find('⟧') {
            Some(end) => rest = &rest[at + end + '⟧'.len_utf8()..],
            None => {
                out.push_str(&rest[at..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out = text.chars().take(limit - 1).collect::<String>();
    out.push('…');
    out
}

fn own_id(text: &str) -> Option<String> {
    let work = waiting_status::without_status_token(agent_doc_element_queue::struck_work_text(
        text.lines().next().unwrap_or_default(),
    ));
    let work = work.trim().trim_start_matches("~~");
    document_queue::queue_prompt_reference_ids(work)
        .into_iter()
        .next()
        .map(|id| id.to_ascii_lowercase())
}

fn line_of(document: &str, offset: usize) -> usize {
    document[..offset.min(document.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

/// State of one live (unstruck) row outside any waiting packet.
fn live_row_state(text: &str) -> (QueueNodeState, Option<QueueLifecycleMarker>) {
    if let Some(marker) = lifecycle_marker(text) {
        return (QueueNodeState::of_marker(marker), Some(marker));
    }
    if agent_doc_document::queue_projection::owner_projection(text).is_some() {
        return (QueueNodeState::Running, None);
    }
    let operator_action = agent_doc_document::queue_projection::operator_action_marker();
    if !operator_action.is_empty() && text.trim_start().starts_with(operator_action) {
        return (QueueNodeState::WaitingOnYou, None);
    }
    (QueueNodeState::Open, None)
}

fn waiting_head_state(state: &WaitingState) -> (QueueNodeState, Option<QueueLifecycleMarker>) {
    match state {
        WaitingState::Attempt(marker) => (QueueNodeState::of_marker(*marker), Some(*marker)),
        WaitingState::Operator | WaitingState::Done => (QueueNodeState::WaitingOnYou, None),
        WaitingState::Answered => (QueueNodeState::Answered, None),
    }
}

/// Generated `**Label:** value` metadata, or a quoted `> ...` echo, inside
/// a waiting packet: neither is an operator answer.
fn is_generated_packet_text(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("**") || text.starts_with("&gt;") || text.starts_with('>')
}

/// The binary-owned root row: its text, without the heading and hidden
/// marker, is exactly the root text. A quoted echo of it is not the root.
fn is_waiting_root_line(line: &str) -> bool {
    let text = strip_html_comments(
        line.trim_start()
            .trim_start_matches("- ")
            .trim_start_matches("### "),
    );
    agent_doc_element_queue::struck_work_text(&text).trim()
        == crate::queue_consume::WAITING_ON_YOU_ROOT_TEXT
}

/// Build the queue tree of `document`.
pub fn queue_tree(document: &str, options: &QueueTreeOptions) -> anyhow::Result<QueueTree> {
    let open_todo_ids = crate::queue_consume::open_waiting_todo_ids(document);
    let mut nodes = Vec::new();
    for component in element::parse(document)?
        .into_iter()
        .filter(|component| component.name == "queue")
    {
        let queue = component
            .attrs
            .get("id")
            .cloned()
            .unwrap_or_else(|| component.name.clone());
        let body = component.content(document);
        let spanned = document_queue::parse_spans(body)?;
        let entries = spanned
            .iter()
            .map(|(entry, _)| entry.clone())
            .collect::<Vec<_>>();
        let mut stack: Vec<usize> = Vec::new();
        for (index, (entry, span)) in spanned.iter().enumerate() {
            let Some(indent) = waiting_packet::entry_list_indent(entry) else {
                continue;
            };
            let Some(node) = node_for_entry(&entries, index, entry, &open_todo_ids) else {
                continue;
            };
            let (state, marker, text, rollup) = node;
            let end_index = waiting_packet::subtree_end(&entries, index).max(index + 1);
            let end_offset = spanned[end_index - 1].1.end;
            let start = component.open_end + span.start;
            // The last byte of the subtree, not the start of the next row.
            let end = (component.open_end + end_offset)
                .saturating_sub(1)
                .max(start);
            while stack.last().is_some_and(|open| *open >= indent) {
                stack.pop();
            }
            let depth = stack.len();
            stack.push(indent);
            let raw = match entry {
                QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => prompt.text.as_str(),
                QueueEntry::Freeform(line) => line.as_str(),
                _ => "",
            };
            nodes.push(QueueTreeNode {
                line: line_of(document, start),
                end_line: line_of(document, end),
                depth,
                queue: queue.clone(),
                state,
                glyph: glyph(state, marker),
                text,
                id: own_id(raw),
                closed: state.is_closed(),
                waiting_item: waiting_packet::is_waiting_item_head(entry),
                rollup,
            });
        }
    }
    let nodes = select(nodes, options);
    Ok(QueueTree {
        summary: QueueTreeSummary::of(&nodes),
        nodes,
    })
}

type NodeParts = (
    QueueNodeState,
    Option<QueueLifecycleMarker>,
    String,
    Option<WaitingRollup>,
);

fn node_for_entry(
    entries: &[QueueEntry],
    index: usize,
    entry: &QueueEntry,
    open_todo_ids: &HashSet<String>,
) -> Option<NodeParts> {
    match entry {
        QueueEntry::Completed(prompt) => {
            Some((QueueNodeState::Done, None, display_text(&prompt.text), None))
        }
        QueueEntry::Prompt(prompt) => {
            if is_waiting_root_line(&prompt.text) {
                return Some((
                    QueueNodeState::Group,
                    None,
                    display_text(&prompt.text),
                    None,
                ));
            }
            if waiting_packet::is_waiting_item_head(entry) {
                let status = waiting_status::waiting_status(entries, index, open_todo_ids)?;
                let (state, marker) = waiting_head_state(&status.state);
                let glyph_marker = marker;
                return Some((
                    state,
                    glyph_marker,
                    display_text(&prompt.text),
                    Some(WaitingRollup {
                        you: status.you,
                        agent: status.agent,
                        done: status.done,
                    }),
                ));
            }
            if waiting_packet::is_packet_row(entries, index) {
                if waiting_packet::is_agent_doc_reply_entry(entries, index) {
                    return None;
                }
                let (state, marker) = match waiting_packet::classify_member(prompt) {
                    WaitingMember::Attempt(marker) => {
                        let marker = lifecycle_marker(&prompt.text).unwrap_or(marker);
                        (QueueNodeState::of_marker(marker), Some(marker))
                    }
                    WaitingMember::Operator
                        if is_generated_packet_text(agent_doc_element_queue::struck_work_text(
                            &prompt.text,
                        )) =>
                    {
                        // Generated metadata or a quoted echo, not an answer.
                        return None;
                    }
                    WaitingMember::Operator => (QueueNodeState::Reply, None),
                    WaitingMember::OfferedChoice => (QueueNodeState::Choice, None),
                    WaitingMember::BinaryFollowUp => (QueueNodeState::FollowUp, None),
                };
                return Some((state, marker, display_text(&prompt.text), None));
            }
            let (state, marker) = live_row_state(&prompt.text);
            Some((state, marker, display_text(&prompt.text), None))
        }
        QueueEntry::Freeform(line) => {
            let first = line.lines().next().unwrap_or_default();
            if is_waiting_root_line(first) {
                return Some((
                    QueueNodeState::Group,
                    None,
                    display_text(first.trim_start().trim_start_matches("- ")),
                    None,
                ));
            }
            let reply = waiting_packet::operator_freeform_text(entries, index)?;
            if waiting_status::is_next_row(entries, index) {
                return None;
            }
            Some((QueueNodeState::Reply, None, display_text(reply), None))
        }
        _ => None,
    }
}

fn normalized_item_key(item: &str) -> String {
    let trimmed = item.trim();
    let trimmed = trimmed.strip_prefix("do ").unwrap_or(trimmed).trim();
    let trimmed = trimmed
        .strip_prefix("[#")
        .and_then(|rest| rest.strip_suffix(']'))
        .or_else(|| trimmed.strip_prefix('#'))
        .unwrap_or(trimmed);
    trimmed.to_ascii_lowercase()
}

fn select(nodes: Vec<QueueTreeNode>, options: &QueueTreeOptions) -> Vec<QueueTreeNode> {
    let mut nodes = nodes;
    if let Some(item) = options.item.as_deref() {
        let key = normalized_item_key(item);
        let raw = item.trim();
        let root = nodes
            .iter()
            .position(|node| node.id.as_deref() == Some(key.as_str()))
            .or_else(|| nodes.iter().position(|node| node.text.contains(raw)));
        nodes = match root {
            Some(root) => {
                let (queue, line, end_line, depth) = (
                    nodes[root].queue.clone(),
                    nodes[root].line,
                    nodes[root].end_line,
                    nodes[root].depth,
                );
                nodes
                    .into_iter()
                    .filter(|node| {
                        node.queue == queue && node.line >= line && node.line <= end_line
                    })
                    .map(|mut node| {
                        node.depth -= depth;
                        node
                    })
                    .collect()
            }
            None => Vec::new(),
        };
    }
    if options.open_only {
        let mut folded: Vec<(String, usize, usize)> = Vec::new();
        nodes.retain(|node| {
            if folded.iter().any(|(queue, line, end)| {
                *queue == node.queue && node.line > *line && node.line <= *end
            }) {
                return false;
            }
            if node.closed {
                folded.push((node.queue.clone(), node.line, node.end_line));
                return false;
            }
            true
        });
    }
    nodes
}

/// Whether `options.item` matched nothing (so the CLI can fail loudly).
pub fn item_matched(tree: &QueueTree, options: &QueueTreeOptions) -> bool {
    options.item.is_none() || !tree.nodes.is_empty() || options.open_only
}

/// Render the compact text view: the summary, then one line per node.
pub fn render_text(tree: &QueueTree) -> String {
    let mut out = tree.summary.line();
    out.push('\n');
    let mut queue: Option<&str> = None;
    for node in &tree.nodes {
        if queue != Some(node.queue.as_str()) {
            out.push_str(&format!("queue {}\n", node.queue));
            queue = Some(node.queue.as_str());
        }
        let indent = "  ".repeat(node.depth + 1);
        let rollup = node
            .rollup
            .map(|rollup| {
                format!(
                    "  (you {} · agent {} · done {})",
                    rollup.you, rollup.agent, rollup.done
                )
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "{:>5} {indent}{} {} · {}{rollup}\n",
            format!("L{}", node.line),
            node.glyph,
            node.state.label(),
            node.text,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
<!-- agent:queue id=\"main\" -->
- ### ~~Ship alpha [#q-alpha]~~
  - ~~step one~~
- ### 🚧 [#subagent:beta#1] Build beta [#q-beta]
- ### ☑️ [#subagent:gamma#1] Fix gamma [#q-gamma]
- ### Plain row
- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->
  - ⟦you: 1 · agent: 0 · done: 0⟧ [#ask] Approve the ask. <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item ▫️
    - **Response:** Approve the ask. ▫️
      - **Timestamp:** 1970-01-01T00:00:00.000Z ▫️
  - ⟦📥 you: 0 · agent: 1 · done: 1⟧ [#answered] Pick a colour. <!-- agent-doc:waiting-item -->
    - **Next:** agent — pick up your reply ▫️
    - ~~older note~~
    - **Choices:** ▫️
      - &gt; **Queue prompt:** Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->: echo
    - blue please
<!-- /agent:queue -->
<!-- agent:todo -->
- [ ] [#ask] Approve the ask.
- [ ] [#answered] Pick a colour.
<!-- /agent:todo -->
";

    fn tree(options: QueueTreeOptions) -> QueueTree {
        queue_tree(DOC, &options).unwrap()
    }

    fn states(tree: &QueueTree) -> Vec<(usize, QueueNodeState, String)> {
        tree.nodes
            .iter()
            .map(|node| (node.depth, node.state, node.text.clone()))
            .collect()
    }

    #[test]
    fn every_queue_row_is_one_node_with_its_state() {
        let tree = tree(QueueTreeOptions::default());
        assert_eq!(
            states(&tree),
            vec![
                (0, QueueNodeState::Done, "Ship alpha [#q-alpha]".to_string()),
                (1, QueueNodeState::Done, "step one".to_string()),
                (
                    0,
                    QueueNodeState::Running,
                    "Build beta [#q-beta]".to_string()
                ),
                (
                    0,
                    QueueNodeState::NeedsIntegration,
                    "Fix gamma [#q-gamma]".to_string()
                ),
                (0, QueueNodeState::Open, "Plain row".to_string()),
                (
                    0,
                    QueueNodeState::Group,
                    "Waiting on you — answer by adding a child line under an item".to_string()
                ),
                (
                    1,
                    QueueNodeState::WaitingOnYou,
                    "[#ask] Approve the ask.".to_string()
                ),
                (
                    1,
                    QueueNodeState::Answered,
                    "[#answered] Pick a colour.".to_string()
                ),
                (2, QueueNodeState::Done, "older note".to_string()),
                (2, QueueNodeState::Reply, "blue please".to_string()),
            ],
            "{}",
            render_text(&tree)
        );
        let ask = tree
            .nodes
            .iter()
            .find(|node| node.id.as_deref() == Some("ask"))
            .unwrap();
        assert!(ask.waiting_item);
        assert_eq!(
            ask.rollup,
            Some(WaitingRollup {
                you: 1,
                agent: 0,
                done: 0
            })
        );
        assert_eq!(ask.line, 8);
        assert_eq!(
            ask.end_line, 11,
            "the fold covers the item's generated packet"
        );
    }

    #[test]
    fn the_summary_counts_states_in_operator_order() {
        let tree = tree(QueueTreeOptions::default());
        assert_eq!(
            tree.summary.line(),
            "9 items: 3 done · 1 waiting on you · 1 answered · 1 running · 1 needs integration · 1 open · 1 reply"
        );
        let rendered = render_text(&tree);
        assert!(rendered.starts_with("9 items: "), "{rendered}");
        assert!(rendered.contains("queue main\n"), "{rendered}");
        assert!(
            rendered.contains(
                "L8     🙋 waiting on you · [#ask] Approve the ask.  (you 1 · agent 0 · done 0)"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn open_only_folds_closed_subtrees() {
        let tree = tree(QueueTreeOptions {
            open_only: true,
            ..QueueTreeOptions::default()
        });
        assert!(
            tree.nodes.iter().all(|node| !node.closed),
            "{}",
            render_text(&tree)
        );
        assert!(
            !tree.nodes.iter().any(|node| node.text == "step one"),
            "a closed row's children fold with it"
        );
        assert!(tree.summary.line().starts_with("6 items: 1 waiting on you"));
    }

    #[test]
    fn item_selects_one_subtree_by_id() {
        for item in ["#answered", "do [#answered]", "answered", "[#answered]"] {
            let tree = tree(QueueTreeOptions {
                item: Some(item.to_string()),
                ..QueueTreeOptions::default()
            });
            assert_eq!(
                states(&tree),
                vec![
                    (
                        0,
                        QueueNodeState::Answered,
                        "[#answered] Pick a colour.".to_string()
                    ),
                    (1, QueueNodeState::Done, "older note".to_string()),
                    (1, QueueNodeState::Reply, "blue please".to_string()),
                ],
                "{item}"
            );
        }
        let missing = QueueTreeOptions {
            item: Some("#nope".to_string()),
            ..QueueTreeOptions::default()
        };
        assert!(!item_matched(&queue_tree(DOC, &missing).unwrap(), &missing));
    }

    #[test]
    fn the_view_is_read_only_json() {
        let tree = tree(QueueTreeOptions::default());
        let json = serde_json::to_value(&tree).unwrap();
        assert_eq!(json["summary"]["items"], 9);
        assert_eq!(json["nodes"][3]["state"], "needs-integration");
        assert_eq!(json["nodes"][0]["closed"], true);
        assert_eq!(json["nodes"][6]["rollup"]["you"], 1);
    }
}

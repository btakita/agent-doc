//! Tree-status query over the queue tree (`#treestatusquery`).
//!
//! The operator asks whether a queue tree or subtree has actionable or
//! deletable work, and the binary answers from the queue tree without a model
//! turn. [`tree_status`] is the one computation; it reads the same derived
//! state the closeout already projects:
//!
//! - each row's state from [`crate::queue_tree`] (lifecycle prefixes, struck
//!   rows, `🙋` gates);
//! - a live Waiting-on-you item's Next action and its decision subtrees from
//!   [`crate::waiting_status`] and [`crate::waiting_subtree`]
//!   (`#waitingtriage`);
//! - a struck row's closability from its response summary's `**State:**` row,
//!   the value the `**Guidance:**` row spells out.
//!
//! The answer lists, for the subtree rooted at the row on a given line, the
//! rows that wait on the operator (`Next: you`), the rows the agent owes
//! (`Next: agent`), the maximal subtrees that are safe to delete, and the rows
//! the binary cannot classify (`needs agent review`).
//!
//! Three surfaces read it: `agent-doc queue tree-status FILE --line N`, the
//! editor actions (through the FFI), and the document form, where preflight
//! replaces a `status?` query with binary-owned Status reply rows
//! ([`answer_status_queries`]). Only `status? deep` queues an agent turn, and
//! only when some row needs agent review. A `deletable?` child asks only
//! whether that prompt item tree is deletable; the answer is one Status row,
//! and the still-open query is not itself a reason to keep the tree open.
//!
//! This module is pure: it reads a document string and returns a new one.

use std::collections::HashSet;

use agent_doc_element::element;
use agent_doc_markdown_ast::tree_status_row::{
    self as syntax, StatusQueryKind, TREE_STATUS_ROW_LABEL,
};
use anyhow::{Result, anyhow};
use serde::Serialize;

use crate::document_queue::{self, QueueEntry};
use crate::queue_consume::{
    AGENT_DOC_REPLY_GLYPH, RESPONSE_STATE_COMPLETE, RESPONSE_STATE_FOLLOW_UP, RESPONSE_STATE_LABEL,
    RESPONSE_STATE_UNPUBLISHED, RESPONSE_STATE_WAITING_ON_OPERATOR,
};
use crate::queue_tree::{self, QueueNodeState, QueueTreeNode, QueueTreeOptions, QueueTreeSummary};
use crate::waiting_packet::{self, entry_list_indent, subtree_end};
use crate::waiting_status::{self, TRIAGE_MIN_SUBTREES};
use crate::waiting_subtree::{self, SubtreeNext, SubtreeStatus};

/// One row of a tree-status answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeStatusRow {
    /// 1-based first document line of the row.
    pub line: usize,
    /// 1-based last document line of the row's subtree (inclusive). Deleting
    /// `line..=end_line` deletes the subtree.
    pub end_line: usize,
    /// The row's state in the queue tree.
    pub state: QueueNodeState,
    /// The glyph an editor gutter shows for the state.
    pub glyph: String,
    /// One line of the row's work text, with binary projection stripped.
    pub text: String,
    /// The row's own `[#id]`, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the row needs next (or why it is deletable / needs review).
    pub action: String,
}

/// The answer to one tree-status query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeStatus {
    /// The queue component's `id` attribute, or its marker name.
    pub queue: String,
    /// 1-based first line of the queried subtree (the component's opening
    /// marker when the whole queue was asked about).
    pub line: usize,
    /// 1-based last line of the queried subtree (inclusive).
    pub end_line: usize,
    /// Whether the query covered the whole queue component.
    pub whole_queue: bool,
    /// The root row's text, or `queue <name>` for the whole queue.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// State counts over the rows in the subtree.
    pub summary: QueueTreeSummary,
    /// Rows waiting on the operator.
    pub you: Vec<TreeStatusRow>,
    /// Rows the agent owes.
    pub agent: Vec<TreeStatusRow>,
    /// Maximal subtrees that are safe to delete.
    pub deletable: Vec<TreeStatusRow>,
    /// Rows the binary cannot classify; `status? deep` queues an agent turn
    /// for them.
    pub needs_review: Vec<TreeStatusRow>,
}

impl TreeStatus {
    /// `1 you · 2 agent · 1 deletable · 0 need review`.
    pub fn counts_line(&self) -> String {
        format!(
            "{} you · {} agent · {} deletable · {} need review",
            self.you.len(),
            self.agent.len(),
            self.deletable.len(),
            self.needs_review.len()
        )
    }

    fn is_empty(&self) -> bool {
        self.you.is_empty()
            && self.agent.is_empty()
            && self.deletable.is_empty()
            && self.needs_review.is_empty()
    }
}

/// Which bucket a classified row lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    You,
    Agent,
    Deletable,
    NeedsReview,
}

#[derive(Default)]
struct Buckets {
    you: Vec<TreeStatusRow>,
    agent: Vec<TreeStatusRow>,
    deletable: Vec<TreeStatusRow>,
    needs_review: Vec<TreeStatusRow>,
}

impl Buckets {
    fn push(&mut self, bucket: Bucket, row: TreeStatusRow) {
        match bucket {
            Bucket::You => self.you.push(row),
            Bucket::Agent => self.agent.push(row),
            Bucket::Deletable => self.deletable.push(row),
            Bucket::NeedsReview => self.needs_review.push(row),
        }
    }
}

/// One queue component's parsed entries and their document lines.
struct ComponentView {
    queue: String,
    open_line: usize,
    close_line: usize,
    entries: Vec<QueueEntry>,
    /// 1-based first line of each entry.
    start_lines: Vec<usize>,
    /// 1-based last line of each entry's own span (inclusive).
    last_lines: Vec<usize>,
}

impl ComponentView {
    fn index_at_line(&self, line: usize) -> Option<usize> {
        self.start_lines.iter().position(|start| *start == line)
    }

    /// `(first, last)` document lines of the subtree rooted at `index`.
    fn subtree_lines(&self, index: usize) -> (usize, usize) {
        let end = subtree_end(&self.entries, index).max(index + 1);
        (self.start_lines[index], self.last_lines[end - 1])
    }

    fn contains_line(&self, line: usize) -> bool {
        self.open_line <= line && line <= self.close_line
    }
}

fn component_views(document: &str) -> Result<Vec<ComponentView>> {
    let mut views = Vec::new();
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
        let start_lines = spanned
            .iter()
            .map(|(_, span)| queue_tree::line_of(document, component.open_end + span.start))
            .collect();
        let last_lines = spanned
            .iter()
            .map(|(_, span)| {
                let last = (component.open_end + span.end)
                    .saturating_sub(1)
                    .max(component.open_end + span.start);
                queue_tree::line_of(document, last)
            })
            .collect();
        views.push(ComponentView {
            queue,
            open_line: queue_tree::line_of(document, component.open_start),
            close_line: queue_tree::line_of(document, component.close_start),
            entries: spanned.into_iter().map(|(entry, _)| entry).collect(),
            start_lines,
            last_lines,
        });
    }
    Ok(views)
}

/// `[#id]` written as `` `#id` `` so a rendered Status row never carries an id
/// reference, strike wrappers removed, whitespace collapsed.
fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("[#") {
        out.push_str(&rest[..at]);
        match rest[at..].find(']') {
            Some(close) => {
                out.push_str(&format!("`#{}`", &rest[at + 2..at + close]));
                rest = &rest[at + close + 1..];
            }
            None => {
                out.push_str(&rest[at..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.replace("~~", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A struck row's closability, read from its response summary's `**State:**`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closability {
    /// No response summary, or it says `Complete.`.
    Complete,
    WaitingOnOperator,
    Unpublished,
    FollowUp,
    /// A `**State:**` value this binary does not render.
    Unrecognized,
}

fn closability(
    document_lines: &[&str],
    node: &QueueTreeNode,
    scope: &[&QueueTreeNode],
) -> Closability {
    let descendants = scope
        .iter()
        .filter(|other| other.line > node.line && other.line <= node.end_line)
        .map(|other| (other.line, other.end_line))
        .collect::<Vec<_>>();
    for line in node.line..=node.end_line {
        if descendants
            .iter()
            .any(|(start, end)| *start <= line && line <= *end)
        {
            continue;
        }
        let Some(source) = document_lines.get(line - 1) else {
            continue;
        };
        let body = source.trim_start();
        let body = body.strip_prefix("- ").unwrap_or(body).trim();
        let Some(value) = body.strip_prefix(RESPONSE_STATE_LABEL) else {
            continue;
        };
        let value = value.trim();
        return if value.starts_with(RESPONSE_STATE_WAITING_ON_OPERATOR) {
            Closability::WaitingOnOperator
        } else if value.starts_with(RESPONSE_STATE_UNPUBLISHED) {
            Closability::Unpublished
        } else if value.starts_with(RESPONSE_STATE_FOLLOW_UP) {
            Closability::FollowUp
        } else if value.is_empty() || value.starts_with(RESPONSE_STATE_COMPLETE) {
            Closability::Complete
        } else {
            Closability::Unrecognized
        };
    }
    Closability::Complete
}

fn row_of(node: &QueueTreeNode, action: impl Into<String>) -> TreeStatusRow {
    TreeStatusRow {
        line: node.line,
        end_line: node.end_line,
        state: node.state,
        glyph: node.glyph.clone(),
        text: node.text.clone(),
        id: node.id.clone(),
        action: action.into(),
    }
}

/// The bucket and action of an ordinary row (outside any live waiting item).
fn classify_row(node: &QueueTreeNode, closability: Closability) -> Option<(Bucket, &'static str)> {
    use QueueNodeState as S;
    Some(match node.state {
        S::Done => match closability {
            Closability::Complete => return None,
            Closability::WaitingOnOperator => (
                Bucket::You,
                "answer the operator request its response summary records",
            ),
            Closability::Unpublished => (
                Bucket::Agent,
                "resolve the unpublished local work its response records (commit, install, publish receipts), or track it as a follow-up",
            ),
            Closability::FollowUp => (
                Bucket::NeedsReview,
                "its response lists follow-up next actions; agent-doc cannot tell who owns them",
            ),
            Closability::Unrecognized => (
                Bucket::NeedsReview,
                "its response summary records a state agent-doc does not recognize",
            ),
        },
        S::WaitingOnYou => (Bucket::You, "answer the operator gate on this row"),
        S::Answered => (Bucket::Agent, "pick up the operator's answer"),
        S::Running => (Bucket::Agent, "let the running attempt finish and report"),
        S::NeedsIntegration => (
            Bucket::Agent,
            "integrate the reported result, then close the row",
        ),
        S::Lost => (
            Bucket::Agent,
            "recover the lost attempt's result, or retry it",
        ),
        S::Reconciling => (
            Bucket::Agent,
            "reconcile the recorded completion, then close the row",
        ),
        S::Blocked => (
            Bucket::Agent,
            "held behind open prerequisite work; runs once that clears",
        ),
        S::Pending | S::Open => (Bucket::Agent, "pick up this queued item"),
        S::Reply => (Bucket::Agent, "pick up this operator reply"),
        S::Unknown => (
            Bucket::NeedsReview,
            "its lifecycle marker is in a conflicting state",
        ),
        S::Skipped => (
            Bucket::NeedsReview,
            "skipped; decide whether to retry it or strike it",
        ),
        S::Choice => (
            Bucket::NeedsReview,
            "an offered choice outside its Choices block",
        ),
        S::FollowUp => (Bucket::NeedsReview, "a legacy follow-up row"),
        S::Group => return None,
    })
}

fn subtree_bucket(subtree: &SubtreeStatus) -> (Bucket, String) {
    let stale = subtree.stale_note();
    match subtree.next() {
        SubtreeNext::You(ask) => {
            let mut action = ask;
            if subtree.agent() > 0 {
                action.push_str(&format!("; then agent — {}", subtree.agent_action()));
            }
            (Bucket::You, format!("{action}{stale}"))
        }
        SubtreeNext::Agent(action) => (Bucket::Agent, format!("{action}{stale}")),
        SubtreeNext::Deletable => (
            Bucket::Deletable,
            "every row here is struck or integrated".to_string(),
        ),
        SubtreeNext::Undetermined => (
            Bucket::NeedsReview,
            "no ask, attempt, or closure is recorded under it".to_string(),
        ),
    }
}

struct Context<'a> {
    view: &'a ComponentView,
    scope: &'a [&'a QueueTreeNode],
    open_todo_ids: &'a HashSet<String>,
    as_of: Option<i64>,
}

impl Context<'_> {
    fn node_at(&self, line: usize) -> Option<&QueueTreeNode> {
        self.scope.iter().copied().find(|node| node.line == line)
    }

    /// A row for the decision subtree rooted at entry `root`.
    fn subtree_row(&self, root: usize, subtree: &SubtreeStatus, action: String) -> TreeStatusRow {
        let (line, end_line) = self.view.subtree_lines(root);
        match self.node_at(line) {
            Some(node) => TreeStatusRow {
                end_line,
                ..row_of(node, action)
            },
            None => TreeStatusRow {
                line,
                end_line,
                state: QueueNodeState::Open,
                glyph: "·".to_string(),
                text: subtree.label.clone(),
                id: None,
                action,
            },
        }
    }

    /// Rows for one decision subtree, filtered by whether the item is
    /// triaged in full (two or more decisions) or only its deletable parts.
    fn push_subtree(
        &self,
        root: usize,
        subtree: &SubtreeStatus,
        all_buckets: bool,
        out: &mut Buckets,
    ) {
        let (bucket, action) = subtree_bucket(subtree);
        if bucket == Bucket::Deletable || all_buckets {
            out.push(bucket, self.subtree_row(root, subtree, action));
        }
    }

    /// A live Waiting-on-you item head: its Next action, then its decision
    /// subtrees.
    fn waiting_head(&self, node: &QueueTreeNode, index: usize, out: &mut Buckets) {
        let entries = &self.view.entries;
        let Some(status) = waiting_status::waiting_status(entries, index, self.open_todo_ids)
        else {
            return;
        };
        let triage =
            waiting_subtree::waiting_triage(entries, index, self.open_todo_ids, self.as_of);
        let action = status.triaged_next_action(&triage);
        let (bucket, action) = match action.strip_prefix("agent — ") {
            Some(rest) => (Bucket::Agent, rest.to_string()),
            None => (
                Bucket::You,
                action.strip_prefix("you — ").unwrap_or(&action).to_string(),
            ),
        };
        out.push(bucket, row_of(node, action));
        let full = triage.subtrees.len() >= TRIAGE_MIN_SUBTREES;
        for (_, subtree) in &triage.subtrees {
            self.push_subtree(subtree.root, subtree, full, out);
        }
    }

    /// A row inside a live waiting item's packet, asked about directly.
    fn packet_root(&self, node: &QueueTreeNode, head: usize, index: usize, out: &mut Buckets) {
        let entries = &self.view.entries;
        let subtree =
            waiting_subtree::subtree_status(entries, head, index, self.open_todo_ids, self.as_of);
        let (bucket, action) = subtree_bucket(&subtree);
        out.push(
            bucket,
            TreeStatusRow {
                end_line: self.view.subtree_lines(index).1,
                ..row_of(node, action)
            },
        );
        let end = subtree_end(entries, index);
        let triage = waiting_subtree::waiting_triage(entries, head, self.open_todo_ids, self.as_of);
        for (_, nested) in &triage.subtrees {
            if nested.root > index && nested.root < end {
                self.push_subtree(nested.root, nested, true, out);
            }
        }
    }

    /// The live waiting head that owns entry `index`, when it is not `index`.
    fn live_packet_owner(&self, index: usize) -> Option<usize> {
        waiting_packet::waiting_owner(&self.view.entries, index).filter(|head| {
            *head != index && matches!(self.view.entries.get(*head), Some(QueueEntry::Prompt(_)))
        })
    }

    fn is_live_waiting_head(&self, node: &QueueTreeNode, index: usize) -> bool {
        node.waiting_item
            && !node.closed
            && matches!(self.view.entries.get(index), Some(QueueEntry::Prompt(_)))
    }
}

/// The status of the subtree rooted at the row on `line` (1-based) of
/// `document`: the deepest queue row whose subtree contains the line, or the
/// whole queue component when the line is inside one but on no row.
pub fn tree_status(document: &str, line: usize) -> Result<TreeStatus> {
    let views = component_views(document)?;
    let view = views
        .iter()
        .find(|view| view.contains_line(line))
        .ok_or_else(|| anyhow!("line {line} is not inside a queue component"))?;
    let tree = queue_tree::queue_tree(document, &QueueTreeOptions::default())?;
    let component_nodes = tree
        .nodes
        .iter()
        .filter(|node| view.contains_line(node.line))
        .collect::<Vec<_>>();
    let root = component_nodes
        .iter()
        .copied()
        .filter(|node| node.line <= line && line <= node.end_line)
        .max_by_key(|node| node.depth);
    let scope = match root {
        Some(root) => component_nodes
            .iter()
            .copied()
            .filter(|node| node.line >= root.line && node.line <= root.end_line)
            .collect::<Vec<_>>(),
        None => component_nodes.clone(),
    };
    let open_todo_ids = crate::queue_consume::open_waiting_todo_ids(document);
    let as_of = crate::queue_consume::latest_response_timestamp(document)
        .and_then(waiting_subtree::parse_timestamp_minutes)
        .map(|(minutes, _)| minutes);
    let context = Context {
        view,
        scope: &scope,
        open_todo_ids: &open_todo_ids,
        as_of,
    };
    let document_lines = document.lines().collect::<Vec<_>>();

    let mut out = Buckets::default();
    let mut covered: Vec<(usize, usize)> = Vec::new();
    let is_covered = |covered: &[(usize, usize)], line: usize| {
        covered
            .iter()
            .any(|(start, end)| *start <= line && line <= *end)
    };
    for node in &scope {
        if is_covered(&covered, node.line) || node.state == QueueNodeState::Group {
            continue;
        }
        let Some(index) = view.index_at_line(node.line) else {
            continue;
        };
        if context.is_live_waiting_head(node, index) {
            context.waiting_head(node, index, &mut out);
            covered.push((node.line, node.end_line));
            continue;
        }
        if let Some(head) = context.live_packet_owner(index) {
            context.packet_root(node, head, index, &mut out);
            covered.push((node.line, node.end_line));
            continue;
        }
        let closable = if node.state == QueueNodeState::Done {
            closability(&document_lines, node, &scope)
        } else {
            Closability::Complete
        };
        if let Some((bucket, action)) = classify_row(node, closable) {
            out.push(bucket, row_of(node, action));
        }
    }

    // Deletable subtrees outside live waiting items: every row struck, every
    // response summary complete, recursively. Only the outermost is listed.
    let eligible = scope
        .iter()
        .copied()
        .filter(|node| {
            view.index_at_line(node.line).is_some_and(|index| {
                !context.is_live_waiting_head(node, index)
                    && context.live_packet_owner(index).is_none()
            })
        })
        .collect::<Vec<_>>();
    let mut deletable = vec![false; eligible.len()];
    for at in (0..eligible.len()).rev() {
        let node = eligible[at];
        if node.state != QueueNodeState::Done
            || closability(&document_lines, node, &scope) != Closability::Complete
        {
            continue;
        }
        let children_ok = scope
            .iter()
            .filter(|other| {
                other.line > node.line
                    && other.line <= node.end_line
                    && other.depth == node.depth + 1
            })
            .all(|child| {
                eligible
                    .iter()
                    .position(|candidate| candidate.line == child.line)
                    .is_some_and(|position| deletable[position])
            });
        deletable[at] = children_ok;
    }
    let mut listed: Vec<(usize, usize)> = Vec::new();
    for (at, node) in eligible.iter().enumerate() {
        if !deletable[at] || is_covered(&listed, node.line) {
            continue;
        }
        listed.push((node.line, node.end_line));
        out.deletable.push(row_of(
            node,
            "every row is struck and its response records no remaining work",
        ));
    }
    out.deletable.sort_by_key(|row| row.line);

    let (line, end_line, text, id) = match root {
        Some(root) => (root.line, root.end_line, root.text.clone(), root.id.clone()),
        None => (
            view.open_line,
            view.close_line,
            format!("queue {}", view.queue),
            None,
        ),
    };
    Ok(TreeStatus {
        queue: view.queue.clone(),
        line,
        end_line,
        whole_queue: root.is_none(),
        text,
        id,
        summary: QueueTreeSummary::of(
            &scope.iter().map(|node| (*node).clone()).collect::<Vec<_>>(),
        ),
        you: out.you,
        agent: out.agent,
        deletable: out.deletable,
        needs_review: out.needs_review,
    })
}

fn line_range(row: &TreeStatusRow) -> String {
    if row.end_line > row.line {
        format!("L{}–L{}", row.line, row.end_line)
    } else {
        format!("L{}", row.line)
    }
}

/// The compact text answer `agent-doc queue tree-status` prints.
pub fn render_text(status: &TreeStatus) -> String {
    let mut out = format!(
        "{} {} — {}\n",
        line_range(&TreeStatusRow {
            line: status.line,
            end_line: status.end_line,
            state: QueueNodeState::Open,
            glyph: String::new(),
            text: String::new(),
            id: None,
            action: String::new(),
        }),
        status.text,
        status.counts_line()
    );
    out.push_str(&format!("{}\n", status.summary.line()));
    let sections: [(&str, &[TreeStatusRow]); 4] = [
        ("Next: you", &status.you),
        ("Next: agent", &status.agent),
        ("Deletable", &status.deletable),
        ("Needs agent review", &status.needs_review),
    ];
    for (title, rows) in sections {
        if rows.is_empty() {
            out.push_str(&format!("{title}: none\n"));
            continue;
        }
        out.push_str(&format!("{title}:\n"));
        for row in rows {
            out.push_str(&format!(
                "  {:>9} {} {} — {}\n",
                line_range(row),
                row.glyph,
                row.text,
                row.action
            ));
        }
    }
    if !status.needs_review.is_empty() {
        out.push_str(
            "Add a `status? deep` child line under the row to queue one agent turn for the rows that need review.\n",
        );
    }
    out
}

/// The deep query's agent prompt, written beneath the Status rows when some
/// row needs agent review.
pub const DEEP_REVIEW_PROMPT: &str = "Review the rows the Status reply above lists under **Needs agent review** and record what each one needs (asked with `status? deep`)";

/// The binary-owned Status reply rows for `status`, rendered at `indent`.
pub fn reply_rows(status: &TreeStatus, indent: usize, kind: StatusQueryKind) -> Vec<String> {
    let pad = " ".repeat(indent);
    let child = " ".repeat(indent + 2);
    let deep = match kind {
        StatusQueryKind::Plain | StatusQueryKind::Deletable => String::new(),
        StatusQueryKind::Deep if status.needs_review.is_empty() => {
            " · deep: no row needs agent review, so no agent turn was queued".to_string()
        }
        StatusQueryKind::Deep => {
            " · deep: one agent turn queued below for the rows that need review".to_string()
        }
    };
    let headline = if status.is_empty() {
        format!("nothing open — {}", status.counts_line())
    } else {
        status.counts_line()
    };
    let mut rows = vec![format!(
        "{pad}- {TREE_STATUS_ROW_LABEL} snapshot — {headline}{deep} {AGENT_DOC_REPLY_GLYPH}"
    )];
    let sections: [(&str, &[TreeStatusRow]); 4] = [
        ("**Next: you:**", &status.you),
        ("**Next: agent:**", &status.agent),
        ("**Deletable:**", &status.deletable),
        ("**Needs agent review:**", &status.needs_review),
    ];
    for (label, items) in sections {
        for row in items {
            rows.push(format!(
                "{child}- {label} “{}” — {} {AGENT_DOC_REPLY_GLYPH}",
                sanitize(&row.text),
                sanitize(&row.action)
            ));
        }
    }
    rows
}

/// One status query found in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Query {
    /// 1-based line of the query (the `status?` line, or the row carrying the
    /// `[status?]` token).
    line: usize,
    kind: StatusQueryKind,
    form: QueryForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryForm {
    /// A `status?` child line at this list indent.
    ChildLine { indent: usize },
    /// A `[status?]` token on the row itself (entry index in its component).
    Token,
}

/// A source line's text without its list bullet, as the typing gate reads it.
fn item_text(source: &str) -> &str {
    let trimmed = source.trim();
    trimmed.strip_prefix("- ").unwrap_or(trimmed).trim()
}

fn first_query(document: &str, allow: &dyn Fn(&str) -> bool) -> Result<Option<Query>> {
    let lines = document.split('\n').collect::<Vec<_>>();
    for view in component_views(document)? {
        for (index, entry) in view.entries.iter().enumerate() {
            let line = view.start_lines[index];
            let source = lines.get(line - 1).copied().unwrap_or_default();
            match entry {
                QueueEntry::Freeform(_) => {
                    if let (Some(kind), Some(indent)) = (
                        syntax::status_query_line_kind(source),
                        entry_list_indent(entry),
                    ) && allow(item_text(source))
                    {
                        return Ok(Some(Query {
                            line,
                            kind,
                            form: QueryForm::ChildLine { indent },
                        }));
                    }
                }
                QueueEntry::Prompt(_) | QueueEntry::Completed(_) => {
                    if let Some((_, kind)) = syntax::find_status_query_token(source)
                        && allow(item_text(source))
                    {
                        return Ok(Some(Query {
                            line,
                            kind,
                            form: QueryForm::Token,
                        }));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

/// One answered query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsweredStatusQuery {
    pub queue: String,
    /// The queried row's text, or `queue <name>`.
    pub target: String,
    pub kind: StatusQueryKind,
    /// `1 you · 2 agent · 1 deletable · 0 need review`.
    pub counts: String,
    /// Whether a deep query queued an agent turn.
    pub queued_agent_turn: bool,
}

/// A document with its status queries answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusQueryAnswers {
    pub content: String,
    pub answered: Vec<AnsweredStatusQuery>,
}

/// Remove the Status rows that answer an earlier query for the same target:
/// Status heads at `indent` inside `first..=last` (1-based) and the rows
/// nested beneath them. Returns how many removed lines preceded `before`.
fn remove_old_status_rows(
    lines: &mut Vec<String>,
    first: usize,
    last: usize,
    indent: usize,
    before: usize,
) -> usize {
    let view = lines
        .iter()
        .map(|line| line.trim_end_matches('\n'))
        .collect::<Vec<_>>();
    let mut remove = Vec::new();
    let mut index = first.saturating_sub(1);
    let last = last.min(view.len());
    while index < last {
        let own_indent = view[index].len() - view[index].trim_start_matches([' ', '\t']).len();
        if own_indent == indent && syntax::is_tree_status_head_row_at(&view, index) {
            remove.push(index);
            let mut child = index + 1;
            while child < last && syntax::is_tree_status_row_at(&view, child) {
                remove.push(child);
                child += 1;
            }
            index = child;
            continue;
        }
        index += 1;
    }
    let preceding = remove.iter().filter(|at| **at + 1 < before).count();
    for at in remove.into_iter().rev() {
        lines.remove(at);
    }
    preceding
}

/// One `**Status:**` row answering `deletable?`.
fn deletable_reply_row(verdict: &str, sentence: &str, indent: usize) -> String {
    format!(
        "{}- {TREE_STATUS_ROW_LABEL} {verdict} — {sentence} {AGENT_DOC_REPLY_GLYPH}",
        " ".repeat(indent)
    )
}

/// Whether the prompt item on `target_line` (or the whole queue, when that
/// line is the queue marker) is safe to delete. The `deletable?` row is
/// already gone, and a still-open query is not a queue-tree node, so it
/// cannot be the named open row.
fn deletable_answer_sentence(document: &str, target_line: usize) -> Result<String> {
    let tree = queue_tree::queue_tree(document, &QueueTreeOptions::default())?;
    if let Some(index) = queue_tree::node_at_line(&tree, target_line) {
        let open = queue_tree::row_closure(&tree, index).and_then(|closure| {
            closure.first_open_nested.or_else(|| {
                queue_tree::row_keeps_tree_alive(&tree, index)
                    .then(|| queue_tree::RowRef::of(&tree.nodes[index]))
            })
        });
        return Ok(match open {
            None => {
                "Safe to delete this prompt item and every row under it; all of them are struck."
                    .to_string()
            }
            Some(open) => format!("Keep this prompt item: {} is still open.", open.label()),
        });
    }
    let views = component_views(document)?;
    let view = views.iter().find(|view| view.contains_line(target_line));
    let open = tree.nodes.iter().enumerate().find(|(index, node)| {
        view.is_none_or(|view| view.contains_line(node.line))
            && queue_tree::row_keeps_tree_alive(&tree, *index)
    });
    Ok(match open {
        None => "Safe to delete this queue; every prompt item in it is struck.".to_string(),
        Some((_, node)) => format!(
            "Keep this queue: {} is still open.",
            queue_tree::RowRef::of(node).label()
        ),
    })
}

fn answer_query(document: &str, query: Query) -> Result<(String, AnsweredStatusQuery)> {
    let views = component_views(document)?;
    let view = views
        .iter()
        .find(|view| view.contains_line(query.line))
        .ok_or_else(|| anyhow!("status query on line {} is outside a queue", query.line))?;
    let mut lines = document
        .split_inclusive('\n')
        .map(str::to_string)
        .collect::<Vec<_>>();
    let tree = queue_tree::queue_tree(document, &QueueTreeOptions::default())?;
    let (target_line, status_indent, insert_at) = match query.form {
        QueryForm::ChildLine { indent } => {
            // The deepest row whose subtree holds the query line is its
            // parent; none means the query asks about the whole queue.
            let parent = tree
                .nodes
                .iter()
                .filter(|node| {
                    view.contains_line(node.line)
                        && node.line < query.line
                        && query.line <= node.end_line
                })
                .max_by_key(|node| node.depth);
            let target = parent.map_or(view.open_line, |node| node.line);
            let (first, last) = parent.map_or((view.open_line, view.close_line), |node| {
                (node.line + 1, node.end_line)
            });
            lines.remove(query.line - 1);
            let removed_before = remove_old_status_rows(
                &mut lines,
                first,
                last.saturating_sub(1),
                indent,
                query.line,
            );
            (target, indent, query.line - 1 - removed_before)
        }
        QueryForm::Token => {
            let index = view
                .index_at_line(query.line)
                .ok_or_else(|| anyhow!("status query row on line {} not found", query.line))?;
            let source = &lines[query.line - 1];
            let (bare, newline) = match source.strip_suffix('\n') {
                Some(bare) => (bare, "\n"),
                None => (source.as_str(), ""),
            };
            let (stripped, _) = syntax::strip_status_query_token(bare)
                .ok_or_else(|| anyhow!("status query token on line {} not found", query.line))?;
            lines[query.line - 1] = format!("{stripped}{newline}");
            let indent = entry_list_indent(&view.entries[index]).unwrap_or(0) + 2;
            let own_last = view.last_lines[index];
            let (_, subtree_last) = view.subtree_lines(index);
            remove_old_status_rows(&mut lines, own_last + 1, subtree_last, indent, own_last + 1);
            (query.line, indent, own_last)
        }
    };
    let cleaned = lines.concat();
    let status = tree_status(&cleaned, target_line)?;
    let (rows, counts, queued_agent_turn) = if query.kind == StatusQueryKind::Deletable {
        let sentence = deletable_answer_sentence(&cleaned, target_line)?;
        let verdict = if sentence.starts_with("Safe to delete") {
            "deletable"
        } else {
            "not deletable"
        };
        (
            vec![deletable_reply_row(verdict, &sentence, status_indent)],
            verdict.to_string(),
            false,
        )
    } else {
        let mut rows = reply_rows(&status, status_indent, query.kind);
        let queued_agent_turn =
            query.kind == StatusQueryKind::Deep && !status.needs_review.is_empty();
        if queued_agent_turn {
            rows.push(format!(
                "{}- {DEEP_REVIEW_PROMPT}",
                " ".repeat(status_indent)
            ));
        }
        (rows, status.counts_line(), queued_agent_turn)
    };
    // A document without a trailing newline keeps that shape: the inserted
    // rows always end in one, and the row before them gains one if needed.
    if insert_at > 0
        && let Some(previous) = lines.get_mut(insert_at - 1)
        && !previous.ends_with('\n')
    {
        previous.push('\n');
    }
    let insert_at = insert_at.min(lines.len());
    for (offset, row) in rows.into_iter().enumerate() {
        lines.insert(insert_at + offset, format!("{row}\n"));
    }
    let answered = AnsweredStatusQuery {
        queue: status.queue.clone(),
        target: status.text.clone(),
        kind: query.kind,
        counts,
        queued_agent_turn,
    };
    Ok((lines.concat(), answered))
}

/// The upper bound on queries answered in one pass (each answer removes its
/// query, so a well-formed document never reaches it).
const MAX_QUERIES_PER_PASS: usize = 64;

/// Answer every status query in `document`'s queue components: each
/// `status?` child line, or `[status?]` token, becomes Status reply rows at
/// the queried row's first child position. `None` when there is no query.
pub fn answer_status_queries(document: &str) -> Result<Option<StatusQueryAnswers>> {
    answer_status_queries_settled(document, &|_| true)
}

/// [`answer_status_queries`], answering only the queries whose source line
/// `allow` accepts (preflight passes its typing gate, so a query the operator
/// is still typing, such as `status?` on its way to `status? deep`, waits).
pub fn answer_status_queries_settled(
    document: &str,
    allow: &dyn Fn(&str) -> bool,
) -> Result<Option<StatusQueryAnswers>> {
    let lower = document.to_ascii_lowercase();
    if !lower.contains(syntax::STATUS_QUERY) && !lower.contains(syntax::DELETABLE_QUERY) {
        return Ok(None);
    }
    let mut content = document.to_string();
    let mut answered = Vec::new();
    for _ in 0..MAX_QUERIES_PER_PASS {
        let Some(query) = first_query(&content, allow)? else {
            break;
        };
        let (next, answer) = answer_query(&content, query)?;
        if next == content {
            break;
        }
        content = next;
        answered.push(answer);
    }
    Ok((!answered.is_empty()).then_some(StatusQueryAnswers { content, answered }))
}

/// `body` without status queries and Status reply rows, for comparing a
/// snapshot with a document across an answered query.
fn without_status_artifacts(body: &str) -> String {
    let lines = body.split_inclusive('\n').collect::<Vec<_>>();
    let trimmed = lines
        .iter()
        .map(|line| line.trim_end_matches('\n'))
        .collect::<Vec<_>>();
    let mut out = String::with_capacity(body.len());
    for (index, line) in lines.iter().enumerate() {
        if syntax::status_query_line_kind(trimmed[index]).is_some()
            || syntax::is_tree_status_row_at(&trimmed, index)
        {
            continue;
        }
        match syntax::strip_status_query_token(trimmed[index]) {
            Some((stripped, _)) => {
                out.push_str(&stripped);
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Carry answered status queries into `snapshot`, per queue component, when
/// the snapshot's queue body equals `before`'s apart from status queries and
/// Status rows (no other unsnapshotted operator queue edit). A component whose
/// deep query queued an agent turn is not carried, so that prompt reaches the
/// agent as a diff. `None` when nothing is carried.
pub fn carry_status_answers_into_snapshot(
    snapshot: &str,
    before: &str,
    after: &str,
) -> Option<String> {
    let queues = |document: &str| {
        element::parse(document).ok().map(|components| {
            components
                .into_iter()
                .filter(|component| component.name == "queue")
                .collect::<Vec<_>>()
        })
    };
    let (snap, prior, next) = (queues(snapshot)?, queues(before)?, queues(after)?);
    if snap.len() != prior.len() || prior.len() != next.len() {
        return None;
    }
    let mut carried = snapshot.to_string();
    let mut changed = false;
    for index in (0..snap.len()).rev() {
        let snap_body = snap[index].content(snapshot);
        let before_body = prior[index].content(before);
        let after_body = next[index].content(after);
        if before_body == after_body
            || snap_body == after_body
            || after_body.contains(DEEP_REVIEW_PROMPT) && !before_body.contains(DEEP_REVIEW_PROMPT)
            || without_status_artifacts(snap_body) != without_status_artifacts(before_body)
        {
            continue;
        }
        carried.replace_range(snap[index].open_end..snap[index].close_start, after_body);
        changed = true;
    }
    changed.then_some(carried)
}

#[cfg(test)]
#[path = "tree_status_tests.rs"]
mod tests;

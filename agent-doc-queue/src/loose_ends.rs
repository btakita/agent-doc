//! `#looseends`: one binary-owned digest of every unresolved queue row.
//!
//! A session document grows long queue trees: operator decisions and
//! actions (`🙋`), rows held behind a blocker (`⛔`), and rows a worker is
//! running. The operator had to read the whole queue to find out what was
//! still open and who owed the next step. `agent:loose-ends` lists them in
//! one place near the top of the document, one line per row:
//!
//! ```text
//! ## Loose ends <!-- agent:loose-ends -->
//! - **Decide** (1)
//!   - `[#q-0e29d5fdf1c4]` decide whether to keep separate 262 and 263 ZIPs
//! - **Do** (1)
//!   - `[#q-cb3ac1276fd3]` install `vsce` and refresh the VS Code package
//! - **Blocked** (1)
//!   - `[#jbfrontendswap]` blocked by `[#prompt-blocker-b0ddfbae2f94]`: Use chromium-bridge to perform the steps
//! - **Running** (1)
//!   - `[#subagent:recapids#1]` The recap says … (owner `subagent:recapids#1`)
//! <!-- /agent:loose-ends -->
//! ```
//!
//! # Source
//!
//! The digest is a pure projection of the read-only queue tree
//! ([`crate::queue_tree`], `#queuetreeview`), the same node states the
//! editors' gutter icons and `agent-doc queue tree` show:
//!
//! - **Decide** and **Do**: live rows waiting on the operator (`🙋`, a
//!   Waiting-on-you item head, or a row carrying `[operator-verify]`). A row
//!   that asks the operator to decide or choose (or ends in a question) is a
//!   decision; any other operator row is an action. A row whose own owner
//!   label is `agent:` is agent work and never appears in either group.
//! - **Blocked**: live `⛔` rows, each with the blocker id painted on it
//!   (`⟦blocked by …⟧`) and a short reason: the blocker row's own text when
//!   the blocker is a row of this document, else the row's text.
//! - **Running**: claimed or running rows, with the attempt owner label
//!   (`[#subagent:x#1]`). The marker is the one preflight and `queue sync`
//!   paint from the durable claim ledger in the same pass, before the digest
//!   is rendered, so it agrees with `agent:active`'s Running group.
//!
//! # Why a separate component and not `agent:active`
//!
//! `agent:active` is a `#managedlistnodrift` projection of the durable
//! claim ledger: session-check, retained-delivery landing checks and late
//! commit recovery compare it byte-for-byte with `render(durable state)`,
//! and it deliberately excludes blocked and operator-gated rows. Decide, Do
//! and Blocked are queue-row states, not attempt states, so folding them into
//! `agent:active` would make that invariant depend on queue text it promises
//! never to read. The digest instead reads the one queue tree projection that
//! already owns every row state, and its Running group comes from the same
//! ledger-painted markers.
//!
//! # Rules
//!
//! One line per row: the row's id (`[#q-…]`, `[#slug]`,
//! `[#prompt-blocker-…]`; a row without one is named by its attempt owner
//! label, else by `queue-free:<hash>` as `agent:active` does), then a short
//! imperative. Empty groups are omitted; with every group empty the body is
//! `Nothing open`. Mirrored rows (the same id in two queues) list once.
//! Lines an operator types under the digest are kept. A second render is a
//! no-op (byte-stable).

use std::collections::HashSet;

use agent_doc_element::element;
use anyhow::Result;

use crate::followup_owner::{
    ActionItemOwner, carries_operator_owned_tag, explicit_owner_label, followup_row_action_item,
};
use crate::queue_tree::{QueueNodeState, QueueTreeNode, QueueTreeOptions, queue_tree};

/// The component name (`<!-- agent:loose-ends -->`).
pub const LOOSE_ENDS_COMPONENT: &str = "loose-ends";

/// The heading the component is inserted under.
pub const LOOSE_ENDS_HEADING: &str = "## Loose ends";

/// The body when nothing is open.
pub const NOTHING_OPEN: &str = "Nothing open";

/// The longest entry text.
const ENTRY_TEXT_LIMIT: usize = 100;

/// A digest group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LooseEndGroup {
    /// The operator decides or chooses.
    Decide,
    /// The operator acts.
    Do,
    /// Held behind a blocker.
    Blocked,
    /// A worker attempt is claimed or running.
    Running,
}

impl LooseEndGroup {
    pub const ALL: [LooseEndGroup; 4] = [
        LooseEndGroup::Decide,
        LooseEndGroup::Do,
        LooseEndGroup::Blocked,
        LooseEndGroup::Running,
    ];

    pub fn title(self) -> &'static str {
        match self {
            LooseEndGroup::Decide => "Decide",
            LooseEndGroup::Do => "Do",
            LooseEndGroup::Blocked => "Blocked",
            LooseEndGroup::Running => "Running",
        }
    }
}

/// One digest line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LooseEnd {
    pub group: LooseEndGroup,
    /// The row's handle without brackets: its own id, its attempt owner label
    /// (`subagent:x#1`), or `queue-free:<hash>`.
    pub id: String,
    /// Short imperative (Blocked: `blocked by …: reason`; Running: the work
    /// with its owner label).
    pub text: String,
}

impl LooseEnd {
    fn handle(&self) -> String {
        if self.id.starts_with("queue-free:") {
            format!("`{}`", self.id)
        } else {
            format!("`[#{}]`", self.id)
        }
    }

    fn line(&self) -> String {
        format!("  - {} {}", self.handle(), self.text)
    }
}

/// The first line of the row in `document` at 1-based `line`, without the
/// list marker and heading prefix.
fn raw_row(document: &str, line: usize) -> &str {
    let raw = document
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or_default()
        .trim();
    let raw = raw
        .strip_prefix("- ")
        .or_else(|| raw.strip_prefix("* "))
        .or_else(|| raw.strip_prefix("+ "))
        .unwrap_or(raw);
    raw.strip_prefix("### ").unwrap_or(raw)
}

/// `text` without a leading `[from:… key=…]` provenance token.
fn without_provenance(text: &str) -> &str {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("[from:")
        && let Some(close) = rest.find(']')
    {
        return rest[close + 1..].trim_start();
    }
    trimmed
}

/// The row's body after a follow-up row's origin quote, and its owner label.
fn row_body(full_text: &str) -> (Option<ActionItemOwner>, String) {
    let text = without_provenance(full_text);
    let body = followup_row_action_item(text).unwrap_or(text);
    (explicit_owner_label(body), body.to_string())
}

/// `text` with inline code spans and quoted passages (`"…"`, `“…”`) blanked,
/// so a quoted row or command never reads as the row's own request.
fn mask_quotes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut closing: Option<char> = None;
    for ch in text.chars() {
        match closing {
            Some(close) if ch == close => {
                closing = None;
                out.push(' ');
            }
            Some(_) => out.extend(std::iter::repeat_n(' ', ch.len_utf8())),
            None => match ch {
                '`' => {
                    closing = Some('`');
                    out.push(' ');
                }
                '"' => {
                    closing = Some('"');
                    out.push(' ');
                }
                '“' => {
                    closing = Some('”');
                    out.extend(std::iter::repeat_n(' ', ch.len_utf8()));
                }
                _ => out.push(ch),
            },
        }
    }
    out
}

/// The decision clause of an operator body: `decide whether …` / `choose …`
/// where it is not the first clause (`the live doc has …; decide whether to
/// clean those up`), as a byte offset into `body`.
fn decision_clause_at(body: &str) -> Option<usize> {
    let masked = mask_quotes(body).to_ascii_lowercase();
    [
        "decide whether",
        "decide on ",
        "choose between",
        "choose whether",
    ]
    .iter()
    .filter_map(|phrase| masked.find(phrase))
    .min()
}

/// The short imperative of an operator row, without its `operator:` label
/// (the group already says who acts), and whether it asks for a decision. A
/// `human (<who>):` label stays: it names the person.
fn operator_imperative(full_text: &str, handle: &str) -> (String, bool) {
    let (_, body) = row_body(full_text);
    let body = without_handle(&body, handle);
    let (label, rest) = split_label(body);
    let lower = rest.trim_start_matches(['*', '_']).to_ascii_lowercase();
    const OPENERS: [&str; 8] = [
        "decide",
        "choose",
        "pick ",
        "select ",
        "approve or",
        "confirm whether",
        "whether ",
        "your call",
    ];
    let opens_with_decision = OPENERS.iter().any(|opener| lower.starts_with(opener));
    let clause = if opens_with_decision {
        None
    } else {
        decision_clause_at(rest)
    };
    let decision = opens_with_decision || clause.is_some() || is_question(rest);
    let text = match clause {
        Some(at) => &rest[at..],
        None => rest,
    };
    let short = crate::response_remaining::short_row_text(text, ENTRY_TEXT_LIMIT);
    let short = match label {
        Some(label) if !label.eq_ignore_ascii_case("operator") => format!("{label}: {short}"),
        _ => short,
    };
    (short, decision)
}

/// `(label, rest)` for a body that opens with an owner label (`operator:`,
/// `human (who):`).
fn split_label(body: &str) -> (Option<&str>, &str) {
    if explicit_owner_label(body).is_some()
        && let Some(colon) = body.find(':')
    {
        let label = body[..colon].trim_matches(|c: char| c == '*' || c == '_' || c.is_whitespace());
        return (
            Some(label),
            body[colon + 1..].trim_start_matches(['*', '_']).trim(),
        );
    }
    (None, body.trim())
}

/// Whether the work text is a question: it ends in `?` once trailing
/// `[#id]` and `[operator-verify]` annotations are set aside.
fn is_question(text: &str) -> bool {
    let mut tail = text.trim_end();
    while tail.ends_with(']')
        && let Some(open) = tail.rfind('[')
    {
        tail = tail[..open].trim_end();
    }
    tail.ends_with('?')
}

/// `text` without a leading `[#handle]` and the `:` after it: the line
/// already names the row by its handle.
fn without_handle<'a>(text: &'a str, handle: &str) -> &'a str {
    let trimmed = text.trim_start();
    match trimmed.strip_prefix(&format!("[#{handle}]")) {
        Some(rest) => rest.trim_start_matches(':').trim_start(),
        None => {
            // Ids are matched case-insensitively.
            let open = format!("[#{handle}]");
            if trimmed.len() >= open.len()
                && trimmed.is_char_boundary(open.len())
                && trimmed[..open.len()].eq_ignore_ascii_case(&open)
            {
                trimmed[open.len()..].trim_start_matches(':').trim_start()
            } else {
                trimmed
            }
        }
    }
}

/// The first sentence of `text` (cut at `. ` outside code), bounded to
/// `limit` characters at a word boundary.
fn first_sentence(text: &str, limit: usize) -> String {
    let text = text.trim();
    let masked = mask_quotes(text);
    let cut = masked
        .match_indices(". ")
        .map(|(at, _)| at)
        .find(|at| text[..*at].chars().count() >= 12)
        .unwrap_or(text.len());
    let mut sentence = text[..cut].trim_end_matches(['.', ' ']).to_string();
    if sentence.chars().count() > limit {
        let byte_limit = sentence
            .char_indices()
            .nth(limit.saturating_sub(1))
            .map_or(sentence.len(), |(at, _)| at);
        let space = sentence[..byte_limit].rfind(' ').unwrap_or(byte_limit);
        sentence.truncate(space);
        if sentence.matches('`').count() % 2 == 1
            && let Some(open) = sentence.rfind('`')
        {
            sentence.truncate(open);
        }
        let trimmed = sentence.trim_end_matches(['.', ',', ':', ';', ' ']).len();
        sentence.truncate(trimmed);
        sentence.push('…');
    }
    sentence
}

fn queue_free_id(text: &str) -> String {
    let normalized = agent_doc_element_queue::normalize_multiline_dedup_text(text);
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "queue-free:{}",
        agent_doc_hash::short_content_hash(&normalized)
    )
}

/// The row's handle: its own id (an authored `[#slug]`, a leading
/// `[#prompt-blocker-…]`, or the binary's `[#q-…]`), else its attempt owner
/// label, else a `queue-free:` hash of its work text.
fn row_handle(full_text: &str, raw: &str) -> String {
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(raw).trim();
    if let Some(id) = agent_doc_element_queue::queue_prompt_stable_id(work) {
        return id;
    }
    if agent_doc_element_queue::followup_origin_reference(work).is_none()
        && let Some(rest) = work.strip_prefix("[#")
        && let Some(close) = rest.find(']')
        && !rest[..close].is_empty()
        && rest[..close]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return rest[..close].to_ascii_lowercase();
    }
    if let Some(owner) = agent_doc_document::queue_projection::owner_projection(raw) {
        return format!("{}#{}", owner.owner, owner.generation);
    }
    queue_free_id(full_text)
}

/// The live queue row whose handle is `id`, for a blocker reason.
fn row_by_handle<'a>(
    document: &str,
    nodes: &'a [QueueTreeNode],
    id: &str,
) -> Option<&'a QueueTreeNode> {
    nodes.iter().find(|node| {
        !node.closed && row_handle(&node.full_text, raw_row(document, node.line)) == id
    })
}

/// Work text of the tracked item (backlog/review/icebox/todo) `id`.
fn tracked_item_text(document: &str, id: &str) -> Option<String> {
    let components = element::parse(document).ok()?;
    for component in &components {
        let name = element::canonical_component_name(&component.name);
        if !matches!(name, "backlog" | "pending" | "review" | "icebox" | "todo") {
            continue;
        }
        let (_, items, _) =
            agent_doc_element_backlog::backlog::parse_items(component.content(document));
        if let Some(item) = items
            .into_iter()
            .find(|item| !item.is_done() && item.id.eq_ignore_ascii_case(id))
        {
            return Some(item.text);
        }
    }
    None
}

fn blocked_entry(
    document: &str,
    nodes: &[QueueTreeNode],
    node: &QueueTreeNode,
    raw: &str,
    handle: &str,
) -> String {
    let blockers = agent_doc_document::queue_projection::blocked_marker_ids(raw);
    let own = first_sentence(
        without_handle(without_provenance(&node.full_text), handle),
        ENTRY_TEXT_LIMIT,
    );
    if blockers.is_empty() {
        let reason = node
            .id
            .as_deref()
            .and_then(|id| tracked_item_text(document, id))
            .map(|text| first_sentence(&text, ENTRY_TEXT_LIMIT))
            .unwrap_or(own);
        return format!("blocked: {reason}");
    }
    let named = blockers
        .iter()
        .map(|id| format!("`[#{id}]`"))
        .collect::<Vec<_>>()
        .join(", ");
    let reason = blockers
        .iter()
        .find_map(|id| {
            row_by_handle(document, nodes, &id.to_ascii_lowercase())
                .map(|blocker| without_provenance(&blocker.full_text).to_string())
                .or_else(|| tracked_item_text(document, id))
        })
        .map(|text| {
            // A blocker row reads `[#id]: reason`; the id is already named.
            first_sentence(
                without_handle(&text, &blockers[0].to_ascii_lowercase()),
                ENTRY_TEXT_LIMIT,
            )
        })
        .unwrap_or(own);
    format!("blocked by {named}: {reason}")
}

fn running_entry(node: &QueueTreeNode, raw: &str, handle: &str) -> String {
    let (_, body) = row_body(&node.full_text);
    let body = without_handle(without_provenance(&body), handle);
    let (_, rest) = split_label(body);
    let short = first_sentence(rest, ENTRY_TEXT_LIMIT);
    match agent_doc_document::queue_projection::owner_projection(raw) {
        Some(owner) if format!("{}#{}", owner.owner, owner.generation) != handle => {
            format!("{short} (owner `{}#{}`)", owner.owner, owner.generation)
        }
        _ => short,
    }
}

/// The loose end the queue tree node `node` is, when it is one: a live
/// operator decision or action, a blocked row, or a running row.
///
/// This is the one classifier behind the digest ([`loose_ends`]) and every
/// top-level row's open-work count ([`crate::open_count`], `#looseends`), so
/// the two never disagree. A row whose work a top-level row elsewhere
/// carries, or that self-repair already did ([`crate::queue_tree::RowResolution`]),
/// is not a loose end: it holds no work, exactly as the tree's 🗑️/📌
/// indicator reads it (`#treefinish`).
///
/// An operator row the operator already answered with a reply row beneath it
/// (`~~Restarted~~` under "restart the IDE") is not a loose end either: the
/// next step is the agent's, picking the answer up, not the operator's.
pub fn node_loose_end(document: &str, nodes: &[QueueTreeNode], index: usize) -> Option<LooseEnd> {
    let node = nodes.get(index)?;
    if node.closed || node.resolution.is_some() {
        return None;
    }
    let raw = raw_row(document, node.line);
    let id = row_handle(&node.full_text, raw);
    let operator_row = node.state == QueueNodeState::WaitingOnYou
        || (node.state == QueueNodeState::Open && carries_operator_owned_tag(raw));
    let (group, text) = if operator_row {
        let (owner, _) = row_body(&node.full_text);
        if owner == Some(ActionItemOwner::Agent) || operator_replied(nodes, index) {
            return None;
        }
        let (imperative, decision) = operator_imperative(&node.full_text, &id);
        let group = if decision {
            LooseEndGroup::Decide
        } else {
            LooseEndGroup::Do
        };
        (group, imperative)
    } else {
        match node.state {
            QueueNodeState::Blocked => (
                LooseEndGroup::Blocked,
                blocked_entry(document, nodes, node, raw, &id),
            ),
            QueueNodeState::Running => (LooseEndGroup::Running, running_entry(node, raw, &id)),
            _ => return None,
        }
    };
    Some(LooseEnd { group, id, text })
}

/// Whether a direct child of the operator row at `index` is the operator's
/// reply to it: a plain row (struck or not) with no lifecycle projection,
/// that is neither a follow-up row nor binary-owned response structure. A
/// follow-up row or a worker attempt nested under the row is new work, not
/// an answer.
fn operator_replied(nodes: &[QueueTreeNode], index: usize) -> bool {
    let node = &nodes[index];
    nodes[index + 1..]
        .iter()
        .take_while(|child| {
            child.queue == node.queue && child.line <= node.end_line && child.depth > node.depth
        })
        .filter(|child| child.depth == node.depth + 1)
        .any(|child| {
            let text = child.full_text.trim();
            matches!(child.state, QueueNodeState::Open | QueueNodeState::Done)
                && !child.waiting_item
                && !text.is_empty()
                && !text.starts_with("**")
                && followup_row_action_item(without_provenance(text)).is_none()
        })
}

/// Every unresolved row of `document`, grouped, in document order.
pub fn loose_ends(document: &str) -> Result<Vec<LooseEnd>> {
    let tree = queue_tree(document, &QueueTreeOptions::default())?;
    let nodes = &tree.nodes;
    let mut seen: HashSet<(LooseEndGroup, String)> = HashSet::new();
    let mut ends = Vec::new();
    for index in 0..nodes.len() {
        let Some(end) = node_loose_end(document, nodes, index) else {
            continue;
        };
        if !seen.insert((end.group, end.id.clone())) {
            continue;
        }
        ends.push(end);
    }
    // Group order, document order within a group.
    ends.sort_by_key(|end| end.group);
    Ok(ends)
}

/// Render the digest body. Empty groups are omitted; nothing open renders
/// [`NOTHING_OPEN`].
pub fn render_loose_ends(ends: &[LooseEnd]) -> String {
    if ends.is_empty() {
        return format!("{NOTHING_OPEN}\n");
    }
    let mut out = String::new();
    for group in LooseEndGroup::ALL {
        let grouped = ends
            .iter()
            .filter(|end| end.group == group)
            .collect::<Vec<_>>();
        if grouped.is_empty() {
            continue;
        }
        out.push_str(&format!("- **{}** ({})\n", group.title(), grouped.len()));
        for end in grouped {
            out.push_str(&end.line());
            out.push('\n');
        }
    }
    out
}

fn is_group_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    line.len() == trimmed.len()
        && LooseEndGroup::ALL
            .iter()
            .any(|group| trimmed.starts_with(&format!("- **{}** (", group.title())))
}

/// The handle token of a binary entry line (`  - `[#id]` …` or
/// `` `queue-free:…` ``).
fn entry_handle(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("  - `")?;
    let close = rest.find('`')?;
    Some(&rest[..close])
}

/// Operator lines in an existing digest body: lines nested under an entry
/// (kept under that entry's handle) and any other line that is not
/// binary-owned (kept after the digest).
fn operator_lines(body: &str) -> (Vec<(String, Vec<String>)>, Vec<String>) {
    let mut nested: Vec<(String, Vec<String>)> = Vec::new();
    let mut loose: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in body.lines() {
        if line.trim().is_empty() || line.trim() == NOTHING_OPEN || is_group_line(line) {
            current = None;
            continue;
        }
        if let Some(handle) = entry_handle(line) {
            current = Some(handle.to_string());
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        match &current {
            Some(handle) if indent > 2 => match nested.iter_mut().find(|(h, _)| h == handle) {
                Some((_, lines)) => lines.push(line.to_string()),
                None => nested.push((handle.clone(), vec![line.to_string()])),
            },
            _ => {
                current = None;
                loose.push(line.to_string());
            }
        }
    }
    (nested, loose)
}

/// The rendered body with the operator lines of `existing` kept.
fn merged_body(rendered: &str, existing: &str) -> String {
    let (nested, loose) = operator_lines(existing);
    if nested.is_empty() && loose.is_empty() {
        return rendered.to_string();
    }
    let mut out = String::new();
    let mut placed: HashSet<&str> = HashSet::new();
    for line in rendered.lines() {
        out.push_str(line);
        out.push('\n');
        if let Some(handle) = entry_handle(line)
            && let Some((key, lines)) = nested.iter().find(|(h, _)| h == handle)
        {
            placed.insert(key.as_str());
            for kept in lines {
                out.push_str(kept);
                out.push('\n');
            }
        }
    }
    // An entry that left the digest keeps its operator lines at the end, so
    // nothing typed is lost.
    for (handle, lines) in &nested {
        if placed.contains(handle.as_str()) {
            continue;
        }
        for kept in lines {
            out.push_str(kept);
            out.push('\n');
        }
    }
    for kept in &loose {
        out.push_str(kept);
        out.push('\n');
    }
    out
}

/// Re-render `agent:loose-ends` from the live queue tree.
///
/// When the component is missing it is inserted just above `agent:active`
/// once something is open (a document without `agent:active` is not a
/// session document with a managed index, and gains nothing). Returns `None`
/// when the document already holds the digest (a second render is a no-op),
/// or when the component is duplicated (ambiguous: left alone).
pub fn reconcile_loose_ends(document: &str) -> Result<Option<String>> {
    reconcile_loose_ends_ensuring(document, false)
}

/// [`reconcile_loose_ends`], inserting a missing component even when nothing
/// is open when `ensure` is set: a snapshot follows the document it baselines,
/// so its component markers match and never read as an operator edit.
pub fn reconcile_loose_ends_ensuring(document: &str, ensure: bool) -> Result<Option<String>> {
    let components = element::parse(document)?;
    let present = components
        .iter()
        .filter(|component| component.name == LOOSE_ENDS_COMPONENT)
        .collect::<Vec<_>>();
    if present.len() > 1 {
        return Ok(None);
    }
    let ends = loose_ends(document)?;
    let rendered = render_loose_ends(&ends);
    if let Some(component) = present.first() {
        let existing = component.content(document);
        let body = merged_body(&rendered, existing);
        if existing == body {
            return Ok(None);
        }
        return Ok(Some(component.replace_content(document, &body)));
    }
    if ends.is_empty() && !ensure {
        return Ok(None);
    }
    let Some(active) = components.iter().find(|component| {
        element::canonical_component_name(&component.name) == element::ACTIVE_COMPONENT
    }) else {
        return Ok(None);
    };
    let line_start = document[..active.open_start]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let section = format!(
        "{LOOSE_ENDS_HEADING} <!-- agent:{LOOSE_ENDS_COMPONENT} -->\n{rendered}<!-- /agent:{LOOSE_ENDS_COMPONENT} -->\n\n"
    );
    let mut out = String::with_capacity(document.len() + section.len());
    out.push_str(&document[..line_start]);
    out.push_str(&section);
    out.push_str(&document[line_start..]);
    Ok(Some(out))
}

/// Whether `document` holds an `agent:loose-ends` component.
pub fn has_loose_ends_component(document: &str) -> bool {
    element::parse(document).is_ok_and(|components| {
        components
            .iter()
            .any(|component| component.name == LOOSE_ENDS_COMPONENT)
    })
}

#[cfg(test)]
#[path = "loose_ends_tests.rs"]
mod tests;

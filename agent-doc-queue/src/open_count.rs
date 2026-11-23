//! `#looseends`: every top-level queue row says how much work its tree still
//! holds.
//!
//! A top-level row with open work beneath it carries one binary-owned count
//! token right after its attempt projection prefix:
//!
//! ```text
//! - ### 🙋 (1 decision, 1 running) [from:recovery key=…] follow up on …: operator: Decide whether … [operator-verify] [#q-0e29d5fdf1c4]
//!   - 🙋 follow up on …: operator: restart the IDE after that install. [operator-verify] [#q-56fa509fdf08]
//!     - ~~Restarted~~
//!   - 🚧 [#subagent:cizip#1] follow up on …: agent: check CI run 45085206369 … [#q-f2ec555891ae]
//! ```
//!
//! # Source
//!
//! The counts come from the one classifier behind the Loose ends digest
//! ([`crate::loose_ends::node_loose_end`]), applied to every row of the tree
//! the top-level row roots (the row itself included): **decision** (the
//! operator decides), **action** (the operator acts), **blocked**, and
//! **running**. A row is counted once per group, mirrored rows included, as
//! in the digest. A row that holds no work for the tree's 🗑️/🔒 indicator (a
//! struck row, a row whose work a top-level row elsewhere carries, an
//! operator cleanup row self-repair already did) counts for neither, and an
//! operator row the operator already replied to beneath it (`~~Restarted~~`)
//! is not counted as theirs. So the digest, the count and the keep indicator
//! read the same tree the same way.
//!
//! # Rules
//!
//! - The token is rendered only while a row **beneath** the top-level row is
//!   a loose end. A row with nothing open below it carries no token: its own
//!   glyph already says what it is, and once its tree is finished the 🗑️/🔒
//!   Guidance takes over. Struck rows are never edited.
//! - Zero groups are left out; the order is fixed (decision, action, blocked,
//!   running). See [`agent_doc_element_queue::render_open_count_token`].
//! - The token is projection, never work text: every parser strips it with
//!   the rest of the leading projection prefix
//!   ([`agent_doc_element_queue::strip_attempt_projection_prefix`] and both
//!   `strip_priority_markers`), so a row's identity, claim key, ack-text match
//!   and dedupe key are the same with or without it.
//! - Preflight and `queue sync` re-render it on every pass, after the
//!   lifecycle paint, so it never drifts from the tree. A token on a row that
//!   is no longer top-level is removed. A second pass changes nothing.

use anyhow::Result;

use crate::loose_ends::{LooseEndGroup, node_loose_end};
use crate::queue_tree::{QueueNodeState, QueueTree, QueueTreeOptions, queue_tree};

/// Open-work counts of one top-level tree, in
/// [`agent_doc_element_queue::OPEN_COUNT_LABELS`] order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpenCounts {
    pub decide: usize,
    pub act: usize,
    pub blocked: usize,
    pub running: usize,
}

impl OpenCounts {
    fn add(&mut self, group: LooseEndGroup) {
        match group {
            LooseEndGroup::Decide => self.decide += 1,
            LooseEndGroup::Do => self.act += 1,
            LooseEndGroup::Blocked => self.blocked += 1,
            LooseEndGroup::Running => self.running += 1,
        }
    }

    pub fn as_array(self) -> [usize; 4] {
        [self.decide, self.act, self.blocked, self.running]
    }

    /// The rendered token, or `None` when nothing is counted.
    pub fn token(self) -> Option<String> {
        agent_doc_element_queue::render_open_count_token(self.as_array())
    }
}

/// The count of the tree rooted at top-level node `top`, and whether a row
/// beneath `top` (not `top` itself) is a loose end.
pub fn tree_open_counts(document: &str, tree: &QueueTree, top: usize) -> (OpenCounts, bool) {
    let mut counts = OpenCounts::default();
    let mut below = false;
    let mut seen = std::collections::HashSet::new();
    for index in crate::queue_tree::tree_indexes(tree, top) {
        let Some(end) = node_loose_end(document, &tree.nodes, index) else {
            continue;
        };
        if !seen.insert((end.group, end.id)) {
            continue;
        }
        counts.add(end.group);
        below |= index != top;
    }
    (counts, below)
}

/// The token the row at `index` of an unfiltered tree should carry.
fn desired_token(document: &str, tree: &QueueTree, index: usize) -> Option<String> {
    let node = &tree.nodes[index];
    if node.depth != 0
        || node.closed
        || node.waiting_item
        || matches!(
            node.state,
            QueueNodeState::Group
                | QueueNodeState::Reply
                | QueueNodeState::Choice
                | QueueNodeState::FollowUp
        )
    {
        return None;
    }
    let (counts, below) = tree_open_counts(document, tree, index);
    if below { counts.token() } else { None }
}

/// `line` (one queue row's first line, without its line ending) with its
/// open-work count token set to `token`, or `None` when it already is (or
/// the line is not a live list row this pass may edit).
pub fn line_with_open_count(line: &str, token: Option<&str>) -> Option<String> {
    let indent = line.len() - line.trim_start().len();
    let after_indent = &line[indent..];
    let marker = ["- ", "* ", "+ "]
        .into_iter()
        .find(|marker| after_indent.starts_with(marker))?;
    let body_start = indent + marker.len();
    let body = &line[body_start..];
    let heading = body
        .strip_prefix("###")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .map_or(0, |rest| body.len() - rest.trim_start().len());
    let text = &body[heading..];
    if text.trim_start().starts_with("~~") {
        return None;
    }
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    if work.trim().is_empty() {
        return None;
    }
    let (prefix, present) = without_open_count_tokens(&text[..text.len() - work.len()]);
    // Only the token is ever rewritten: a row already carrying exactly the
    // right one (or none, when none is due) keeps every byte.
    if present.len() == usize::from(token.is_some()) && present.first().copied() == token {
        return None;
    }
    let prefix = prefix.trim_end();
    let mut rendered = String::with_capacity(line.len() + 32);
    rendered.push_str(&line[..body_start + heading]);
    if !prefix.is_empty() {
        rendered.push_str(prefix);
        rendered.push(' ');
    }
    if let Some(token) = token {
        rendered.push_str(token);
        rendered.push(' ');
    }
    rendered.push_str(work);
    (rendered != line).then_some(rendered)
}

/// A projection prefix without any open-work count token in it, and the
/// tokens it held.
fn without_open_count_tokens(prefix: &str) -> (String, Vec<&str>) {
    let mut out = String::with_capacity(prefix.len());
    let mut present = Vec::new();
    let mut rest = prefix;
    while let Some(at) = rest.find('(') {
        out.push_str(&rest[..at]);
        match agent_doc_element_queue::strip_open_count_token(&rest[at..]) {
            Some(after) => {
                present.push(&rest[at..rest.len() - after.len()]);
                rest = after.trim_start();
            }
            None => {
                out.push('(');
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);
    (out, present)
}

/// Re-render every top-level row's open-work count from the live queue tree
/// (see the module docs). `None` when every row already carries the right
/// token (a second pass is a no-op).
pub fn reconcile_open_counts(document: &str) -> Result<Option<String>> {
    let tree = queue_tree(document, &QueueTreeOptions::default())?;
    let mut edits: Vec<(usize, Option<String>)> = Vec::new();
    for index in 0..tree.nodes.len() {
        let node = &tree.nodes[index];
        if node.closed {
            continue;
        }
        edits.push((node.line, desired_token(document, &tree, index)));
    }
    if edits.is_empty() {
        return Ok(None);
    }
    edits.sort_by_key(|(line, _)| *line);
    let mut out = String::with_capacity(document.len() + 64);
    let mut changed = false;
    let mut pending = edits.iter().peekable();
    for (number, line) in document.split_inclusive('\n').enumerate() {
        let number = number + 1;
        let mut edit = None;
        while let Some((at, token)) = pending.peek() {
            if *at < number {
                pending.next();
                continue;
            }
            if *at == number {
                edit = Some(token.as_deref());
                pending.next();
            }
            break;
        }
        let ending_len = line.len() - line.trim_end_matches(['\n', '\r']).len();
        let (content, ending) = line.split_at(line.len() - ending_len);
        match edit.and_then(|token| line_with_open_count(content, token)) {
            Some(rendered) => {
                changed = true;
                out.push_str(&rendered);
                out.push_str(ending);
            }
            None => out.push_str(line),
        }
    }
    Ok(changed.then_some(out))
}

#[cfg(test)]
#[path = "open_count_tests.rs"]
mod tests;

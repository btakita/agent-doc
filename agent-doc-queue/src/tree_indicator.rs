//! `#treetopglyph`: the row where the operator can cut a queue tree carries
//! the glyph, not only its response's Guidance.
//!
//! `#treefinish` put one keep/delete indicator in a struck root's
//! `**Guidance:**` line (`🗑️ Safe to delete this tree.`), several lines below
//! the row it is about. The operator asked for the glyph on the row itself:
//!
//! ```text
//! - ### 🙋 🔒 follow up …: operator: approve the release. [operator-verify] [#q-…]
//!   - ~~🗑️ agent: check CI run 45085206369 … [#q-…]~~
//!     - **Response [#r-…]**
//!   - 🚧 [#subagent:x#1] agent: publish the plugin [#q-…]
//! - ### ~~🗑️ Fix the parser. ⟦published⟧~~
//!   - **Response [#r-…]**
//! ```
//!
//! # Rules
//!
//! A subtree is **deletable** when every row in it, its top row included, is
//! struck and none still holds work for the tree's indicator
//! ([`crate::queue_tree::row_holds_tree_open`]: a struck waiting head whose
//! Todo is open, or a row whose own response says work remains), and none is a
//! waiting item or a row inside a waiting packet.
//!
//! - **🗑️** goes on the top-most row of each deletable subtree: the
//!   top-level row when its whole tree is deletable, otherwise every
//!   deletable subtree whose parent is not.
//! - **🔒** goes on the top-level row of a tree that is not deletable but
//!   holds at least one 🗑️ subtree: keep this row, cut at the 🗑️ rows. A
//!   keep queue's (`#keepunstrike`) top-level row is always kept.
//! - Nothing else carries a glyph, so a stale one is removed and a glyph
//!   flips as the tree changes. A loop queue, whose pass roster owns its
//!   strikes, and the binary-owned Waiting-on-you root's tree carry none.
//!
//! # Placement
//!
//! On a struck row the glyph is the first token inside the strike
//! (`~~🗑️ text~~`), so every struck-row parser still sees `~~…~~`. On a live
//! row it ends the attempt projection prefix (`🙋 🔒 text`,
//! `🚧 [#subagent:x#1] 🔒 text`). Either way it is projection: every parser
//! strips it with the rest of the prefix
//! ([`agent_doc_element_queue::tree_indicator_token`]), so a row's identity,
//! claim key, ack-text match, merge key and dedupe key never include it.
//!
//! Only the glyph is ever rewritten, and only when the row does not already
//! carry exactly the one it should, so a second pass is a no-op.

use std::collections::HashMap;

use agent_doc_element_queue::{
    TreeIndicator, prefix_without_tree_indicators, split_leading_tree_indicators,
    strip_attempt_projection_prefix,
};

use crate::queue_tree::{QueueNodeState, QueueTree, QueueTreeOptions, queue_tree};

/// `(loop queue, keeps its top-level row unstruck)` per queue id.
pub(crate) fn queue_policies(document: &str) -> HashMap<String, (bool, bool)> {
    let mut policies: HashMap<String, (bool, bool)> = HashMap::new();
    let Ok(components) = agent_doc_element::element::parse(document) else {
        return policies;
    };
    for component in components
        .into_iter()
        .filter(|component| component.name == "queue")
    {
        let queue = component
            .attrs
            .get("id")
            .cloned()
            .unwrap_or_else(|| component.name.clone());
        let looping = component
            .attrs
            .contains_key(crate::queue_schedule::QUEUE_LOOP_ATTR);
        let keep_root = crate::queue_schedule::queue_keeps_top_level_unstruck(&component.attrs);
        let policy = policies.entry(queue).or_insert((false, false));
        policy.0 |= looping;
        policy.1 |= keep_root;
    }
    policies
}

/// Whether the row at `index`, as a row, may be deleted with its subtree.
fn row_deletable(tree: &QueueTree, index: usize) -> bool {
    let node = &tree.nodes[index];
    node.closed
        && !node.waiting_item
        && !matches!(
            node.state,
            QueueNodeState::Group
                | QueueNodeState::Reply
                | QueueNodeState::Choice
                | QueueNodeState::FollowUp
        )
        && !crate::queue_tree::row_holds_tree_open(tree, index)
}

/// The glyph every node of an unfiltered tree should carry (see the module
/// docs), by node index.
pub fn desired_tree_indicators(
    tree: &QueueTree,
    policies: &HashMap<String, (bool, bool)>,
) -> Vec<Option<TreeIndicator>> {
    let nodes = &tree.nodes;
    let rows = (0..nodes.len())
        .map(|index| row_deletable(tree, index))
        .collect::<Vec<_>>();
    let subtree_deletable =
        |index: usize| crate::queue_tree::tree_indexes(tree, index).all(|i| rows[i]);
    let mut desired = vec![None; nodes.len()];
    for top in 0..nodes.len() {
        let root = &nodes[top];
        if root.depth != 0 || root.state == QueueNodeState::Group {
            continue;
        }
        let (looping, keep_root) = policies.get(&root.queue).copied().unwrap_or_default();
        if looping {
            continue;
        }
        if !keep_root && subtree_deletable(top) {
            desired[top] = Some(TreeIndicator::Delete);
            continue;
        }
        let tree_end = crate::queue_tree::tree_indexes(tree, top).end;
        let mut index = top + 1;
        let mut cut = false;
        while index < tree_end {
            if subtree_deletable(index) {
                desired[index] = Some(TreeIndicator::Delete);
                cut = true;
                index = crate::queue_tree::tree_indexes(tree, index).end;
            } else {
                index += 1;
            }
        }
        if cut {
            desired[top] = Some(TreeIndicator::Keep);
        }
    }
    desired
}

/// `line` (one queue row's first line, without its line ending) carrying
/// exactly `indicator` (none when `None`), or `None` when it already does or
/// is not a list row this pass may edit.
pub fn line_with_tree_indicator(line: &str, indicator: Option<TreeIndicator>) -> Option<String> {
    let indent = line.len() - line.trim_start().len();
    let after_indent = &line[indent..];
    // `#orderedsteps`: a numbered step carries its glyph after the number.
    let marker_len = ["- ", "* ", "+ "]
        .into_iter()
        .find(|marker| after_indent.starts_with(marker))
        .map(str::len)
        .or_else(|| {
            agent_doc_element_queue::ordered_list_marker_len(after_indent).map(|len| len + 1)
        })?;
    let body_start = indent + marker_len;
    let body = &line[body_start..];
    let heading = body
        .strip_prefix("###")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .map_or(0, |rest| body.len() - rest.trim_start().len());
    let lead = &line[..body_start + heading];
    let text = &body[heading..];
    let want = indicator.into_iter().collect::<Vec<_>>();
    if let Some(inner) = text.strip_prefix("~~") {
        // A struck row: the glyph is the first token inside the strike.
        let (present, rest) = split_leading_tree_indicators(inner);
        if present == want {
            return None;
        }
        // A legacy `~~### …~~` row, a strike with nothing left inside, or an
        // older strike that still holds lifecycle projection (`~~🚧 x~~`,
        // which preflight normalizes first) is never edited.
        if rest.is_empty()
            || rest.starts_with("~~")
            || rest.starts_with("###")
            || strip_attempt_projection_prefix(rest).len() != rest.len()
        {
            return None;
        }
        let mut rendered = String::with_capacity(line.len() + 8);
        rendered.push_str(lead);
        rendered.push_str("~~");
        if let Some(indicator) = indicator {
            rendered.push_str(indicator.glyph());
            rendered.push(' ');
        }
        rendered.push_str(rest.trim_start());
        return (rendered != line).then_some(rendered);
    }
    let work = strip_attempt_projection_prefix(text);
    // A row struck some other way (`~text~`, a projection prefix outside the
    // strike) is never edited: a glyph in front would hide its strike.
    if work.trim().is_empty() || work.starts_with('~') {
        return None;
    }
    // A live row is never deletable; on one, a 🗑️ is only ever removed.
    let indicator = indicator.filter(|indicator| *indicator == TreeIndicator::Keep);
    let want = indicator.into_iter().collect::<Vec<_>>();
    let (prefix, present) = prefix_without_tree_indicators(&text[..text.len() - work.len()]);
    if present == want {
        return None;
    }
    // The glyph goes before a trailing open-work count (`#looseends`), where
    // that pass also leaves it: `🙋 🔒 (1 decision, 1 running) text`.
    let prefix = prefix.trim();
    let (before, count) = split_trailing_open_count(prefix);
    let mut rendered = String::with_capacity(line.len() + 8);
    rendered.push_str(lead);
    for part in [
        Some(before.trim_end()),
        indicator.map(TreeIndicator::glyph),
        count,
    ] {
        if let Some(part) = part.filter(|part| !part.is_empty()) {
            rendered.push_str(part);
            rendered.push(' ');
        }
    }
    rendered.push_str(work);
    (rendered != line).then_some(rendered)
}

/// `prefix` split before its trailing open-work count token, when it ends
/// with one (`🙋 (1 decision)` → `("🙋 ", Some("(1 decision)"))`).
fn split_trailing_open_count(prefix: &str) -> (&str, Option<&str>) {
    prefix
        .rfind('(')
        .filter(|&at| {
            agent_doc_element_queue::strip_open_count_token(&prefix[at..])
                .is_some_and(|after| after.trim().is_empty())
        })
        .map_or((prefix, None), |at| (&prefix[..at], Some(&prefix[at..])))
}

/// Re-render every queue row's tree glyph from the current queue tree (see
/// the module docs). Returns the document and how many rows changed, or
/// `None` when every row already carries the right glyph.
pub fn reconcile_tree_indicators(document: &str) -> Option<(String, usize)> {
    let tree = queue_tree(document, &QueueTreeOptions::default()).ok()?;
    if tree.nodes.is_empty() {
        return None;
    }
    let desired = desired_tree_indicators(&tree, &queue_policies(document));
    let by_line = tree
        .nodes
        .iter()
        .zip(desired)
        .map(|(node, indicator)| (node.line, indicator))
        .collect::<HashMap<_, _>>();
    let mut out = String::with_capacity(document.len() + 64);
    let mut changed = 0;
    for (number, line) in document.split_inclusive('\n').enumerate() {
        let Some(indicator) = by_line.get(&(number + 1)) else {
            out.push_str(line);
            continue;
        };
        let ending_len = line.len() - line.trim_end_matches(['\n', '\r']).len();
        let (content, ending) = line.split_at(line.len() - ending_len);
        match line_with_tree_indicator(content, *indicator) {
            Some(rendered) => {
                changed += 1;
                out.push_str(&rendered);
                out.push_str(ending);
            }
            None => out.push_str(line),
        }
    }
    (changed > 0).then_some((out, changed))
}

#[cfg(test)]
#[path = "tree_indicator_tests.rs"]
mod tests;

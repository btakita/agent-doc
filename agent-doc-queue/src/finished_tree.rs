//! `#treefinish`: a finished queue tree is struck from the root down, and
//! its root says whether it may be deleted.
//!
//! A struck row's response used to read the tree around it and say, in one
//! long sentence, which part of it could go ("This row is done, but its root
//! item … is still open: 2 rows in its tree are open, first …. Safe to delete
//! this completed row …; keep the rest of the tree."). A finished tree was
//! struck only sometimes: the runtime projection closes a plain group parent
//! once every leaf is struck, but a root with work of its own, or a tree
//! whose last open rows were delegated to top-level follow-up rows, stayed
//! live, and its sub-rows never were struck.
//!
//! There is now one rule. A top-level row's tree is **finished** when no row
//! in it holds work ([`crate::queue_tree::row_holds_tree_open`]):
//!
//! - every row is struck, or only a label (a plain group parent), or
//!   resolved: a nested follow-up row whose action item a top-level row
//!   elsewhere in the queue carries (delegated), or an operator follow-up row
//!   asking to clean up an agent-doc artifact that self-repair already
//!   removed ([`crate::queue_tree::RowResolution`]);
//! - no row's own response says work remains (its `**State:**` is
//!   `Complete.`). A response's action items are resolved by the rules of
//!   [`crate::response_remaining`], where an item whose follow-up row lives
//!   in another top-level tree counts as delegated.
//!
//! [`strike_finished_trees`] strikes the root and every unstruck descendant
//! row of each finished tree, on every sync (preflight and `queue sync`).
//! The strike is the canonical one (`~~…~~` around the row's work text; the
//! binary-owned `🙋` projection prefix is dropped), so operator text is kept
//! byte-for-byte. It never strikes:
//!
//! - a tree with any row that holds work, an operator row that is still
//!   open included;
//! - a tree holding a Waiting-on-you item or packet rows;
//! - a row a live worker claims (`is_claimed`), or a row whose tracked id is
//!   still open (its strike would read as that item's completion), and then
//!   none of its tree;
//! - the top-level row of a `keep` queue (`#keepunstrike`), or anything in a
//!   `loop` queue, whose pass roster owns its strikes.
//!
//! A struck tree is finished and fully struck, so a second pass changes
//! nothing.

use std::collections::HashMap;

use crate::queue_tree::{QueueNodeState, QueueTree, QueueTreeOptions};

/// Whether the tree rooted at top-level node `top` of an unfiltered
/// [`crate::queue_tree::queue_tree`] is finished (see the module docs).
pub fn tree_finished(tree: &QueueTree, top: usize) -> bool {
    let Some(root) = tree.nodes.get(top) else {
        return false;
    };
    if root.depth != 0 || root.state == QueueNodeState::Group {
        return false;
    }
    crate::queue_tree::tree_indexes(tree, top).all(|index| {
        let node = &tree.nodes[index];
        !node.waiting_item
            && !matches!(
                node.state,
                QueueNodeState::Group
                    | QueueNodeState::Reply
                    | QueueNodeState::Choice
                    | QueueNodeState::FollowUp
            )
            && !crate::queue_tree::row_holds_tree_open(tree, index)
    })
}

/// Strike every finished tree from the root down (see the module docs).
/// `is_claimed(row_text)` says whether a live worker claims the row.
/// Returns the document and how many trees were struck, or `None` when no
/// finished tree has an unstruck row.
pub fn strike_finished_trees(
    document: &str,
    is_claimed: &dyn Fn(&str) -> bool,
) -> Option<(String, usize)> {
    let tree = crate::queue_tree::queue_tree(document, &QueueTreeOptions::default()).ok()?;
    if !tree.nodes.iter().any(|node| !node.closed) {
        return None;
    }
    let policies = crate::tree_indicator::queue_policies(document);
    let items = agent_doc_markdown_ast::mutations::item_nodes_all(document, "queue").ok()?;
    let mut by_line: HashMap<usize, Vec<&agent_doc_markdown_ast::mutations::MutationItemNode>> =
        HashMap::new();
    for item in &items {
        by_line
            .entry(crate::queue_tree::line_of(document, item.item.start_byte))
            .or_default()
            .push(item);
    }
    // A row whose tracked id is still open (`agent:todo`, backlog, icebox,
    // pending) is not struck: its strike would read as the item's completion
    // (`#todolink`).
    let mut open_ids: Option<std::collections::HashSet<String>> = None;
    let mut tracked_open = |text: &str| {
        let work = agent_doc_element_queue::strip_attempt_projection_prefix(
            text.trim().trim_start_matches("### "),
        );
        let Some(id) = crate::linked_todo_queue::linked_queue_todo_id(work) else {
            return false;
        };
        open_ids
            .get_or_insert_with(|| {
                agent_doc_element::todo::open_todo_ids(document)
                    .into_iter()
                    .map(|id| id.to_ascii_lowercase())
                    .chain(crate::queue_continuation::active_tracked_ids(document))
                    .collect()
            })
            .contains(&id.to_ascii_lowercase())
    };
    let mut keys: Vec<String> = Vec::new();
    let mut trees = 0;
    for top in 0..tree.nodes.len() {
        if tree.nodes[top].depth != 0 || !tree_finished(&tree, top) {
            continue;
        }
        let (looping, keep_root) = policies
            .get(&tree.nodes[top].queue)
            .copied()
            .unwrap_or_default();
        if looping {
            continue;
        }
        let targets = crate::queue_tree::tree_indexes(&tree, top)
            .filter(|&index| !tree.nodes[index].closed && !(keep_root && index == top))
            .collect::<Vec<_>>();
        if targets.is_empty() {
            continue;
        }
        let mut tree_keys = Vec::new();
        let mut safe = true;
        for index in targets {
            // Exactly one live item on the row's line, or the tree is left
            // alone: a strike never guesses which row it means.
            let live = by_line
                .get(&tree.nodes[index].line)
                .map(|items| {
                    items
                        .iter()
                        .filter(|item| !item.item.struck)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            match live.as_slice() {
                [item] if !is_claimed(&item.item.text) && !tracked_open(&item.item.text) => {
                    tree_keys.push(item.node_key.clone())
                }
                _ => {
                    safe = false;
                    break;
                }
            }
        }
        if safe {
            keys.extend(tree_keys);
            trees += 1;
        }
    }
    if keys.is_empty() {
        return None;
    }
    let refs = keys.iter().map(String::as_str).collect::<Vec<_>>();
    let struck =
        agent_doc_markdown_ast::mutations::consume_nodes_all(document, "queue", &refs).ok()?;
    (struck != document).then_some((struck, trees))
}

#[cfg(test)]
#[path = "finished_tree_tests.rs"]
mod tests;

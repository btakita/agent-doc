//! Node-level exchange merge built on the exchange document-tree
//! (`agent_doc_markdown_ast::exchange_tree`). This is Phase 2 of
//! `tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md`.
//!
//! [`merge_exchange_nodes`] merges the `agent:exchange` body as a list of distinct
//! [`ExchangeNode`]s instead of one text blob. Its guarantee: **no node's body is
//! ever merged into a different node**, so a response can never absorb another
//! response's text or a user prompt (the cross-response bleed the whole-doc `yrs`
//! merge allows). Operator-visible (`theirs`) content is authoritative and kept in
//! order; agent response turns present in `ours` but absent from `theirs` (matched
//! by [`ExchangeNode::node_id`]) are appended before any trailing boundary marker.
//!
//! This function is intentionally NOT yet wired into `document_cell_merge`'s hot path —
//! that swap is the deliberate, separately-reviewed Phase 3 step. It exists now as
//! the verified drop-in, mirroring `merge_exchange_inner`'s append-agent-new-turns
//! contract at the node level.

use agent_doc_markdown_ast::exchange_tree::{
    ExchangeNode, ExchangeNodeKind, parse_exchange_nodes, render_exchange_nodes,
};
use std::collections::HashSet;

/// Is `trimmed` a `<!-- agent:boundary:… -->` marker line?
fn is_boundary_marker(trimmed: &str) -> bool {
    trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with("-->")
}

/// Does this node end with a trailing boundary marker line (ignoring trailing
/// blank lines)? A boundary marker rides at the very end of the exchange body and
/// must stay last, so appended turns are inserted before it.
fn node_trailing_boundary_split(node: &ExchangeNode) -> Option<(Vec<String>, Vec<String>)> {
    let mut split = node.lines.len();
    let mut saw_boundary = false;
    while split > 0 {
        let t = node.lines[split - 1].trim_end_matches(['\n', '\r']).trim();
        if t.is_empty() {
            split -= 1;
        } else if is_boundary_marker(t) {
            saw_boundary = true;
            split -= 1;
            break;
        } else {
            break;
        }
    }
    if saw_boundary {
        Some((node.lines[..split].to_vec(), node.lines[split..].to_vec()))
    } else {
        None
    }
}

/// Merge exchange bodies at the node level. `theirs` (operator-visible) is
/// authoritative and preserved in order; response turns present in `ours` (agent)
/// but absent from `theirs` — keyed by [`ExchangeNode::node_id`] — are appended in
/// order, before a trailing boundary marker if one is present. `base` is accepted
/// for signature parity with the 3-way merge and reserved for per-node body merge
/// in Phase 3; node bodies are never cross-merged here.
pub fn merge_exchange_nodes(_base: &str, ours: &str, theirs: &str) -> String {
    // `#exchangeconverge`: collapse any duplicate node identities the operator
    // side already carries (a poisoned/scrambled buffer from a prior failed 3-way
    // merge) BEFORE merging, so the merge output converges instead of preserving
    // mirror-ordered duplicates. Node identity keys on the response heading /
    // prompt body, so this only removes true duplicates — distinct turns and
    // distinct prompts are untouched.
    let (theirs_nodes, _) = dedup_nodes_by_identity(parse_exchange_nodes(theirs));
    let ours_nodes = parse_exchange_nodes(ours);

    let theirs_ids: HashSet<String> = theirs_nodes.iter().map(ExchangeNode::node_id).collect();

    // Agent-new *response* turns only. Prompts are operator-owned; the agent side
    // never synthesizes a prompt node into the operator's exchange.
    let agent_new: Vec<&ExchangeNode> = ours_nodes
        .iter()
        .filter(|n| matches!(n.kind, ExchangeNodeKind::Response { .. }))
        .filter(|n| !theirs_ids.contains(&n.node_id()))
        .collect();

    if agent_new.is_empty() {
        return render_exchange_nodes(&theirs_nodes);
    }

    // Insert appended turns before a trailing boundary marker carried by theirs'
    // last node (if any), so the boundary stays at the very end.
    let mut out_nodes: Vec<ExchangeNode> = theirs_nodes.clone();
    let trailing_boundary: Vec<String> = match out_nodes.last_mut() {
        Some(last) => match node_trailing_boundary_split(last) {
            Some((head, boundary_tail)) => {
                last.lines = head;
                boundary_tail
            }
            None => Vec::new(),
        },
        None => Vec::new(),
    };

    for n in agent_new {
        out_nodes.push(n.clone());
    }

    let mut out = render_exchange_nodes(&out_nodes);
    out.push_str(&trailing_boundary.concat());
    out
}

/// `#exchangeconverge` — collapse nodes that share a [`ExchangeNode::node_id`],
/// keeping the first occurrence in document order. Returns the deduped list and
/// whether any duplicate was removed.
///
/// Node identity keys on the response heading (`r:hash(key)`) / the prompt body
/// (`p:hash(body)`), so two nodes collide only when they are the same response
/// turn or a byte-identical prompt — exactly the non-adjacent / mirror-ordered
/// duplicates a failed 3-way merge leaves behind. Distinct turns and distinct
/// prompts have distinct ids and are never collapsed. This is the same identity
/// the node merge keys on, so the operation is idempotent.
fn dedup_nodes_by_identity(nodes: Vec<ExchangeNode>) -> (Vec<ExchangeNode>, bool) {
    let mut seen: HashSet<String> = HashSet::with_capacity(nodes.len());
    let mut out: Vec<ExchangeNode> = Vec::with_capacity(nodes.len());
    let mut removed = false;
    for node in nodes {
        if seen.insert(node.node_id()) {
            out.push(node);
        } else {
            removed = true;
        }
    }
    (out, removed)
}

/// `#exchangeconverge` — collapse duplicate exchange nodes in an `agent:exchange`
/// body by node identity (see [`dedup_nodes_by_identity`]). Idempotent
/// convergence pass that repairs a buffer already poisoned with mirror-ordered
/// duplicates (a prior failed CRDT/3-way merge). When there is nothing to
/// collapse, the input is returned **verbatim** so a clean exchange is never
/// reformatted by the parse→render round-trip.
pub fn dedup_exchange_nodes_by_identity(exchange: &str) -> String {
    let (nodes, removed) = dedup_nodes_by_identity(parse_exchange_nodes(exchange));
    if !removed {
        return exchange.to_string();
    }
    render_exchange_nodes(&nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_new_response_is_appended_without_bleeding_into_operator_turns() {
        let base = "### Re: A — opus\n\nAnswer A.\n";
        let theirs = "### Re: A — opus\n\nAnswer A.\n\nOperator note between turns.\n";
        let ours = "### Re: A — opus\n\nAnswer A.\n\n### Re: B — opus\n\nAnswer B.\n";
        let merged = merge_exchange_nodes(base, ours, theirs);
        // Operator content preserved verbatim...
        assert!(merged.contains("Operator note between turns."));
        // ...agent-new turn B appended...
        assert!(merged.contains("### Re: B — opus"));
        assert!(merged.contains("Answer B."));
        // ...and A's node did not absorb B's text.
        let a_start = merged.find("### Re: A").unwrap();
        let b_start = merged.find("### Re: B").unwrap();
        assert!(a_start < b_start);
    }

    #[test]
    fn duplicate_turn_same_key_is_not_reappended() {
        let base = "";
        let theirs = "### Re: A — opus\n\nAnswer A.\n";
        let ours = "### Re: A — opus\n\nAnswer A (agent copy).\n";
        let merged = merge_exchange_nodes(base, ours, theirs);
        // Only one A heading — the agent's same-key turn is not duplicated, and
        // the operator's body is kept (no cross-node body overwrite).
        assert_eq!(merged.matches("### Re: A").count(), 1);
        assert!(merged.contains("Answer A."));
    }

    #[test]
    fn appended_turn_lands_before_a_trailing_boundary_marker() {
        let base = "";
        let theirs = "### Re: A — opus\n\nAnswer A.\n<!-- agent:boundary:abc123 -->\n";
        let ours = "### Re: B — opus\n\nAnswer B.\n";
        let merged = merge_exchange_nodes(base, ours, theirs);
        let b_idx = merged.find("### Re: B").unwrap();
        let boundary_idx = merged.find("<!-- agent:boundary:abc123 -->").unwrap();
        assert!(
            b_idx < boundary_idx,
            "appended turn must precede the trailing boundary marker"
        );
    }

    #[test]
    fn operator_prompt_node_is_never_lost_or_merged_into_a_response() {
        let base = "";
        let theirs = "❯ Do the thing.\n\n### Re: Done — opus\n\nDid it.\n";
        let ours = "### Re: Done — opus\n\nDid it.\n\n### Re: Extra — opus\n\nMore.\n";
        let merged = merge_exchange_nodes(base, ours, theirs);
        assert!(merged.contains("❯ Do the thing."));
        assert!(merged.contains("### Re: Extra — opus"));
    }

    #[test]
    fn no_agent_new_turns_returns_operator_content_unchanged() {
        let theirs = "### Re: A — opus\n\nAnswer A.\n\nOperator tail.\n";
        let merged = merge_exchange_nodes("", theirs, theirs);
        assert_eq!(merged, theirs);
    }

    #[test]
    fn dedup_collapses_mirror_ordered_duplicate_response_turns() {
        // `#exchangeconverge`: a poisoned buffer with the same response turn
        // repeated non-adjacently must converge to a single occurrence.
        let poisoned =
            "### Re: A — opus\n\nAnswer A.\n\n❯ Prompt.\n\n### Re: A — opus\n\nAnswer A.\n";
        let converged = dedup_exchange_nodes_by_identity(poisoned);
        assert_eq!(
            converged.matches("### Re: A").count(),
            1,
            "the duplicated response turn collapses to one"
        );
        assert!(
            converged.contains("❯ Prompt."),
            "distinct prompt is preserved"
        );
    }

    #[test]
    fn dedup_collapses_byte_identical_duplicate_prompts() {
        let poisoned = "❯ Do the thing.\n\n❯ Do the thing.\n";
        let converged = dedup_exchange_nodes_by_identity(poisoned);
        assert_eq!(converged.matches("❯ Do the thing.").count(), 1);
    }

    #[test]
    fn dedup_preserves_distinct_nodes_and_is_verbatim_when_clean() {
        // Distinct prompts/responses have distinct ids — never collapsed — and a
        // clean exchange is returned byte-for-byte (no reformatting round-trip).
        let clean = "❯ First prompt.\n\n### Re: A — opus\n\nAnswer A.\n\n❯ Second prompt.\n";
        let out = dedup_exchange_nodes_by_identity(clean);
        assert_eq!(out, clean);
    }

    #[test]
    fn dedup_is_idempotent() {
        let poisoned = "### Re: A — opus\n\nAnswer A.\n\n### Re: A — opus\n\nAnswer A.\n";
        let once = dedup_exchange_nodes_by_identity(poisoned);
        let twice = dedup_exchange_nodes_by_identity(&once);
        assert_eq!(once, twice, "convergence pass is idempotent");
    }

    #[test]
    fn merge_exchange_nodes_converges_a_poisoned_operator_side() {
        // The node merge now collapses duplicate identities the operator side
        // already carries, so the merge output converges instead of preserving
        // mirror-ordered duplicates.
        let poisoned_theirs = "### Re: A — opus\n\nAnswer A.\n\n### Re: A — opus\n\nAnswer A.\n";
        let merged = merge_exchange_nodes("", poisoned_theirs, poisoned_theirs);
        assert_eq!(
            merged.matches("### Re: A").count(),
            1,
            "the merge collapses the operator side's duplicate turn"
        );
    }
}

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
    ExchangeNode, ExchangeNodeKind, duplicate_response_cell_node_indices, parse_exchange_nodes,
    render_exchange_nodes,
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
    // `#exchangeconverge`: collapse duplicate response cells the operator side
    // already carries (a poisoned/scrambled buffer from a prior failed 3-way
    // merge) BEFORE merging, so the merge output converges instead of preserving
    // mirror-ordered response duplicates. A repeated operator prompt is kept
    // (`#identicalpromptdedupe`).
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

/// `#exchangeconverge` — collapse repeated identities. A repeated response cell
/// keeps the copy whose headings carry a `[#r-…]` id when one does, and
/// otherwise the first occurrence in document order
/// (`#dedupekeepsannotatedcopy`). Returns the deduped list and whether anything
/// was removed.
///
/// Responses collide only as whole cells (`#responseblockmovedtail`): every
/// `### ` heading opens a node, so one node of a multi-section response is not
/// a turn, and two replies that share an opening node or a section are still
/// distinct turns. Salient nodes still collapse on their cycle id. Operator
/// prompts are kept even when the body is byte-identical (`#identicalpromptdedupe`):
/// a second `yes` is a repeat, not a duplicate. The operation stays idempotent.
fn dedup_nodes_by_identity(nodes: Vec<ExchangeNode>) -> (Vec<ExchangeNode>, bool) {
    let duplicate_cell_nodes = duplicate_response_cell_node_indices(&nodes);
    let mut seen: HashSet<String> = HashSet::with_capacity(nodes.len());
    let mut out: Vec<ExchangeNode> = Vec::with_capacity(nodes.len());
    let mut removed = false;
    for (index, node) in nodes.into_iter().enumerate() {
        let keep = if matches!(node.kind, ExchangeNodeKind::Prompt) {
            true
        } else if matches!(node.kind, ExchangeNodeKind::Response { .. }) {
            !duplicate_cell_nodes.contains(&index)
        } else {
            seen.insert(node.node_id())
        };
        if keep {
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

    /// `#responseblockmovedtail`: the document-cell merge runs this pass on every
    /// merged exchange, so it must not delete the opening node of a reply that
    /// shares it with an earlier reply to the same prompt.
    #[test]
    fn replies_sharing_an_opening_node_keep_their_headings() {
        let exchange = concat!(
            "### Re: Response\n\n> **Queue prompt:** tags\n\n",
            "### Batch integrated\n\nPushed.\n\n",
            "### Re: Response\n\n> **Queue prompt:** tags\n\n",
            "### Tags integrated\n\nFixed.\n",
        );
        assert_eq!(dedup_exchange_nodes_by_identity(exchange), exchange);
    }

    /// `#dedupekeepsannotatedcopy`: exchange convergence uses the same cell
    /// dedupe, so a plain twin ahead of the lookup heading must not win.
    #[test]
    fn dedup_keeps_the_annotated_response_heading() {
        let exchange = concat!(
            "### Re: A\n\nAnswer A.\n\n",
            "❯ Prompt.\n\n",
            "### Re: A [#r-0123456789ab]\n\nAnswer A.\n",
        );
        let converged = dedup_exchange_nodes_by_identity(exchange);
        assert_eq!(converged.matches("### Re: A").count(), 1);
        assert!(converged.contains("### Re: A [#r-0123456789ab]"));
        assert!(!converged.contains("### Re: A\n"));
        assert!(converged.contains("❯ Prompt."));
    }

    #[test]
    fn repeated_whole_multi_section_cell_collapses() {
        let cell = "### Re: A\n\nSummary.\n\n### Details\n\nBody.\n\n";
        let exchange = format!("{cell}\u{276f} Between.\n\n{cell}");
        assert_eq!(
            dedup_exchange_nodes_by_identity(&exchange),
            format!("{cell}\u{276f} Between.\n\n")
        );
    }

    #[test]
    fn byte_identical_agent_turn_is_not_reappended() {
        let base = "";
        let theirs = "### Re: A — opus\n\nAnswer A.\n";
        // Agent re-delivers the byte-identical turn (idempotent redelivery).
        let ours = "### Re: A — opus\n\nAnswer A.\n";
        let merged = merge_exchange_nodes(base, ours, theirs);
        // Same heading + same body ⇒ same identity ⇒ not duplicated.
        assert_eq!(merged.matches("### Re: A").count(), 1);
        assert!(merged.contains("Answer A."));
    }

    #[test]
    fn same_heading_distinct_body_agent_turn_is_preserved() {
        // `#qcellmerge-response-body-id`: a fresh agent response whose heading
        // coincides with a prior turn (same-topic / same-preset follow-up) but
        // carries a *different* body is a genuinely new turn. Heading-only identity
        // treated it as a duplicate and the merge dropped the fresh body ("a
        // cell-merge chose the existing content"). Body-aware identity keeps it.
        let base = "### Re: A — opus\n\nAnswer A.\n";
        let theirs = "### Re: A — opus\n\nAnswer A.\n";
        let ours = "### Re: A — opus\n\nAnswer A.\n\n### Re: A — opus\n\nA totally new answer for the follow-up.\n";
        let merged = merge_exchange_nodes(base, ours, theirs);
        assert_eq!(
            merged.matches("### Re: A").count(),
            2,
            "the fresh same-heading turn must be appended, not dropped"
        );
        assert!(merged.contains("A totally new answer for the follow-up."));
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
    fn dedup_keeps_byte_identical_duplicate_prompts() {
        let repeated = "❯ Do the thing.\n\n❯ Do the thing.\n";
        let converged = dedup_exchange_nodes_by_identity(repeated);
        assert_eq!(converged.matches("❯ Do the thing.").count(), 2);
        assert_eq!(converged, repeated);
    }

    #[test]
    fn dedup_keeps_a_second_yes_prompt() {
        let repeated = "❯ yes\n\n❯ yes\n";
        let converged = dedup_exchange_nodes_by_identity(repeated);
        assert_eq!(converged.matches("❯ yes").count(), 2);
        assert_eq!(converged, repeated);
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

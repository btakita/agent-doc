//! `lazily::SeqCrdt`-backed exchange structure — Phase 3 of
//! `tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md`.
//!
//! This models the `agent:exchange` body as a real conflict-free sequence CRDT
//! ([`lazily::SeqCrdt`]) whose elements are whole [`ExchangeNode`]s keyed by the
//! body-aware [`ExchangeNode::node_id`] (Phase 3.1). It is the structural
//! substrate the whole-text merge lacked: because every response and every prompt
//! is a distinct `SeqCrdt` element, **no node's body can ever bleed into another
//! node** on a merge (the cross-response scramble `#qcellmerge1`), and two
//! replicas that each append a distinct turn both survive a `SeqCrdt::merge`.
//!
//! Scope of THIS increment (deliberately bounded — the live hot path is
//! `document_cell_merge::merge_exchange_inner`, which is NOT changed here):
//!
//! - `SeqCrdt` owns exchange **structure + order + presence**. `ExchangeNode` is
//!   the element value, so the merged tree reconstructs nodes without re-parsing.
//! - Node **bodies stay operator-authoritative whole text**, not a per-node
//!   `TextCrdt` char-merge. A sound per-node char-merge needs persisted CRDT
//!   *lineage* (a forked `.seqcrdt` state file), not two trees rebuilt from
//!   strings each merge — reconstructing bodies independently and char-unioning
//!   them would garble a shared node. That lineage work is the next Phase 3 step;
//!   until then, for a node present on both sides the operator's (theirs) text
//!   wins verbatim, which is exactly the "operator-visible text is authoritative"
//!   rule.
//! - [`merge_exchange_crdt`] is the operator-authoritative merge over the CRDT
//!   store: theirs' order/text is preserved, agent-new **response** turns absent
//!   from theirs are appended (before a trailing boundary marker), and — unlike
//!   the current string merge — a node the operator **deleted** (present in
//!   `base` and `ours`, absent from `theirs`) is NOT resurrected. That deletion
//!   guard is the concrete correctness gain this migration lands using `base`,
//!   the parameter `merge_exchange_nodes` reserved for Phase 3.
//!
//! It is intentionally not yet wired into the merge hot path; it is the reviewed
//! drop-in the eventual `merge_by_component`(`exchange`) swap routes through.

use agent_doc_markdown_ast::exchange_tree::{
    ExchangeNode, ExchangeNodeKind, parse_exchange_nodes, render_exchange_nodes,
};
use lazily::{PeerId, SeqCrdt};
use std::collections::HashSet;

/// Peer id for a document rebuilt from operator (theirs) disk/buffer state.
const THEIRS_PEER: u64 = 1;

/// A `lazily::SeqCrdt`-backed exchange: an ordered, conflict-free sequence of
/// distinct [`ExchangeNode`]s keyed by [`ExchangeNode::node_id`].
#[derive(Clone)]
pub struct ExchangeCrdt {
    seq: SeqCrdt<String, ExchangeNode>,
}

impl ExchangeCrdt {
    /// Build a replica owned by `peer` from a node list, inserting each node at
    /// the back in document order. A repeated `node_id` (a poisoned/scrambled
    /// buffer from a prior failed merge) collapses to its first occurrence, so
    /// construction is convergent (`#exchangeconverge`) — `insert_between` is a
    /// no-op once the id exists.
    pub fn from_nodes(peer: u64, nodes: &[ExchangeNode], now_micros: u64) -> Self {
        let mut seq = SeqCrdt::new(PeerId(peer));
        for n in nodes {
            let id = n.node_id();
            if seq.contains(&id) {
                continue;
            }
            seq.insert_back(id, n.clone(), now_micros);
        }
        Self { seq }
    }

    /// Parse an `agent:exchange` inner body and build a replica from its nodes.
    pub fn from_inner(peer: u64, inner: &str, now_micros: u64) -> Self {
        Self::from_nodes(peer, &parse_exchange_nodes(inner), now_micros)
    }

    /// Live nodes in sequence order.
    pub fn nodes(&self) -> Vec<ExchangeNode> {
        self.seq.values().into_iter().map(|(_, n)| n).collect()
    }

    /// Live node ids in sequence order.
    pub fn ids(&self) -> Vec<String> {
        self.seq.order()
    }

    /// Is `id` present and live (not tombstoned)?
    pub fn contains(&self, id: &str) -> bool {
        self.seq.contains(&id.to_string())
    }

    /// Append a node at the back (used to graft an agent-new turn onto the
    /// operator-authoritative structure).
    pub fn push_back(&mut self, node: ExchangeNode, now_micros: u64) {
        let id = node.node_id();
        if self.seq.contains(&id) {
            return;
        }
        self.seq.insert_back(id, node, now_micros);
    }

    /// Tombstone a node by id. Returns whether it was live.
    pub fn remove(&mut self, id: &str, now_micros: u64) -> bool {
        self.seq.remove(&id.to_string(), now_micros)
    }

    /// Fork this replica under a new peer id, preserving element lineage so a
    /// later [`Self::merge`] converges (both replicas' inserts survive).
    pub fn fork(&self, peer: u64) -> Self {
        Self {
            seq: self.seq.fork(PeerId(peer)),
        }
    }

    /// Conflict-free merge of another replica's structure into this one. Elements
    /// present on either side survive; concurrent inserts both land in a single
    /// converged order (fractional positions ⇒ no reorder bleed).
    pub fn merge(&mut self, other: &ExchangeCrdt, now_micros: u64) -> bool {
        self.seq.merge(&other.seq, now_micros)
    }

    /// Render the live nodes back to a byte-stable `agent:exchange` inner body.
    pub fn render(&self) -> String {
        render_exchange_nodes(&self.nodes())
    }
}

/// Is `trimmed` a `<!-- agent:boundary:… -->` marker line?
fn is_boundary_marker(trimmed: &str) -> bool {
    let Some(inner) = trimmed
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
    else {
        return false;
    };
    inner.trim().starts_with("agent:boundary:")
}

/// Split a trailing `<!-- agent:boundary:… -->` marker (and any trailing blank
/// lines after it) off a node's `lines`, returning `(head_lines, boundary_tail)`
/// when a boundary marker rides at the end, else `None`.
fn split_trailing_boundary(node: &ExchangeNode) -> Option<(Vec<String>, Vec<String>)> {
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

/// Strip a trailing boundary marker (and trailing blank lines) that a block scan
/// swept into an agent turn's `lines` — the boundary belongs to the structural
/// skeleton theirs already carries, so an appended agent turn must not re-emit it.
fn strip_trailing_boundary_and_blanks(node: &ExchangeNode) -> ExchangeNode {
    let mut lines: &[String] = &node.lines;
    while let Some(last) = lines.last() {
        let t = last.trim_end_matches(['\n', '\r']).trim();
        if t.is_empty() || is_boundary_marker(t) {
            lines = &lines[..lines.len() - 1];
        } else {
            break;
        }
    }
    ExchangeNode {
        kind: node.kind.clone(),
        lines: lines.to_vec(),
    }
}

/// Operator-authoritative exchange merge over the [`ExchangeCrdt`] store.
///
/// `theirs` (operator-visible) is authoritative: its nodes, order, and text are
/// preserved verbatim (mirror-ordered duplicate identities collapse on
/// construction — `#exchangeconverge`). Agent-new **response** turns present in
/// `ours` but absent from `theirs` — keyed by [`ExchangeNode::node_id`] — are
/// appended, before a trailing boundary marker if one is present. Prompt nodes
/// are operator-owned and never synthesized from `ours`.
///
/// **`base` deletion guard (the Phase 3 gain):** an agent turn absent from
/// `theirs` but present in `base` is a turn the operator **deleted** since the
/// shared baseline — it is NOT re-appended, so a stale agent replica cannot
/// resurrect operator-deleted content. Turns genuinely new to `ours` (absent
/// from both `base` and `theirs`) are still appended.
pub fn merge_exchange_crdt(base: &str, ours: &str, theirs: &str, now_micros: u64) -> String {
    let tree = ExchangeCrdt::from_inner(THEIRS_PEER, theirs, now_micros);
    let theirs_ids: HashSet<String> = tree.ids().into_iter().collect();
    let base_ids: HashSet<String> = parse_exchange_nodes(base)
        .iter()
        .map(ExchangeNode::node_id)
        .collect();

    let ours_nodes = parse_exchange_nodes(ours);
    let agent_new: Vec<ExchangeNode> = ours_nodes
        .into_iter()
        .filter(|n| matches!(n.kind, ExchangeNodeKind::Response { .. }))
        .filter(|n| {
            let id = n.node_id();
            // absent from theirs AND not an operator-deleted (base-present) turn.
            !theirs_ids.contains(&id) && !base_ids.contains(&id)
        })
        .map(|n| strip_trailing_boundary_and_blanks(&n))
        .collect();

    if agent_new.is_empty() {
        return tree.render();
    }

    // Preserve a trailing boundary marker at the very end: split it off the last
    // theirs node, append the agent-new turns, then re-attach it to the final
    // node. Assemble at the node-list level so the boundary stays last.
    let mut nodes = tree.nodes();
    let trailing_boundary: Vec<String> = match nodes.last_mut() {
        Some(last) => match split_trailing_boundary(last) {
            Some((head, boundary_tail)) => {
                last.lines = head;
                boundary_tail
            }
            None => Vec::new(),
        },
        None => Vec::new(),
    };
    nodes.extend(agent_new);
    let mut out = render_exchange_nodes(&nodes);
    out.push_str(&trailing_boundary.concat());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_000;
    /// Peer id for the agent (ours) candidate in the fork/merge demo.
    const OURS_PEER: u64 = 2;

    #[test]
    fn round_trips_an_exchange_body_byte_stable_through_the_crdt() {
        let inner = "❯ First prompt.\n\n### Re: A — opus\n\nAnswer A.\n\n❯ Second prompt.\n";
        let tree = ExchangeCrdt::from_inner(THEIRS_PEER, inner, T0);
        assert_eq!(
            tree.render(),
            inner,
            "parse→SeqCrdt→render must be byte-stable"
        );
    }

    #[test]
    fn every_node_is_a_distinct_crdt_element_no_cross_bleed() {
        let inner =
            "### Re: A — opus\n\nAnswer A.\n\n❯ A prompt.\n\n### Re: B — opus\n\nAnswer B.\n";
        let tree = ExchangeCrdt::from_inner(THEIRS_PEER, inner, T0);
        // Three distinct elements: response A, prompt, response B.
        assert_eq!(tree.ids().len(), 3, "each turn is its own SeqCrdt element");
        let nodes = tree.nodes();
        assert!(matches!(nodes[0].kind, ExchangeNodeKind::Response { .. }));
        assert_eq!(nodes[1].kind, ExchangeNodeKind::Prompt);
        assert!(matches!(nodes[2].kind, ExchangeNodeKind::Response { .. }));
    }

    #[test]
    fn concurrent_appends_on_forked_replicas_both_survive_a_seqcrdt_merge() {
        // The core anti-scramble guarantee at the CRDT level: two replicas that
        // fork from a common base and each append a distinct turn converge with
        // BOTH turns present after merge — no lost update, no cross-bleed.
        let base = ExchangeCrdt::from_inner(0, "### Re: A — opus\n\nAnswer A.\n", T0);
        let mut ours = base.fork(OURS_PEER);
        let mut theirs = base.fork(THEIRS_PEER);

        let node_b = parse_exchange_nodes("### Re: B — opus\n\nAnswer B.\n")
            .into_iter()
            .next()
            .unwrap();
        let node_c = parse_exchange_nodes("### Re: C — opus\n\nAnswer C.\n")
            .into_iter()
            .next()
            .unwrap();
        ours.push_back(node_b, T0 + 1);
        theirs.push_back(node_c, T0 + 2);

        theirs.merge(&ours, T0 + 3);
        let ids: HashSet<String> = theirs.ids().into_iter().collect();
        assert_eq!(ids.len(), 3, "A + B + C all survive the merge");
        let rendered = theirs.render();
        assert!(rendered.contains("Answer A."));
        assert!(rendered.contains("Answer B."));
        assert!(rendered.contains("Answer C."));
    }

    #[test]
    fn removed_node_tombstones_and_drops_from_order() {
        let inner = "### Re: A — opus\n\nAnswer A.\n\n### Re: B — opus\n\nAnswer B.\n";
        let mut tree = ExchangeCrdt::from_inner(THEIRS_PEER, inner, T0);
        let a_id = tree.ids()[0].clone();
        assert!(tree.remove(&a_id, T0 + 1));
        assert!(!tree.contains(&a_id));
        assert_eq!(tree.ids().len(), 1);
        assert!(!tree.render().contains("Answer A."));
    }

    #[test]
    fn construction_collapses_mirror_ordered_duplicate_identities() {
        // A poisoned buffer with the same response turn repeated non-adjacently
        // converges to a single element (the `insert_between` no-op-on-existing).
        let poisoned =
            "### Re: A — opus\n\nAnswer A.\n\n❯ Prompt.\n\n### Re: A — opus\n\nAnswer A.\n";
        let tree = ExchangeCrdt::from_inner(THEIRS_PEER, poisoned, T0);
        assert_eq!(tree.render().matches("### Re: A").count(), 1);
        assert!(tree.render().contains("❯ Prompt."));
    }

    #[test]
    fn merge_appends_agent_new_response_without_bleeding_into_operator_turns() {
        let base = "### Re: A — opus\n\nAnswer A.\n";
        let theirs = "### Re: A — opus\n\nAnswer A.\n\nOperator note between turns.\n";
        let ours = "### Re: A — opus\n\nAnswer A.\n\n### Re: B — opus\n\nAnswer B.\n";
        let merged = merge_exchange_crdt(base, ours, theirs, T0);
        assert!(merged.contains("Operator note between turns."));
        assert!(merged.contains("### Re: B — opus"));
        let a = merged.find("### Re: A").unwrap();
        let b = merged.find("### Re: B").unwrap();
        assert!(a < b, "A must not absorb B");
    }

    #[test]
    fn merge_honors_an_operator_deletion_via_base() {
        // `base` and `ours` both carry turn B; the operator deleted B (absent from
        // `theirs`). The stale agent replica must NOT resurrect it.
        let base = "### Re: A — opus\n\nAnswer A.\n\n### Re: B — opus\n\nAnswer B.\n";
        let ours = "### Re: A — opus\n\nAnswer A.\n\n### Re: B — opus\n\nAnswer B.\n";
        let theirs = "### Re: A — opus\n\nAnswer A.\n";
        let merged = merge_exchange_crdt(base, ours, theirs, T0);
        assert!(merged.contains("Answer A."));
        assert!(
            !merged.contains("Answer B."),
            "operator-deleted turn B must not be resurrected"
        );
    }

    #[test]
    fn merge_appends_a_genuinely_new_turn_absent_from_base() {
        // Turn B is new to `ours` (absent from base AND theirs) — a real new
        // answer that must be appended.
        let base = "### Re: A — opus\n\nAnswer A.\n";
        let ours = "### Re: A — opus\n\nAnswer A.\n\n### Re: B — opus\n\nAnswer B.\n";
        let theirs = "### Re: A — opus\n\nAnswer A.\n";
        let merged = merge_exchange_crdt(base, ours, theirs, T0);
        assert!(merged.contains("Answer B."), "new turn appended");
    }

    #[test]
    fn merge_lands_appended_turn_before_a_trailing_boundary_marker() {
        let base = "";
        let theirs = "### Re: A — opus\n\nAnswer A.\n<!-- agent:boundary:abc123 -->\n";
        let ours = "### Re: B — opus\n\nAnswer B.\n";
        let merged = merge_exchange_crdt(base, ours, theirs, T0);
        let b = merged.find("### Re: B").unwrap();
        let boundary = merged.find("<!-- agent:boundary:abc123 -->").unwrap();
        assert!(b < boundary, "appended turn precedes the trailing boundary");
    }

    #[test]
    fn merge_never_synthesizes_a_prompt_node_from_ours() {
        let base = "";
        let theirs = "❯ Do the thing.\n\n### Re: Done — opus\n\nDid it.\n";
        // ours carries an extra prompt-looking node; it must not be grafted in.
        let ours = "❯ A spurious agent prompt.\n\n### Re: Done — opus\n\nDid it.\n";
        let merged = merge_exchange_crdt(base, ours, theirs, T0);
        assert!(merged.contains("❯ Do the thing."));
        assert!(!merged.contains("spurious agent prompt"));
    }

    #[test]
    fn merge_with_no_agent_new_turns_returns_operator_content_verbatim() {
        let theirs = "### Re: A — opus\n\nAnswer A.\n\nOperator tail.\n";
        let merged = merge_exchange_crdt("", theirs, theirs, T0);
        assert_eq!(merged, theirs);
    }

    #[test]
    fn merge_does_not_reappend_a_byte_identical_agent_turn() {
        let base = "";
        let theirs = "### Re: A — opus\n\nAnswer A.\n";
        let ours = "### Re: A — opus\n\nAnswer A.\n";
        let merged = merge_exchange_crdt(base, ours, theirs, T0);
        assert_eq!(merged.matches("### Re: A").count(), 1);
    }
}

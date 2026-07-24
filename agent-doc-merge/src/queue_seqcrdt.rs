//! `lazily::SeqCrdt`-backed queue structure — Phase A of
//! `tasks/software/plan-crdt-structural-ops.md`.
//!
//! This mirrors [`crate::exchange_seqcrdt::ExchangeCrdt`]: the `agent:queue`
//! body's node-key-addressable items become a conflict-free sequence CRDT
//! ([`lazily::SeqCrdt`]) whose elements are [`QueueNode`]s keyed by the durable
//! `node_key` produced by [`agent_doc_markdown_ast::mutations::item_nodes`].
//!
//! Two structural ops cover the six `flock`-serialized consume sites:
//! - [`QueueCrdt::strike`] — tombstone a node (consume / strike / prune / ack).
//!   A tombstone is observationally terminal: [`SeqCrdt::remove`] sets the
//!   per-element `deleted` LWW register, and `order`/`values`/`contains`/`get`
//!   all skip deleted entries, so a struck node disappears from the live view no
//!   matter what a concurrent [`QueueCrdt::mark_done`] writes to its value.
//! - [`QueueCrdt::mark_done`] — flip a node to its completed (`- ~~text~~`)
//!   state via [`SeqCrdt::set_value`], leaving it present and rendered done.
//!
//! Because `deleted` and `value` are independent LWW registers, **a tombstone
//! wins over a concurrent mark_done for every interleaving**: the strike sets
//! `deleted=true`, the mark_done only updates `value`, and the deleted filter is
//! applied at read — the struck node is gone from the live projection regardless
//! of value stamp ordering. That is the convergence gain this substrate lands.
//!
//! Scope of THIS increment (deliberately bounded — the live hot path is the
//! `flock`-guarded whole-text rewrite in `agent-doc-queue-io`, which is NOT
//! changed here): the CRDT substrate plus convergence-law tests in isolation.
//! It is intentionally not yet wired into any consume site; Phases B–E do that.

use agent_doc_markdown_ast::mutations::item_nodes;
use lazily::{PeerId, SeqCrdt};

/// Peer id for a document rebuilt from operator (theirs) disk/buffer state.
const THEIRS_PEER: u64 = 1;

/// One queue entry's CRDT element value: its renderable item text plus a
/// completed flag. The durable identity is the external `node_key` (the
/// [`SeqCrdt`] key); this value carries only the mutable per-node state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueNode {
    /// Item text with strike/pin markers stripped (the
    /// [`agent_doc_markdown_ast::overlay::Item`] `text`), renderable as a single
    /// `- {text}` queue line.
    pub text: String,
    /// `true` once [`QueueCrdt::mark_done`] has flipped this entry to its
    /// completed (`- ~~text~~`) rendering.
    pub done: bool,
}

impl QueueNode {
    /// An active (not-yet-completed) queue node.
    pub fn active(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            done: false,
        }
    }

    /// A completed queue node (renders struck-through).
    pub fn done(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            done: true,
        }
    }
}

/// A `lazily::SeqCrdt`-backed queue: an ordered, conflict-free sequence of
/// [`QueueNode`]s keyed by durable `node_key`, mirroring [`ExchangeCrdt`].
///
/// [`ExchangeCrdt`]: crate::exchange_seqcrdt::ExchangeCrdt
#[derive(Clone)]
pub struct QueueCrdt {
    seq: SeqCrdt<String, QueueNode>,
}

impl QueueCrdt {
    /// Build a replica owned by `peer` from explicit `(node_key, node)` pairs,
    /// inserting each at the back in order. A repeated `node_key` collapses to
    /// its first occurrence, so construction is convergent — `insert_back` is a
    /// no-op once the key exists.
    pub fn from_nodes(peer: u64, nodes: &[(String, QueueNode)], now_micros: u64) -> Self {
        let mut seq = SeqCrdt::new(PeerId(peer));
        for (key, node) in nodes {
            if seq.contains(key) {
                continue;
            }
            seq.insert_back(key.clone(), node.clone(), now_micros);
        }
        Self { seq }
    }

    /// Parse an `agent:queue` inner body and build a replica from its
    /// node-key-addressable items. [`item_nodes`] needs a full document to
    /// locate the component, so the body is wrapped in a canonical
    /// single-component envelope; the resulting `node_key`s
    /// (`queue:0:{item.id}:{occurrence}`) are stable for a given body.
    ///
    /// Only node-key-addressable items are loaded — the structural-op targets.
    /// Free-text / fence / preset scaffolding is not modeled here; this is the
    /// item projection, not a byte-stable round-trip of an arbitrary polluted
    /// queue body. An item already rendered `~~struck~~` (completed) in the
    /// source loads as [`QueueNode::done`] (still live — completion is a state,
    /// not a tombstone); only an explicit [`Self::strike`] tombstones a node.
    pub fn from_inner(peer: u64, body: &str, now_micros: u64) -> Self {
        let doc = format!("<!-- agent:queue -->\n{body}<!-- /agent:queue -->\n");
        let mut seq = SeqCrdt::new(PeerId(peer));
        if let Ok(nodes) = item_nodes(&doc, "queue") {
            for n in nodes {
                let key = n.node_key;
                if seq.contains(&key) {
                    continue;
                }
                let node = QueueNode {
                    text: n.item.text,
                    done: n.item.struck,
                };
                seq.insert_back(key, node, now_micros);
            }
        }
        Self { seq }
    }

    /// Live `(node_key, node)` pairs in sequence order.
    pub fn nodes(&self) -> Vec<(String, QueueNode)> {
        self.seq.values()
    }

    /// Live node keys in sequence order.
    pub fn ids(&self) -> Vec<String> {
        self.seq.order()
    }

    /// Is `node_key` present and live (not tombstoned)?
    pub fn contains(&self, node_key: &str) -> bool {
        self.seq.contains(&node_key.to_string())
    }

    /// Read a live node by key, if present and not tombstoned.
    pub fn get(&self, node_key: &str) -> Option<QueueNode> {
        self.seq.get(&node_key.to_string())
    }

    /// Append a node at the back under the given durable `node_key`. A repeated
    /// key is a no-op (the existing entry wins), so grafting is convergent.
    pub fn push_back(&mut self, node_key: &str, node: QueueNode, now_micros: u64) {
        let key = node_key.to_string();
        if self.seq.contains(&key) {
            return;
        }
        self.seq.insert_back(key, node, now_micros);
    }

    /// Tombstone a node by key (consume / strike / prune / ack). Returns whether
    /// the tombstone registered against a known entry. A tombstone is
    /// observationally terminal — it wins over any concurrent [`Self::mark_done`]
    /// because the deleted-element filter is applied at every read.
    pub fn strike(&mut self, node_key: &str, now_micros: u64) -> bool {
        self.seq.remove(&node_key.to_string(), now_micros)
    }

    /// Flip a node to its completed (`- ~~text~~`) state. Returns whether the
    /// update applied against a live entry. Read-modify-write over the current
    /// value so the item text is preserved.
    pub fn mark_done(&mut self, node_key: &str, now_micros: u64) -> bool {
        match self.seq.get(&node_key.to_string()) {
            Some(mut node) => {
                node.done = true;
                self.seq.set_value(&node_key.to_string(), node, now_micros)
            }
            None => false,
        }
    }

    /// Fork this replica under a new peer id, preserving element lineage so a
    /// later [`Self::merge`] converges (both replicas' inserts survive).
    pub fn fork(&self, peer: u64) -> Self {
        Self {
            seq: self.seq.fork(PeerId(peer)),
        }
    }

    /// Conflict-free merge of another replica's state into this one. Elements
    /// present on either side survive; concurrent inserts both land in a single
    /// converged order. Per-field LWW of value, position, and tombstone — a
    /// tombstone wins observationally over any concurrent value edit.
    pub fn merge(&mut self, other: &QueueCrdt, now_micros: u64) -> bool {
        self.seq.merge(&other.seq, now_micros)
    }

    /// Render the live nodes back to an `agent:queue` inner body — one
    /// `- {text}` line per active node, `- ~~{text}~~` per completed node, in
    /// sequence order. Tombstoned nodes are absent.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (_, node) in self.nodes() {
            if node.done {
                out.push_str("- ~~");
                out.push_str(&node.text);
                out.push_str("~~\n");
            } else {
                out.push_str("- ");
                out.push_str(&node.text);
                out.push('\n');
            }
        }
        out
    }
}

/// The peer id for the operator (theirs) replica, re-exported for callers that
/// build a queue CRDT from operator-visible state.
pub fn theirs_peer() -> u64 {
    THEIRS_PEER
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_000;
    /// Peer id for the agent (ours) candidate in the fork/merge demos.
    const OURS_PEER: u64 = 2;

    fn key(suffix: &str) -> String {
        format!("queue:0:{suffix}:0")
    }

    #[test]
    fn from_nodes_round_trips_an_item_only_queue_body() {
        let body = "- do [#alpha]\n- do [#beta]\n";
        let tree = QueueCrdt::from_inner(THEIRS_PEER, body, T0);
        assert_eq!(tree.ids().len(), 2, "each item is its own SeqCrdt element");
        // The rendered projection matches the input byte for byte for an
        // item-only body (no fences / freeform scaffolding to drop).
        assert_eq!(tree.render(), body, "parse→SeqCrdt→render is byte-stable");
    }

    #[test]
    fn from_inner_loads_a_completed_item_as_done_not_tombstoned() {
        // A source `~~struck~~` item is a COMPLETED entry — still live, rendered
        // done — not a CRDT tombstone. Only strike() tombstones.
        let body = "- do [#alpha]\n- ~~do [#beta]~~\n";
        let tree = QueueCrdt::from_inner(THEIRS_PEER, body, T0);
        assert_eq!(tree.ids().len(), 2, "completed item is still live");
        let beta_key = tree.ids()[1].clone();
        let beta = tree.get(&beta_key).unwrap();
        assert!(beta.done, "struck source loads as done");
        assert_eq!(tree.render(), "- do [#alpha]\n- ~~do [#beta]~~\n");
    }

    #[test]
    fn strike_tombstones_and_drops_from_live_view() {
        let mut tree = QueueCrdt::from_inner(THEIRS_PEER, "- a\n- b\n", T0);
        let a = tree.ids()[0].clone();
        assert!(tree.strike(&a, T0 + 1));
        assert!(!tree.contains(&a), "struck node is not live");
        assert_eq!(tree.ids().len(), 1);
        assert!(!tree.render().contains("- a\n"), "struck node is absent from render");
    }

    #[test]
    fn mark_done_flips_a_node_to_completed_rendering() {
        let mut tree = QueueCrdt::from_inner(THEIRS_PEER, "- do [#alpha]\n", T0);
        let a = tree.ids()[0].clone();
        assert!(tree.mark_done(&a, T0 + 1));
        assert_eq!(tree.render(), "- ~~do [#alpha]~~\n", "done renders struck-through");
        assert!(tree.get(&a).unwrap().done);
        assert!(tree.contains(&a), "marked-done node is still live");
    }

    #[test]
    fn merge_is_commutative_and_idempotent() {
        let base = QueueCrdt::from_nodes(
            OURS_PEER,
            &[(key("a"), QueueNode::active("do [#a]"))],
            T0,
        );
        let mut ours = base.fork(OURS_PEER);
        let mut theirs = base.fork(THEIRS_PEER);
        ours.push_back(&key("b"), QueueNode::active("do [#b]"), T0 + 1);
        theirs.push_back(&key("c"), QueueNode::active("do [#c]"), T0 + 2);

        // Commutative: merge(theirs, ours) ≡ merge(ours, theirs).
        let mut left = ours.clone();
        left.merge(&theirs, T0 + 3);
        let mut right = theirs.clone();
        right.merge(&ours, T0 + 4);
        let left_ids: std::collections::HashSet<String> = left.ids().into_iter().collect();
        let right_ids: std::collections::HashSet<String> = right.ids().into_iter().collect();
        assert_eq!(left_ids, right_ids, "merge is commutative");
        assert_eq!(left_ids.len(), 3, "a + b + c all survive");

        // Idempotent: re-merging the same replica changes nothing observable.
        let before = left.ids();
        left.merge(&theirs, T0 + 5);
        assert_eq!(left.ids(), before, "merge is idempotent");
    }

    #[test]
    fn concurrent_appends_on_forked_replicas_both_survive_a_seqcrdt_merge() {
        let base = QueueCrdt::from_inner(THEIRS_PEER, "- a\n", T0);
        let mut ours = base.fork(OURS_PEER);
        let mut theirs = base.fork(THEIRS_PEER);
        ours.push_back(&key("b"), QueueNode::active("b"), T0 + 1);
        theirs.push_back(&key("c"), QueueNode::active("c"), T0 + 2);

        theirs.merge(&ours, T0 + 3);
        let ids: std::collections::HashSet<String> = theirs.ids().into_iter().collect();
        assert_eq!(ids.len(), 3, "a + b + c all survive the merge");
        let rendered = theirs.render();
        assert!(rendered.contains("- a\n"));
        assert!(rendered.contains("- b\n"));
        assert!(rendered.contains("- c\n"));
    }

    #[test]
    fn tombstone_wins_over_concurrent_mark_done() {
        // The core convergence law: one replica strikes X, a concurrent replica
        // marks X done. After merge, X is tombstoned (absent) regardless of the
        // merge order or relative timestamps — the deleted-element filter wins.
        let base = QueueCrdt::from_inner(THEIRS_PEER, "- do [#alpha]\n", T0);
        let alpha = base.ids()[0].clone();

        let mut ours = base.fork(OURS_PEER);
        let mut theirs = base.fork(THEIRS_PEER);
        ours.strike(&alpha, T0 + 1);
        theirs.mark_done(&alpha, T0 + 2);

        let mut merged = ours.clone();
        merged.merge(&theirs, T0 + 3);
        assert!(
            !merged.contains(&alpha),
            "tombstone wins: struck node is absent after merge"
        );
        assert_eq!(merged.ids().len(), 0);
        assert_eq!(merged.render(), "", "struck node renders nothing");

        // Symmetric: merge the other direction too.
        let mut merged2 = theirs.clone();
        merged2.merge(&ours, T0 + 4);
        assert!(!merged2.contains(&alpha), "tombstone wins in both merge orders");
    }

    #[test]
    fn tombstone_wins_even_when_mark_done_happens_later() {
        // mark_done touches only the value register, never `deleted`, so a later
        // mark_done cannot un-strike a tombstoned node.
        let base = QueueCrdt::from_inner(THEIRS_PEER, "- a\n", T0);
        let a = base.ids()[0].clone();

        let mut ours = base.fork(OURS_PEER);
        ours.strike(&a, T0 + 1);
        // A much later mark_done on a stale replica that never saw the strike.
        let mut stale = base.fork(THEIRS_PEER);
        stale.mark_done(&a, T0 + 1_000);

        let mut merged = ours.clone();
        merged.merge(&stale, T0 + 2);
        assert!(!merged.contains(&a), "later mark_done cannot resurrect a tombstone");
    }

    #[test]
    fn fork_diverge_merge_converges_on_independent_ops() {
        // Replica A marks alpha done and appends b; replica B strikes beta and
        // appends c. Merge converges: alpha done, beta gone, b and c present.
        let base = QueueCrdt::from_inner(THEIRS_PEER, "- alpha\n- beta\n", T0);
        let alpha = base.ids()[0].clone();
        let beta = base.ids()[1].clone();

        let mut ours = base.fork(OURS_PEER);
        ours.mark_done(&alpha, T0 + 1);
        ours.push_back(&key("b"), QueueNode::active("b-new"), T0 + 2);

        let mut theirs = base.fork(THEIRS_PEER);
        theirs.strike(&beta, T0 + 3);
        theirs.push_back(&key("c"), QueueNode::active("c-new"), T0 + 4);

        let mut merged = ours.clone();
        merged.merge(&theirs, T0 + 5);

        assert!(merged.contains(&alpha), "alpha survives (done, not struck)");
        assert!(!merged.contains(&beta), "beta is tombstoned");
        assert!(merged.get(&alpha).unwrap().done, "alpha's mark_done survived");
        assert!(merged.contains(&key("b")));
        assert!(merged.contains(&key("c")));
        assert_eq!(merged.ids().len(), 3, "alpha + b + c");
        let rendered = merged.render();
        assert!(rendered.contains("- ~~alpha~~\n"), "alpha rendered done");
        assert!(!rendered.contains("beta"), "beta absent");
        assert!(rendered.contains("- b-new\n"));
        assert!(rendered.contains("- c-new\n"));
    }

    #[test]
    fn strike_and_mark_done_are_idempotent_under_repeated_local_apply() {
        let mut tree = QueueCrdt::from_inner(THEIRS_PEER, "- a\n- b\n", T0);
        let a = tree.ids()[0].clone();
        let b = tree.ids()[1].clone();

        // Repeated strike of the same node: first applies, repeats are no-ops
        // against the already-tombstoned entry.
        assert!(tree.strike(&a, T0 + 1));
        tree.strike(&a, T0 + 2);
        assert!(!tree.contains(&a));

        // Repeated mark_done converges to done.
        assert!(tree.mark_done(&b, T0 + 3));
        tree.mark_done(&b, T0 + 4);
        assert!(tree.get(&b).unwrap().done);
        assert_eq!(tree.render(), "- ~~b~~\n");
    }

    #[test]
    fn construction_collapses_duplicate_node_keys() {
        // A poisoned buffer with the same node_key twice converges to one
        // element — insert_back is a no-op once the key exists.
        let dup = vec![
            (key("a"), QueueNode::active("do [#a]")),
            (key("a"), QueueNode::active("do [#a]")),
        ];
        let tree = QueueCrdt::from_nodes(THEIRS_PEER, &dup, T0);
        assert_eq!(tree.ids().len(), 1, "duplicate node_key collapses");
    }
}

//! Per-cell document projection (`#qcellmerge1` groundwork).
//!
//! agent-doc's shipped [`crdt::merge`](crate::crdt::merge) reconciles concurrent
//! edits with a **whole-document** CRDT merge. When two sides touch unrelated
//! cells the yrs text merge can still splice content across component / item
//! boundaries — the recurring corruption class where, e.g., a queue head's text
//! ends up inside an exchange block.
//!
//! This module is **additive groundwork** for the eventual per-cell cutover. It
//! does *not* change `crdt::merge`. Instead it:
//!
//! 1. Projects a parsed agent-doc document into an **ordered, keyed, per-component
//!    model** ([`project_document`]): for each component occurrence an ordered
//!    `Vec<(NodeKey, ItemValue)>` keyed by the stable
//!    `component:occurrence:item-id:index` node-key scheme.
//! 2. Computes **minimal per-item ops** between two document revisions
//!    ([`diff_document`]) using the lazily [`reconcile`] LIS keyed-diff — so a
//!    structural edit becomes a handful of `Insert`/`Remove`/`Move`/`Update`
//!    ops scoped to one component, never a whole-component (or whole-document)
//!    replace.
//! 3. Drives a reactive [`CellTree`]/[`CellMap`] representation
//!    ([`CellDocTree`]) so an edit to one item invalidates only that item's
//!    value cell — the core anti-corruption property that the eventual
//!    per-cell merge needs.
//!
//! ## Reuse of the existing parse + key scheme
//!
//! - Component framing is parsed with [`crate::component::parse`] (the single
//!   document-component parser).
//! - Item segmentation reuses the crdt module's keyed-child splitters
//!   ([`crate::crdt::split_list_children`] /
//!   [`crate::crdt::split_exchange_children`]) and their durable
//!   `list_item_key` identity (`id:<id>` / `txt:<normalized>`),
//!   so the same items that the live merge keys on are the items projected
//!   here. The crdt module's `PREAMBLE_KEY` leading-text child is preserved.
//!
//! The exposed [`NodeKey`] is the `component:occurrence:item-id:index` string
//! used throughout `turn_scope` / `op_log` (e.g. `queue:0:beta:0`), where
//! `item-id` is the durable child key.

use lazily::{CellMap, CellTree, Context, DiffOp, reconcile};

use crate::component::{self, Component};
use crate::crdt::{PREAMBLE_KEY, is_list_component, split_exchange_children, split_list_children};

/// The stable per-item identity: `component:occurrence:item-id:index`, matching
/// the node-key scheme used by `turn_scope`/`op_log` (e.g. `queue:0:beta:0`).
///
/// `item-id` is the durable child key (`id:<id>` or `txt:<normalized>`), which
/// is stable across strike / text edits; `index` is the child's position within
/// the occurrence at projection time and is informational only (the durable
/// identity is `component:occurrence:item-id`, so a pure reorder keeps the key
/// minus its trailing index — see [`node_key_identity`]).
pub type NodeKey = String;

/// An item's content: the exact source slice of the keyed child (lossless —
/// concatenating an occurrence's item values reproduces its component body).
pub type ItemValue = String;

/// Build the `component:occurrence:item-id:index` node key.
fn make_node_key(component: &str, occurrence: usize, item_id: &str, index: usize) -> NodeKey {
    format!("{component}:{occurrence}:{item_id}:{index}")
}

/// The reorder-stable identity of a node key: everything but the trailing
/// `:index`. Two revisions of the same item (same component/occurrence/item-id)
/// share this even if the item moved, so the keyed diff matches them.
pub fn node_key_identity(key: &str) -> &str {
    match key.rfind(':') {
        Some(i) => &key[..i],
        None => key,
    }
}

/// One component occurrence projected into an ordered keyed item sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentOccurrence {
    /// Component name (`queue`, `backlog`, `exchange`, …).
    pub component: String,
    /// 0-based occurrence index of this component name within the document.
    pub occurrence: usize,
    /// Ordered `(node_key, item_value)` pairs for this occurrence's keyed items.
    /// Keyed by [`NodeKey`]; values are the lossless source slices.
    pub items: Vec<(NodeKey, ItemValue)>,
}

impl ComponentOccurrence {
    /// The diff-stable keyed view (`identity` → value), dropping the positional
    /// `:index` suffix so a pure reorder is matched as a `Move`, not
    /// remove+insert. This is what feeds the lazily [`reconcile`].
    fn keyed_for_diff(&self) -> Vec<(String, ItemValue)> {
        self.items
            .iter()
            .map(|(k, v)| (node_key_identity(k).to_string(), v.clone()))
            .collect()
    }
}

/// Per-component diff between two document revisions: the component identity plus
/// the minimal lazily op set for its item level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDiff {
    /// Component name.
    pub component: String,
    /// 0-based occurrence index.
    pub occurrence: usize,
    /// Minimal per-item ops (`Insert`/`Remove`/`Move`/`Update`) keyed by the
    /// reorder-stable node-key identity (`component:occurrence:item-id`).
    pub ops: Vec<DiffOp<String, ItemValue>>,
}

impl ComponentDiff {
    /// True when this component has no item-level changes (no cross-cell splice
    /// touched it).
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// Project a parsed document body into ordered keyed item sequences, one per
/// component occurrence.
///
/// Components whose body is not item-structured (not a list component and not
/// `exchange`, or with no splittable / uniquely-keyed children) are projected as
/// a single whole-component item keyed `component:occurrence:body:0` — so the
/// diff degrades to a whole-component `Update` (still scoped to that occurrence,
/// never cross-component) exactly where the live merge would fall back to the
/// flat path.
pub fn project_document(doc: &str) -> Vec<ComponentOccurrence> {
    let components = match component::parse(doc) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut occurrence_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut out = Vec::with_capacity(components.len());
    for comp in &components {
        let occ = occurrence_counts.entry(comp.name.clone()).or_insert(0);
        let occurrence = *occ;
        *occ += 1;
        out.push(project_component(doc, comp, occurrence));
    }
    out
}

/// Project a single component occurrence into ordered keyed items.
fn project_component(doc: &str, comp: &Component, occurrence: usize) -> ComponentOccurrence {
    let body = comp.content(doc);
    let children = split_keyed_children(&comp.name, body);
    let items = match children {
        Some(children) => children
            .into_iter()
            .enumerate()
            .map(|(index, (key, text))| (make_node_key(&comp.name, occurrence, &key, index), text))
            .collect(),
        // Unsplittable / non-item component: one whole-body item. The reserved
        // `body` item-id never collides with a real child key (`id:`/`txt:`/
        // PREAMBLE) so a body-only component still gets a stable key.
        None => vec![(
            make_node_key(&comp.name, occurrence, "body", 0),
            body.to_string(),
        )],
    };
    ComponentOccurrence {
        component: comp.name.clone(),
        occurrence,
        items,
    }
}

/// Split a component body into `(item_key, text)` pairs reusing the crdt module's
/// keyed-child segmentation. Returns `None` (caller falls back to a single
/// whole-body item) when the body is not item-structured or its keys are not
/// unique — mirroring the live merge's fall-back-to-flat condition.
fn split_keyed_children(name: &str, body: &str) -> Option<Vec<(String, String)>> {
    let children = if name == "exchange" {
        split_exchange_children(body)
    } else if is_list_component(name) {
        split_list_children(body)
    } else {
        None
    }?;
    // Sanity: unique keys, else keyed reconciliation is unsound (two identical
    // free-text items). Disambiguate the PREAMBLE child, which is always first.
    let mut seen = std::collections::HashSet::new();
    let mut pairs = Vec::with_capacity(children.len());
    for child in &children {
        let key = if child.key == PREAMBLE_KEY {
            "preamble".to_string()
        } else {
            child.key.clone()
        };
        if !seen.insert(key.clone()) {
            return None;
        }
        pairs.push((key, child.text.clone()));
    }
    Some(pairs)
}

/// The minimal per-item op set between two revisions of one component
/// occurrence. Pure groundwork: a structural edit becomes minimal per-item ops
/// (the per-cell-merge enabler), not a whole-component replace.
fn diff_occurrence(old: &ComponentOccurrence, new: &ComponentOccurrence) -> ComponentDiff {
    let ops = reconcile(&old.keyed_for_diff(), &new.keyed_for_diff());
    ComponentDiff {
        component: new.component.clone(),
        occurrence: new.occurrence,
        ops,
    }
}

/// Diff two document revisions into per-component minimal op sets.
///
/// Matches component occurrences by `(component, occurrence)`. A component that
/// exists in only one revision yields a diff that inserts / removes its items.
/// Crucially, ops are **scoped per component occurrence**: an edit in component
/// A produces no ops for component B (no cross-cell splice). Components with no
/// item-level change are omitted (use the full vector if you need empties).
pub fn diff_document(old_doc: &str, new_doc: &str) -> Vec<ComponentDiff> {
    let old = project_document(old_doc);
    let new = project_document(new_doc);

    let mut old_by_key: std::collections::HashMap<(String, usize), &ComponentOccurrence> =
        std::collections::HashMap::new();
    for occ in &old {
        old_by_key.insert((occ.component.clone(), occ.occurrence), occ);
    }
    let mut new_keys: std::collections::HashSet<(String, usize)> = std::collections::HashSet::new();

    let mut diffs = Vec::new();

    // Components present in `new` (matched or inserted).
    for occ in &new {
        new_keys.insert((occ.component.clone(), occ.occurrence));
        let diff = match old_by_key.get(&(occ.component.clone(), occ.occurrence)) {
            Some(old_occ) => diff_occurrence(old_occ, occ),
            None => {
                // Wholly new occurrence → insert every item.
                let empty = ComponentOccurrence {
                    component: occ.component.clone(),
                    occurrence: occ.occurrence,
                    items: Vec::new(),
                };
                diff_occurrence(&empty, occ)
            }
        };
        if !diff.is_empty() {
            diffs.push(diff);
        }
    }

    // Components present only in `old` → remove every item.
    for occ in &old {
        if !new_keys.contains(&(occ.component.clone(), occ.occurrence)) {
            let empty = ComponentOccurrence {
                component: occ.component.clone(),
                occurrence: occ.occurrence,
                items: Vec::new(),
            };
            let diff = diff_occurrence(occ, &empty);
            if !diff.is_empty() {
                diffs.push(diff);
            }
        }
    }

    diffs
}

/// A reactive, per-item representation of a document: a [`CellTree`] root whose
/// children are component-occurrence subtrees, each of whose children are item
/// value cells.
///
/// The tree id scheme: the root id is `""`, a component-occurrence node id is
/// `component:occurrence`, and an item leaf id is its reorder-stable node-key
/// identity (`component:occurrence:item-id`). Applying [`ComponentDiff`] ops via
/// [`CellDocTree::apply`] mutates only the touched item cells / order signals —
/// a single-item edit invalidates exactly that leaf's value cell and nothing
/// else (per-cell isolation).
pub struct CellDocTree {
    root: CellTree<String, String>,
    /// Per component-occurrence ordered item map (parallel to the tree's item
    /// children) so ops apply via the LIS-driven [`CellMap`] path.
    occurrences: std::collections::HashMap<(String, usize), CellMap<String, String>>,
}

impl CellDocTree {
    /// Build a reactive tree from a document revision.
    pub fn from_document(ctx: &Context, doc: &str) -> Self {
        let root: CellTree<String, String> = CellTree::leaf(ctx, String::new(), String::new());
        let mut occurrences = std::collections::HashMap::new();
        for occ in project_document(doc) {
            let occ_id = format!("{}:{}", occ.component, occ.occurrence);
            let occ_node = root.insert_child(ctx, occ_id.clone(), String::new());
            let map: CellMap<String, String> = CellMap::new(ctx);
            for (key, value) in &occ.items {
                let identity = node_key_identity(key).to_string();
                occ_node.insert_child(ctx, identity.clone(), value.clone());
                map.entry(ctx, identity, value.clone());
            }
            occurrences.insert((occ.component.clone(), occ.occurrence), map);
        }
        Self { root, occurrences }
    }

    /// Root tree handle (for wiring derived computeds).
    pub fn root(&self) -> &CellTree<String, String> {
        &self.root
    }

    /// Reactively read an item's value by its reorder-stable identity, or `None`
    /// if absent. A reader is invalidated only when *that* item changes.
    pub fn item_value(
        &self,
        ctx: &Context,
        component: &str,
        occurrence: usize,
        identity: &str,
    ) -> Option<String> {
        self.occurrences
            .get(&(component.to_string(), occurrence))
            .and_then(|m| m.get(ctx, &identity.to_string()))
    }

    /// Reactive ordered item identities for a component occurrence.
    pub fn item_ids(&self, ctx: &Context, component: &str, occurrence: usize) -> Vec<String> {
        self.occurrences
            .get(&(component.to_string(), occurrence))
            .map(|m| m.keys(ctx))
            .unwrap_or_default()
    }

    /// Apply a component diff's minimal ops to the corresponding occurrence's
    /// item cells, driving the reactive tree to the new shape with minimal
    /// invalidation (stable items untouched, moves atomic, only changed values
    /// rewritten). Keeps the [`CellTree`] children in sync with the [`CellMap`].
    pub fn apply(&mut self, ctx: &Context, diff: &ComponentDiff) {
        let occ_key = (diff.component.clone(), diff.occurrence);
        let occ_id = format!("{}:{}", diff.component, diff.occurrence);
        let occ_node = self
            .root
            .child(&occ_id)
            .unwrap_or_else(|| self.root.insert_child(ctx, occ_id.clone(), String::new()));
        let map = self
            .occurrences
            .entry(occ_key)
            .or_insert_with(|| CellMap::new(ctx));
        for op in &diff.ops {
            match op {
                DiffOp::Remove { key } => {
                    map.remove(ctx, key);
                    occ_node.remove_child(ctx, key);
                }
                DiffOp::Insert { key, value, index } => {
                    map.entry(ctx, key.clone(), value.clone());
                    map.move_to(ctx, key, *index);
                    occ_node.insert_child(ctx, key.clone(), value.clone());
                    occ_node.move_child(ctx, key, *index);
                }
                DiffOp::Move { key, to } => {
                    map.move_to(ctx, key, *to);
                    occ_node.move_child(ctx, key, *to);
                }
                DiffOp::Update { key, value } => {
                    map.set(ctx, key.clone(), value.clone());
                    if let Some(child) = occ_node.child(key) {
                        child.set(ctx, value.clone());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
---
agent_doc_format: template
---
<!-- agent:queue -->
- do [#alpha] first task
- do [#beta] second task
- do [#gamma] third task
<!-- /agent:queue -->
<!-- agent:backlog -->
- [#one] backlog item one
- [#two] backlog item two
<!-- /agent:backlog -->
";

    fn queue_diff(diffs: &[ComponentDiff]) -> &ComponentDiff {
        diffs
            .iter()
            .find(|d| d.component == "queue")
            .expect("queue diff present")
    }

    #[test]
    fn project_keys_use_component_occurrence_item_index_scheme() {
        let occs = project_document(DOC);
        let queue = occs
            .iter()
            .find(|o| o.component == "queue")
            .expect("queue projected");
        assert_eq!(queue.occurrence, 0);
        let keys: Vec<&str> = queue.items.iter().map(|(k, _)| k.as_str()).collect();
        // component:occurrence:item-id:index — item-id is the durable `id:<id>`.
        assert_eq!(keys[0], "queue:0:id:alpha:0");
        assert_eq!(keys[1], "queue:0:id:beta:1");
        assert_eq!(keys[2], "queue:0:id:gamma:2");
    }

    #[test]
    fn reorder_yields_moves_not_replace() {
        // Move gamma to the top: a pure reorder.
        let reordered = DOC.replace(
            "- do [#alpha] first task\n- do [#beta] second task\n- do [#gamma] third task\n",
            "- do [#gamma] third task\n- do [#alpha] first task\n- do [#beta] second task\n",
        );
        let diffs = diff_document(DOC, &reordered);
        let q = queue_diff(&diffs);
        assert!(
            q.ops.iter().all(|op| matches!(op, DiffOp::Move { .. })),
            "pure reorder must be moves only, not remove+insert: {:?}",
            q.ops
        );
        assert!(!q.ops.is_empty(), "reorder must produce at least one move");
        assert!(q.ops.iter().all(|op| matches!(op, DiffOp::Move { .. })),);
    }

    #[test]
    fn insert_one_item_yields_single_insert() {
        let added = DOC.replace(
            "- do [#gamma] third task\n",
            "- do [#gamma] third task\n- do [#delta] fourth task\n",
        );
        let diffs = diff_document(DOC, &added);
        let q = queue_diff(&diffs);
        let inserts: Vec<_> = q
            .ops
            .iter()
            .filter(|op| matches!(op, DiffOp::Insert { .. }))
            .collect();
        assert_eq!(inserts.len(), 1, "exactly one insert: {:?}", q.ops);
        assert!(matches!(
            inserts[0],
            DiffOp::Insert { key, .. } if key == "queue:0:id:delta"
        ));
        // No spurious removes/updates on the siblings.
        assert!(
            !q.ops
                .iter()
                .any(|op| matches!(op, DiffOp::Remove { .. } | DiffOp::Update { .. })),
            "insert must not touch siblings: {:?}",
            q.ops
        );
    }

    #[test]
    fn remove_one_item_yields_single_remove() {
        let removed = DOC.replace("- do [#beta] second task\n", "");
        let diffs = diff_document(DOC, &removed);
        let q = queue_diff(&diffs);
        let removes: Vec<_> = q
            .ops
            .iter()
            .filter(|op| matches!(op, DiffOp::Remove { .. }))
            .collect();
        assert_eq!(removes.len(), 1, "exactly one remove: {:?}", q.ops);
        assert!(matches!(
            removes[0],
            DiffOp::Remove { key } if key == "queue:0:id:beta"
        ));
    }

    #[test]
    fn single_item_edit_yields_one_update_isolated() {
        // Edit only beta's text; its `#id` key is stable across the edit.
        let edited = DOC.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task EDITED\n",
        );
        let diffs = diff_document(DOC, &edited);
        let q = queue_diff(&diffs);
        assert_eq!(
            q.ops.len(),
            1,
            "exactly one op for a single-item edit: {:?}",
            q.ops
        );
        match &q.ops[0] {
            DiffOp::Update { key, value } => {
                assert_eq!(key, "queue:0:id:beta");
                assert!(value.contains("EDITED"));
            }
            other => panic!("expected a single Update, got {other:?}"),
        }
        // Per-cell isolation: alpha and gamma produced no ops at all.
        assert!(
            !q.ops.iter().any(|op| {
                let k = match op {
                    DiffOp::Update { key, .. }
                    | DiffOp::Move { key, .. }
                    | DiffOp::Remove { key } => key.as_str(),
                    DiffOp::Insert { key, .. } => key.as_str(),
                };
                k.contains("alpha") || k.contains("gamma")
            }),
            "sibling items must be untouched: {:?}",
            q.ops
        );
    }

    #[test]
    fn cross_component_isolation() {
        // Edit only a queue item; backlog must produce no ops (no cross-splice).
        let edited = DOC.replace(
            "- do [#alpha] first task\n",
            "- do [#alpha] first task CHANGED\n",
        );
        let diffs = diff_document(DOC, &edited);
        assert!(
            diffs.iter().all(|d| d.component != "backlog"),
            "an edit in queue must produce no ops for backlog: {diffs:?}"
        );
        // And the queue diff is exactly one Update.
        let q = queue_diff(&diffs);
        assert_eq!(q.ops.len(), 1);
        assert!(matches!(&q.ops[0], DiffOp::Update { key, .. } if key == "queue:0:id:alpha"));
    }

    #[test]
    fn reactive_tree_single_edit_spares_sibling_cell() {
        let ctx = Context::new();
        let mut tree = CellDocTree::from_document(&ctx, DOC);

        // A reader of the STABLE sibling alpha's value.
        let alpha_view = {
            let tree_ids = tree.item_value(&ctx, "queue", 0, "queue:0:id:alpha");
            assert!(tree_ids.is_some());
            ctx.computed({
                let map = tree
                    .occurrences
                    .get(&("queue".to_string(), 0))
                    .unwrap()
                    .clone();
                move |ctx| {
                    map.get(ctx, &"queue:0:id:alpha".to_string())
                        .unwrap_or_default()
                }
            })
        };
        let _ = ctx.get(&alpha_view);
        assert!(ctx.is_set(&alpha_view));

        // Edit only beta and apply its diff.
        let edited = DOC.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task EDITED\n",
        );
        let diffs = diff_document(DOC, &edited);
        for d in &diffs {
            tree.apply(&ctx, d);
        }

        // beta's cell updated…
        assert!(
            tree.item_value(&ctx, "queue", 0, "queue:0:id:beta")
                .unwrap()
                .contains("EDITED")
        );
        // …and alpha's value reader stayed cached (per-cell isolation).
        assert!(
            ctx.is_set(&alpha_view),
            "sibling value cell must not be invalidated by a per-item edit"
        );
    }

    #[test]
    fn reactive_tree_reorder_keeps_membership_cached() {
        let ctx = Context::new();
        let mut tree = CellDocTree::from_document(&ctx, DOC);
        let occ_node = tree.root().child(&"queue:0".to_string()).unwrap();
        let len_view = {
            let node = occ_node.clone();
            ctx.computed(move |ctx| node.len(ctx))
        };
        assert_eq!(ctx.get(&len_view), 3);

        let reordered = DOC.replace(
            "- do [#alpha] first task\n- do [#beta] second task\n- do [#gamma] third task\n",
            "- do [#gamma] third task\n- do [#alpha] first task\n- do [#beta] second task\n",
        );
        let diffs = diff_document(DOC, &reordered);
        for d in &diffs {
            tree.apply(&ctx, d);
        }
        // Order changed…
        assert_eq!(
            tree.item_ids(&ctx, "queue", 0),
            vec![
                "queue:0:id:gamma".to_string(),
                "queue:0:id:alpha".to_string(),
                "queue:0:id:beta".to_string(),
            ]
        );
        // …but membership count reader stayed cached (pure reorder).
        assert!(
            ctx.is_set(&len_view),
            "a pure reorder must not invalidate the membership-count reader"
        );
        assert_eq!(ctx.get(&len_view), 3);
    }

    #[test]
    fn unsplittable_component_diffs_as_whole_body_update() {
        // A status-like component with no list items → one whole-body item.
        let doc = "\
<!-- agent:status -->
working on it
<!-- /agent:status -->
";
        let edited = doc.replace("working on it", "done now");
        let diffs = diff_document(doc, &edited);
        let s = diffs
            .iter()
            .find(|d| d.component == "status")
            .expect("status diff");
        assert_eq!(s.ops.len(), 1);
        assert!(matches!(&s.ops[0], DiffOp::Update { key, .. } if key == "status:0:body"));
    }
}

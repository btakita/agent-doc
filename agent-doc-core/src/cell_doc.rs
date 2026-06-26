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

use lazily::{CellMap, CellTree, Context, DiffOp, TextCrdt, reconcile};

use crate::component::{self, Component};
use crate::crdt::{PREAMBLE_KEY, is_list_component, split_exchange_children, split_list_children};
use crate::queue_item_lifecycle::QueueItemLifecycle;

/// Environment variable that gates the live CRDT merge per-cell
/// 3-way path ([`merge_3way`], routed from [`crate::crdt::merge_by_component`]).
/// **Default ON** with an env kill-switch: per-cell merge is the production
/// default. Only an explicit falsy value (`0`/`false`/`off`/`no`) turns it off;
/// absent / empty / any other value ⇒ ON. See [`cell_merge_enabled`].
pub const CELL_MERGE_ENV: &str = "AGENT_DOC_CELL_MERGE";

/// Process-global serialization lock for tests that mutate the
/// `AGENT_DOC_CELL_MERGE` env var. The env var is process-wide, so flag-toggling
/// tests across modules (`cell_doc`, `crdt`) must serialize through this single
/// lock to avoid racing each other. `pub(crate)` so sibling test modules share it.
#[cfg(test)]
pub(crate) static CELL_MERGE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Environment variable that opts a same-cell *both-sides-changed* divergence
/// into the op-level [`TextCrdt`] 3-way merge (`#qcellmerge1` opcapture rung).
///
/// **Sub-gate of [`CELL_MERGE_ENV`], default ON** with an env kill-switch. With
/// the per-cell merge seam ON ([`cell_merge_enabled`]), the `(both changed)`
/// branch first attempts a deterministic character-granular 3-way merge: two
/// edits to DISJOINT regions of one cell converge with BOTH preserved and NO
/// conflict; a true same-region overlap still records a [`CellConflict`] and
/// applies the policy. An explicit falsy value (`0`/`false`/`off`/`no`) restores
/// the legacy policy-only behavior. Forced OFF when the master switch is
/// explicitly disabled. See [`cell_merge_opcapture_enabled`] / [`op_level_merge`].
pub const CELL_MERGE_OPCAPTURE_ENV: &str = "AGENT_DOC_CELL_MERGE_OPCAPTURE";

/// Environment variable that opts a recorded same-region [`CellConflict`] into an
/// **operator-visible in-band conflict marker** in the merged document
/// (`#qcellconflict` conflict-surfacing rung).
///
/// **Sub-gate of [`CELL_MERGE_ENV`], default ON** with an env kill-switch. With
/// the per-cell merge seam ON, each genuine same-region content conflict appends
/// a deterministic, framing-safe HTML-comment conflict block to the conflicted
/// node's value, surfacing BOTH versions verbatim while keeping ours as the
/// active/structural text (never a fabricated blend). An explicit falsy value
/// (`0`/`false`/`off`/`no`) restores the legacy behavior: the conflict is
/// resolved ours-wins and only logged to stderr ([`log_conflicts`]) — the
/// operator never sees the losing side in the document. Forced OFF when the
/// master switch is explicitly disabled. A lawful lifecycle join is never a
/// conflict and never marked. See [`surface_conflict_markers`] / [`conflict_marker`].
pub const CELL_MERGE_CONFLICT_MARKERS_ENV: &str = "AGENT_DOC_CELL_MERGE_CONFLICT_MARKERS";

/// Parse an *explicitly* falsy on/off env value (`0`/`false`/`off`/`no`,
/// case/space-insensitive). The kill-switch counterpart for the default-ON
/// per-cell merge gates: an absent var, an empty value, or any unrecognized
/// value is NOT falsy — only the recognized kill-switch tokens disable a gate.
fn env_falsy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "0" | "false" | "off" | "no")
        }
        Err(_) => false,
    }
}

/// Whether the per-cell 3-way merge routing seam is enabled. **Default ON** with
/// an env kill-switch: per-cell merge is the production default and only an
/// explicit falsy `AGENT_DOC_CELL_MERGE` (`0`/`false`/`off`/`no`, case/space-
/// insensitive) turns it off. Absent / empty / any non-falsy value ⇒ ON.
pub fn cell_merge_enabled() -> bool {
    !env_falsy(CELL_MERGE_ENV)
}

/// Whether the op-level [`TextCrdt`] 3-way merge for same-cell both-sides edits is
/// enabled (`#qcellmerge1` opcapture). **Default ON** with an env kill-switch:
/// only an explicit falsy `AGENT_DOC_CELL_MERGE_OPCAPTURE` turns it off. As a
/// sub-feature of the per-cell merge stack, it is also forced OFF whenever the
/// master switch [`cell_merge_enabled`] is explicitly disabled — disabling the
/// master kills the whole stack regardless of the sub-gate value.
pub fn cell_merge_opcapture_enabled() -> bool {
    cell_merge_enabled() && !env_falsy(CELL_MERGE_OPCAPTURE_ENV)
}

/// Whether recorded same-region [`CellConflict`]s are surfaced as operator-visible
/// in-band conflict markers (`#qcellconflict`). **Default ON** with an env
/// kill-switch: only an explicit falsy `AGENT_DOC_CELL_MERGE_CONFLICT_MARKERS`
/// turns it off. As a sub-feature of the per-cell merge stack, it is also forced
/// OFF whenever the master switch [`cell_merge_enabled`] is explicitly disabled —
/// disabling the master kills the whole stack regardless of the sub-gate value.
/// Independent of [`cell_merge_opcapture_enabled`] otherwise (opcapture *reduces*
/// the conflict set by cleanly merging disjoint edits; this *surfaces* the
/// irreducible same-region remainder), so the two sub-gates compose freely.
pub fn cell_merge_conflict_markers_enabled() -> bool {
    cell_merge_enabled() && !env_falsy(CELL_MERGE_CONFLICT_MARKERS_ENV)
}

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

// ---------------------------------------------------------------------------
// Phase 2 (`#qcellmerge1` cutover): 3-way per-cell merge.
//
// `crdt::merge` is a *3-way* merge: `merge(base, ours, theirs)`. The 2-way
// `diff_document` above projects/diffs two revisions; the cutover composes TWO
// keyed diffs (base→ours and base→theirs) per component occurrence into one
// merged item list, surfacing a real conflict when the SAME item is updated to
// different text on both sides (`#qnodemerge5`: never fabricate a blend).
// ---------------------------------------------------------------------------

/// What kind of same-item-both-sides divergence produced a [`CellConflict`].
///
/// Only a [`ConflictKind::Content`] divergence is a *genuine* conflict — two
/// sides edited the SAME item to different text at the SAME lawful lifecycle
/// level (both `Live`, or both `Struck`), so neither side's intent subsumes the
/// other and a deterministic [`ConflictPolicy`] must pick a winner. A
/// strike-vs-unstrike is NOT a conflict (it is a lawful `Live < Struck`
/// lifecycle join, struck wins) and is never recorded here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictKind {
    /// Both sides at the same lifecycle level edited the item to different text.
    #[default]
    Content,
}

/// A surfaced 3-way conflict: the same keyed item was updated to different text
/// on both sides at the SAME lawful lifecycle level. The merge applies a
/// deterministic policy (ours-wins, see [`ConflictPolicy`]) and records the
/// conflict here so callers never silently lose the losing side.
///
/// Lifecycle joins (a strike vs. a stale un-strike) are *not* conflicts — they
/// resolve deterministically to the struck side (`Live < Struck`) and are
/// excluded from this list. Only genuine same-level content disagreements count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellConflict {
    /// Component name (`queue`, `exchange`, …) the conflict occurred in.
    pub component: String,
    /// 0-based occurrence index of that component.
    pub occurrence: usize,
    /// Reorder-stable item identity (`component:occurrence:item-id`).
    pub identity: String,
    /// The kind of divergence (currently always [`ConflictKind::Content`]; a
    /// lifecycle join is never recorded as a conflict).
    pub kind: ConflictKind,
    /// The value chosen by the merge policy (kept in `merged_text`).
    pub chosen: ItemValue,
    /// `ours` side's value (the agent / committed-snapshot side).
    pub ours: ItemValue,
    /// `theirs` side's value (the live editor / disk side).
    pub theirs: ItemValue,
}

/// How a same-item-both-sides conflict is resolved deterministically. Recorded
/// in the outcome regardless; the chosen value lands in `merged_text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictPolicy {
    /// Keep `ours` (the agent / committed-snapshot side). Default — mirrors the
    /// `exchange` committed-history spine in `reconcile_component_body`.
    #[default]
    OursWins,
    /// Keep `theirs` (the live editor / disk side).
    TheirsWins,
}

/// Outcome of a 3-way per-cell merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellMergeOutcome {
    /// The merged document text. When `fell_back` is true this is empty and the
    /// caller must run the legacy whole-doc / `merge_by_component` path instead.
    pub merged_text: String,
    /// Real conflicts surfaced (never silently dropped). Empty for a clean merge.
    pub conflicts: Vec<CellConflict>,
    /// True when the per-cell merge declined (structural divergence, duplicate
    /// keys, unsplittable framing). The caller falls back; `merged_text` is empty.
    pub fell_back: bool,
}

impl CellMergeOutcome {
    fn fallback() -> Self {
        CellMergeOutcome {
            merged_text: String::new(),
            conflicts: Vec::new(),
            fell_back: true,
        }
    }
}

/// One top-level node of a document: an interstitial text slot or a component
/// occurrence (framing + projected keyed items).
enum DocNode {
    Interstitial(String),
    Component {
        framing: ComponentFraming,
        occurrence: ComponentOccurrence,
    },
}

/// A component occurrence's exact open/close marker slices, so a merged body can
/// be reframed losslessly (`open + merged_body + close`).
struct ComponentFraming {
    open: String,
    close: String,
}

/// Parse a document into ordered top-level [`DocNode`]s: interstitials around
/// each top-level component, with each component projected into keyed items.
/// `None` on a parse error (caller falls back).
fn parse_doc_nodes(doc: &str) -> Option<Vec<DocNode>> {
    let comps = component::parse(doc).ok()?;
    // Top-level components only (mirror `crdt::segment_into_nodes`).
    let mut top: Vec<&Component> = comps
        .iter()
        .filter(|c| {
            !comps.iter().any(|o| {
                !std::ptr::eq(o, *c) && o.open_start <= c.open_start && c.close_end <= o.close_end
            })
        })
        .collect();
    top.sort_by_key(|c| c.open_start);

    let mut occurrence_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut nodes = Vec::with_capacity(top.len() * 2 + 1);
    let mut cursor = 0usize;
    for c in top {
        nodes.push(DocNode::Interstitial(doc[cursor..c.open_start].to_string()));
        let occ_idx = occurrence_counts.entry(c.name.clone()).or_insert(0);
        let occurrence = *occ_idx;
        *occ_idx += 1;
        let framing = ComponentFraming {
            open: doc[c.open_start..c.open_end].to_string(),
            close: doc[c.close_start..c.close_end].to_string(),
        };
        nodes.push(DocNode::Component {
            framing,
            occurrence: project_component(doc, c, occurrence),
        });
        cursor = c.close_end;
    }
    nodes.push(DocNode::Interstitial(doc[cursor..].to_string()));
    Some(nodes)
}

/// The ordered component-name sequence of a node list (for structural pairing).
fn component_name_sequence(nodes: &[DocNode]) -> Vec<&str> {
    nodes
        .iter()
        .filter_map(|n| match n {
            DocNode::Component { occurrence, .. } => Some(occurrence.component.as_str()),
            DocNode::Interstitial(_) => None,
        })
        .collect()
}

/// True for an item-structured component occurrence whose keys are unique. The
/// whole-body fallback occurrence (single `…:body` item) is treated as
/// item-structured too — its single keyed item still composes cleanly.
fn keys_unique(occ: &ComponentOccurrence) -> bool {
    let mut seen = std::collections::HashSet::new();
    occ.items
        .iter()
        .all(|(k, _)| seen.insert(node_key_identity(k).to_string()))
}

/// The result of composing one occurrence: the merged ordered `(identity,
/// value)` item list plus any conflicts surfaced.
type ComposedOccurrence = (Vec<(String, ItemValue)>, Vec<CellConflict>);

/// Compose the base→ours and base→theirs keyed item lists for one component
/// occurrence into a merged ordered `(identity, value)` list, recording any
/// same-item-both-sides conflicts.
///
/// Ordering policy mirrors `reconcile_component_body`: list components
/// (`queue`/`backlog`/`review`/`done`) use `theirs` (the live/operator side) as
/// the order spine; `exchange` uses `ours` (append-only committed history) as
/// the spine. Items present on only one side weave in after their nearest placed
/// predecessor.
fn compose_occurrence(
    base: &ComponentOccurrence,
    ours: &ComponentOccurrence,
    theirs: &ComponentOccurrence,
    policy: ConflictPolicy,
) -> Option<ComposedOccurrence> {
    // The two keyed op sets the cutover composes: base→ours and base→theirs,
    // each via the lazily LIS keyed `reconcile`. From each we read which item
    // identities that side *touched* (Insert/Update/Move) and which it *removed*.
    // The per-key resolution below composes the two op streams: an identity
    // touched on one side but not the other takes that side; an identity updated
    // on BOTH sides to different text is a real conflict; an identity removed on
    // one side and untouched on the other is dropped.
    let base_to_ours = reconcile(&base.keyed_for_diff(), &ours.keyed_for_diff());
    let base_to_theirs = reconcile(&base.keyed_for_diff(), &theirs.keyed_for_diff());
    let ours_removed = removed_keys(&base_to_ours);
    let theirs_removed = removed_keys(&base_to_theirs);
    let ours_updated = updated_keys(&base_to_ours);
    let theirs_updated = updated_keys(&base_to_theirs);

    // Reorder-stable identity → value, for each side.
    let to_map = |occ: &ComponentOccurrence| -> std::collections::HashMap<String, ItemValue> {
        occ.items
            .iter()
            .map(|(k, v)| (node_key_identity(k).to_string(), v.clone()))
            .collect()
    };
    let base_map = to_map(base);
    let ours_map = to_map(ours);
    let theirs_map = to_map(theirs);

    let identity_seq = |occ: &ComponentOccurrence| -> Vec<String> {
        occ.items
            .iter()
            .map(|(k, _)| node_key_identity(k).to_string())
            .collect()
    };

    let is_exchange = ours.component == "exchange";
    // `#queuestatemachine2`/`#qheadresidue`: list components (queue/backlog/
    // review/done) carry per-item strike lifecycle, so a matched-but-different
    // item is first routed through the `Live < Struck` join (anti-resurrection),
    // mirroring `crdt::reconcile_list_item_lifecycle`. A struck side beats a stale
    // un-strike from the persisted base/agent side regardless of the ours/theirs
    // content policy. `exchange` blocks are not strike-bearing and keep the
    // content-policy path.
    let lifecycle_governed = is_list_component(&ours.component);
    // Order spine: theirs (live) for list components, ours (history) for exchange.
    let (spine, weave) = if is_exchange {
        (identity_seq(ours), identity_seq(theirs))
    } else {
        (identity_seq(theirs), identity_seq(ours))
    };
    let order = weave_order(&spine, &weave);

    let mut merged: Vec<(String, ItemValue)> = Vec::with_capacity(order.len());
    let mut conflicts = Vec::new();
    for id in &order {
        let in_b = base_map.contains_key(id);
        let in_o = ours_map.get(id);
        let in_t = theirs_map.get(id);
        let resolved: Option<ItemValue> = match (in_o, in_t) {
            (Some(o), Some(t)) => {
                if o == t {
                    Some(o.clone())
                } else if lifecycle_governed {
                    // Lifecycle-aware join (`Live < Struck`). Classify each side's
                    // visible lifecycle and join: if exactly one side is at the
                    // joined (higher-ranked) level, that side's text governs — a
                    // struck side can never be resurrected by a stale un-strike,
                    // and this is NOT a conflict (lawful lifecycle progression).
                    let o_life = QueueItemLifecycle::classify(o);
                    let t_life = QueueItemLifecycle::classify(t);
                    let joined = o_life.join(t_life);
                    match (o_life == joined, t_life == joined) {
                        // Only ours is at the lawful level → ours' text governs.
                        (true, false) => Some(o.clone()),
                        // Only theirs is at the lawful level → theirs' text governs.
                        (false, true) => Some(t.clone()),
                        // Both at the same lifecycle level but text differs → fall
                        // through to the content-conflict resolution below.
                        _ => Some(resolve_content_divergence(
                            id,
                            o,
                            t,
                            in_b,
                            base_map.get(id),
                            &ours_updated,
                            &theirs_updated,
                            ours,
                            policy,
                            &mut conflicts,
                        )),
                    }
                } else {
                    // Non-lifecycle component (exchange): content policy only.
                    Some(resolve_content_divergence(
                        id,
                        o,
                        t,
                        in_b,
                        base_map.get(id),
                        &ours_updated,
                        &theirs_updated,
                        ours,
                        policy,
                        &mut conflicts,
                    ))
                }
            }
            // Present only in ours: ours kept/edited it, or theirs removed it.
            // Honor theirs' removal only if ours did NOT touch it (and it is not
            // protected append-only `exchange` history).
            (Some(o), None) => {
                if theirs_removed.contains(id) && !ours_updated.contains(id) && !is_exchange {
                    None
                } else {
                    Some(o.clone())
                }
            }
            // Present only in theirs: symmetric.
            (None, Some(t)) => {
                if ours_removed.contains(id) && !theirs_updated.contains(id) && !is_exchange {
                    None
                } else {
                    Some(t.clone())
                }
            }
            (None, None) => None,
        };
        if let Some(v) = resolved {
            merged.push((id.clone(), v));
        }
    }
    Some((merged, conflicts))
}

/// Resolve a same-item-both-sides text divergence that is NOT subsumed by a
/// lifecycle join (either a non-lifecycle component, or both sides at the same
/// lifecycle level). Records a genuine [`ConflictKind::Content`] conflict when
/// both sides changed the item vs base, applying the deterministic
/// [`ConflictPolicy`]. Returns the chosen value.
#[allow(clippy::too_many_arguments)]
fn resolve_content_divergence(
    id: &str,
    o: &ItemValue,
    t: &ItemValue,
    in_b: bool,
    base_value: Option<&ItemValue>,
    ours_updated: &std::collections::HashSet<String>,
    theirs_updated: &std::collections::HashSet<String>,
    occ: &ComponentOccurrence,
    policy: ConflictPolicy,
    conflicts: &mut Vec<CellConflict>,
) -> ItemValue {
    // Each side's change is read from its reconcile op set (Update/Insert).
    let o_changed = ours_updated.contains(id) || !in_b;
    let t_changed = theirs_updated.contains(id) || !in_b;
    match (o_changed, t_changed) {
        // Only ours changed → take ours.
        (true, false) => o.clone(),
        // Only theirs changed → take theirs.
        (false, true) => t.clone(),
        // Both changed to different text at the same lifecycle level. With the
        // opcapture sub-gate ON (the default), first attempt a real op-level
        // 3-way merge: if the two sides edited DISJOINT regions of the cell, that
        // converges cleanly with both edits preserved and is NOT a conflict. Only
        // a genuine same-region overlap falls through to policy. With opcapture
        // explicitly OFF (the legacy behavior), every both-changed cell is a REAL
        // content conflict resolved by the deterministic policy.
        (true, true) => {
            if cell_merge_opcapture_enabled()
                && let Some(base) = base_value
                && let Some(merged) = op_level_merge(base, o, t)
            {
                // Disjoint edits converged op-by-op — no information was lost, so
                // no conflict is recorded.
                return merged;
            }
            let chosen = match policy {
                ConflictPolicy::OursWins => o.clone(),
                ConflictPolicy::TheirsWins => t.clone(),
            };
            conflicts.push(CellConflict {
                component: occ.component.clone(),
                occurrence: occ.occurrence,
                identity: id.to_string(),
                kind: ConflictKind::Content,
                chosen: chosen.clone(),
                ours: o.clone(),
                theirs: t.clone(),
            });
            chosen
        }
        // Neither side changed vs base but o != t — impossible (both equal base ⇒
        // o == t). Defensive: take ours.
        (false, false) => o.clone(),
    }
}

/// One contiguous character-range replacement of `base[start..end)` (char
/// indices, half-open) by `replacement`, derived from a common-prefix /
/// common-suffix diff. A pure insertion has `start == end`; a pure deletion has
/// an empty `replacement`. This is the minimal single-span edit that turns base
/// into the side.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CharSpanEdit {
    /// First differing char index in base (inclusive).
    start: usize,
    /// One past the last replaced char index in base (exclusive).
    end: usize,
    /// The replacement characters inserted in `[start, end)`.
    replacement: Vec<char>,
}

/// Compute the minimal single contiguous `base → side` character edit by peeling
/// the shared prefix and suffix. Deterministic and allocation-light. Returns
/// `None` when the strings are identical (no edit). The middle (between the
/// common prefix and suffix) is treated as one replaced region — this is exactly
/// the granularity the disjoint-region test needs, and matches how a localized
/// human/agent edit shows up (one inserted/replaced run).
fn char_span_edit(base: &[char], side: &[char]) -> Option<CharSpanEdit> {
    if base == side {
        return None;
    }
    // Common prefix length.
    let mut start = 0usize;
    let max_pre = base.len().min(side.len());
    while start < max_pre && base[start] == side[start] {
        start += 1;
    }
    // Common suffix length, not overlapping the already-matched prefix.
    let mut suffix = 0usize;
    let max_suf = (base.len() - start).min(side.len() - start);
    while suffix < max_suf && base[base.len() - 1 - suffix] == side[side.len() - 1 - suffix] {
        suffix += 1;
    }
    let end = base.len() - suffix;
    let replacement = side[start..side.len() - suffix].to_vec();
    Some(CharSpanEdit {
        start,
        end,
        replacement,
    })
}

/// Whether two base-relative replaced ranges overlap. Adjacent (touching but not
/// crossing) ranges are treated as **overlapping** because two edits that meet at
/// the same boundary are not information-theoretically independent — keep them on
/// the conflict path. Empty ranges (pure insertions) at the *same* point also
/// overlap (both sides inserted at one anchor); distinct insertion points do not.
fn ranges_overlap(a: &CharSpanEdit, b: &CharSpanEdit) -> bool {
    // Two pure insertions (empty range) collide only at the identical anchor.
    if a.start == a.end && b.start == b.end {
        return a.start == b.start;
    }
    // Otherwise overlap iff the half-open intervals intersect, treating a pure
    // insertion as touching the char boundary it sits at (closed at that point).
    let (a_lo, a_hi) = (a.start, a.end.max(a.start));
    let (b_lo, b_hi) = (b.start, b.end.max(b.start));
    a_lo < b_hi && b_lo < a_hi
        // Insertion exactly inside the other side's replaced span boundary.
        || (a.start == a.end && a.start > b_lo && a.start < b_hi)
        || (b.start == b.end && b.start > a_lo && b.start < a_hi)
}

/// Apply one [`CharSpanEdit`] (a base-relative replace) to a forked [`TextCrdt`]
/// via visible-index `delete`/`insert`. The fork still holds the base characters
/// at base indices, so `start`/`end` are valid visible indices on it.
fn apply_span_edit(crdt: &mut TextCrdt, edit: &CharSpanEdit) {
    // Delete the replaced base chars (from the back so earlier indices stay valid).
    for idx in (edit.start..edit.end).rev() {
        crdt.delete(idx);
    }
    // Insert the replacement at the span start.
    for (i, ch) in edit.replacement.iter().enumerate() {
        crdt.insert(edit.start + i, *ch);
    }
}

/// Op-level 3-way merge of one cell's text (`#qcellmerge1` opcapture).
///
/// Builds a base [`TextCrdt`], forks it to a deterministic "ours" peer (`1`) and
/// "theirs" peer (`2`), replays each side's single-span char edit onto its fork,
/// then merges. Returns `Some(merged_text)` ONLY when the two edits touch
/// **disjoint** regions of the base (so the merge is unambiguous and preserves
/// both edits); returns `None` for a true same-region overlap, leaving the caller
/// on the conflict + [`ConflictPolicy`] path. Fully deterministic: fixed peer ids
/// (ours-before-theirs), no clocks or randomness.
fn op_level_merge(base: &str, ours: &str, theirs: &str) -> Option<String> {
    let base_chars: Vec<char> = base.chars().collect();
    let ours_edit = char_span_edit(&base_chars, &ours.chars().collect::<Vec<_>>())?;
    let theirs_edit = char_span_edit(&base_chars, &theirs.chars().collect::<Vec<_>>())?;

    // Overlapping edits are an irreducible same-region conflict (`#qnodemerge5`):
    // do NOT op-merge them — fall through to the policy path.
    if ranges_overlap(&ours_edit, &theirs_edit) {
        return None;
    }

    // Deterministic peer ids: ours=1, theirs=2 (stable ordering, no time/random).
    let base_crdt = TextCrdt::from_str(1, base);
    let mut ours_fork = base_crdt.fork(1);
    let mut theirs_fork = base_crdt.fork(2);
    apply_span_edit(&mut ours_fork, &ours_edit);
    apply_span_edit(&mut theirs_fork, &theirs_edit);
    ours_fork.merge(&theirs_fork);
    Some(ours_fork.text())
}

/// Identities the side `Remove`d relative to base.
fn removed_keys(ops: &[DiffOp<String, ItemValue>]) -> std::collections::HashSet<String> {
    ops.iter()
        .filter_map(|op| match op {
            DiffOp::Remove { key } => Some(key.clone()),
            _ => None,
        })
        .collect()
}

/// Identities the side `Update`d (changed value) relative to base.
fn updated_keys(ops: &[DiffOp<String, ItemValue>]) -> std::collections::HashSet<String> {
    ops.iter()
        .filter_map(|op| match op {
            DiffOp::Update { key, .. } => Some(key.clone()),
            _ => None,
        })
        .collect()
}

/// Weave `weave`-only keys into the `spine` order: keep the spine order, then
/// insert each weave-only key after its nearest preceding already-placed weave
/// key (mirrors `crdt::order_union`).
fn weave_order(spine: &[String], weave: &[String]) -> Vec<String> {
    let spine_set: std::collections::HashSet<&str> = spine.iter().map(|s| s.as_str()).collect();
    let mut result: Vec<String> = spine.to_vec();
    for (i, k) in weave.iter().enumerate() {
        if spine_set.contains(k.as_str()) || result.iter().any(|r| r == k) {
            continue;
        }
        let mut insert_pos = 0usize;
        for j in (0..i).rev() {
            if let Some(p) = result.iter().position(|r| r == &weave[j]) {
                insert_pos = p + 1;
                break;
            }
        }
        result.insert(insert_pos, k.clone());
    }
    result
}

/// 3-way per-cell merge (`#qcellmerge1` cutover): the lazily-`reconcile`-based
/// counterpart of [`crate::crdt::merge`].
///
/// For each top-level component occurrence (keyed by `node_key_identity`), the
/// base→ours and base→theirs op sets ([`reconcile`]) are composed into one merged
/// item list:
/// - an item changed on only one side → that side's value/position,
/// - an item removed on one side and untouched on the other → removed,
/// - the **same item updated on BOTH sides to different text → a real conflict**
///   ([`CellConflict`]) resolved by [`ConflictPolicy`] (default ours-wins),
///   never a fabricated blend.
///
/// List components order by `theirs` (operator/live order wins); `exchange`
/// stays append-only (ordered by `ours`, base blocks protected from deletion).
/// Merged components are recombined in document order inside ours' framing /
/// interstitials.
///
/// On any **structural divergence** — unequal component-name sequences, a
/// duplicate key, or an unsplittable component — the outcome signals `fell_back`
/// and the caller runs the existing whole-doc / `merge_by_component` path. The
/// merge never emits malformed framing.
pub fn merge_3way(base_doc: &str, ours_doc: &str, theirs_doc: &str) -> CellMergeOutcome {
    if ours_doc == theirs_doc {
        return CellMergeOutcome {
            merged_text: ours_doc.to_string(),
            conflicts: Vec::new(),
            fell_back: false,
        };
    }

    let ours_nodes = match parse_doc_nodes(ours_doc) {
        Some(n) => n,
        None => return CellMergeOutcome::fallback(),
    };
    let theirs_nodes = match parse_doc_nodes(theirs_doc) {
        Some(n) => n,
        None => return CellMergeOutcome::fallback(),
    };
    let base_nodes = parse_doc_nodes(base_doc).unwrap_or_default();

    let ours_names = component_name_sequence(&ours_nodes);
    let theirs_names = component_name_sequence(&theirs_nodes);
    // Structural divergence in the component set/order → fall back.
    if ours_names != theirs_names {
        return CellMergeOutcome::fallback();
    }
    // Inline-mode doc (no components): per-cell merge has nothing to isolate.
    if ours_names.is_empty() {
        return CellMergeOutcome::fallback();
    }

    // Index base occurrences by (component, occurrence) for per-cell base resolution.
    let mut base_by_key: std::collections::HashMap<(String, usize), &ComponentOccurrence> =
        std::collections::HashMap::new();
    for n in &base_nodes {
        if let DocNode::Component { occurrence, .. } = n {
            base_by_key.insert(
                (occurrence.component.clone(), occurrence.occurrence),
                occurrence,
            );
        }
    }

    // Collect base interstitials positionally so the per-cell merge can do a
    // 3-way merge of the structural text AROUND components (not just whitespace —
    // it can carry operator scratch comments / late tail edits). Only valid when
    // base has the SAME top-level component-name sequence as ours/theirs; on any
    // structural divergence we leave it empty and fall back to ours-wins.
    let base_interstitials: Vec<&str> = if component_name_sequence(&base_nodes) == ours_names {
        base_nodes
            .iter()
            .filter_map(|n| match n {
                DocNode::Interstitial(s) => Some(s.as_str()),
                DocNode::Component { .. } => None,
            })
            .collect()
    } else {
        Vec::new()
    };

    // Walk ours/theirs in lockstep (same component-name sequence ⇒ same shape).
    let policy = ConflictPolicy::default();
    let mut out = String::new();
    let mut all_conflicts = Vec::new();
    let empty_occurrence = |o: &ComponentOccurrence| ComponentOccurrence {
        component: o.component.clone(),
        occurrence: o.occurrence,
        items: Vec::new(),
    };

    let mut theirs_iter = theirs_nodes.iter();
    let mut interstitial_index = 0usize;
    for ours_node in &ours_nodes {
        let theirs_node = theirs_iter.next().expect("node sequences aligned");
        match (ours_node, theirs_node) {
            (DocNode::Interstitial(o), DocNode::Interstitial(t)) => {
                // Interstitial framing (whitespace + operator scratch text around
                // components). 3-way merge it so a one-sided addition (e.g. a
                // scratch comment or late tail edit on the live/disk side only) is
                // preserved instead of being clobbered by the other side. Base is
                // aligned positionally; absent base ⇒ legacy ours-wins.
                let base = base_interstitials.get(interstitial_index).copied();
                let chosen = match base {
                    Some(b) if *o == b => t.as_str(),     // only theirs diverged
                    Some(b) if t == b => o.as_str(),      // only ours diverged
                    _ => o.as_str(),                      // both diverged / no base ⇒ ours-wins
                };
                out.push_str(chosen);
                interstitial_index += 1;
            }
            (
                DocNode::Component {
                    framing: o_framing,
                    occurrence: o_occ,
                },
                DocNode::Component {
                    occurrence: t_occ, ..
                },
            ) => {
                let base_occ = base_by_key
                    .get(&(o_occ.component.clone(), o_occ.occurrence))
                    .copied();
                let base_holder;
                let base_ref = match base_occ {
                    Some(b) => b,
                    None => {
                        base_holder = empty_occurrence(o_occ);
                        &base_holder
                    }
                };
                // Duplicate keys ⇒ keyed reconciliation unsound ⇒ fall back whole-doc.
                if !keys_unique(o_occ) || !keys_unique(t_occ) || !keys_unique(base_ref) {
                    return CellMergeOutcome::fallback();
                }
                let (mut merged_items, conflicts) =
                    match compose_occurrence(base_ref, o_occ, t_occ, policy) {
                        Some(r) => r,
                        None => return CellMergeOutcome::fallback(),
                    };
                // `#qcellconflict` (default ON, env kill-switch): surface each
                // recorded same-region content conflict as an operator-visible
                // in-band marker after the conflicted node's (active, ours) value.
                // Explicitly OFF ⇒ no mutation, so the body is byte-identical to
                // the legacy ours-wins output.
                if cell_merge_conflict_markers_enabled() {
                    surface_conflict_markers(&mut merged_items, &conflicts);
                }
                all_conflicts.extend(conflicts);
                // Recombine the body losslessly from the merged item values, then
                // reframe inside ours' open/close markers.
                let body: String = merged_items.iter().map(|(_, v)| v.as_str()).collect();
                out.push_str(&o_framing.open);
                out.push_str(&body);
                out.push_str(&o_framing.close);
            }
            // Shape mismatch despite equal name sequences (interstitial vs
            // component): structural — fall back.
            _ => return CellMergeOutcome::fallback(),
        }
    }

    // Structural safety net: the recombined document must re-segment to the same
    // top-level component name sequence (never emit malformed framing).
    match parse_doc_nodes(&out) {
        Some(merged_nodes) if component_name_sequence(&merged_nodes) == ours_names => {
            log_conflicts(&all_conflicts);
            CellMergeOutcome {
                merged_text: out,
                conflicts: all_conflicts,
                fell_back: false,
            }
        }
        _ => CellMergeOutcome::fallback(),
    }
}

/// Build the deterministic, framing-safe in-band conflict marker for one
/// [`CellConflict`] (`#qcellconflict`).
///
/// The marker is a single **HTML comment** (`<!-- … -->`) — git-style
/// `<<<<<<< / ======= / >>>>>>>` rails showing BOTH sides verbatim, with `ours`
/// (the active/structural text already present in the body) named first. Its
/// inner content begins with `cell-merge-conflict` (deliberately **not** an
/// `agent:` / `/agent:` prefix), so [`crate::component::parse`] treats it as an
/// ordinary comment and never as a component open/close marker. It is emitted as
/// a comment specifically so it can never:
/// - introduce a new top-level component (its inner text never starts with
///   `agent:` / `/agent:`, and the verbatim sides are guarded by
///   [`marker_is_framing_safe`]), so `component_name_sequence` is unchanged, and
/// - create a new keyed list item or `### Re:` block (a comment line is neither a
///   `- ` list line nor a `### Re:` heading), so item segmentation is unchanged.
///
/// The block always ends in `\n` and is appended *after* the conflicted node's
/// verbatim value (which itself already ends in `\n`), so the body stays
/// line-structured and round-trips. Fully deterministic: stable rail tokens,
/// fixed ours-before-theirs ordering, no clocks/randomness.
fn conflict_marker(conflict: &CellConflict) -> String {
    let policy_chose_ours = conflict.chosen == conflict.ours;
    let chosen_side = if policy_chose_ours { "ours" } else { "theirs" };
    // Trim a single trailing newline from each side so the verbatim text sits on
    // its own line(s) inside the rails without a spurious blank line; the rails
    // themselves provide the line structure.
    let ours = conflict.ours.strip_suffix('\n').unwrap_or(&conflict.ours);
    let theirs = conflict
        .theirs
        .strip_suffix('\n')
        .unwrap_or(&conflict.theirs);
    format!(
        "<!-- cell-merge-conflict component={} occurrence={} identity={} kind={:?} chosen={} (operator: resolve; ours kept active)\n\
         <<<<<<< ours\n{ours}\n=======\n{theirs}\n>>>>>>> theirs\n-->\n",
        conflict.component, conflict.occurrence, conflict.identity, conflict.kind, chosen_side,
    )
}

/// Defensive guard: a conflict marker must not contain any agent component
/// framing marker (`<!-- agent:NAME -->` / `<!-- /agent:NAME -->`), which would
/// change the document's top-level component-name sequence. Verbatim side text is
/// operator/agent content, so in principle a side could itself contain such a
/// marker; if so we decline to surface that conflict in-band (it stays
/// stderr-only) rather than risk corrupting framing. Returns true when safe.
fn marker_is_framing_safe(conflict: &CellConflict) -> bool {
    // The rails/comment scaffolding is fixed and safe; only the verbatim side
    // text could carry framing. `component::parse` keys off `<!-- agent:` /
    // `<!-- /agent:` openers, so reject either side carrying one.
    let carries_framing =
        |s: &str| s.contains("<!-- agent:") || s.contains("<!-- /agent:") || s.contains("-->");
    !(carries_framing(&conflict.ours) || carries_framing(&conflict.theirs))
}

/// Append an operator-visible in-band conflict marker after each conflicted
/// node's value in `merged_items` (`#qcellconflict`). The conflicted node keeps
/// its active value (ours under the default policy); the marker surfaces BOTH
/// versions verbatim so the operator can resolve. A conflict whose side text
/// would itself perturb framing ([`marker_is_framing_safe`]) is skipped (left
/// stderr-only). Mutates the merged item values in place; never reorders.
fn surface_conflict_markers(merged_items: &mut [(String, ItemValue)], conflicts: &[CellConflict]) {
    for conflict in conflicts {
        if !marker_is_framing_safe(conflict) {
            eprintln!(
                "cell_merge conflict identity={} not surfaced in-band (side text carries framing); stderr-only",
                conflict.identity,
            );
            continue;
        }
        // Find the conflicted node by identity and append the marker to its
        // active value. The node's value is the chosen (active) side, already in
        // the body; appending after it keeps it the structural text.
        if let Some((_, value)) = merged_items
            .iter_mut()
            .find(|(id, _)| id == &conflict.identity)
        {
            // Keep the value line-terminated, then append the marker block. If the
            // value somehow lacks a trailing newline (last item, no EOL), add one
            // so the comment starts on its own line.
            if !value.ends_with('\n') {
                value.push('\n');
            }
            value.push_str(&conflict_marker(conflict));
        }
    }
}

/// Emit one structured stderr line per surfaced conflict (honest operator-visible
/// surface, beyond a bare count). Lifecycle joins are never in this list, so each
/// line is a genuine same-level content disagreement that the policy resolved.
fn log_conflicts(conflicts: &[CellConflict]) {
    for c in conflicts {
        eprintln!(
            "cell_merge conflict kind={:?} component={} occurrence={} identity={} policy_chose_ours={}",
            c.kind,
            c.component,
            c.occurrence,
            c.identity,
            c.chosen == c.ours,
        );
    }
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

    // ----- 3-way per-cell merge (`merge_3way`) -----------------------------

    /// Serialize the env-var-mutating tests: `AGENT_DOC_CELL_MERGE` is
    /// process-global, so parallel tests that set/remove it would race. Reuses the
    /// crate-wide lock so `crdt` flag tests serialize against these too.
    use super::CELL_MERGE_ENV_LOCK as ENV_LOCK;

    const BASE3: &str = "\
---
agent_doc_format: template
---
<!-- agent:queue -->
- do [#alpha] first task
- do [#beta] second task
- do [#gamma] third task
<!-- /agent:queue -->
<!-- agent:exchange -->
### Re: earlier
prior response.
<!-- /agent:exchange -->
";

    #[test]
    fn concurrent_edits_to_different_items_both_land_no_conflict() {
        // ours edits alpha, theirs edits gamma — different items, same component.
        let ours = BASE3.replace(
            "- do [#alpha] first task\n",
            "- do [#alpha] first task OURS\n",
        );
        let theirs = BASE3.replace(
            "- do [#gamma] third task\n",
            "- do [#gamma] third task THEIRS\n",
        );
        let out = merge_3way(BASE3, &ours, &theirs);
        assert!(!out.fell_back, "should not fall back");
        assert!(out.conflicts.is_empty(), "no conflict: {:?}", out.conflicts);
        assert!(out.merged_text.contains("first task OURS"));
        assert!(out.merged_text.contains("third task THEIRS"));
        // beta untouched, no splice.
        assert!(out.merged_text.contains("- do [#beta] second task\n"));
    }

    #[test]
    fn concurrent_edits_to_different_components_both_land() {
        // ours edits the queue, theirs edits the exchange — zero cross-component.
        let ours = BASE3.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task OURS\n",
        );
        let theirs = BASE3.replace("prior response.", "prior response. THEIRS");
        let out = merge_3way(BASE3, &ours, &theirs);
        assert!(!out.fell_back);
        assert!(out.conflicts.is_empty());
        assert!(out.merged_text.contains("second task OURS"));
        assert!(out.merged_text.contains("prior response. THEIRS"));
    }

    #[test]
    fn exchange_response_does_not_splice_into_queue() {
        // The classic corruption repro: one side appends a `### Re:` response,
        // the other edits a queue item. The response must NOT land inside queue.
        let with_response = BASE3.replace(
            "### Re: earlier\nprior response.\n",
            "### Re: earlier\nprior response.\n### Re: new prompt\nbrand new agent response.\n",
        );
        let edited_queue = BASE3.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task EDITED\n",
        );
        let out = merge_3way(BASE3, &with_response, &edited_queue);
        assert!(!out.fell_back, "should per-cell merge");
        // Locate the queue component body and assert the response is not in it.
        let q_open = out.merged_text.find("<!-- agent:queue -->").unwrap();
        let q_close = out.merged_text.find("<!-- /agent:queue -->").unwrap();
        let queue_body = &out.merged_text[q_open..q_close];
        assert!(
            !queue_body.contains("### Re:"),
            "exchange response spliced into queue body: {queue_body}"
        );
        // Both edits landed in their own components.
        assert!(out.merged_text.contains("brand new agent response."));
        assert!(out.merged_text.contains("second task EDITED"));
    }

    #[test]
    fn same_item_edited_both_sides_surfaces_conflict() {
        // Both sides edit beta to DIFFERENT text → a real conflict, not a blend.
        // Serialize against marker-toggling tests + explicitly disable the
        // conflict-marker sub-gate (default-ON kill-switch) so the "theirs absent"
        // assertion is deterministic and exercises the legacy ours-wins path.
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, "0");
        }
        let ours = BASE3.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task OURS-VERSION\n",
        );
        let theirs = BASE3.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task THEIRS-VERSION\n",
        );
        let out = merge_3way(BASE3, &ours, &theirs);
        assert!(!out.fell_back);
        assert_eq!(out.conflicts.len(), 1, "one conflict: {:?}", out.conflicts);
        let c = &out.conflicts[0];
        assert_eq!(c.component, "queue");
        assert_eq!(c.identity, "queue:0:id:beta");
        assert!(c.ours.contains("OURS-VERSION"));
        assert!(c.theirs.contains("THEIRS-VERSION"));
        // Default policy is ours-wins; the chosen value lands and is NOT a blend.
        assert!(c.chosen.contains("OURS-VERSION"));
        assert!(out.merged_text.contains("OURS-VERSION"));
        assert!(
            !out.merged_text.contains("THEIRS-VERSION"),
            "no fabricated blend: theirs value must not also appear"
        );
        // SAFETY: still holding the lock — restore the default (ON).
        unsafe {
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }
    }

    #[test]
    fn reorder_one_side_edit_other_side_compose() {
        // ours reorders (gamma to top), theirs edits alpha's text.
        let ours = BASE3.replace(
            "- do [#alpha] first task\n- do [#beta] second task\n- do [#gamma] third task\n",
            "- do [#gamma] third task\n- do [#alpha] first task\n- do [#beta] second task\n",
        );
        let theirs = BASE3.replace(
            "- do [#alpha] first task\n",
            "- do [#alpha] first task ALPHA-EDIT\n",
        );
        let out = merge_3way(BASE3, &ours, &theirs);
        assert!(!out.fell_back);
        assert!(out.conflicts.is_empty(), "reorder+edit compose cleanly");
        // The edit landed…
        assert!(out.merged_text.contains("first task ALPHA-EDIT"));
        // …and the order spine is theirs (live side) for a list component:
        // theirs kept base order alpha,beta,gamma, so the merged queue follows it.
        let q_open = out.merged_text.find("<!-- agent:queue -->").unwrap();
        let q_close = out.merged_text.find("<!-- /agent:queue -->").unwrap();
        let body = &out.merged_text[q_open..q_close];
        let pa = body.find("ALPHA-EDIT").unwrap();
        let pg = body.find("[#gamma]").unwrap();
        assert!(
            pa < pg,
            "theirs (live) order is the spine: alpha before gamma"
        );
    }

    #[test]
    fn structural_divergence_falls_back() {
        // theirs drops the exchange component entirely → unequal component set.
        let theirs = "\
---
agent_doc_format: template
---
<!-- agent:queue -->
- do [#alpha] first task
- do [#beta] second task
- do [#gamma] third task
<!-- /agent:queue -->
";
        let ours = BASE3.replace("- do [#alpha] first task\n", "- do [#alpha] first task X\n");
        let out = merge_3way(BASE3, &ours, theirs);
        assert!(out.fell_back, "structural divergence must fall back");
        assert!(out.merged_text.is_empty());

        let _guard = ENV_LOCK.lock().unwrap();
        // And with the flag ON, the legacy crdt::merge path still returns a valid
        // doc for that input (the seam falls through transparently).
        // SAFETY: single-threaded test, restored immediately.
        unsafe {
            std::env::set_var(CELL_MERGE_ENV, "1");
        }
        let base_state = crate::crdt::CrdtDoc::from_text(BASE3).encode_state();
        let legacy = crate::crdt::merge_by_component(Some(&base_state), &ours, theirs).unwrap();
        unsafe {
            std::env::remove_var(CELL_MERGE_ENV);
        }
        assert!(legacy.contains("<!-- agent:queue -->"));
        assert!(legacy.contains("first task X"));
    }

    #[test]
    fn flag_off_byte_identical_to_legacy() {
        // For representative inputs the kill-switched crdt::merge_by_component
        // output is deterministic and exercises the legacy whole-doc path. Per-cell
        // merge is default-ON, so the legacy path is reached via the explicit
        // kill-switch.
        let ours = BASE3.replace(
            "- do [#alpha] first task\n",
            "- do [#alpha] first task OURS\n",
        );
        let theirs = BASE3.replace(
            "- do [#gamma] third task\n",
            "- do [#gamma] third task THEIRS\n",
        );
        let base_state = crate::crdt::CrdtDoc::from_text(BASE3).encode_state();

        let _guard = ENV_LOCK.lock().unwrap();
        // Flag explicitly OFF via the kill-switch.
        unsafe {
            std::env::set_var(CELL_MERGE_ENV, "0");
        }
        let off = crate::crdt::merge_by_component(Some(&base_state), &ours, &theirs).unwrap();

        // Re-run, still OFF — deterministic, identical.
        let off2 = crate::crdt::merge_by_component(Some(&base_state), &ours, &theirs).unwrap();
        assert_eq!(off, off2, "legacy path is deterministic with the flag off");
        assert!(!cell_merge_enabled(), "kill-switch must disable");
        // The legacy path produced a valid merged doc.
        assert!(off.contains("first task OURS"));
        assert!(off.contains("third task THEIRS"));
        // SAFETY: still holding the lock — restore the default (ON).
        unsafe {
            std::env::remove_var(CELL_MERGE_ENV);
        }
    }

    #[test]
    fn cell_merge_enabled_default_on_with_kill_switch() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Default-ON: absent var ⇒ enabled.
        unsafe {
            std::env::remove_var(CELL_MERGE_ENV);
        }
        assert!(cell_merge_enabled(), "absent ⇒ default ON");
        // Empty / truthy / any unrecognized value ⇒ still ON.
        for v in ["1", "true", "on", "yes", "TRUE", " On ", "", "maybe"] {
            unsafe {
                std::env::set_var(CELL_MERGE_ENV, v);
            }
            assert!(cell_merge_enabled(), "{v:?} should stay ON");
        }
        // Only an explicit falsy kill-switch turns it off.
        for v in ["0", "false", "off", "no", "FALSE", " Off "] {
            unsafe {
                std::env::set_var(CELL_MERGE_ENV, v);
            }
            assert!(!cell_merge_enabled(), "{v:?} should kill (OFF)");
        }
        unsafe {
            std::env::remove_var(CELL_MERGE_ENV);
        }
    }

    #[test]
    fn cell_merge_opcapture_default_on_and_master_kill_switch_gates_it() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Default-ON: both vars absent ⇒ opcapture enabled.
        unsafe {
            std::env::remove_var(CELL_MERGE_ENV);
            std::env::remove_var(CELL_MERGE_OPCAPTURE_ENV);
        }
        assert!(cell_merge_opcapture_enabled(), "absent ⇒ default ON");
        // Explicit opcapture kill-switch turns the sub-feature off.
        for v in ["0", "false", "off", "no"] {
            unsafe {
                std::env::set_var(CELL_MERGE_OPCAPTURE_ENV, v);
            }
            assert!(!cell_merge_opcapture_enabled(), "{v:?} should kill");
        }
        // Master kill-switch forces the sub-feature OFF regardless of its value.
        unsafe {
            std::env::set_var(CELL_MERGE_OPCAPTURE_ENV, "1");
            std::env::set_var(CELL_MERGE_ENV, "0");
        }
        assert!(
            !cell_merge_opcapture_enabled(),
            "master kill-switch must force the sub-feature OFF"
        );
        unsafe {
            std::env::remove_var(CELL_MERGE_ENV);
            std::env::remove_var(CELL_MERGE_OPCAPTURE_ENV);
        }
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

    // ----- anti-resurrection lifecycle join (`#queuestatemachine2`/`#qheadresidue`)

    /// Extract a component body from a merged doc.
    fn component_body<'a>(merged: &'a str, name: &str) -> &'a str {
        let open = format!("<!-- agent:{name} -->");
        let close = format!("<!-- /agent:{name} -->");
        merged
            .split(&open)
            .nth(1)
            .and_then(|s| s.split(&close).next())
            .unwrap_or("")
    }

    fn live_count(body: &str, needle: &str) -> usize {
        body.lines()
            .filter(|l| l.contains(needle) && !l.contains("~~"))
            .count()
    }

    /// Mirror of `crdt::merge_by_component_struck_id_head_not_resurrected_by_stale_unstrike`.
    /// BASE struck, OURS (agent) un-strikes, THEIRS (disk) stays struck → merged
    /// stays struck. The lifecycle join (`Live < Struck`) overrides ours-wins.
    #[test]
    fn struck_id_head_not_resurrected_by_stale_unstrike_cell_path() {
        let base = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- ~~do [#abcd] fix the thing~~\n<!-- /agent:queue -->\n";
        // OURS un-strikes (stale) AND appends an exchange response.
        let ours = "<!-- agent:exchange -->\n### Re: topic\nAgent response body.\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- do [#abcd] fix the thing\n<!-- /agent:queue -->\n";
        // THEIRS correctly struck.
        let theirs = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- ~~do [#abcd] fix the thing~~\n<!-- /agent:queue -->\n";

        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back, "should per-cell merge");
        let body = component_body(&out.merged_text, "queue");
        assert!(
            body.contains("~~do [#abcd] fix the thing~~"),
            "struck head must stay struck:\n{body}"
        );
        assert_eq!(
            live_count(body, "do [#abcd]"),
            0,
            "stale un-strike resurrected the struck head LIVE:\n{body}"
        );
        // A strike-vs-unstrike is a lawful lifecycle join, NOT a conflict.
        assert!(
            out.conflicts.is_empty(),
            "lifecycle join must not be a conflict: {:?}",
            out.conflicts
        );
        // The agent's exchange response still landed (no cross-cell loss).
        assert!(out.merged_text.contains("Agent response body."));
    }

    /// Free-text variant: the struck head is an operator free-text line (no `#id`).
    #[test]
    fn struck_free_text_head_not_resurrected_by_stale_unstrike_cell_path() {
        let base = "<!-- agent:queue -->\n- ~~Still getting JB File Cache Conflict dialogs.~~\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- Still getting JB File Cache Conflict dialogs.\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- ~~Still getting JB File Cache Conflict dialogs.~~\n<!-- /agent:queue -->\n";

        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back);
        let body = component_body(&out.merged_text, "queue");
        assert!(
            body.contains("~~Still getting JB File Cache Conflict dialogs.~~"),
            "struck free-text head must stay struck:\n{body}"
        );
        assert_eq!(
            live_count(body, "Still getting JB File Cache Conflict dialogs."),
            0,
            "stale un-strike resurrected the struck free-text head LIVE:\n{body}"
        );
        assert!(out.conflicts.is_empty());
    }

    /// Multi-cycle no-churn proof: strike → persist struck as next base → re-merge
    /// with a stale un-strike side → stays struck across every cycle.
    #[test]
    fn struck_head_stable_across_cycles_no_churn_cell_path() {
        let mut base =
            "<!-- agent:queue -->\n- ~~do [#abcd] answered~~\n<!-- /agent:queue -->\n".to_string();
        for cycle in 0..3 {
            let ours = "<!-- agent:queue -->\n- do [#abcd] answered\n<!-- /agent:queue -->\n";
            let theirs = "<!-- agent:queue -->\n- ~~do [#abcd] answered~~\n<!-- /agent:queue -->\n";
            let out = merge_3way(&base, ours, theirs);
            assert!(!out.fell_back, "cycle {cycle}: fell back");
            let body = component_body(&out.merged_text, "queue");
            assert_eq!(
                live_count(body, "do [#abcd]"),
                0,
                "cycle {cycle}: head resurrected LIVE (churn):\n{body}"
            );
            assert!(
                body.contains("~~do [#abcd] answered~~"),
                "cycle {cycle}: struck head lost:\n{body}"
            );
            assert_eq!(
                body.matches("do [#abcd] answered").count(),
                1,
                "cycle {cycle}: head duplicated:\n{body}"
            );
            assert!(out.conflicts.is_empty(), "cycle {cycle}: spurious conflict");
            // Persist the lawful (struck) state as the next cycle's base.
            base = out.merged_text;
        }
    }

    /// Backlog component gets the same anti-resurrection treatment as queue.
    #[test]
    fn struck_backlog_head_not_resurrected_cell_path() {
        let base =
            "<!-- agent:backlog -->\n- ~~[#bk1] backlog answered~~\n<!-- /agent:backlog -->\n";
        let ours = "<!-- agent:backlog -->\n- [#bk1] backlog answered\n<!-- /agent:backlog -->\n";
        let theirs =
            "<!-- agent:backlog -->\n- ~~[#bk1] backlog answered~~\n<!-- /agent:backlog -->\n";

        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back);
        let body = component_body(&out.merged_text, "backlog");
        assert!(
            body.contains("~~[#bk1] backlog answered~~"),
            "struck backlog head must stay struck:\n{body}"
        );
        assert_eq!(
            live_count(body, "[#bk1]"),
            0,
            "backlog head resurrected:\n{body}"
        );
        assert!(out.conflicts.is_empty());
    }

    /// Review component gets the same anti-resurrection treatment.
    #[test]
    fn struck_review_head_not_resurrected_cell_path() {
        let base = "<!-- agent:review -->\n- ~~[#rv1] review item~~\n<!-- /agent:review -->\n";
        let ours = "<!-- agent:review -->\n- [#rv1] review item\n<!-- /agent:review -->\n";
        let theirs = "<!-- agent:review -->\n- ~~[#rv1] review item~~\n<!-- /agent:review -->\n";

        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back);
        let body = component_body(&out.merged_text, "review");
        assert!(body.contains("~~[#rv1] review item~~"));
        assert_eq!(live_count(body, "[#rv1]"), 0);
        assert!(out.conflicts.is_empty());
    }

    /// The symmetric case: OURS struck, THEIRS un-strikes (stale) → still struck.
    /// Anti-resurrection must hold regardless of which side carries the strike.
    #[test]
    fn struck_head_holds_when_theirs_is_the_stale_unstrike() {
        let base = "<!-- agent:queue -->\n- ~~do [#zz] q~~\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- ~~do [#zz] q~~\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#zz] q\n<!-- /agent:queue -->\n";

        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back);
        let body = component_body(&out.merged_text, "queue");
        assert!(
            body.contains("~~do [#zz] q~~"),
            "struck side must win:\n{body}"
        );
        assert_eq!(live_count(body, "do [#zz]"), 0);
        assert!(out.conflicts.is_empty());
    }

    /// Strike-vs-unstrike is NOT recorded as a conflict (explicit honesty check).
    #[test]
    fn strike_vs_unstrike_is_not_a_conflict() {
        let base = "<!-- agent:queue -->\n- ~~do [#abcd] x~~\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#abcd] x\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- ~~do [#abcd] x~~\n<!-- /agent:queue -->\n";
        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back);
        assert!(
            out.conflicts.is_empty(),
            "a lawful lifecycle join must never be a conflict: {:?}",
            out.conflicts
        );
    }

    /// A genuine same-level content conflict (both Live, different text) IS
    /// recorded as exactly one `Content` conflict, ours-wins, theirs not present.
    #[test]
    fn genuine_same_level_content_conflict_is_recorded() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Conflict-marker surfacing is default-ON; explicitly disable it via the
        // kill-switch so the legacy "theirs absent in-band" assertion is exercised.
        unsafe {
            std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, "0");
        }
        let base = "<!-- agent:queue -->\n- do [#beta] orig\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] OURS-VERSION\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#beta] THEIRS-VERSION\n<!-- /agent:queue -->\n";

        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back);
        assert_eq!(out.conflicts.len(), 1, "one conflict: {:?}", out.conflicts);
        let c = &out.conflicts[0];
        assert_eq!(c.component, "queue");
        assert_eq!(c.identity, "queue:0:id:beta");
        assert_eq!(c.kind, ConflictKind::Content);
        assert!(c.ours.contains("OURS-VERSION"));
        assert!(c.theirs.contains("THEIRS-VERSION"));
        assert!(c.chosen.contains("OURS-VERSION"), "default ours-wins");
        assert!(out.merged_text.contains("OURS-VERSION"));
        assert!(
            !out.merged_text.contains("THEIRS-VERSION"),
            "no fabricated blend: theirs value must not also appear:\n{}",
            out.merged_text
        );
        // SAFETY: still holding the lock — restore the default (ON).
        unsafe {
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }
    }

    /// Both sides struck the SAME head to DIFFERENT text (e.g. an operator note
    /// appended on one struck copy) → both at `Struck` level → a genuine content
    /// conflict, NOT silently dropped.
    #[test]
    fn both_sides_struck_different_text_is_a_content_conflict() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Conflict-marker surfacing is default-ON; explicitly disable it via the
        // kill-switch so the legacy "theirs absent in-band" assertion is exercised.
        unsafe {
            std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, "0");
        }
        let base = "<!-- agent:queue -->\n- do [#k] live\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- ~~do [#k] struck OURS~~\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- ~~do [#k] struck THEIRS~~\n<!-- /agent:queue -->\n";

        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back);
        assert_eq!(
            out.conflicts.len(),
            1,
            "both-struck-differ is a conflict: {:?}",
            out.conflicts
        );
        assert_eq!(out.conflicts[0].kind, ConflictKind::Content);
        assert!(out.merged_text.contains("struck OURS"));
        assert!(!out.merged_text.contains("struck THEIRS"));
        // SAFETY: still holding the lock — restore the default (ON).
        unsafe {
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }
    }

    // ----- op-level TextCrdt 3-way merge (`#qcellmerge1` opcapture) ----------

    /// Unit-level proof of the disjoint-region detector + op-level merge,
    /// independent of env gating. Two edits to non-overlapping spans of one base
    /// string converge with BOTH preserved.
    #[test]
    fn op_level_merge_disjoint_edits_converge_both_present() {
        let base = "do [#beta] the original task body";
        // ours edits the FRONT ("the original" → "the EDITED original"); theirs
        // edits the BACK ("task body" → "task BODY!!"). Disjoint regions.
        let ours = "do [#beta] the EDITED original task body";
        let theirs = "do [#beta] the original task BODY!!";
        let merged = op_level_merge(base, ours, theirs).expect("disjoint edits merge");
        assert!(merged.contains("EDITED"), "ours edit present: {merged}");
        assert!(merged.contains("BODY!!"), "theirs edit present: {merged}");
    }

    /// A true SAME-region overlap returns `None` from the op-level merger, so the
    /// caller stays on the conflict + policy path (the `#qnodemerge5` remainder).
    #[test]
    fn op_level_merge_same_region_overlap_is_none() {
        let base = "do [#beta] original";
        // Both sides replace the SAME trailing word with different text.
        let ours = "do [#beta] OURS";
        let theirs = "do [#beta] THEIRS";
        assert!(
            op_level_merge(base, ours, theirs).is_none(),
            "overlapping same-region edits must NOT op-merge"
        );
    }

    /// Span-edit / overlap unit checks: prefix/suffix peel, pure insert, overlap.
    #[test]
    fn char_span_edit_and_overlap_detection() {
        let base: Vec<char> = "abcdef".chars().collect();
        // Replace the middle "cd" with "XY".
        let e = char_span_edit(&base, &"abXYef".chars().collect::<Vec<_>>()).unwrap();
        assert_eq!((e.start, e.end), (2, 4));
        assert_eq!(e.replacement, vec!['X', 'Y']);
        // Identical → no edit.
        assert!(char_span_edit(&base, &base).is_none());
        // Disjoint spans do not overlap.
        let front = char_span_edit(&base, &"aZcdef".chars().collect::<Vec<_>>()).unwrap();
        let back = char_span_edit(&base, &"abcdeZ".chars().collect::<Vec<_>>()).unwrap();
        assert!(!ranges_overlap(&front, &back), "front vs back disjoint");
        // Same-region edits overlap.
        let o = char_span_edit(&base, &"abXXef".chars().collect::<Vec<_>>()).unwrap();
        let t = char_span_edit(&base, &"abYYef".chars().collect::<Vec<_>>()).unwrap();
        assert!(ranges_overlap(&o, &t), "same middle region overlaps");
    }

    /// (a) End-to-end with the opcapture gate ON: two DISJOINT-region edits to the
    /// SAME exchange item converge with BOTH present and NO conflict.
    #[test]
    fn opcapture_on_disjoint_same_cell_edits_both_land_no_conflict() {
        let _guard = ENV_LOCK.lock().unwrap();
        // A stable `#id`-keyed queue item: the identity survives a text edit on
        // EITHER side, so both edits land on the SAME item (the `(both changed)`
        // branch). ours edits the FRONT, theirs the BACK — disjoint regions.
        let base = "<!-- agent:queue -->\n- do [#beta] the original answer body here\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] the EDITED answer body here\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#beta] the original answer body THERE!\n<!-- /agent:queue -->\n";

        // SAFETY: single-threaded under ENV_LOCK; restored before unlock.
        unsafe {
            std::env::set_var(CELL_MERGE_OPCAPTURE_ENV, "1");
        }
        let out = merge_3way(base, ours, theirs);
        unsafe {
            std::env::remove_var(CELL_MERGE_OPCAPTURE_ENV);
        }

        assert!(!out.fell_back, "should not fall back: {out:?}");
        assert!(
            out.conflicts.is_empty(),
            "disjoint op-merge records NO conflict: {:?}",
            out.conflicts
        );
        assert!(
            out.merged_text.contains("EDITED"),
            "ours edit: {}",
            out.merged_text
        );
        assert!(
            out.merged_text.contains("THERE!"),
            "theirs edit: {}",
            out.merged_text
        );
    }

    /// (b) End-to-end with the gate ON but a TRUE same-region overlap: still a
    /// recorded `Content` conflict, ours-wins, theirs absent.
    #[test]
    fn opcapture_on_same_region_overlap_still_conflicts() {
        let _guard = ENV_LOCK.lock().unwrap();
        let base = "<!-- agent:queue -->\n- do [#beta] original\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] OURS-ONLY\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#beta] THEIRS-ONLY\n<!-- /agent:queue -->\n";

        // SAFETY: single-threaded under ENV_LOCK; restored before unlock. The
        // conflict-marker sub-gate is explicitly killed (it is default-ON) so
        // "theirs absent in-band" is deterministic.
        unsafe {
            std::env::set_var(CELL_MERGE_OPCAPTURE_ENV, "1");
            std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, "0");
        }
        let out = merge_3way(base, ours, theirs);
        unsafe {
            std::env::remove_var(CELL_MERGE_OPCAPTURE_ENV);
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }

        assert!(!out.fell_back);
        assert_eq!(
            out.conflicts.len(),
            1,
            "overlap still conflicts even with opcapture ON: {:?}",
            out.conflicts
        );
        assert_eq!(out.conflicts[0].kind, ConflictKind::Content);
        assert!(out.merged_text.contains("OURS-ONLY"), "ours-wins policy");
        assert!(
            !out.merged_text.contains("THEIRS-ONLY"),
            "no blend on a real overlap: {}",
            out.merged_text
        );
    }

    /// (c) Gate explicitly OFF reproduces the legacy ours-wins behavior unchanged:
    /// even DISJOINT same-cell edits record a conflict and drop theirs.
    #[test]
    fn opcapture_off_legacy_ours_wins_unchanged() {
        let _guard = ENV_LOCK.lock().unwrap();
        let base = "<!-- agent:queue -->\n- do [#beta] the original answer body here\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] the EDITED answer body here\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#beta] the original answer body THERE!\n<!-- /agent:queue -->\n";

        // SAFETY: opcapture is default-ON, so explicitly engage its kill-switch to
        // exercise the legacy policy-only path. Also disable conflict-marker
        // surfacing (also default-ON) so the "theirs absent in-band" assertion is
        // deterministic.
        unsafe {
            std::env::set_var(CELL_MERGE_OPCAPTURE_ENV, "0");
            std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, "0");
        }
        assert!(!cell_merge_opcapture_enabled(), "opcapture kill-switch off");

        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back);
        assert_eq!(
            out.conflicts.len(),
            1,
            "OFF: disjoint same-cell edits are a conflict (legacy behavior): {:?}",
            out.conflicts
        );
        assert!(out.merged_text.contains("EDITED"), "ours-wins keeps ours");
        assert!(
            !out.merged_text.contains("THERE!"),
            "OFF: theirs disjoint edit is dropped (no op-merge): {}",
            out.merged_text
        );
        // SAFETY: still holding the lock — restore the defaults (ON).
        unsafe {
            std::env::remove_var(CELL_MERGE_OPCAPTURE_ENV);
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }
    }

    // ----- conflict surfacing (`#qcellconflict`) ----------------------------

    /// Run `merge_3way` with the `AGENT_DOC_CELL_MERGE_CONFLICT_MARKERS` sub-gate
    /// set to `value` for the duration, restoring it under the env lock.
    fn merge_with_markers(
        value: Option<&str>,
        base: &str,
        ours: &str,
        theirs: &str,
    ) -> CellMergeOutcome {
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: single-threaded under ENV_LOCK; restored before unlock.
        unsafe {
            match value {
                Some(v) => std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, v),
                None => std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV),
            }
        }
        let out = merge_3way(base, ours, theirs);
        unsafe {
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }
        out
    }

    /// (a) Gate ON: a true same-region same-cell overlap surfaces an
    /// operator-visible conflict containing BOTH `ours` and `theirs` verbatim, and
    /// the merged doc still round-trips (does NOT set `fell_back`).
    #[test]
    fn conflict_markers_on_surfaces_both_versions_and_round_trips() {
        let base = "<!-- agent:queue -->\n- do [#beta] original\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] OURS-VERSION\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#beta] THEIRS-VERSION\n<!-- /agent:queue -->\n";

        let out = merge_with_markers(Some("1"), base, ours, theirs);
        assert!(!out.fell_back, "marker must not break round-trip: {out:?}");
        assert_eq!(out.conflicts.len(), 1, "one conflict: {:?}", out.conflicts);

        // BOTH versions appear verbatim in the operator-visible surface.
        assert!(
            out.merged_text.contains("OURS-VERSION"),
            "ours verbatim: {}",
            out.merged_text
        );
        assert!(
            out.merged_text.contains("THEIRS-VERSION"),
            "theirs surfaced for operator resolution: {}",
            out.merged_text
        );
        // The git-style conflict rails are present.
        assert!(
            out.merged_text.contains("<<<<<<< ours"),
            "{}",
            out.merged_text
        );
        assert!(out.merged_text.contains("======="), "{}", out.merged_text);
        assert!(
            out.merged_text.contains(">>>>>>> theirs"),
            "{}",
            out.merged_text
        );
        // The active/structural list line is still ours (no fabricated blend):
        // the literal `- do [#beta] OURS-VERSION` line is intact.
        assert!(
            out.merged_text.contains("- do [#beta] OURS-VERSION\n"),
            "ours stays the active list line: {}",
            out.merged_text
        );
        // The marker is an HTML comment (framing-safe), so re-parsing yields the
        // SAME single top-level queue component.
        let parsed = component::parse(&out.merged_text).expect("merged doc parses");
        let top: Vec<&str> = parsed.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(top, vec!["queue"], "component sequence unchanged: {top:?}");
    }

    /// (b) Gate ON: disjoint-region edits (opcapture clean) and single-side edits
    /// produce NO conflict marker — only a genuine same-region overlap does.
    #[test]
    fn conflict_markers_on_no_marker_for_nonconflicting_edits() {
        // Single-side edit: ours edits beta, theirs untouched.
        let base = "<!-- agent:queue -->\n- do [#a] one\n- do [#b] two\n- do [#c] three\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#a] one\n- do [#b] two EDITED\n- do [#c] three\n<!-- /agent:queue -->\n";
        let out = merge_with_markers(Some("1"), base, ours, base);
        assert!(!out.fell_back);
        assert!(out.conflicts.is_empty(), "single-side edit is no conflict");
        assert!(
            !out.merged_text.contains("<<<<<<<"),
            "no conflict marker for a single-side edit: {}",
            out.merged_text
        );

        // Disjoint edits to DIFFERENT items: ours edits a, theirs edits c.
        let theirs = "<!-- agent:queue -->\n- do [#a] one\n- do [#b] two\n- do [#c] three EDITED-T\n<!-- /agent:queue -->\n";
        let ours2 = "<!-- agent:queue -->\n- do [#a] one EDITED-O\n- do [#b] two\n- do [#c] three\n<!-- /agent:queue -->\n";
        let out2 = merge_with_markers(Some("1"), base, &ours2, theirs);
        assert!(!out2.fell_back);
        assert!(
            out2.conflicts.is_empty(),
            "different-item edits never conflict"
        );
        assert!(
            !out2.merged_text.contains("<<<<<<<"),
            "no marker for disjoint different-item edits: {}",
            out2.merged_text
        );
        assert!(out2.merged_text.contains("one EDITED-O"));
        assert!(out2.merged_text.contains("three EDITED-T"));

        // Disjoint same-cell edits with opcapture ON: clean op-merge, no conflict,
        // so the conflict-marker gate has nothing to surface.
        {
            let _g = ENV_LOCK.lock().unwrap();
            let dbase = "<!-- agent:queue -->\n- do [#beta] the original answer body here\n<!-- /agent:queue -->\n";
            let dours = "<!-- agent:queue -->\n- do [#beta] the EDITED answer body here\n<!-- /agent:queue -->\n";
            let dtheirs = "<!-- agent:queue -->\n- do [#beta] the original answer body THERE!\n<!-- /agent:queue -->\n";
            // SAFETY: single-threaded under ENV_LOCK; both restored before unlock.
            unsafe {
                std::env::set_var(CELL_MERGE_OPCAPTURE_ENV, "1");
                std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, "1");
            }
            let out3 = merge_3way(dbase, dours, dtheirs);
            unsafe {
                std::env::remove_var(CELL_MERGE_OPCAPTURE_ENV);
                std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
            }
            assert!(!out3.fell_back);
            assert!(
                out3.conflicts.is_empty(),
                "opcapture-clean disjoint edits are no conflict: {:?}",
                out3.conflicts
            );
            assert!(
                !out3.merged_text.contains("<<<<<<<"),
                "no marker when opcapture cleanly merges: {}",
                out3.merged_text
            );
        }
    }

    /// (c) Gate ON: a strike-vs-unstrike lifecycle join is a lawful join, NOT a
    /// conflict, so it produces NO conflict marker.
    #[test]
    fn conflict_markers_on_no_marker_for_lifecycle_join() {
        let base = "<!-- agent:queue -->\n- ~~do [#abcd] x~~\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#abcd] x\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- ~~do [#abcd] x~~\n<!-- /agent:queue -->\n";

        let out = merge_with_markers(Some("1"), base, ours, theirs);
        assert!(!out.fell_back);
        assert!(
            out.conflicts.is_empty(),
            "lifecycle join is never a conflict: {:?}",
            out.conflicts
        );
        assert!(
            !out.merged_text.contains("<<<<<<<"),
            "no conflict marker for a lawful lifecycle join: {}",
            out.merged_text
        );
        // The struck side still wins (anti-resurrection), unchanged by this rung.
        let body = component_body(&out.merged_text, "queue");
        assert!(body.contains("~~do [#abcd] x~~"), "{body}");
    }

    /// (d) Gate explicitly OFF (kill-switch): byte-identical to the legacy
    /// behavior — ours-wins, conflict only on stderr, NO in-band marker. The
    /// default (absent) is now ON and DOES surface the marker, proving the gate
    /// flipped to default-ON-with-kill-switch.
    #[test]
    fn conflict_markers_off_byte_identical_to_legacy() {
        let base = "<!-- agent:queue -->\n- do [#beta] original\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] OURS-VERSION\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#beta] THEIRS-VERSION\n<!-- /agent:queue -->\n";

        // Explicit kill-switch ("0") ⇒ legacy ours-wins, no in-band marker.
        let off = merge_with_markers(Some("0"), base, ours, theirs);
        assert!(off.merged_text.contains("OURS-VERSION"));
        assert!(
            !off.merged_text.contains("THEIRS-VERSION"),
            "OFF: theirs not surfaced in-band: {}",
            off.merged_text
        );
        assert!(
            !off.merged_text.contains("<<<<<<<"),
            "OFF: no conflict marker: {}",
            off.merged_text
        );
        // The conflict is still RECORDED (stderr surface) regardless of gate.
        assert_eq!(off.conflicts.len(), 1, "conflict still recorded OFF");

        // Default (absent) is now ON: the same overlap DOES surface the marker.
        let absent = merge_with_markers(None, base, ours, theirs);
        assert!(
            absent.merged_text.contains("THEIRS-VERSION") && absent.merged_text.contains("<<<<<<<"),
            "default-ON must surface both versions in-band: {}",
            absent.merged_text
        );
        // The OFF path must NOT be byte-identical to the new default — the
        // kill-switch genuinely changes behavior.
        assert_ne!(
            off.merged_text, absent.merged_text,
            "kill-switch must diverge from the default-ON marker output"
        );
    }

    /// Sub-gate default-ON + kill-switch + master-gate parsing, mirroring the
    /// sibling gate tests.
    #[test]
    fn conflict_markers_gate_default_on_with_kill_switch() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Master stays default-ON (absent) throughout the sub-gate checks.
        unsafe {
            std::env::remove_var(CELL_MERGE_ENV);
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }
        assert!(cell_merge_conflict_markers_enabled(), "absent ⇒ default ON");
        // Empty / truthy / unrecognized ⇒ still ON.
        for v in ["1", "true", "on", "yes", "TRUE", " On ", "", "maybe"] {
            unsafe {
                std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, v);
            }
            assert!(
                cell_merge_conflict_markers_enabled(),
                "{v:?} should stay ON"
            );
        }
        // Explicit falsy kill-switch turns it off.
        for v in ["0", "false", "off", "no", "FALSE", " Off "] {
            unsafe {
                std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, v);
            }
            assert!(
                !cell_merge_conflict_markers_enabled(),
                "{v:?} should kill (OFF)"
            );
        }
        // Master kill-switch forces the sub-feature OFF regardless of its value.
        unsafe {
            std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, "1");
            std::env::set_var(CELL_MERGE_ENV, "0");
        }
        assert!(
            !cell_merge_conflict_markers_enabled(),
            "master kill-switch must force the sub-feature OFF"
        );
        unsafe {
            std::env::remove_var(CELL_MERGE_ENV);
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }
    }

    /// A conflict whose side text would itself perturb framing is NOT surfaced
    /// in-band (stays stderr-only), and the doc still round-trips.
    #[test]
    fn conflict_markers_on_skips_framing_unsafe_side_text() {
        // theirs' value carries a `-->` that would prematurely close the marker
        // comment, so the marker is declined for safety.
        let base = "<!-- agent:queue -->\n- do [#beta] original\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] OURS-VERSION\n<!-- /agent:queue -->\n";
        let theirs =
            "<!-- agent:queue -->\n- do [#beta] THEIRS with --> inside\n<!-- /agent:queue -->\n";

        let out = merge_with_markers(Some("1"), base, ours, theirs);
        assert!(!out.fell_back, "must still round-trip: {out:?}");
        assert_eq!(out.conflicts.len(), 1, "conflict still recorded");
        assert!(
            !out.merged_text.contains("<<<<<<<"),
            "framing-unsafe side must not be surfaced in-band: {}",
            out.merged_text
        );
        // Ours-wins still holds.
        assert!(out.merged_text.contains("OURS-VERSION"));
    }
}

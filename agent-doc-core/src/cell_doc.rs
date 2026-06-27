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

use lazily::{
    CellMap, CellTree, Context, DiffOp, SemTree, TextCrdt, apply_to_map, apply_to_tree, reconcile,
};

use crate::component::{self, Component};
use crate::crdt::{
    EditorOp, PREAMBLE_KEY, is_list_component, replay_editor_ops, split_exchange_children,
    split_list_children,
};
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

fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "on" | "yes")
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
/// in-band conflict markers (`#qcellconflict`). **Default OFF** as of
/// #provauth1/#provauth4: same-cell conflicts now resolve by component
/// authorship (operator-owned cells keep the operator's edit, agent-owned cells
/// keep the binary's — see [`component_conflict_policy`]), so an in-band
/// `<<<<<<<` marker would re-surface an operator action as a suspicious conflict,
/// which is exactly what the provenance principle forbids. The marker machinery
/// is retained behind an explicit opt-in (`AGENT_DOC_CELL_MERGE_CONFLICT_MARKERS`
/// truthy) for debugging/inspection only. As a sub-feature of the per-cell merge
/// stack it is still forced OFF whenever [`cell_merge_enabled`] is disabled.
pub fn cell_merge_conflict_markers_enabled() -> bool {
    cell_merge_enabled() && env_truthy(CELL_MERGE_CONFLICT_MARKERS_ENV)
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

/// True when an occurrence projected to the reserved single whole-body item
/// (unsplittable / non-item component — `split_keyed_children` returned `None`)
/// rather than real keyed children. The keyed compose is only sound when ours
/// and theirs agree on splittability; a body-only-vs-keyed mismatch mirrors the
/// legacy `reconcile_component_body` `split(...)?` guard (either side
/// unsplittable ⇒ whole-component leaf merge).
fn is_body_only(occ: &ComponentOccurrence) -> bool {
    occ.items.len() == 1
        && occ.items[0].0 == make_node_key(&occ.component, occ.occurrence, "body", 0)
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

// ---------------------------------------------------------------------------
// Op-capture rung (`#qcellmerge1` op routing): span-aware projection.
//
// `project_document` exposes `(NodeKey, ItemValue)` without byte spans, so a
// captured absolute-offset [`EditorOp`] cannot be located in a cell. This rung
// adds a parallel *span-aware* projection ([`project_document_spans`]) that, for
// each keyed item, records its **byte span** `(start, end)` in the FULL source
// document. A component body sits at `&doc[comp.open_end..comp.close_start]`
// (see `component::Component::content`), and the splitters are lossless
// (`children.map(text).collect() == body`), so each item's full-doc span is
// `comp.open_end + accumulated_child_len .. + item_len`. An absolute op offset
// then resolves to the unique containing item — or to none (structural framing /
// boundary-crossing), which forces a conservative fallback.
// ---------------------------------------------------------------------------

/// A half-open byte span `[start, end)` into the full source document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    /// Inclusive start byte offset in the full document.
    pub start: usize,
    /// Exclusive end byte offset in the full document.
    pub end: usize,
}

impl ByteSpan {
    /// True when `offset` lies within `[start, end)`.
    fn contains_offset(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }

    /// True when the whole half-open range `[lo, hi)` lies within this span
    /// (`lo == hi` permitted at any in-span boundary — an empty edit point).
    fn contains_range(&self, lo: usize, hi: usize) -> bool {
        lo >= self.start && hi <= self.end && lo <= hi
    }
}

/// One projected item plus its full-document byte span. `node_key` /  `value`
/// mirror [`ComponentOccurrence::items`]; `span` is the lossless source range of
/// `value` in the full document (so concatenating an occurrence's spans in order
/// reproduces its body, and the spans tile the body with no gaps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSpan {
    /// Component name (`queue`, `exchange`, …).
    pub component: String,
    /// 0-based occurrence index of that component name.
    pub occurrence: usize,
    /// Full `component:occurrence:item-id:index` node key.
    pub node_key: NodeKey,
    /// Reorder-stable identity (`component:occurrence:item-id`).
    pub identity: String,
    /// The item's exact source text.
    pub value: ItemValue,
    /// The item's byte span in the FULL document.
    pub span: ByteSpan,
}

/// Project a document into span-aware items, one entry per keyed item across all
/// component occurrences, in document order. Back-compatible companion to
/// [`project_document`] (which the 26 existing callers/tests keep using).
///
/// Only the **item interiors** are projected: the component open/close markers,
/// frontmatter, and any inter-component interstitial text are deliberately NOT
/// covered, so an op landing in that structural framing resolves to no item and
/// forces the conservative fallback in [`route_ops_to_cells`].
///
/// Returns `None` on a parse error (caller falls back).
pub fn project_document_spans(doc: &str) -> Option<Vec<ItemSpan>> {
    let components = component::parse(doc).ok()?;
    // Top-level components only (mirror `parse_doc_nodes` / `segment_into_nodes`).
    let top: Vec<&Component> = components
        .iter()
        .filter(|c| {
            !components.iter().any(|o| {
                !std::ptr::eq(o, *c) && o.open_start <= c.open_start && c.close_end <= o.close_end
            })
        })
        .collect();
    let mut top = top;
    top.sort_by_key(|c| c.open_start);

    let mut occurrence_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut out = Vec::new();
    for comp in top {
        let occ = occurrence_counts.entry(comp.name.clone()).or_insert(0);
        let occurrence = *occ;
        *occ += 1;
        // Body sits at [open_end, close_start); each item span is body-relative
        // offset translated by `open_end` into the full document.
        let body_start = comp.open_end;
        let body = comp.content(doc);
        let occ_proj = project_component(doc, comp, occurrence);
        let mut cursor = body_start;
        for (node_key, value) in occ_proj.items {
            let start = cursor;
            let end = start + value.len();
            cursor = end;
            out.push(ItemSpan {
                component: comp.name.clone(),
                occurrence,
                identity: node_key_identity(&node_key).to_string(),
                node_key,
                value,
                span: ByteSpan { start, end },
            });
        }
        // Lossless invariant: the item spans must exactly tile the body. If the
        // splitter ever drifted from `content`, decline (caller falls back) — we
        // never want an op routed against a mis-tiled body.
        debug_assert_eq!(cursor, body_start + body.len(), "item spans must tile body");
        if cursor != body_start + body.len() {
            return None;
        }
    }
    Some(out)
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

/// Per-component **authorship** conflict policy (#provauth1/#provauth4 — operator
/// provenance authority). When the same keyed cell was edited on both sides, the
/// *owner* of that component wins, deterministically and cleanly — no in-band
/// conflict marker.
///
/// - **Agent-owned** components (`exchange`, `status`, `log`) → [`OursWins`]:
///   the binary authors the response/status/log spine, so the committed (ours)
///   side is authoritative there (mirrors the `exchange` history spine and the
///   `#pcwcwarn` "agent owns exchange" reconcile rule).
/// - **Operator-owned** components (`queue`, `backlog`, `review`, `done`,
///   `icebox`, and any other) → [`TheirsWins`]: the operator edits these in the
///   live editor, so their edit is authoritative and must never be reset or
///   surfaced as a suspicious conflict ("all operator actions should never be
///   reset or treated as suspicious"). This is what makes the operator the
///   authority over their own cells instead of ours-wins + a `<<<<<<<` marker.
///
/// [`OursWins`]: ConflictPolicy::OursWins
/// [`TheirsWins`]: ConflictPolicy::TheirsWins
pub fn component_conflict_policy(component: &str) -> ConflictPolicy {
    match component {
        "exchange" | "status" | "log" => ConflictPolicy::OursWins,
        _ => ConflictPolicy::TheirsWins,
    }
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

/// Reconcile one component marker line operator-authoritatively (`#qmarkerauth`).
///
/// The marker (`<!-- agent:name attrs -->` / `<!-- /agent:name -->`) is
/// operator-authored configuration (priority / preset / `go` / activation / custom
/// attributes), so `theirs` (the live editor / disk side) wins over `ours` (the
/// agent / snapshot side) whenever the operator changed it; an agent-side change is
/// honored only when the operator left the marker at base. With no base, `theirs`
/// (operator) is authoritative. The marker analogue of the field-wise frontmatter
/// merge (`#fmreset`): an operator marker edit is never reverted by the merge path.
fn reconcile_marker_operator_authoritative<'a>(
    base: Option<&str>,
    ours: &'a str,
    theirs: &'a str,
) -> &'a str {
    if ours == theirs {
        return ours;
    }
    match base {
        Some(b) if theirs != b => theirs,
        Some(_) => ours,
        None => theirs,
    }
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

    // Index base occurrences + framings by (component, occurrence) for per-cell
    // base resolution. The framing map (`#qmarkerauth`) lets the component branch
    // reconcile the operator-owned marker line 3-way instead of always reframing
    // with ours' marker.
    let mut base_by_key: std::collections::HashMap<(String, usize), &ComponentOccurrence> =
        std::collections::HashMap::new();
    let mut base_framing_by_key: std::collections::HashMap<(String, usize), &ComponentFraming> =
        std::collections::HashMap::new();
    for n in &base_nodes {
        if let DocNode::Component {
            framing,
            occurrence,
        } = n
        {
            base_by_key.insert(
                (occurrence.component.clone(), occurrence.occurrence),
                occurrence,
            );
            base_framing_by_key.insert(
                (occurrence.component.clone(), occurrence.occurrence),
                framing,
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
    // Conflict policy is chosen per component by authorship (#provauth1/#provauth4)
    // inside the component branch, not a single global default.
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
                    Some(b) if *o == b => t.as_str(), // only theirs diverged
                    Some(b) if t == b => o.as_str(),  // only ours diverged
                    _ => o.as_str(),                  // both diverged / no base ⇒ ours-wins
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
                    framing: t_framing,
                    occurrence: t_occ,
                },
            ) => {
                // Splittability parity with the legacy `reconcile_component_body`
                // (`#qcellmerge1` finalize-path parity): keyed reconciliation is
                // only sound when ours AND theirs both project to real keyed
                // children. When one side splits into keyed children (e.g. an
                // `exchange` carrying a `### Re:` response on the agent/ours side)
                // but the other projects to the reserved single whole-body item
                // (no `### Re:` yet on the live/disk side), pairing a `body` item
                // against `preamble`/`### Re:` items is unsound: the body item is
                // present-only-on-one-side, so the weave keeps it verbatim ALONGSIDE
                // the keyed side's items — duplicating the prompt and retaining the
                // stale `<!-- agent:boundary:… -->` marker that ours replaced. A
                // whitespace-only body item contributes nothing and is harmless
                // (the keyed side wins cleanly — e.g. a fresh `### Re:` against an
                // empty exchange), so only a body item carrying real content forces
                // the fallback. The legacy `split(ours)? / split(theirs)?` guard
                // leaf-merges the whole component here; mirror that by falling back
                // so the caller runs the legacy whole-doc / per-node merge unchanged.
                if is_body_only(o_occ) != is_body_only(t_occ) {
                    let body_only_side = if is_body_only(o_occ) { o_occ } else { t_occ };
                    let body_only_has_content = body_only_side
                        .items
                        .iter()
                        .any(|(_, value)| !value.trim().is_empty());
                    if body_only_has_content {
                        return CellMergeOutcome::fallback();
                    }
                }
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
                // #provauth1/#provauth4: resolve same-cell conflicts by component
                // AUTHORSHIP — operator-owned cells (queue/backlog/…) keep the
                // operator's (theirs) edit, agent-owned cells (exchange/status/log)
                // keep the binary's (ours). The owner wins cleanly; no in-band
                // `<<<<<<<` marker is injected (operator actions are never reset or
                // surfaced as suspicious), superseding the legacy #qcellconflict
                // ours-wins+marker behavior.
                let comp_policy = component_conflict_policy(&o_occ.component);
                let (mut merged_items, conflicts) =
                    match compose_occurrence(base_ref, o_occ, t_occ, comp_policy) {
                        Some(r) => r,
                        None => return CellMergeOutcome::fallback(),
                    };
                // In-band conflict markers are OFF by default (#provauth1/#provauth4):
                // the owner already won cleanly above, so surfacing a `<<<<<<<`
                // marker would re-expose an operator action as a suspicious
                // conflict. Retained behind the explicit opt-in env gate only.
                if cell_merge_conflict_markers_enabled() {
                    surface_conflict_markers(&mut merged_items, &conflicts);
                }
                all_conflicts.extend(conflicts);
                // Recombine the body losslessly from the merged item values, then
                // reframe with the operator-authoritative marker (`#qmarkerauth`).
                // Previously this always reframed with OURS' marker, so an operator
                // attribute edit on the `<!-- agent:queue … -->` marker was silently
                // deleted on the next merge. The marker is operator-owned config, so
                // an operator change (theirs ≠ base) wins; an agent change wins only
                // when the operator left the marker at base.
                let body: String = merged_items.iter().map(|(_, v)| v.as_str()).collect();
                let base_framing = base_framing_by_key
                    .get(&(o_occ.component.clone(), o_occ.occurrence))
                    .copied();
                let open = reconcile_marker_operator_authoritative(
                    base_framing.map(|f| f.open.as_str()),
                    &o_framing.open,
                    &t_framing.open,
                );
                let close = reconcile_marker_operator_authoritative(
                    base_framing.map(|f| f.close.as_str()),
                    &o_framing.close,
                    &t_framing.close,
                );
                out.push_str(open);
                out.push_str(&body);
                out.push_str(close);
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

// ---------------------------------------------------------------------------
// Op-capture rung (`#qcellmerge1` op routing): route captured editor ops to the
// cell whose span contains them, replay per cell, then run the SAME per-cell
// 3-way join as `merge_3way` — true per-cell op isolation instead of a text-diff
// guess.
//
// Scope boundary (NOT built here): if the SAME cell is changed by BOTH ours and
// theirs (ours edited it AND theirs has routed ops for it), this rung does NOT
// character-level interleave — that is plan Phase 6 `#lztextcrdt`. The reused
// `merge_3way` join treats such a cell via the existing same-level conflict
// policy (ours-wins, recorded as a conflict; surfaced as an in-band conflict
// marker when conflict-surfacing is enabled). The win here is: ops in DIFFERENT
// cells are perfectly isolated and intention-preserved via real replay, and a
// single-sided same-cell op replays exactly — and because the join delegates to
// `merge_3way`, the disjoint-edit `op_level_merge` and conflict-surfacing
// additions on this branch are inherited automatically by the op-routed path.
// ---------------------------------------------------------------------------

/// Route captured editor ops to the cell (item span) that contains each one,
/// translating each op's absolute offset to a **cell-local** offset
/// (`op.offset - span.start`).
///
/// Returns `None` (the conservative bail — caller falls back to the text-diff
/// path) if ANY op:
/// - lands in structural framing (a component open/close marker, frontmatter, or
///   inter-item / inter-component interstitial text), i.e. is covered by no item
///   span;
/// - crosses a cell boundary (for a `Delete`, the whole `[offset, offset+len)`
///   range must lie within ONE item span);
/// - cannot be unambiguously placed (out-of-bounds, or the document failed to
///   project into spans).
///
/// Insert ops use `[offset, offset]` (a point), so an insert exactly at a cell's
/// trailing boundary is attributed to that cell only when the point is `< end`;
/// an insert at the very end of an item (== next item's start) attributes to the
/// next item, matching the editor's left-gravity-free absolute offset.
pub fn route_ops_to_cells(
    doc: &str,
    ops: &[EditorOp],
) -> Option<std::collections::HashMap<NodeKey, Vec<EditorOp>>> {
    if ops.is_empty() {
        return Some(std::collections::HashMap::new());
    }
    let spans = project_document_spans(doc)?;
    let mut routed: std::collections::HashMap<NodeKey, Vec<EditorOp>> =
        std::collections::HashMap::new();
    for op in ops {
        let (node_key, span) = match op {
            EditorOp::Insert { offset, .. } => {
                // A point edit: the containing item is the one whose half-open
                // span includes `offset`. An offset at the document's end, or in
                // framing, is covered by no item span → bail.
                let found = spans
                    .iter()
                    .find(|s| s.span.contains_offset(*offset))
                    .or_else(|| {
                        // Allow an insert exactly at an item's exclusive end only
                        // when no later item starts there (i.e. it is the trailing
                        // edge of the last item body). This keeps an append at the
                        // end of a cell body inside that cell rather than bailing.
                        spans.iter().find(|s| {
                            s.span.end == *offset && !spans.iter().any(|o| o.span.start == *offset)
                        })
                    });
                match found {
                    Some(s) => (s.node_key.clone(), s.span),
                    None => return None,
                }
            }
            EditorOp::Delete { offset, len } => {
                let hi = offset.checked_add(*len)?;
                // The WHOLE delete range must lie within one item span.
                let found = spans.iter().find(|s| s.span.contains_range(*offset, hi));
                match found {
                    Some(s) => (s.node_key.clone(), s.span),
                    None => return None,
                }
            }
        };
        let local = translate_to_cell_local(op, span.start)?;
        routed.entry(node_key).or_default().push(local);
    }
    Some(routed)
}

/// Translate an absolute-offset op into a cell-local op by subtracting the
/// containing span's start. `None` if the offset is below the span start
/// (should not happen once routed, but a conservative guard).
fn translate_to_cell_local(op: &EditorOp, span_start: usize) -> Option<EditorOp> {
    match op {
        EditorOp::Insert { offset, text } => Some(EditorOp::Insert {
            offset: offset.checked_sub(span_start)?,
            text: text.clone(),
        }),
        EditorOp::Delete { offset, len } => Some(EditorOp::Delete {
            offset: offset.checked_sub(span_start)?,
            len: *len,
        }),
    }
}

/// Op-aware 3-way per-cell merge: build the `theirs` side per cell by **replaying
/// the real captured ops** routed to that cell (not a text-diff guess), then run
/// the SAME per-cell 3-way join as [`merge_3way`].
///
/// When routing succeeds, every cell that received ops is reconstructed via
/// `replay_editor_ops(base_cell_text, routed_ops)`; cells with no routed ops keep
/// their base text. The reconstructed bodies are reassembled into a synthetic
/// `theirs` document inside the base framing, and the merge delegates to
/// [`merge_3way`] (base, ours, theirs-from-ops) so the lifecycle `Live<Struck`
/// join, the disjoint-edit `op_level_merge`, and honest conflict recording (plus
/// conflict-surfacing when enabled) all apply unchanged — the real-op path
/// inherits every engine addition on this branch.
///
/// If routing returns `None` (an op crossed a boundary or hit framing), the
/// outcome has `fell_back = true` and the caller uses the text-diff / legacy
/// path. A reconstructed `theirs` whose per-cell replay fails (stale ops) also
/// falls back.
pub fn merge_3way_with_ops(
    base: &str,
    ours: &str,
    theirs: &str,
    theirs_ops: &[EditorOp],
) -> CellMergeOutcome {
    // Route the live (theirs) ops against the BASE document — the captured ops
    // are absolute offsets into the document the editor mutated FROM, which is
    // the merge base.
    let routed = match route_ops_to_cells(base, theirs_ops) {
        Some(r) => r,
        None => return CellMergeOutcome::fallback(),
    };

    // Reconstruct theirs per cell from the base, replaying routed ops in place,
    // then reframe losslessly. We rebuild the full theirs text and delegate to
    // `merge_3way` so the per-cell join is the single shared implementation.
    let reconstructed_theirs = match reconstruct_theirs_from_ops(base, &routed) {
        Some(t) => t,
        None => return CellMergeOutcome::fallback(),
    };

    // Safety gate (mirrors the crdt op-replay invariant): the per-cell replay
    // must reproduce the editor-observed `theirs` byte-for-byte. If it does not,
    // the ops were captured against a divergent base (missed event / stale) — do
    // not trust them; fall back so the result is never worse than the diff path.
    if reconstructed_theirs != theirs {
        eprintln!(
            "[cell_merge] op-routing replay != theirs (stale/misaligned ops) — falling back to text-diff path"
        );
        return CellMergeOutcome::fallback();
    }

    merge_3way(base, ours, &reconstructed_theirs)
}

/// Rebuild a full document from `base`, replacing each routed cell's body slice
/// with its op-replayed text. Cells with no routed ops keep base text. Returns
/// `None` if the base fails to project into spans or any cell's replay is
/// out-of-bounds (stale ops).
fn reconstruct_theirs_from_ops(
    base: &str,
    routed: &std::collections::HashMap<NodeKey, Vec<EditorOp>>,
) -> Option<String> {
    let spans = project_document_spans(base)?;
    // Rebuild the document by walking the base bytes and substituting each item
    // span's text with its op-replayed value. Item spans tile each component body
    // with no gaps; framing between/around them is copied verbatim from base.
    let mut out = String::new();
    let mut cursor = 0usize;
    for item in &spans {
        // Copy verbatim framing before this item (markers, frontmatter, gaps).
        out.push_str(&base[cursor..item.span.start]);
        let cell_text = match routed.get(&item.node_key) {
            Some(ops) => replay_editor_ops(&item.value, ops)?,
            None => item.value.clone(),
        };
        out.push_str(&cell_text);
        cursor = item.span.end;
    }
    out.push_str(&base[cursor..]);
    Some(out)
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

    /// Incrementally update this tree from `old_doc` to `new_doc`, using the
    /// document diff + apply path instead of rebuilding every occurrence.
    pub fn update_to(&mut self, ctx: &Context, old_doc: &str, new_doc: &str) {
        for diff in diff_document(old_doc, new_doc) {
            self.apply(ctx, &diff);
        }

        let new_occurrences: std::collections::HashSet<(String, usize)> = project_document(new_doc)
            .into_iter()
            .map(|occ| (occ.component, occ.occurrence))
            .collect();
        let existing: Vec<(String, usize)> = self.occurrences.keys().cloned().collect();
        for (component, occurrence) in existing {
            if !new_occurrences.contains(&(component.clone(), occurrence)) {
                self.occurrences.remove(&(component.clone(), occurrence));
                self.root
                    .remove_child(ctx, &format!("{component}:{occurrence}"));
            }
        }
    }

    /// A small agent-doc semantic query over the reactive tree: unresolved
    /// prompt counts per subtree, memoized by lazily's `SemTree`.
    pub fn unresolved_prompt_counts(&self, ctx: &Context) -> SemTree<String, usize> {
        SemTree::build(ctx, &self.root, |value: &String, kids: &[usize]| {
            unresolved_prompt_count(value) + kids.iter().sum::<usize>()
        })
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
        apply_to_map(ctx, map, &diff.ops);
        apply_to_tree(ctx, &occ_node, &diff.ops);
    }
}

fn unresolved_prompt_count(value: &str) -> usize {
    value
        .lines()
        .filter(|line| line.contains("[#") && !line.contains("~~"))
        .count()
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

    fn tree_snapshot(
        ctx: &Context,
        tree: &CellDocTree,
    ) -> Vec<(String, usize, Vec<(String, String)>)> {
        let mut occurrences: Vec<(String, usize)> = tree.occurrences.keys().cloned().collect();
        occurrences.sort();
        occurrences
            .into_iter()
            .map(|(component, occurrence)| {
                let map = tree
                    .occurrences
                    .get(&(component.clone(), occurrence))
                    .expect("occurrence map present");
                let map_items: Vec<(String, String)> = map
                    .keys(ctx)
                    .into_iter()
                    .map(|id| {
                        let value = map.get(ctx, &id).expect("mapped value present");
                        (id, value)
                    })
                    .collect();
                let occ_id = format!("{component}:{occurrence}");
                let occ_node = tree.root().child(&occ_id).expect("occurrence node present");
                assert_eq!(
                    occ_node.child_ids(ctx),
                    map_items
                        .iter()
                        .map(|(id, _)| id.clone())
                        .collect::<Vec<_>>(),
                    "CellTree child order must match CellMap order for {occ_id}"
                );
                for (id, value) in &map_items {
                    assert_eq!(
                        occ_node.child(id).expect("child present").get(ctx),
                        *value,
                        "CellTree child value must match CellMap value for {id}"
                    );
                }
                (component, occurrence, map_items)
            })
            .collect()
    }

    fn assert_incremental_matches_rebuild(ctx: &Context, tree: &CellDocTree, doc: &str) {
        let rebuilt = CellDocTree::from_document(ctx, doc);
        assert_eq!(tree_snapshot(ctx, tree), tree_snapshot(ctx, &rebuilt));
    }

    fn eager_unresolved_prompt_count(doc: &str) -> usize {
        project_document(doc)
            .into_iter()
            .flat_map(|occ| occ.items.into_iter().map(|(_, value)| value))
            .map(|value| unresolved_prompt_count(&value))
            .sum()
    }

    #[test]
    fn update_to_matches_rebuild_across_revisions() {
        let ctx = Context::new();
        let mut tree = CellDocTree::from_document(&ctx, DOC);
        let mut old = DOC.to_string();

        let rev1 = old.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task EDITED\n",
        );
        let rev2 = rev1.replace(
            "- do [#alpha] first task\n- do [#beta] second task EDITED\n- do [#gamma] third task\n",
            "- do [#gamma] third task\n- do [#alpha] first task\n- do [#beta] second task EDITED\n",
        );
        let rev3 = format!(
            "{rev2}<!-- agent:review -->\n- do [#review] inspect\n<!-- /agent:review -->\n"
        );
        let rev4 = rev3.replace(
            "<!-- agent:backlog -->\n- [#one] backlog item one\n- [#two] backlog item two\n<!-- /agent:backlog -->\n",
            "",
        );
        let rev5 = rev4
            .replace(
                "- do [#alpha] first task\n",
                "- do [#alpha] first task CHANGED\n",
            )
            .replace(
                "- do [#review] inspect\n",
                "- do [#review] inspect EDITED\n",
            );

        for new_doc in [rev1, rev2, rev3, rev4, rev5] {
            tree.update_to(&ctx, &old, &new_doc);
            assert_incremental_matches_rebuild(&ctx, &tree, &new_doc);
            old = new_doc;
        }
    }

    #[test]
    fn sem_tree_unresolved_prompt_counts_are_subtree_scoped() {
        use std::cell::Cell as StdCell;
        use std::rc::Rc;

        let ctx = Context::new();
        let mut tree = CellDocTree::from_document(&ctx, DOC);
        let counts = tree.unresolved_prompt_counts(&ctx);
        assert_eq!(counts.value(&ctx), eager_unresolved_prompt_count(DOC));
        assert_eq!(counts.node_value(&ctx, &"queue:0".to_string()), Some(3));
        assert_eq!(counts.node_value(&ctx, &"backlog:0".to_string()), Some(2));

        let queue_alpha_calls = Rc::new(StdCell::new(0usize));
        let backlog_one_calls = Rc::new(StdCell::new(0usize));
        let probe = SemTree::build(&ctx, tree.root(), {
            let queue_alpha_calls = Rc::clone(&queue_alpha_calls);
            let backlog_one_calls = Rc::clone(&backlog_one_calls);
            move |value: &String, kids: &[usize]| {
                if value.contains("#alpha") {
                    queue_alpha_calls.set(queue_alpha_calls.get() + 1);
                }
                if value.contains("#one") {
                    backlog_one_calls.set(backlog_one_calls.get() + 1);
                }
                unresolved_prompt_count(value) + kids.iter().sum::<usize>()
            }
        });

        assert_eq!(probe.value(&ctx), 5);
        let queue_alpha_baseline = queue_alpha_calls.get();
        let backlog_one_baseline = backlog_one_calls.get();
        assert_eq!(probe.value(&ctx), 5);
        assert_eq!(
            queue_alpha_calls.get(),
            queue_alpha_baseline,
            "memoized re-read must not recompute queue leaf"
        );
        assert_eq!(
            backlog_one_calls.get(),
            backlog_one_baseline,
            "memoized re-read must not recompute backlog leaf"
        );

        let backlog_slot = probe.node(&"backlog:0".to_string()).unwrap();
        let edited = DOC.replace(
            "- do [#alpha] first task\n",
            "- ~~do [#alpha] first task~~\n",
        );
        tree.update_to(&ctx, DOC, &edited);
        assert!(
            ctx.is_set(&backlog_slot),
            "editing queue must not invalidate the backlog semantic subtree"
        );
        assert_eq!(probe.node_value(&ctx, &"queue:0".to_string()), Some(2));
        assert_eq!(probe.node_value(&ctx, &"backlog:0".to_string()), Some(2));
        assert_eq!(queue_alpha_calls.get(), queue_alpha_baseline + 1);
        assert_eq!(
            backlog_one_calls.get(),
            backlog_one_baseline,
            "backlog semantic leaf must stay cached after a queue edit"
        );

        let updated_counts = tree.unresolved_prompt_counts(&ctx);
        assert_eq!(
            updated_counts.value(&ctx),
            eager_unresolved_prompt_count(&edited)
        );
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
        // queue is operator-owned (#provauth1/#provauth4), so the conflict
        // resolves THEIRS-wins (the operator's edit is authoritative) cleanly,
        // with the losing (ours) side not blended in-band.
        let _guard = ENV_LOCK.lock().unwrap();
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
        // queue is operator-owned ⇒ theirs-wins; the chosen value is NOT a blend.
        assert!(c.chosen.contains("THEIRS-VERSION"));
        assert!(out.merged_text.contains("THEIRS-VERSION"));
        assert!(
            !out.merged_text.contains("OURS-VERSION"),
            "no fabricated blend: the losing ours value must not also appear"
        );
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

    /// `#qcellmerge1` finalize-path parity: when ours projects the `exchange`
    /// into keyed `### Re:` children (the agent placed a response) but theirs is
    /// still a single whole-body item carrying a live prompt + the stale boundary
    /// (no `### Re:` yet), the keyed compose would weave theirs' body in verbatim
    /// alongside ours' items — duplicating the prompt and keeping the stale
    /// `<!-- agent:boundary:base1234 -->` marker that ours replaced. The
    /// splittability-mismatch guard must fall back so the caller runs the legacy
    /// whole-component leaf merge (which drops the stale boundary).
    #[test]
    fn keyed_exchange_vs_body_only_with_content_falls_back() {
        let base = "<!-- agent:exchange -->\n\
❯ Please reply\n\
<!-- agent:boundary:base1234 -->\n\
<!-- /agent:exchange -->\n";
        // OURS: the agent's boundary-aware append replaced the boundary with a
        // `### Re:` response + a fresh boundary (keyed children).
        let ours = "<!-- agent:exchange -->\n\
❯ Please reply\n\
### Re: answer\n\nDone.\n\n\
<!-- agent:boundary:fresh5678 -->\n\
<!-- /agent:exchange -->\n";
        // THEIRS: the operator typed another line before the old boundary; still
        // no `### Re:`, so it projects to one whole-body item with real content.
        let theirs = "<!-- agent:exchange -->\n\
❯ Please reply\n\
while I was typing the next queue item\n\
<!-- agent:boundary:base1234 -->\n\
<!-- /agent:exchange -->\n";

        let out = merge_3way(base, ours, theirs);
        assert!(
            out.fell_back,
            "keyed-vs-body-only-with-content exchange must fall back to the legacy leaf merge"
        );
        assert!(out.merged_text.is_empty());
    }

    /// Counterpart: a keyed-vs-body-only mismatch where the body-only side is
    /// EMPTY (a fresh `### Re:` against an empty exchange) is harmless — the keyed
    /// side wins cleanly, so the per-cell path must NOT fall back.
    #[test]
    fn keyed_exchange_vs_empty_body_only_does_not_fall_back() {
        let base = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let ours =
            "<!-- agent:exchange -->\n### Re: answer\nResponse body.\n<!-- /agent:exchange -->\n";
        let theirs = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";

        let out = merge_3way(base, ours, theirs);
        assert!(
            !out.fell_back,
            "an empty body-only exchange must stay on the per-cell path"
        );
        assert!(out.merged_text.contains("Response body."));
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
        // queue is operator-owned ⇒ theirs-wins (#provauth1/#provauth4).
        assert!(c.chosen.contains("THEIRS-VERSION"), "operator-owned theirs-wins");
        assert!(out.merged_text.contains("THEIRS-VERSION"));
        assert!(
            !out.merged_text.contains("OURS-VERSION"),
            "no fabricated blend: losing ours value must not also appear:\n{}",
            out.merged_text
        );
    }

    /// Both sides struck the SAME head to DIFFERENT text (e.g. an operator note
    /// appended on one struck copy) → both at `Struck` level → a genuine content
    /// conflict, NOT silently dropped.
    #[test]
    fn both_sides_struck_different_text_is_a_content_conflict() {
        let _guard = ENV_LOCK.lock().unwrap();
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
        // queue is operator-owned ⇒ theirs-wins (#provauth1/#provauth4).
        assert!(out.merged_text.contains("struck THEIRS"));
        assert!(!out.merged_text.contains("struck OURS"));
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

    #[test]
    fn merge_3way_preserves_operator_marker_attribute_qmarkerauth() {
        // #qmarkerauth: the operator adds attributes to the agent:queue marker
        // (`<!-- agent:queue priority go -->`) on the live/disk (theirs) side. The
        // agent (ours) side still has the base marker. The cell merge MUST keep the
        // operator's marker attributes — never reframe with ours' bare marker (the
        // "adding an attribute to agent:queue deletes the attribute" bug).
        let base = "<!-- agent:queue -->\n- do [#beta] task\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] task\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue priority go -->\n- do [#beta] task\n<!-- /agent:queue -->\n";
        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back, "should not fall back: {out:?}");
        assert!(
            out.merged_text
                .contains("<!-- agent:queue priority go -->"),
            "operator marker attribute lost (reverted to ours' marker): {}",
            out.merged_text
        );
    }

    #[test]
    fn merge_3way_keeps_agent_marker_when_operator_left_it_at_base_qmarkerauth() {
        // The inverse: only the AGENT (ours) changed the marker; the operator left
        // it at base. The agent change is honored (operator didn't touch it).
        let base = "<!-- agent:queue -->\n- do [#beta] task\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue go -->\n- do [#beta] task\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#beta] task EDITED\n<!-- /agent:queue -->\n";
        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back, "should not fall back: {out:?}");
        assert!(
            out.merged_text.contains("<!-- agent:queue go -->"),
            "agent marker change lost: {}",
            out.merged_text
        );
        assert!(
            out.merged_text.contains("EDITED"),
            "operator body edit lost: {}",
            out.merged_text
        );
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
        // queue is operator-owned ⇒ theirs-wins (#provauth1/#provauth4).
        assert!(out.merged_text.contains("THEIRS-ONLY"), "operator-owned theirs-wins");
        assert!(
            !out.merged_text.contains("OURS-ONLY"),
            "no blend on a real overlap: {}",
            out.merged_text
        );
    }

    /// (c) Gate explicitly OFF reproduces the legacy ours-wins behavior unchanged:
    /// even DISJOINT same-cell edits record a conflict and drop theirs.
    #[test]
    fn opcapture_off_legacy_owner_wins_unchanged() {
        let _guard = ENV_LOCK.lock().unwrap();
        let base = "<!-- agent:queue -->\n- do [#beta] the original answer body here\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] the EDITED answer body here\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#beta] the original answer body THERE!\n<!-- /agent:queue -->\n";

        // SAFETY: opcapture is default-ON, so explicitly engage its kill-switch to
        // exercise the policy-only path (no op-level merge).
        unsafe {
            std::env::set_var(CELL_MERGE_OPCAPTURE_ENV, "0");
        }
        assert!(!cell_merge_opcapture_enabled(), "opcapture kill-switch off");

        let out = merge_3way(base, ours, theirs);
        assert!(!out.fell_back);
        assert_eq!(
            out.conflicts.len(),
            1,
            "OFF: disjoint same-cell edits are a conflict (no op-merge): {:?}",
            out.conflicts
        );
        // queue is operator-owned ⇒ theirs-wins (#provauth1/#provauth4); with
        // opcapture OFF the disjoint ours edit is dropped, not blended.
        assert!(out.merged_text.contains("THERE!"), "operator-owned theirs-wins");
        assert!(
            !out.merged_text.contains("EDITED"),
            "OFF: losing ours disjoint edit is dropped (no op-merge): {}",
            out.merged_text
        );
        // SAFETY: still holding the lock — restore the default (ON).
        unsafe {
            std::env::remove_var(CELL_MERGE_OPCAPTURE_ENV);
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
        // The active/structural list line is theirs — queue is operator-owned, so
        // theirs-wins (#provauth1/#provauth4); the marker (opt-in ON) still shows
        // both verbatim, but the active line is the operator's.
        assert!(
            out.merged_text.contains("- do [#beta] THEIRS-VERSION\n"),
            "theirs stays the active list line: {}",
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
    fn conflict_markers_default_off_optin_surfaces() {
        let base = "<!-- agent:queue -->\n- do [#beta] original\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:queue -->\n- do [#beta] OURS-VERSION\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:queue -->\n- do [#beta] THEIRS-VERSION\n<!-- /agent:queue -->\n";

        // queue is operator-owned ⇒ theirs-wins. Default (absent) is OFF
        // (#provauth1/#provauth4): clean theirs-wins, NO in-band marker.
        let absent = merge_with_markers(None, base, ours, theirs);
        assert!(absent.merged_text.contains("THEIRS-VERSION"));
        assert!(
            !absent.merged_text.contains("OURS-VERSION"),
            "default OFF: losing ours not surfaced: {}",
            absent.merged_text
        );
        assert!(
            !absent.merged_text.contains("<<<<<<<"),
            "default OFF: no conflict marker: {}",
            absent.merged_text
        );
        // The conflict is still RECORDED (stderr surface) regardless of gate.
        assert_eq!(absent.conflicts.len(), 1, "conflict still recorded");

        // Explicit opt-in ("1") DOES surface both versions in-band for inspection.
        let on = merge_with_markers(Some("1"), base, ours, theirs);
        assert!(
            on.merged_text.contains("THEIRS-VERSION")
                && on.merged_text.contains("OURS-VERSION")
                && on.merged_text.contains("<<<<<<<"),
            "opt-in ON surfaces both versions in-band: {}",
            on.merged_text
        );
        // Opt-in ON must diverge from the default-OFF clean output.
        assert_ne!(
            absent.merged_text, on.merged_text,
            "opt-in must diverge from the default-OFF clean output"
        );
    }

    /// Sub-gate default-OFF + explicit opt-in + master-gate parsing
    /// (#provauth1/#provauth4 flipped the marker default to OFF).
    #[test]
    fn conflict_markers_gate_default_off_with_optin() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Master stays default-ON (absent) throughout the sub-gate checks.
        unsafe {
            std::env::remove_var(CELL_MERGE_ENV);
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }
        assert!(
            !cell_merge_conflict_markers_enabled(),
            "absent ⇒ default OFF"
        );
        // Only an explicit truthy value opts the marker surfacing IN.
        for v in ["1", "true", "on", "yes", "TRUE", " On "] {
            unsafe {
                std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, v);
            }
            assert!(
                cell_merge_conflict_markers_enabled(),
                "{v:?} should opt-in ON"
            );
        }
        // Falsy / empty / unrecognized ⇒ stays OFF (default).
        for v in ["0", "false", "off", "no", "FALSE", " Off ", "", "maybe"] {
            unsafe {
                std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, v);
            }
            assert!(
                !cell_merge_conflict_markers_enabled(),
                "{v:?} should stay OFF"
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
        // queue is operator-owned ⇒ theirs-wins; the active line is theirs (the
        // `-->` is harmless as ordinary list text, only unsafe inside a marker).
        assert!(out.merged_text.contains("THEIRS with --> inside"));
    }

    // -----------------------------------------------------------------------
    // Op-capture front-end (`#qcellmerge1` op routing): span projection,
    // op routing, op-replayed `theirs`, and composition with the engine.
    // -----------------------------------------------------------------------

    /// A simple single-component base used by the op-routing tests. Each queue
    /// item sits on its own line; the item span includes the trailing newline
    /// (the keyed-child splitter is lossless line-by-line).
    const OPDOC: &str = "\
<!-- agent:queue -->
- do [#alpha] first task
- do [#beta] second task
- do [#gamma] third task
<!-- /agent:queue -->
";

    /// `project_document_spans` tiles each component body losslessly: the item
    /// spans, concatenated in order, exactly reproduce the body text, and each
    /// span's slice equals its projected value.
    #[test]
    fn project_document_spans_tiles_body_losslessly() {
        let spans = project_document_spans(OPDOC).expect("spans project");
        assert_eq!(spans.len(), 3, "three queue items: {spans:?}");
        // Each span slices to its own value.
        for s in &spans {
            assert_eq!(&OPDOC[s.span.start..s.span.end], s.value.as_str());
        }
        // Spans are contiguous and tile the queue body with no gaps/overlap.
        for w in spans.windows(2) {
            assert_eq!(w[0].span.end, w[1].span.start, "spans must be contiguous");
        }
        // The concatenation reproduces the body between the open/close markers.
        let body_start = OPDOC.find("first task").unwrap() - "- do [#alpha] ".len();
        let body_end = OPDOC.find("<!-- /agent:queue -->").unwrap();
        let reassembled: String = spans.iter().map(|s| s.value.clone()).collect();
        assert_eq!(reassembled, &OPDOC[body_start..body_end]);
        // Identities are the durable reorder-stable keys.
        assert_eq!(spans[0].identity, "queue:0:id:alpha");
        assert_eq!(spans[2].identity, "queue:0:id:gamma");
    }

    /// An op landing wholly inside one cell routes to that cell with a
    /// cell-local offset; a delete that crosses a cell boundary forces the
    /// conservative `None` bail (caller falls back).
    #[test]
    fn route_ops_to_cells_places_by_span_and_bails_on_boundary_cross() {
        let spans = project_document_spans(OPDOC).unwrap();
        let beta = spans
            .iter()
            .find(|s| s.identity == "queue:0:id:beta")
            .unwrap();
        // Insert " THEIRS" just before beta's trailing newline (inside beta).
        let abs = OPDOC.find("second task").unwrap() + "second task".len();
        assert!(beta.span.contains_offset(abs), "offset must be in beta");
        let ops = vec![EditorOp::Insert {
            offset: abs,
            text: " THEIRS".to_string(),
        }];
        let routed = route_ops_to_cells(OPDOC, &ops).expect("routes");
        assert_eq!(routed.len(), 1, "exactly one cell touched");
        let (key, cell_ops) = routed.iter().next().unwrap();
        assert_eq!(node_key_identity(key), "queue:0:id:beta");
        // The op's offset is translated to cell-local (offset - span.start).
        match &cell_ops[0] {
            EditorOp::Insert { offset, text } => {
                assert_eq!(*offset, abs - beta.span.start, "cell-local offset");
                assert_eq!(text, " THEIRS");
            }
            other => panic!("expected insert, got {other:?}"),
        }

        // A delete spanning from inside beta into gamma crosses a boundary → bail.
        let gamma = spans
            .iter()
            .find(|s| s.identity == "queue:0:id:gamma")
            .unwrap();
        let cross_len = gamma.span.start + 1 - beta.span.start;
        let crossing = vec![EditorOp::Delete {
            offset: beta.span.start,
            len: cross_len,
        }];
        assert!(
            route_ops_to_cells(OPDOC, &crossing).is_none(),
            "boundary-crossing delete must bail to None"
        );

        // An op in structural framing (the open marker) is covered by no item
        // span → bail.
        let framing = vec![EditorOp::Insert {
            offset: 1,
            text: "x".to_string(),
        }];
        assert!(
            route_ops_to_cells(OPDOC, &framing).is_none(),
            "framing op must bail to None"
        );
    }

    /// The real-op path reconstructs `theirs` from captured ops and merges via
    /// `merge_3way`: a single-sided op in one cell lands, untouched cells stay.
    #[test]
    fn merge_3way_with_ops_replays_real_ops_and_merges() {
        // theirs edits beta via a real insert; ours is unchanged from base.
        let abs = OPDOC.find("second task").unwrap() + "second task".len();
        let ops = vec![EditorOp::Insert {
            offset: abs,
            text: " THEIRS".to_string(),
        }];
        let theirs = OPDOC.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task THEIRS\n",
        );
        let out = merge_3way_with_ops(OPDOC, OPDOC, &theirs, &ops);
        assert!(!out.fell_back, "real-op path should merge, not fall back");
        assert!(out.conflicts.is_empty(), "single-sided: no conflict");
        assert!(out.merged_text.contains("second task THEIRS"));
        // Untouched cells preserved.
        assert!(out.merged_text.contains("- do [#alpha] first task\n"));
        assert!(out.merged_text.contains("- do [#gamma] third task\n"));
    }

    /// The byte-for-byte safety gate: if the routed-op replay does NOT reproduce
    /// the editor-observed `theirs` (stale / misaligned ops), the path declines
    /// (`fell_back`) instead of trusting the ops.
    #[test]
    fn merge_3way_with_ops_safety_gate_falls_back_on_stale_ops() {
        // Ops claim a " THEIRS" insert, but the supplied `theirs` text says
        // something different (DIVERGENT) — replay(base, ops) != theirs.
        let abs = OPDOC.find("second task").unwrap() + "second task".len();
        let ops = vec![EditorOp::Insert {
            offset: abs,
            text: " THEIRS".to_string(),
        }];
        let divergent_theirs = OPDOC.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task DIVERGENT\n",
        );
        let out = merge_3way_with_ops(OPDOC, OPDOC, &divergent_theirs, &ops);
        assert!(
            out.fell_back,
            "stale/misaligned ops must trip the byte-for-byte safety gate"
        );
    }

    /// A boundary-crossing op makes routing return `None`, so the op-aware path
    /// declines and the caller uses the text-diff fallback.
    #[test]
    fn merge_3way_with_ops_falls_back_on_framing_op() {
        // An op in the open-marker framing routes to no cell → fall back.
        let ops = vec![EditorOp::Insert {
            offset: 1,
            text: "x".to_string(),
        }];
        // theirs text is irrelevant here (routing bails before the safety gate).
        let out = merge_3way_with_ops(OPDOC, OPDOC, OPDOC, &ops);
        assert!(out.fell_back, "framing op must fall back");
    }

    /// COMPOSITION: the op-routed path delegates to `merge_3way`, so a same-cell
    /// concurrent edit (ours edits beta, theirs has a routed op for beta) still
    /// gets the engine's conflict-surfacing — the real-op front-end inherits the
    /// branch's engine additions.
    #[test]
    fn op_routed_same_cell_inherits_conflict_surfacing() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Force the conflict-marker sub-gate ON deterministically.
        unsafe {
            std::env::set_var(CELL_MERGE_CONFLICT_MARKERS_ENV, "1");
        }
        // ours edits beta one way; theirs' real op edits beta a DIFFERENT way to
        // the SAME region → a genuine same-cell conflict after delegation.
        let ours = OPDOC.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task OURS-VERSION\n",
        );
        let abs = OPDOC.find("second task").unwrap() + "second task".len();
        let ops = vec![EditorOp::Insert {
            offset: abs,
            text: " THEIRS-VERSION".to_string(),
        }];
        let theirs = OPDOC.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task THEIRS-VERSION\n",
        );
        let out = merge_3way_with_ops(OPDOC, &ours, &theirs, &ops);
        unsafe {
            std::env::remove_var(CELL_MERGE_CONFLICT_MARKERS_ENV);
        }
        assert!(!out.fell_back, "real-op path should merge: {out:?}");
        assert_eq!(
            out.conflicts.len(),
            1,
            "same-cell divergence surfaces a conflict via merge_3way: {:?}",
            out.conflicts
        );
        let c = &out.conflicts[0];
        assert_eq!(c.identity, "queue:0:id:beta");
        assert!(c.ours.contains("OURS-VERSION"));
        assert!(c.theirs.contains("THEIRS-VERSION"));
        // Conflict-surfacing (engine addition) reached the op-routed result:
        // both versions appear in-band via the surfaced marker block.
        assert!(
            out.merged_text.contains("OURS-VERSION") && out.merged_text.contains("THEIRS-VERSION"),
            "op-routed path must inherit in-band conflict-surfacing: {}",
            out.merged_text
        );
    }

    /// COMPOSITION: the op-routed path also inherits the disjoint-edit
    /// `op_level_merge` — two real ops in DIFFERENT regions of the SAME cell
    /// converge with BOTH preserved and NO conflict.
    #[test]
    fn op_routed_disjoint_same_cell_edits_inherit_op_level_merge() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Ensure the opcapture sub-gate is at its default-ON behavior.
        unsafe {
            std::env::remove_var(CELL_MERGE_OPCAPTURE_ENV);
        }
        // ours appends to the END of beta's text; theirs' real op inserts at the
        // START of beta's content — disjoint regions of the same cell.
        let ours = OPDOC.replace(
            "- do [#beta] second task\n",
            "- do [#beta] second task OURS_TAIL\n",
        );
        // theirs op: insert "THEIRS_HEAD " right after "[#beta] ".
        let head_abs = OPDOC.find("second task").unwrap();
        let ops = vec![EditorOp::Insert {
            offset: head_abs,
            text: "THEIRS_HEAD ".to_string(),
        }];
        let theirs = OPDOC.replace(
            "- do [#beta] second task\n",
            "- do [#beta] THEIRS_HEAD second task\n",
        );
        let out = merge_3way_with_ops(OPDOC, &ours, &theirs, &ops);
        assert!(!out.fell_back, "real-op path should merge: {out:?}");
        assert!(
            out.conflicts.is_empty(),
            "disjoint same-cell edits must converge with no conflict: {:?}",
            out.conflicts
        );
        assert!(
            out.merged_text.contains("THEIRS_HEAD") && out.merged_text.contains("OURS_TAIL"),
            "both disjoint edits must survive (op_level_merge): {}",
            out.merged_text
        );
    }

    /// When ops are ABSENT, the merge takes the existing text-diff path
    /// (`merge_3way`), proving the op front-end is purely additive — nothing is
    /// wasted in the degraded (no-capture) mode.
    #[test]
    fn ops_absent_uses_text_diff_path() {
        // ours edits alpha, theirs edits gamma — different cells, no ops.
        let ours = OPDOC.replace(
            "- do [#alpha] first task\n",
            "- do [#alpha] first task OURS\n",
        );
        let theirs = OPDOC.replace(
            "- do [#gamma] third task\n",
            "- do [#gamma] third task THEIRS\n",
        );
        // Direct text-diff engine call (the path merge_inner uses when ops==None).
        let out = merge_3way(OPDOC, &ours, &theirs);
        assert!(!out.fell_back);
        assert!(out.conflicts.is_empty());
        assert!(out.merged_text.contains("first task OURS"));
        assert!(out.merged_text.contains("third task THEIRS"));
    }
}

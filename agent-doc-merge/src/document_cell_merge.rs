//! Semantic, node-keyed three-way merge over the component overlay.
//!
//! Phase 4 (`#semmerge-owner`) of `tasks/agent-doc/plan-semantic-ast-merge.md`:
//! a **pure** decision function that merges a base snapshot, the agent result
//! (`ours`), and the operator editor buffer (`theirs`) by *node identity* rather
//! than by text lines. It is the merge-**policy** layer on top of the Phase 2
//! [`agent_doc_markdown_ast::overlay`] node-keyed substrate — not a second CRDT, and with **no
//! agent-doc coupling** (no IPC, no commit path, no git).
//!
//! The classic line-based merge sees two concurrent edits to *different* items as
//! a text conflict and fail-closes (dropping the agent's work). This function
//! instead keys every change on a stable [`agent_doc_markdown_ast::overlay::Item::id`] within its
//! component (component identity = projected name + effective `id=` +
//! occurrence, see `ComponentIdentity`) so node-disjoint change-sets — the
//! common case — merge cleanly, and only true same-node conflicts surface, each
//! with a deterministic outcome.
//!
//! ## Conflict policy (transition table)
//!
//! Per node, keyed by id, classified by component:
//!
//! | base | ours (agent) | theirs (operator) | outcome |
//! |------|--------------|-------------------|---------|
//! | present | unchanged | unchanged | [`OutcomeKind::Keep`] |
//! | present | edited | unchanged | [`OutcomeKind::AppliedAgentEdit`] |
//! | present | unchanged | edited | [`OutcomeKind::AppliedOperatorEdit`] |
//! | present | edited | edited (same text) | [`OutcomeKind::Convergent`] |
//! | present | edited | edited (diff text) | [`OutcomeKind::OperatorWonConflict`] + advisory |
//! | present | edited/struck | deleted | [`OutcomeKind::DeletionKept`] + advisory |
//! | present | deleted (struck/absent) | edited | [`OutcomeKind::OperatorRevived`] + advisory |
//! | absent | added | absent | [`OutcomeKind::AppliedAgentAdd`] |
//! | absent | absent | added | [`OutcomeKind::AppliedOperatorAdd`] |
//! | absent | added | added (diff ids) | [`OutcomeKind::AppliedBothAdd`] |
//! | absent | added | added (same id, diff text) | [`OutcomeKind::OperatorWonConflict`] + advisory |
//!
//! "edited" means the item's `text` **or** any of its `struck` / `pinned` /
//! `agent_pinned` flags differ from base. A `struck:true` node is *present-but-
//! struck* (a flag edit), **not** deleted; a DELETE means the id is gone entirely
//! from that side's component.
//!
//! ## Frontmatter
//!
//! The leading `---` … `---` YAML block is treated as a set of scalar key-nodes
//! parsed line-by-line (`key: value`). The same per-key transition applies:
//! operator-only change ⇒ operator value; agent-only change ⇒ agent value; both
//! changed differently ⇒ operator wins + advisory. This is intentionally *not* a full
//! YAML parser. If a key's value is non-scalar (the line is not a clean
//! `key: value`, e.g. a block/sequence value spanning multiple lines) the merge
//! falls back conservatively: the **operator** frontmatter block wins as a whole
//! when the operator changed the frontmatter at all, otherwise the **agent**
//! block is kept. The common scalar case (`queue: start` → `queue: stop`) merges
//! per-key with full fidelity.
//!
//! ## `merged_doc` reconstruction & documented assumption
//!
//! The operator buffer (`theirs`) is the structural skeleton for layout: its
//! non-component prose, component placement/order, and per-component item order
//! are preserved. For each component the item list is rebuilt by applying the
//! transition outcomes — operator items kept (with agent edits applied for ids the
//! operator did not delete, operator-deleted nodes omitted), then agent-added
//! nodes (absent in operator) appended at the end of the matching component in
//! agent order. Components that exist only in `ours` (e.g. a brand-new exchange
//! turn appended as a new `### Re:` block / item) are carried over.
//!
//! **Documented assumption (this phase):** faithful three-way reconstruction of
//! arbitrary *non-component prose* drift is out of scope. The reconstruction uses
//! the operator buffer's prose verbatim; if the agent edited prose *outside* any
//! component, that prose edit is not merged (the operator skeleton wins). The
//! component-level merge (queue / backlog / review / exchange items + frontmatter
//! scalars) — the core of the data-loss bug this fixes — is complete and exact.
//! See the `prose_skeleton_is_operator` test.

use agent_doc_markdown_ast::overlay::{
    Component, ComponentMarkerScan, Item, ItemSurface, components,
};

/// The classification of a single node's merge outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutcomeKind {
    /// Present and unchanged on both sides.
    Keep,
    /// Agent edited the node, operator left it unchanged.
    AppliedAgentEdit,
    /// Operator edited the node, agent left it unchanged.
    AppliedOperatorEdit,
    /// Both edited the node to the same text — applied once.
    Convergent,
    /// Both edited the node differently — operator text wins; an advisory is emitted.
    OperatorWonConflict,
    /// Operator deleted a node the agent edited/struck — the deletion stands and
    /// the node is omitted from `merged_doc`; an advisory is emitted.
    DeletionKept,
    /// Agent deleted/struck a node the operator edited — the operator edit wins
    /// (the node is un-struck / revived); an advisory is emitted.
    OperatorRevived,
    /// Node added only by the agent (absent in base and operator).
    AppliedAgentAdd,
    /// Node added only by the operator (absent in base and agent).
    AppliedOperatorAdd,
    /// Node-disjoint adds on both sides (different ids) — union, no conflict.
    AppliedBothAdd,
}

/// A per-node outcome record: which component, which id, and the classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeOutcome {
    /// Component name the node belongs to. `__frontmatter__` for frontmatter keys.
    pub component: String,
    /// The node's stable id (item id, or frontmatter key name).
    pub id: String,
    pub kind: OutcomeKind,
}

/// Why a semantic merge conflict is projected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeConflictReason {
    /// Operator deleted a node the agent had edited or struck; the deletion stands.
    OperatorDeletedAgentEditedNode,
    /// Operator and agent edited the same node differently — operator content won.
    SameNodeOperatorOverride,
    /// Agent deleted/struck a node the operator edited — operator revived it.
    OperatorRevivedAgentDeletedNode,
}

impl MergeConflictReason {
    /// Stable snake_case token for serialization, logging, and the Phase-4
    /// current-cycle projection (`cycle_state` persists the token, not
    /// the enum, so the orchestration crate stays decoupled from this type's
    /// representation).
    pub fn token(&self) -> &'static str {
        match self {
            MergeConflictReason::OperatorDeletedAgentEditedNode => {
                "operator_deleted_agent_edited_node"
            }
            MergeConflictReason::SameNodeOperatorOverride => "same_node_operator_override",
            MergeConflictReason::OperatorRevivedAgentDeletedNode => {
                "operator_revived_agent_deleted_node"
            }
        }
    }
}

/// A reactive advisory for a non-applied agent change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConflictAdvisory {
    /// Component name (`__frontmatter__` for frontmatter keys).
    pub component: String,
    /// The node's stable id (item id, or frontmatter key name).
    pub id: String,
    pub reason: MergeConflictReason,
    /// Human-readable detail for diagnostics and editor presentation.
    pub detail: String,
}

/// The result of a [`document_cell_merge`]: the merged document plus the per-node
/// outcomes and any conflict advisories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentCellMerge {
    pub merged_doc: String,
    pub outcomes: Vec<NodeOutcome>,
    pub conflict_advisories: Vec<MergeConflictAdvisory>,
    /// `#hap7` / `#qdup` deleted-structure rule: human-readable notes for nodes the
    /// operator deleted this cycle that the agent's content had targeted (edited).
    /// The deletion stands — the in-editor document is the source of truth and the
    /// agent change is NOT merged back — but the fact is surfaced in `agent:exchange`
    /// (and recorded here) so the operator sees the dropped agent edit this cycle,
    /// independent of the cycle-local conflict projection. Each note is also injected
    /// into the `exchange` component of [`Self::merged_doc`].
    pub exchange_notes: Vec<String>,
}

/// Sentinel component name used for frontmatter scalar key-nodes in outcomes/advisories.
pub const FRONTMATTER_COMPONENT: &str = "__frontmatter__";

/// Merge `base`, `ours_agent`, and `theirs_operator` by node identity.
///
/// Pure: no IO, no git, no agent-doc coupling. See the [module docs](self) for the
/// full transition table, the frontmatter handling, and the documented prose
/// reconstruction assumption.
pub fn document_cell_merge(
    base: &str,
    ours_agent: &str,
    theirs_operator: &str,
) -> DocumentCellMerge {
    let mut outcomes = Vec::new();
    let mut conflict_advisories = Vec::new();

    // --- Frontmatter merge -------------------------------------------------
    let (base_fm, _base_body) = split_frontmatter(base);
    let (ours_fm, _ours_body) = split_frontmatter(ours_agent);
    let (theirs_fm, theirs_body) = split_frontmatter(theirs_operator);
    let merged_fm = merge_frontmatter_owned(
        base_fm,
        ours_fm,
        theirs_fm,
        &mut outcomes,
        &mut conflict_advisories,
    );

    // --- Component merge over the operator body (skeleton) -----------------
    let base_comps = components(base);
    let ours_comps = components(ours_agent);
    let theirs_comps = components(theirs_operator);

    let merged_body = merge_components_into_body(
        theirs_body,
        theirs_operator.len() - theirs_body.len(),
        &theirs_comps,
        &base_comps,
        &ours_comps,
        ours_agent,
        &mut outcomes,
        &mut conflict_advisories,
    );

    // `#hap7` / `#qdup` / `#qnodemerge5`: when the operator's edit wins over
    // an agent-targeted same node, surface the non-applied agent side in
    // `agent:exchange`. Deletions and same-node overrides both keep the
    // operator/editor value; the note makes that conflict visible instead of
    // silently fabricating a merged text neither side wrote.
    let exchange_notes: Vec<String> = conflict_advisories
        .iter()
        .filter_map(|advisory| match advisory.reason {
            MergeConflictReason::OperatorDeletedAgentEditedNode => {
                Some(deletion_note(&advisory.component, &advisory.id))
            }
            MergeConflictReason::SameNodeOperatorOverride => Some(conflict_note(advisory)),
            MergeConflictReason::OperatorRevivedAgentDeletedNode => None,
        })
        .collect();

    let merged_body = inject_exchange_notes(merged_body, &exchange_notes);

    let merged_doc = match merged_fm {
        Some(fm) => format!("{fm}{merged_body}"),
        None => merged_body,
    };

    DocumentCellMerge {
        merged_doc,
        outcomes,
        conflict_advisories,
        exchange_notes,
    }
}

/// `#hap7` / `#qdup`: the canonical one-line note recorded when the operator
/// deleted a structural node the agent's content targeted. A blockquote line
/// (never a `### ` heading or `❯` prompt) so it cannot be misclassified as a
/// response turn or a user prompt by the convergence / drift gates.
fn deletion_note(component: &str, id: &str) -> String {
    format!(
        "> ⚠️ agent-doc: operator deleted `{component}` node `#{id}` that this turn targeted — deletion stands (the editor is the source of truth); the agent's change was not merged back."
    )
}

/// `#qnodemerge5`: canonical one-line note for a true same-leaf conflict. The
/// operator/editor value is already the merged value; the note carries the
/// rejected agent side so the conflict is visible to the operator.
fn conflict_note(advisory: &MergeConflictAdvisory) -> String {
    format!("> agent-doc conflict: {}.", advisory.detail)
}

/// Inject conflict notes into the `exchange` component of a merged body.
///
/// Notes are appended to the exchange inner, before a trailing
/// `<!-- agent:boundary:… -->` marker if one is present (same convention as
/// appended response turns), else just before the close marker. A note already
/// present in the exchange inner is not re-added (idempotent across repeated
/// convergence attempts in a cycle and across cycles where `base` already carries
/// it). If there is no `exchange` component the notes cannot be surfaced here and
/// the body is returned unchanged (the cycle-local conflict projection still fires).
fn inject_exchange_notes(body: String, notes: &[String]) -> String {
    if notes.is_empty() {
        return body;
    }
    // Only inject notes not already present anywhere in the body (dedup guard).
    let fresh: Vec<&String> = notes
        .iter()
        .filter(|n| !body.contains(n.as_str()))
        .collect();
    if fresh.is_empty() {
        return body;
    }

    let mut out = String::new();
    let mut inside_exchange = false;
    let mut inner: Vec<String> = Vec::new();
    let mut injected = false;
    // `#fencedmarkerscan`: fenced marker lines never open or close the exchange.
    let markers = ComponentMarkerScan::new(&body);
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let content = line.trim_end_matches(['\n', '\r']);
        if !inside_exchange {
            if markers.open_at(line_start, content).map(|(raw, _)| raw).as_deref()
                == Some(EXCHANGE_COMPONENT)
            {
                inside_exchange = true;
            }
            out.push_str(line);
            continue;
        }
        if markers.close_at(line_start, content).as_deref() == Some(EXCHANGE_COMPONENT) {
            // Emit the buffered inner with the notes inserted before any trailing
            // boundary marker, then the close marker.
            let boundary_idx = inner
                .iter()
                .rposition(|l| is_boundary_marker(l.trim_end_matches(['\n', '\r']).trim()));
            let mut note_block = String::new();
            // Separate the notes from preceding content with a blank line when the
            // preceding inner content does not already end with one.
            let preceding_nonblank = match boundary_idx {
                Some(idx) => inner[..idx]
                    .iter()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .is_some(),
                None => inner.iter().rev().find(|l| !l.trim().is_empty()).is_some(),
            };
            if preceding_nonblank {
                note_block.push('\n');
            }
            for n in &fresh {
                note_block.push_str(n);
                note_block.push('\n');
            }
            match boundary_idx {
                Some(idx) => {
                    for l in &inner[..idx] {
                        out.push_str(l);
                    }
                    out.push_str(&note_block);
                    for l in &inner[idx..] {
                        out.push_str(l);
                    }
                }
                None => {
                    for l in &inner {
                        out.push_str(l);
                    }
                    out.push_str(&note_block);
                }
            }
            out.push_str(line);
            inside_exchange = false;
            injected = true;
            continue;
        }
        inner.push(line.to_string());
    }
    if !injected {
        // No exchange close marker found (no exchange component) — return unchanged.
        return body;
    }
    out
}

/// The set of structural nodes considered "active" in the current agent turn —
/// the in-flight prompt and its response area (the `exchange` tail). Used by
/// [`document_cell_merge_scoped`] to gate which same-node conflicts raise an
/// [`MergeConflictAdvisory`].
///
/// `#msn6` / `#smturnactive` (document_cell_merge Phase 6, turn-active-area merge
/// gating): a same-node operator↔agent collision OUTSIDE the turn-active area
/// auto-resolves to the operator value with no advisory noise; only a collision
/// INSIDE the active area produces an advisory. The merged document is identical
/// either way (the operator always wins a same-node conflict) — only advisory
/// emission is scoped, so this can never lose content.
///
/// A node is active when its whole component is marked active
/// ([`active_component`](Self::active_component)) OR the exact `(component, id)`
/// pair was added ([`with_node`](Self::with_node)). The common caller marks the
/// `exchange` component active because the turn-active area lives entirely in the
/// exchange tail (the operator editing the queue / a backlog item while the agent
/// writes its response is therefore out-of-area and auto-resolves).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveNodes {
    components: std::collections::HashSet<String>,
    nodes: std::collections::HashSet<(String, String)>,
}

impl ActiveNodes {
    /// An empty active set. In [`document_cell_merge_scoped`] this means "nothing is
    /// active" — every out-of-area conflict auto-resolves and NO advisories are emitted.
    /// Callers that want the legacy all-active behavior call [`document_cell_merge`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark an entire component active (every node in it). The typical turn-active
    /// scoping is `ActiveNodes::new().active_component("exchange")`.
    pub fn active_component(mut self, component: impl Into<String>) -> Self {
        self.components.insert(component.into());
        self
    }

    /// Mark a single `(component, id)` node active. Use for node-granular scoping
    /// within a component (e.g. only the current turn's response heading in
    /// `exchange`, not older turns).
    pub fn with_node(mut self, component: impl Into<String>, id: impl Into<String>) -> Self {
        self.nodes.insert((component.into(), id.into()));
        self
    }

    /// Is `(component, id)` in the turn-active area?
    pub fn is_active(&self, component: &str, id: &str) -> bool {
        self.components.contains(component)
            || self
                .nodes
                .contains(&(component.to_string(), id.to_string()))
    }

    /// True when no active area was specified.
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.nodes.is_empty()
    }
}

/// Like [`document_cell_merge`], but scope advisory emission to a turn-active node-set
/// (`#msn6` / `#smturnactive`, Phase 6). The merged document and per-node
/// outcomes are IDENTICAL to [`document_cell_merge`] — the operator still wins every
/// same-node conflict, so no content is ever lost or changed. Only
/// [`DocumentCellMerge::conflict_advisories`] is filtered: a same-node conflict whose node
/// is NOT in `active` auto-resolves silently (no advisory noise), while a conflict
/// inside the active area still raises its [`MergeConflictAdvisory`].
///
/// This lets the live-prompt-drift convergence path project an advisory only when the operator's
/// concurrent edit collided with the in-flight turn's own response area, instead
/// of flagging every unrelated operator edit (queue strike, backlog tweak,
/// frontmatter flip) that happened to touch the same node the agent did.
pub fn document_cell_merge_scoped(
    base: &str,
    ours_agent: &str,
    theirs_operator: &str,
    active: &ActiveNodes,
) -> DocumentCellMerge {
    let mut sm = document_cell_merge(base, ours_agent, theirs_operator);
    sm.conflict_advisories
        .retain(|advisory| active.is_active(&advisory.component, &advisory.id));
    sm
}

// ===========================================================================
// Frontmatter
// ===========================================================================

/// Split a document into `(Some(frontmatter_block_including_fences), body)` when a
/// leading `---\n … \n---\n` block is present, else `(None, whole_doc)`.
///
/// The returned frontmatter string includes the opening and closing `---` fences
/// and the trailing newline after the closing fence (so `fm + body == doc`).
fn split_frontmatter(doc: &str) -> (Option<&str>, &str) {
    if !doc.starts_with("---\n") && doc != "---" && !doc.starts_with("---\r\n") {
        return (None, doc);
    }
    // Locate the closing fence: a line that is exactly `---`.
    let after_open = &doc["---\n".len()..];
    let mut offset = "---\n".len();
    for line in after_open.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            let end = offset + line.len();
            return (Some(&doc[..end]), &doc[end..]);
        }
        offset += line.len();
    }
    (None, doc)
}

/// One scalar key line from a frontmatter block.
#[derive(Clone, PartialEq, Eq)]
struct FmKey {
    key: String,
    value: String,
}

/// Parse a frontmatter block (including the `---` fences) into ordered scalar
/// key/value pairs. Returns `None` if any content line is not a clean
/// `key: value` scalar (signalling the caller to fall back to whole-block policy).
fn parse_frontmatter_scalars(block: &str) -> Option<Vec<FmKey>> {
    let mut out = Vec::new();
    let mut seen_open = false;
    for line in block.lines() {
        let trimmed_end = line.trim_end_matches('\r');
        if trimmed_end.trim() == "---" {
            if !seen_open {
                seen_open = true;
                continue;
            }
            // closing fence
            break;
        }
        if !seen_open {
            // Content before the open fence is unexpected — bail to whole-block.
            return None;
        }
        let t = trimmed_end.trim_end();
        if t.is_empty() {
            // Preserve blank lines as a structural marker we cannot key — bail to
            // whole-block policy to avoid reordering/losing structure.
            return None;
        }
        // A clean scalar: `key: value`, key has no leading whitespace (top-level).
        if t.starts_with(char::is_whitespace) {
            return None; // nested mapping / sequence item
        }
        let (key, value) = match t.split_once(':') {
            Some((k, v)) => (k.trim().to_string(), v.trim().to_string()),
            None => return None,
        };
        if key.is_empty() {
            return None;
        }
        out.push(FmKey { key, value });
    }
    Some(out)
}

/// Merge the three frontmatter blocks. Returns the merged block (including fences
/// and trailing newline) or `None` when no side had frontmatter.
fn merge_frontmatter_owned(
    base: Option<&str>,
    ours: Option<&str>,
    theirs: Option<&str>,
    outcomes: &mut Vec<NodeOutcome>,
    advisories: &mut Vec<MergeConflictAdvisory>,
) -> Option<String> {
    if base.is_none() && ours.is_none() && theirs.is_none() {
        return None;
    }

    let base_keys = base.and_then(parse_frontmatter_scalars);
    let ours_keys = ours.and_then(parse_frontmatter_scalars);
    let theirs_keys = theirs.and_then(parse_frontmatter_scalars);

    // Per-key scalar merge requires all present sides to parse cleanly.
    let scalar_ok = base.is_none_or(|_| base_keys.is_some())
        && ours.is_none_or(|_| ours_keys.is_some())
        && theirs.is_none_or(|_| theirs_keys.is_some());

    if !scalar_ok {
        // Conservative whole-block fallback: operator wins if it changed the block.
        let operator_changed = base != theirs;
        let chosen = if operator_changed {
            theirs
        } else {
            ours.or(theirs)
        };
        return chosen.or(base).map(|s| s.to_string());
    }

    let base_keys = base_keys.unwrap_or_default();
    let ours_keys = ours_keys.unwrap_or_default();
    let theirs_keys = theirs_keys.unwrap_or_default();

    let get = |keys: &[FmKey], k: &str| keys.iter().find(|e| e.key == k).map(|e| e.value.clone());

    // Preserve operator key order, then append agent-only keys, then base-only.
    let mut order: Vec<String> = Vec::new();
    let push_unique = |order: &mut Vec<String>, k: &str| {
        if !order.iter().any(|o| o == k) {
            order.push(k.to_string());
        }
    };
    for e in &theirs_keys {
        push_unique(&mut order, &e.key);
    }
    for e in &ours_keys {
        push_unique(&mut order, &e.key);
    }
    for e in &base_keys {
        push_unique(&mut order, &e.key);
    }

    let mut merged: Vec<FmKey> = Vec::new();
    for key in &order {
        let b = get(&base_keys, key);
        let o = get(&ours_keys, key);
        let t = get(&theirs_keys, key);

        let agent_changed = o != b;
        let operator_changed = t != b;

        let (value, outcome): (Option<String>, Option<OutcomeKind>) =
            match (b.clone(), o.clone(), t.clone()) {
                // Key dropped on both edited sides → deletion (rare for scalars).
                (Some(_), None, None) => (None, None),
                _ => {
                    if agent_changed && operator_changed {
                        if o == t {
                            (t.clone(), Some(OutcomeKind::Convergent))
                        } else {
                            // operator wins + advisory
                            advisories.push(MergeConflictAdvisory {
                                component: FRONTMATTER_COMPONENT.to_string(),
                                id: key.clone(),
                                reason: MergeConflictReason::SameNodeOperatorOverride,
                                detail: format!(
                                    "frontmatter `{key}`: operator set `{}` (agent wanted `{}`)",
                                    t.clone().unwrap_or_default(),
                                    o.clone().unwrap_or_default()
                                ),
                            });
                            (t.clone(), Some(OutcomeKind::OperatorWonConflict))
                        }
                    } else if operator_changed {
                        (
                            t.clone(),
                            Some(if b.is_none() {
                                OutcomeKind::AppliedOperatorAdd
                            } else {
                                OutcomeKind::AppliedOperatorEdit
                            }),
                        )
                    } else if agent_changed {
                        (
                            o.clone(),
                            Some(if b.is_none() {
                                OutcomeKind::AppliedAgentAdd
                            } else {
                                OutcomeKind::AppliedAgentEdit
                            }),
                        )
                    } else {
                        // unchanged on both
                        (
                            b.clone().or_else(|| t.clone()).or_else(|| o.clone()),
                            Some(OutcomeKind::Keep),
                        )
                    }
                }
            };

        if let Some(kind) = outcome {
            outcomes.push(NodeOutcome {
                component: FRONTMATTER_COMPONENT.to_string(),
                id: key.clone(),
                kind,
            });
        }
        if let Some(v) = value {
            merged.push(FmKey {
                key: key.clone(),
                value: v,
            });
        }
    }

    let mut s = String::from("---\n");
    for e in &merged {
        if e.value.is_empty() {
            s.push_str(&format!("{}:\n", e.key));
        } else {
            s.push_str(&format!("{}: {}\n", e.key, e.value));
        }
    }
    s.push_str("---\n");
    Some(s)
}

// ===========================================================================
// Components
// ===========================================================================

/// Whether two items represent the same content + flag state (i.e. unchanged).
fn item_unchanged(a: &Item, b: &Item) -> bool {
    a.text == b.text
        && a.struck == b.struck
        && a.pinned == b.pinned
        && a.agent_pinned == b.agent_pinned
        && a.kind == b.kind
}

fn find_item<'a>(comp: Option<&'a Component>, id: &str) -> Option<&'a Item> {
    comp?.items.iter().find(|i| i.id == id)
}

/// The queue id a lone unkeyed runnable component carries: the legacy
/// singleton identity (`agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID`, which
/// this crate cannot depend on). Queue maintenance rewrites
/// `<!-- agent:run -->` to `<!-- agent:run id=default -->` when a second queue
/// is created, so the two spellings name one component.
const DEFAULT_QUEUE_ID: &str = "default";

/// The identity a component is paired by across `base`, `ours`, and `theirs`.
///
/// A component is the same component on two sides when its projected overlay
/// name (`run`/`loop` -> `queue`, GH #374), its effective `id=`, and its
/// occurrence among that side's components with the same name and id all
/// agree. Pairing by name alone matched the FIRST component of a name, so in a
/// document with `agent:run id="main"` and `agent:loop id=issues` the second
/// queue was merged against the first queue's items.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ComponentIdentity {
    name: String,
    /// Explicit `id=`; `default` for a lone unkeyed queue; `None` otherwise.
    id: Option<String>,
    /// Zero-based index among this side's components sharing `name` and `id`:
    /// document order for unkeyed components, and a deterministic tiebreak for
    /// a (malformed) duplicate explicit id.
    occurrence: usize,
}

/// A component's explicit `id=` attribute. A bare `id` flag, an empty value, or
/// the literal `true` is not an id (the queue graph rejects those as malformed).
fn explicit_component_id(component: &Component) -> Option<String> {
    let attrs = agent_doc_element::element::parse_attrs(&component.attrs);
    let id = attrs.get("id")?.trim();
    (!id.is_empty() && id != "true").then(|| id.to_string())
}

/// Every component's [`ComponentIdentity`], index-aligned with `comps`.
fn component_identities(comps: &[Component]) -> Vec<ComponentIdentity> {
    let queue_count = comps.iter().filter(|c| c.name == "queue").count();
    let mut seen: std::collections::HashMap<(String, Option<String>), usize> =
        std::collections::HashMap::new();
    comps
        .iter()
        .map(|c| {
            let id = explicit_component_id(c).or_else(|| {
                (c.name == "queue" && queue_count == 1).then(|| DEFAULT_QUEUE_ID.to_string())
            });
            let slot = seen.entry((c.name.clone(), id.clone())).or_insert(0);
            let occurrence = *slot;
            *slot += 1;
            ComponentIdentity {
                name: c.name.clone(),
                id,
                occurrence,
            }
        })
        .collect()
}

/// Which `base` and `ours` component each `theirs` component pairs with, and
/// which `ours` components have no `theirs` counterpart (agent-only).
///
/// Pairing is exact on [`ComponentIdentity`]: it never falls back to a
/// same-name component with a different id, so an unpaired component is
/// classified on its own (operator-only or agent-only) rather than merged
/// against a guessed counterpart.
struct ComponentPairing {
    /// Per `theirs` index: `(base index, ours index)`.
    theirs: Vec<(Option<usize>, Option<usize>)>,
    /// Per `ours` index: the paired `base` index.
    ours_base: Vec<Option<usize>>,
    /// Per `ours` index: whether a `theirs` component pairs with it.
    ours_paired: Vec<bool>,
}

impl ComponentPairing {
    fn new(base: &[Component], ours: &[Component], theirs: &[Component]) -> Self {
        let base_ids = component_identities(base);
        let ours_ids = component_identities(ours);
        let theirs_ids = component_identities(theirs);
        let index = |ids: &[ComponentIdentity], key: &ComponentIdentity| {
            ids.iter().position(|candidate| candidate == key)
        };
        let theirs_pairs: Vec<(Option<usize>, Option<usize>)> = theirs_ids
            .iter()
            .map(|key| (index(&base_ids, key), index(&ours_ids, key)))
            .collect();
        let mut ours_paired = vec![false; ours.len()];
        for (_, ours_idx) in &theirs_pairs {
            if let Some(i) = ours_idx {
                ours_paired[*i] = true;
            }
        }
        let ours_base = ours_ids.iter().map(|key| index(&base_ids, key)).collect();
        ComponentPairing {
            theirs: theirs_pairs,
            ours_base,
            ours_paired,
        }
    }
}

/// The raw bullet line for an item, reconstructed from `Item.raw`. We re-emit a
/// `- ` bullet to keep a stable, parseable shape (the overlay strips the bullet
/// into `raw`, so we restore one). Backlog tasks already carry their `N.`/`[ ]`
/// in `raw`; we still prefix `- ` only when `raw` does not already begin with a
/// list/ordered bullet shape, to avoid double bullets.
fn item_line(item: &Item) -> String {
    // A multiline queue prompt (title + fenced block between `---` rules) is
    // not a list item: its `raw` is the complete block, emitted verbatim.
    if item.surface == ItemSurface::MultilinePrompt {
        return item.raw.trim_end_matches(['\n', '\r']).to_string();
    }
    let raw = item.raw.trim();
    format!("- {raw}")
}

fn note_snippet(text: &str) -> String {
    let collapsed = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('`', "'");
    const MAX: usize = 220;
    if collapsed.chars().count() <= MAX {
        collapsed
    } else {
        let mut s = collapsed.chars().take(MAX).collect::<String>();
        s.push_str("...");
        s
    }
}

fn same_node_conflict_detail(
    component: &str,
    id: &str,
    operator_text: &str,
    agent_text: &str,
) -> String {
    format!(
        "{component} `#{id}` had concurrent edits; operator version kept: \"{}\"; agent version not merged: \"{}\"",
        note_snippet(operator_text),
        note_snippet(agent_text)
    )
}

/// Rebuild a single component's inner item lines per the transition outcomes,
/// returning the lines (without the open/close markers) joined by `\n`.
#[allow(clippy::too_many_arguments)]
fn merge_component_items(
    name: &str,
    base_comp: Option<&Component>,
    ours_comp: Option<&Component>,
    theirs_comp: &Component,
    outcomes: &mut Vec<NodeOutcome>,
    advisories: &mut Vec<MergeConflictAdvisory>,
) -> Vec<String> {
    merge_component_item_slots(name, base_comp, ours_comp, theirs_comp, outcomes, advisories)
        .into_iter()
        .map(|(_, line)| line)
        .collect()
}

/// [`merge_component_items`] with each merged line tagged by the index of the
/// operator item it replaces (`None` for an agent-only add), so the body
/// rebuild can put every merged node back at its operator position.
#[allow(clippy::too_many_arguments)]
fn merge_component_item_slots(
    name: &str,
    base_comp: Option<&Component>,
    ours_comp: Option<&Component>,
    theirs_comp: &Component,
    outcomes: &mut Vec<NodeOutcome>,
    advisories: &mut Vec<MergeConflictAdvisory>,
) -> Vec<(Option<usize>, String)> {
    let mut lines: Vec<(Option<usize>, String)> = Vec::new();

    // 1. Walk operator items in operator order — they are the layout skeleton.
    for (t_index, t_item) in theirs_comp.items.iter().enumerate() {
        let id = &t_item.id;
        let b_item = find_item(base_comp, id);
        let o_item = find_item(ours_comp, id);

        let operator_changed = match b_item {
            Some(b) => !item_unchanged(t_item, b),
            None => true, // operator-added
        };

        match (b_item, o_item) {
            // Present in base.
            (Some(b), Some(o)) => {
                let agent_changed = !item_unchanged(o, b);
                if agent_changed && operator_changed {
                    if o.text == t_item.text
                        && o.struck == t_item.struck
                        && o.pinned == t_item.pinned
                        && o.agent_pinned == t_item.agent_pinned
                    {
                        // Convergent — both reached the same shape.
                        lines.push((Some(t_index), item_line(t_item)));
                        record(outcomes, name, id, OutcomeKind::Convergent);
                    } else {
                        // Same-node conflict — operator wins + advisory.
                        lines.push((Some(t_index), item_line(t_item)));
                        record(outcomes, name, id, OutcomeKind::OperatorWonConflict);
                        advisories.push(MergeConflictAdvisory {
                            component: name.to_string(),
                            id: id.clone(),
                            reason: MergeConflictReason::SameNodeOperatorOverride,
                            detail: same_node_conflict_detail(
                                name,
                                id,
                                &item_line(t_item),
                                &item_line(o),
                            ),
                        });
                    }
                } else if agent_changed {
                    // Agent edited, operator unchanged.
                    // Detect agent-delete-via-strike vs operator-edit handled below;
                    // here operator is unchanged so just apply the agent edit.
                    lines.push((Some(t_index), item_line(o)));
                    record(outcomes, name, id, OutcomeKind::AppliedAgentEdit);
                } else if operator_changed {
                    // Operator edited, agent unchanged.
                    lines.push((Some(t_index), item_line(t_item)));
                    record(outcomes, name, id, OutcomeKind::AppliedOperatorEdit);
                } else {
                    // Unchanged on both.
                    lines.push((Some(t_index), item_line(t_item)));
                    record(outcomes, name, id, OutcomeKind::Keep);
                }
            }
            // Present in base, absent in agent → agent deleted it.
            (Some(b), None) => {
                if operator_changed {
                    // Agent deleted (absent), operator edited → operator revives.
                    let _ = b;
                    lines.push((Some(t_index), item_line(t_item)));
                    record(outcomes, name, id, OutcomeKind::OperatorRevived);
                    advisories.push(MergeConflictAdvisory {
                        component: name.to_string(),
                        id: id.clone(),
                        reason: MergeConflictReason::OperatorRevivedAgentDeletedNode,
                        detail: format!(
                            "{name} `{id}`: agent removed it but operator edited it — operator's version kept (revived)"
                        ),
                    });
                } else {
                    // Agent deleted, operator unchanged → honor the deletion.
                    // Omit the line; record as an applied agent edit (deletion).
                    record(outcomes, name, id, OutcomeKind::AppliedAgentEdit);
                }
            }
            // Absent in base, present in operator → operator-added.
            (None, Some(_o)) => {
                // Added by both. Same id.
                if find_item(ours_comp, id).map(|o| &o.text) == Some(&t_item.text) {
                    lines.push((Some(t_index), item_line(t_item)));
                    record(outcomes, name, id, OutcomeKind::AppliedBothAdd);
                } else {
                    // Same id, different text → operator wins + advisory.
                    lines.push((Some(t_index), item_line(t_item)));
                    record(outcomes, name, id, OutcomeKind::OperatorWonConflict);
                    advisories.push(MergeConflictAdvisory {
                        component: name.to_string(),
                        id: id.clone(),
                        reason: MergeConflictReason::SameNodeOperatorOverride,
                        detail: same_node_conflict_detail(
                            name,
                            id,
                            &item_line(t_item),
                            find_item(ours_comp, id)
                                .map(item_line)
                                .as_deref()
                                .unwrap_or(""),
                        ),
                    });
                }
            }
            // Absent in base, absent in agent → operator-only add.
            (None, None) => {
                lines.push((Some(t_index), item_line(t_item)));
                record(outcomes, name, id, OutcomeKind::AppliedOperatorAdd);
            }
        }
    }

    // 2. Agent-only nodes: present in agent, absent in operator. Decide between an
    //    agent add (absent in base) and an operator deletion (present in base).
    if let Some(ours) = ours_comp {
        for o_item in &ours.items {
            let id = &o_item.id;
            if theirs_comp.items.iter().any(|t| &t.id == id) {
                continue; // already handled in the operator walk
            }
            let b_item = find_item(base_comp, id);
            match b_item {
                None => {
                    // Absent in base & operator, present in agent → agent add.
                    lines.push((None, item_line(o_item)));
                    record(outcomes, name, id, OutcomeKind::AppliedAgentAdd);
                }
                Some(b) => {
                    // Present in base, absent in operator → operator deleted it.
                    let agent_edited = !item_unchanged(o_item, b);
                    if agent_edited {
                        // Deletion stands + advisory (operator deleted an agent-edited node).
                        record(outcomes, name, id, OutcomeKind::DeletionKept);
                        advisories.push(MergeConflictAdvisory {
                            component: name.to_string(),
                            id: id.clone(),
                            reason: MergeConflictReason::OperatorDeletedAgentEditedNode,
                            detail: format!(
                                "{name} `{id}`: operator deleted the node the agent edited — deletion stands"
                            ),
                        });
                    } else {
                        // Operator deleted an unchanged node → deletion stands, no advisory.
                        record(outcomes, name, id, OutcomeKind::DeletionKept);
                    }
                }
            }
        }
    }

    lines
}

/// The exchange component name (response-turn component).
const EXCHANGE_COMPONENT: &str = "exchange";

/// One `### ` heading-block from an exchange body: the heading line plus the
/// following prose/blockquote lines up to (but not including) the next `### `
/// heading or end of buffer.
struct HeadingBlock {
    /// Normalized identity key (strike wrapper + trailing ` (HEAD)` stripped).
    key: String,
    /// The verbatim lines of the block (heading line first, then its body),
    /// each retaining its trailing newline as captured.
    lines: Vec<String>,
}

/// Is `trimmed` an h3 heading line (`### …`)? Mirrors the exchange turn shape.
fn is_h3_heading(trimmed: &str) -> bool {
    trimmed.starts_with("### ") || trimmed == "###"
}

/// Normalize a `### ` heading line into a stable turn-identity key: strip the
/// leading `### ` prefix, a surrounding `~~…~~` strike wrapper, and a trailing
/// ` (HEAD)` boundary annotation (transient — must not affect identity).
fn normalize_heading_key(trimmed: &str) -> String {
    let body = trimmed.strip_prefix("###").unwrap_or(trimmed).trim();
    let mut t = body.trim();
    // Strip a surrounding strike wrapper.
    if t.len() >= 4 && t.starts_with("~~") && t.ends_with("~~") {
        t = t[2..t.len() - 2].trim();
    }
    // Strip a trailing ` (HEAD)` boundary annotation.
    if let Some(stripped) = t.strip_suffix("(HEAD)") {
        t = stripped.trim_end();
    }
    t.to_string()
}

/// Split a sequence of exchange inner lines into `(leading_lines, blocks)` where
/// `leading_lines` is any content before the first `### ` heading and `blocks` is
/// the ordered list of heading-keyed turn blocks.
fn split_heading_blocks(lines: &[String]) -> (Vec<String>, Vec<HeadingBlock>) {
    let mut leading: Vec<String> = Vec::new();
    let mut blocks: Vec<HeadingBlock> = Vec::new();
    let mut current: Option<HeadingBlock> = None;

    for line in lines {
        let trimmed = line.trim_end_matches(['\n', '\r']).trim();
        if is_h3_heading(trimmed) {
            if let Some(b) = current.take() {
                blocks.push(b);
            }
            current = Some(HeadingBlock {
                key: normalize_heading_key(trimmed),
                lines: vec![line.clone()],
            });
        } else if let Some(b) = current.as_mut() {
            b.lines.push(line.clone());
        } else {
            leading.push(line.clone());
        }
    }
    if let Some(b) = current.take() {
        blocks.push(b);
    }
    (leading, blocks)
}

/// Merge the `exchange` component's inner body.
///
/// Exchange response turns are append-only `### Re:` heading-prose blocks, not
/// list items, so the standard item merge cannot reconstruct them. This function
/// preserves theirs' inner content verbatim (its skeleton) and appends any
/// `### ` heading-blocks present in `ours_agent` but absent in `theirs_operator`
/// (keyed by [`normalize_heading_key`]), inserted before a trailing
/// `<!-- agent:boundary:… -->` marker if one is present. When neither side uses
/// heading-prose turns it falls back to the standard list-item merge so the
/// existing bullet-based exchange behavior is unchanged.
#[allow(clippy::too_many_arguments)]
fn merge_exchange_inner(
    theirs_inner: &[String],
    base_comp: Option<&Component>,
    ours_comp: Option<&Component>,
    theirs_comp: Option<&Component>,
    ours_source: &str,
    outcomes: &mut Vec<NodeOutcome>,
    advisories: &mut Vec<MergeConflictAdvisory>,
) -> String {
    let (theirs_leading, theirs_blocks) = split_heading_blocks(theirs_inner);

    // The overlay only models bullet items, so heading-prose `### Re:` turns are
    // invisible to it (and the overlay's `end_byte` is unreliable for an
    // item-less component). Recover ours' raw inner lines with a direct line scan
    // of the agent source between the exchange open and close markers. (`ours_comp`
    // is still used below for the bullet-only fallback path.)
    let ours_inner: Vec<String> = component_inner_lines(EXCHANGE_COMPONENT, ours_source);
    let (_ours_leading, ours_blocks) = split_heading_blocks(&ours_inner);

    let uses_heading_turns = !theirs_blocks.is_empty() || !ours_blocks.is_empty();

    if !uses_heading_turns {
        // No heading-prose turns on either side — delegate to the list-item merge
        // so the existing bullet-based exchange behavior is preserved exactly.
        let mut out = String::new();
        if let Some(tc) = theirs_comp {
            let merged = merge_component_items(
                EXCHANGE_COMPONENT,
                base_comp,
                ours_comp,
                tc,
                outcomes,
                advisories,
            );
            for ml in &merged {
                out.push_str(ml);
                out.push('\n');
            }
        }
        return out;
    }

    // Heading-prose regime. Preserve theirs' inner verbatim, then append agent-new
    // heading-blocks before a trailing boundary marker (if present), else at end.
    //
    // `#qcellmerge-response-body-id`: identity is heading **and** body, not heading
    // alone. A fresh agent response whose heading coincides with a prior turn (a
    // same-topic / same-preset follow-up drained from the queue) has a different
    // body, so it is a genuinely new turn that must be appended — keying only on
    // the heading made it a false duplicate and the merge dropped it ("a cell-merge
    // chose the existing content"). Byte-identical re-appends still match (same
    // heading + body) and are still filtered, so idempotent redelivery never
    // duplicates a turn.
    let theirs_ids: std::collections::HashSet<String> = theirs_blocks
        .iter()
        .map(|b| agent_doc_markdown_ast::exchange_tree::response_identity_digest(&b.lines))
        .collect();

    // Determine the insertion point: theirs' inner is `leading` + each block's
    // lines. A trailing `<!-- agent:boundary:… -->` marker line lives at the very
    // end (in theirs' leading lines if there are no blocks, or in the last
    // block's trailing lines). We rebuild the inner explicitly to control where
    // the boundary marker sits relative to the appended blocks.
    let agent_new: Vec<&HeadingBlock> = ours_blocks
        .iter()
        .filter(|b| {
            !theirs_ids.contains(
                &agent_doc_markdown_ast::exchange_tree::response_identity_digest(&b.lines),
            )
        })
        .collect();

    // Flatten theirs' inner back to its line vector, then split off a trailing
    // boundary marker line (and any trailing blank lines that follow it) so the
    // appended turns land before it.
    let mut flat: Vec<String> = Vec::new();
    flat.extend(theirs_leading.iter().cloned());
    for b in &theirs_blocks {
        flat.extend(b.lines.iter().cloned());
    }

    // Find the last boundary-marker line index, if any.
    let boundary_idx = flat
        .iter()
        .rposition(|l| is_boundary_marker(l.trim_end_matches(['\n', '\r']).trim()));

    // Build the appended-turns text once, recording an `AppliedAgentAdd` per turn.
    // A trailing boundary-marker line that the block scan swept into the agent's
    // last turn belongs to the structural skeleton (theirs already carries it),
    // so it is stripped here to avoid emitting a duplicate boundary marker.
    let mut appended = String::new();
    for b in &agent_new {
        record(
            outcomes,
            EXCHANGE_COMPONENT,
            &b.key,
            OutcomeKind::AppliedAgentAdd,
        );
        let mut block_lines: &[String] = &b.lines;
        while let Some(last) = block_lines.last() {
            let t = last.trim_end_matches(['\n', '\r']).trim();
            if t.is_empty() || is_boundary_marker(t) {
                block_lines = &block_lines[..block_lines.len() - 1];
            } else {
                break;
            }
        }
        for l in block_lines {
            appended.push_str(l);
            if !l.ends_with('\n') {
                appended.push('\n');
            }
        }
    }

    let mut out = String::new();
    match boundary_idx {
        Some(idx) => {
            // Emit everything before the boundary line, then the appended turns,
            // then the boundary line and any trailing content verbatim.
            for l in &flat[..idx] {
                push_exchange_segment(&mut out, l);
            }
            push_exchange_segment(&mut out, &appended);
            for l in &flat[idx..] {
                push_exchange_segment(&mut out, l);
            }
        }
        None => {
            for l in &flat {
                push_exchange_segment(&mut out, l);
            }
            push_exchange_segment(&mut out, &appended);
        }
    }
    // `#exchangeconverge`: with `document_cell_merge_enabled` (default ON) this is the
    // immediate replacement for the non-convergent heading-turn merge — collapse
    // any mirror-ordered duplicate response/prompt identities a prior poisoned
    // buffer carries, so the merge output converges instead of accreting
    // duplicates across cycles (the runaway-growth wedge). Keyed on
    // `ExchangeNode::node_id`, so only true duplicates collapse; idempotent and
    // byte-for-byte verbatim when there is nothing to collapse. The kill-switch
    // (`document_cell_merge_enabled = false`) restores the prior pass-through behavior.
    if crate::document_cell::document_cell_merge_enabled() {
        crate::exchange_node_merge::dedup_exchange_nodes_by_identity(&out)
    } else {
        out
    }
}

/// Join distinct exchange segments without allowing one Markdown node to absorb
/// the next when an upstream projection omitted a trailing line terminator.
fn push_exchange_segment(out: &mut String, segment: &str) {
    if segment.is_empty() {
        return;
    }
    if !out.is_empty()
        && !out.ends_with('\n')
        && !segment.starts_with('\n')
        && !segment.starts_with('\r')
    {
        out.push('\n');
    }
    out.push_str(segment);
}

/// Recognize an `<!-- agent:boundary:HASH -->` marker line.
fn is_boundary_marker(trimmed: &str) -> bool {
    let Some(inner) = trimmed
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
    else {
        return false;
    };
    inner.trim().starts_with("agent:boundary:")
}

/// Reconstruct a named component's inner raw lines (everything between its open
/// and close markers, exclusive) with a direct line scan of `source`.
///
/// This deliberately does not use the overlay's `Component` byte span: the
/// overlay only parses bullet items into nodes, so an exchange component made of
/// heading-prose turns (no items) records an unreliable `end_byte`. A line scan
/// recovers the verbatim inner content the heading-block append needs. Marker
/// lines are classified by the overlay's shared fence-aware
/// [`ComponentMarkerScan`] (`#fencedmarkerscan`): a response that quotes a
/// fenced `<!-- /agent:exchange -->` line does not end the exchange early.
fn component_inner_lines(name: &str, source: &str) -> Vec<String> {
    let markers = ComponentMarkerScan::new(source);
    let mut lines: Vec<String> = Vec::new();
    let mut inside = false;
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let content = line.trim_end_matches(['\n', '\r']);
        if !inside {
            if markers.open_at(line_start, content).map(|(raw, _)| raw).as_deref() == Some(name)
            {
                inside = true;
            }
            continue;
        }
        if markers.close_at(line_start, content).as_deref() == Some(name) {
            break;
        }
        lines.push(line.to_string());
    }
    lines
}

fn record(outcomes: &mut Vec<NodeOutcome>, comp: &str, id: &str, kind: OutcomeKind) {
    outcomes.push(NodeOutcome {
        component: comp.to_string(),
        id: id.to_string(),
        kind,
    });
}

/// Reconstruct a non-`exchange` component's inner body from its buffered operator
/// lines (`inner`, each still carrying its trailing `\n`) and the merged item set.
///
/// `#qdup-freetext` — root cause of the persistent `live_prompt_drift` churn: a
/// queue head that is multi-line *free text* (e.g. a pasted-console bug report: a
/// `:pushpin:` line plus a fenced code block, between `---` rules) is not a `- `
/// list item, so the overlay never parses it as an [`Item`]. The previous
/// reconstruction emitted only the bullet items and dropped every other inner
/// line, so the semantic merge silently lost that free-text head, tripped the
/// `dropped_queue_prompt_lines_after_content_ours` anti-data-loss gate, and
/// declined the merge on every cycle — leaving every IPC write stuck on the
/// blocked `content_ours` carry-forward path.
///
/// Preserve operator non-item lines (free text, fenced blocks, separators,
/// blanks) verbatim and replace each operator item node in place, at its own
/// source span, with its merged line (`#fencedmarkerscan`). An operator item
/// the merge deleted leaves nothing; agent-only adds follow the last operator
/// item, or are appended when the operator component has no items.
///
/// `inner_start` is the operator-document byte offset of the first inner line,
/// the same coordinate space as `theirs_items` spans. Splicing by span (rather
/// than emitting every merged item at the first `- ` bullet and keeping all
/// non-bullet lines) keeps a multiline queue prompt (a title plus a fenced
/// block, an overlay item since `#multilineheadbullet`) from being emitted
/// twice, and keeps rows on both sides of a pasted fence in their positions.
fn merge_nonexchange_inner(
    inner: &[String],
    inner_start: usize,
    theirs_items: &[Item],
    slots: &[(Option<usize>, String)],
) -> String {
    let text = inner.concat();
    let inner_end = inner_start + text.len();
    let mut spans: Vec<(usize, usize, usize)> = theirs_items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.start_byte >= inner_start && item.end_byte <= inner_end)
        .map(|(index, item)| (index, item.start_byte - inner_start, item.end_byte - inner_start))
        .collect();
    spans.sort_by_key(|&(_, start, _)| start);
    let push_line = |out: &mut String, line: &str| {
        out.push_str(line);
        out.push('\n');
    };
    let mut out = String::new();
    let mut cursor = 0usize;
    for (position, &(index, start, end)) in spans.iter().enumerate() {
        if start < cursor {
            continue;
        }
        out.push_str(&text[cursor..start]);
        let span = &text[start..end];
        for (_, line) in slots.iter().filter(|(slot, _)| *slot == Some(index)) {
            if span.ends_with('\n') {
                push_line(&mut out, line);
            } else {
                out.push_str(line);
            }
        }
        if position + 1 == spans.len() {
            for (_, line) in slots.iter().filter(|(slot, _)| slot.is_none()) {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                push_line(&mut out, line);
            }
        }
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    if spans.is_empty() {
        for (_, line) in slots {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            push_line(&mut out, line);
        }
    }
    out
}

/// Rebuild the operator body, replacing each component's inner items with the
/// merged item set, and appending any agent-only components at the end.
///
/// Line-scanning: the operator body is the layout skeleton. Non-component lines
/// pass through verbatim. When an open marker is seen, all lines up to the close
/// marker are dropped and replaced by the merged item lines for that component.
///
/// Each opener is resolved to the overlay component that starts on that line
/// (`theirs_body_offset` is where `theirs_body` begins in the operator source),
/// and that component is merged against its [`ComponentPairing`] counterparts,
/// never against the first same-name component on the other sides.
#[allow(clippy::too_many_arguments)]
fn merge_components_into_body(
    theirs_body: &str,
    theirs_body_offset: usize,
    theirs_comps: &[Component],
    base_comps: &[Component],
    ours_comps: &[Component],
    ours_source: &str,
    outcomes: &mut Vec<NodeOutcome>,
    advisories: &mut Vec<MergeConflictAdvisory>,
) -> String {
    let pairing = ComponentPairing::new(base_comps, ours_comps, theirs_comps);
    let sides = MergeSides {
        pairing: &pairing,
        theirs_comps,
        base_comps,
        ours_comps,
        ours_source,
    };
    let mut out = String::new();
    // The currently-open component: its raw tag name (matched against the close
    // marker) and the index of the overlay component that opens on that line.
    let mut open: Option<(String, Option<usize>)> = None;
    // The inner lines of the currently-open component are buffered verbatim so the
    // reconstruction can preserve operator content the item model cannot represent:
    // `### Re:` heading-prose turns in `exchange`, and free-text / fenced / separator
    // lines in any other component (e.g. a multi-line pasted-console queue head).
    // At the close marker the buffer is rewritten — by [`merge_exchange_inner`] for
    // `exchange`, or [`merge_nonexchange_inner`] elsewhere.
    let mut component_inner: Vec<String> = Vec::new();
    // Operator-document offset of the open component's first inner line.
    let mut inner_start = theirs_body_offset;
    let mut offset = theirs_body_offset;
    // `#fencedmarkerscan`: marker lines inside fenced/indented code are
    // content. The scan is built over the body, so it is indexed by
    // body-relative offsets; `theirs_comps` use whole-document offsets.
    let markers = ComponentMarkerScan::new(theirs_body);

    for line in theirs_body.split_inclusive('\n') {
        let line_start = offset;
        let body_line_start = offset - theirs_body_offset;
        offset += line.len();
        let content = line.trim_end_matches('\n');

        if let Some((name, theirs_idx)) = open.as_ref() {
            // Inside a component: look for its close marker.
            if let Some(close) = markers.close_at(body_line_start, content)
                && &close == name
            {
                let merged = sides.merge_component(
                    *theirs_idx,
                    &component_inner,
                    inner_start,
                    outcomes,
                    advisories,
                );
                out.push_str(&merged);
                component_inner.clear();
                out.push_str(line); // close marker line, verbatim
                open = None;
            } else {
                // Buffer the inner line verbatim for the reconstruction pass.
                component_inner.push(line.to_string());
            }
            continue;
        }

        // Not inside a component.
        if let Some((name, _)) = markers.open_at(body_line_start, content) {
            out.push_str(line); // open marker line, verbatim
            let theirs_idx = theirs_comps.iter().position(|c| c.start_byte == line_start);
            open = Some((name, theirs_idx));
            inner_start = offset;
            continue;
        }
        // Plain prose line — pass through.
        out.push_str(line);
    }

    // An unterminated operator component (no recognized close) — flush its items.
    if let Some((_, theirs_idx)) = open.take() {
        let merged = sides.merge_component(
            theirs_idx,
            &component_inner,
            inner_start,
            outcomes,
            advisories,
        );
        if !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&merged);
    }

    // Append agent-only components (no operator counterpart by identity).
    for (ours_idx, oc) in ours_comps.iter().enumerate() {
        if pairing.ours_paired[ours_idx] {
            continue;
        }
        // Every item in an agent-only component is an agent add (or revival of a
        // base node the operator dropped along with the component). Classify per id.
        let base_comp = pairing.ours_base[ours_idx].map(|i| &base_comps[i]);
        let mut lines = Vec::new();
        for item in &oc.items {
            let kind = if find_item(base_comp, &item.id).is_some() {
                OutcomeKind::AppliedAgentEdit
            } else {
                OutcomeKind::AppliedAgentAdd
            };
            record(outcomes, &oc.name, &item.id, kind);
            lines.push(item_line(item));
        }
        if !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("\n<!-- agent:{}", oc.name));
        if !oc.attrs.is_empty() {
            out.push(' ');
            out.push_str(&oc.attrs);
        }
        out.push_str(" -->\n");
        for l in &lines {
            out.push_str(l);
            out.push('\n');
        }
        out.push_str(&format!("<!-- /agent:{} -->\n", oc.name));
    }

    out
}

/// The three parsed sides of a body merge plus their component pairing.
struct MergeSides<'a> {
    pairing: &'a ComponentPairing,
    theirs_comps: &'a [Component],
    base_comps: &'a [Component],
    ours_comps: &'a [Component],
    ours_source: &'a str,
}

impl MergeSides<'_> {
    /// Merge the inner body of the operator component at `theirs_idx` against
    /// its paired base and agent components.
    ///
    /// `None` means the overlay recognizes no component at that opener (the
    /// body scan is a plain line scan; the overlay also skips markers inside
    /// code), so there is nothing to pair: the operator's inner lines are kept
    /// verbatim rather than merged against a same-name component elsewhere.
    fn merge_component(
        &self,
        theirs_idx: Option<usize>,
        inner: &[String],
        inner_start: usize,
        outcomes: &mut Vec<NodeOutcome>,
        advisories: &mut Vec<MergeConflictAdvisory>,
    ) -> String {
        let Some(theirs_idx) = theirs_idx else {
            return inner.concat();
        };
        let tc = &self.theirs_comps[theirs_idx];
        let (base_idx, ours_idx) = self.pairing.theirs[theirs_idx];
        let base_comp = base_idx.map(|i| &self.base_comps[i]);
        let ours_comp = ours_idx.map(|i| &self.ours_comps[i]);
        if tc.name == EXCHANGE_COMPONENT {
            // Exchange: preserve theirs' inner lines verbatim, then append any
            // `### Re:` heading-blocks that exist only in the agent body.
            // The heading-turn scan reads the agent source from the PAIRED
            // agent exchange's opener, never the first exchange by name; with
            // no paired agent exchange there are no agent turns to append.
            let ours_exchange_source = ours_comp.map_or("", |c| &self.ours_source[c.start_byte..]);
            merge_exchange_inner(
                inner,
                base_comp,
                ours_comp,
                Some(tc),
                ours_exchange_source,
                outcomes,
                advisories,
            )
        } else {
            // `#qdup-freetext`: rebuild the component preserving operator
            // non-bullet inner content (free-text heads, fenced blocks,
            // separators) verbatim while replacing the bullet-item region with
            // the merged item set. Dropping that content used to make the merge
            // silently lose a free-text queue head, trip the anti-data-loss
            // gate, and decline forever (the persistent `live_prompt_drift`
            // churn).
            let slots = merge_component_item_slots(
                &tc.name, base_comp, ours_comp, tc, outcomes, advisories,
            );
            merge_nonexchange_inner(inner, inner_start, &tc.items, &slots)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_reason_tokens_are_stable() {
        // These tokens are the stable cycle-local conflict projection format.
        // cycle_state persists and preflight surfaces — keep them stable.
        assert_eq!(
            MergeConflictReason::OperatorDeletedAgentEditedNode.token(),
            "operator_deleted_agent_edited_node"
        );
        assert_eq!(
            MergeConflictReason::SameNodeOperatorOverride.token(),
            "same_node_operator_override"
        );
        assert_eq!(
            MergeConflictReason::OperatorRevivedAgentDeletedNode.token(),
            "operator_revived_agent_deleted_node"
        );
    }

    // ----- helpers ---------------------------------------------------------

    fn q(items: &str) -> String {
        format!("<!-- agent:queue -->\n{items}<!-- /agent:queue -->\n")
    }

    fn outcome_for<'a>(m: &'a DocumentCellMerge, id: &str) -> Option<&'a NodeOutcome> {
        m.outcomes.iter().find(|o| o.id == id)
    }

    fn reparses_to_ids(doc: &str, comp: &str) -> Vec<String> {
        components(doc)
            .into_iter()
            .find(|c| c.name == comp)
            .map(|c| c.items.into_iter().map(|i| i.id).collect())
            .unwrap_or_default()
    }

    /// GH #374: the runnable component merges by node identity whatever its
    /// spelling or anchor. The body scan matched the raw tag name (`run`)
    /// against overlay components already projected to `queue`, so an
    /// `agent:run` merge emitted an EMPTY component (every item dropped); and a
    /// heading-anchored opener was not recognized at all, so the agent's strike
    /// was silently replaced by the operator copy.
    #[test]
    fn runnable_component_merges_identically_for_every_spelling_and_anchor() {
        for (open, close) in [
            ("<!-- agent:queue -->", "<!-- /agent:queue -->"),
            ("<!-- agent:run -->", "<!-- /agent:run -->"),
            ("## Run <!-- agent:run priority -->", "<!-- /agent:run -->"),
            ("## Queue <!-- agent:queue -->", "<!-- /agent:queue -->"),
        ] {
            let doc = |items: &str| format!("# T\n\n{open}\n{items}{close}\n");
            let base = doc("- do [#a] task\n");
            let ours = doc("- ~~do [#a] task~~\n");
            let theirs = doc("- do [#a] task\n- do [#b] next\n");

            let m = document_cell_merge(&base, &ours, &theirs);

            assert_eq!(
                m.merged_doc,
                doc("- ~~do [#a] task~~\n- do [#b] next\n"),
                "{open}"
            );
            assert!(
                !m.outcomes.is_empty() && m.outcomes.iter().all(|o| o.component == "queue"),
                "{open}: {:?}",
                m.outcomes
            );
        }
    }

    /// The two runnable components of the live `agent-doc-bugs.md` shape.
    fn two_queue_doc(main: &str, issues: &str) -> String {
        format!(
            "# T\n\n<!-- agent:run id=\"main\" -->\n{main}<!-- /agent:run -->\n\n\
             <!-- agent:loop id=issues -->\n{issues}<!-- /agent:loop -->\n"
        )
    }

    /// Item ids per runnable component, keyed by its `id=`, in document order.
    fn queue_ids_by_component_id(doc: &str) -> Vec<(String, Vec<String>)> {
        components(doc)
            .into_iter()
            .filter(|c| c.name == "queue")
            .map(|c| {
                (
                    explicit_component_id(&c).unwrap_or_default(),
                    c.items.into_iter().map(|i| i.id).collect(),
                )
            })
            .collect()
    }

    /// Multi-queue documents pair components by `id=`, not by name. Pairing by
    /// projected name took the FIRST `queue` on every side, so the `issues`
    /// loop was merged against `main`'s items: the agent's `issues` strike and
    /// add were measured against the wrong base, and the shared item id `#a`
    /// let one queue's edit land on the other.
    #[test]
    fn two_runnable_components_merge_their_items_independently() {
        let base = two_queue_doc("- do [#a] main task\n", "- do [#a] issue task\n");
        // Agent: strike main's #a, add #c to issues.
        let ours = two_queue_doc(
            "- ~~do [#a] main task~~\n",
            "- do [#a] issue task\n- do [#c] agent issue\n",
        );
        // Operator: add #b to main, edit issues' #a.
        let theirs = two_queue_doc(
            "- do [#a] main task\n- do [#b] operator main\n",
            "- do [#a] issue task, edited\n",
        );

        let m = document_cell_merge(&base, &ours, &theirs);

        assert_eq!(
            m.merged_doc,
            two_queue_doc(
                "- ~~do [#a] main task~~\n- do [#b] operator main\n",
                "- do [#a] issue task, edited\n- do [#c] agent issue\n",
            ),
        );
        assert!(
            m.conflict_advisories.is_empty(),
            "{:?}",
            m.conflict_advisories
        );
        let kinds: Vec<(&str, &OutcomeKind)> = m
            .outcomes
            .iter()
            .map(|o| (o.id.as_str(), &o.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("a", &OutcomeKind::AppliedAgentEdit),
                ("b", &OutcomeKind::AppliedOperatorAdd),
                ("a", &OutcomeKind::AppliedOperatorEdit),
                ("c", &OutcomeKind::AppliedAgentAdd),
            ],
        );
        assert_eq!(
            queue_ids_by_component_id(&m.merged_doc),
            vec![
                ("main".to_string(), vec!["a".to_string(), "b".to_string()]),
                ("issues".to_string(), vec!["a".to_string(), "c".to_string()]),
            ],
        );
    }

    /// An `id=` pairs a component wherever it moved: the operator reordered the
    /// two queues while the agent edited both, and each agent edit lands on its
    /// own component in the operator's new order.
    #[test]
    fn id_keyed_components_pair_across_an_operator_reorder() {
        let base = two_queue_doc("- do [#m] main\n", "- do [#i] issue\n");
        let ours = two_queue_doc(
            "- ~~do [#m] main~~\n",
            "- do [#i] issue\n- do [#j] new issue\n",
        );
        let theirs = "# T\n\n<!-- agent:loop id=issues -->\n- do [#i] issue\n<!-- /agent:loop -->\n\n\
                      <!-- agent:run id=\"main\" -->\n- do [#m] main\n<!-- /agent:run -->\n";

        let m = document_cell_merge(&base, &ours, theirs);

        assert_eq!(
            m.merged_doc,
            "# T\n\n<!-- agent:loop id=issues -->\n- do [#i] issue\n- do [#j] new issue\n<!-- /agent:loop -->\n\n\
             <!-- agent:run id=\"main\" -->\n- ~~do [#m] main~~\n<!-- /agent:run -->\n",
        );
        assert!(
            m.conflict_advisories.is_empty(),
            "{:?}",
            m.conflict_advisories
        );
    }

    /// A component never pairs with a same-name component of a different `id=`:
    /// with no counterpart it is classified on its own instead of being merged
    /// against a guessed one. Name-only pairing folded the agent's `agentq`
    /// item into the operator's `opq` queue; by identity, `opq` keeps its
    /// operator item and `agentq` is carried as a separate agent-only component.
    #[test]
    fn components_with_different_ids_never_pair() {
        let base = "# T\n";
        let ours = "# T\n<!-- agent:run id=agentq -->\n- do [#a] agent\n<!-- /agent:run -->\n";
        let theirs = "# T\n<!-- agent:run id=opq -->\n- do [#b] operator\n<!-- /agent:run -->\n";

        let m = document_cell_merge(base, ours, theirs);

        assert_eq!(
            m.merged_doc,
            "# T\n<!-- agent:run id=opq -->\n- do [#b] operator\n<!-- /agent:run -->\n\n\
             <!-- agent:queue id=agentq -->\n- do [#a] agent\n<!-- /agent:queue -->\n",
        );
        assert_eq!(
            queue_ids_by_component_id(&m.merged_doc),
            vec![
                ("opq".to_string(), vec!["b".to_string()]),
                ("agentq".to_string(), vec!["a".to_string()]),
            ],
        );
    }

    /// A lone unkeyed queue IS the `default` queue: queue maintenance rewrites
    /// its opener to `id=default` when it creates a second queue, so the agent's
    /// edit of the unkeyed queue lands on the operator's `id=default` queue.
    #[test]
    fn lone_unkeyed_queue_pairs_with_id_default() {
        let base = "<!-- agent:run -->\n- do [#a] task\n<!-- /agent:run -->\n";
        let ours = "<!-- agent:run -->\n- ~~do [#a] task~~\n<!-- /agent:run -->\n";
        let theirs = "<!-- agent:run id=default -->\n- do [#a] task\n<!-- /agent:run -->\n\n\
                      <!-- agent:run id=later -->\n- do [#z] later\n<!-- /agent:run -->\n";

        let m = document_cell_merge(base, ours, theirs);

        assert_eq!(
            m.merged_doc,
            "<!-- agent:run id=default -->\n- ~~do [#a] task~~\n<!-- /agent:run -->\n\n\
             <!-- agent:run id=later -->\n- do [#z] later\n<!-- /agent:run -->\n",
        );
    }

    /// Repeated unkeyed components of one name pair by occurrence order, so the
    /// second one's items are merged against the second one's, not the first's.
    #[test]
    fn repeated_unkeyed_components_pair_by_occurrence() {
        let doc = |first: &str, second: &str| {
            format!(
                "<!-- agent:review -->\n{first}<!-- /agent:review -->\n\n\
                 <!-- agent:review -->\n{second}<!-- /agent:review -->\n"
            )
        };
        let base = doc("- check [#a] one\n", "- check [#a] two\n");
        let ours = doc("- check [#a] one\n", "- ~~check [#a] two~~\n");
        let theirs = doc("- check [#a] one, edited\n", "- check [#a] two\n");

        let m = document_cell_merge(&base, &ours, &theirs);

        assert_eq!(
            m.merged_doc,
            doc("- check [#a] one, edited\n", "- ~~check [#a] two~~\n"),
        );
        assert!(
            m.conflict_advisories.is_empty(),
            "{:?}",
            m.conflict_advisories
        );
    }

    // ----- transition table: one test per row ------------------------------

    #[test]
    fn row_present_unchanged_unchanged_keep() {
        let base = q("- do [#a] task\n");
        let m = document_cell_merge(&base, &base, &base);
        assert_eq!(outcome_for(&m, "a").unwrap().kind, OutcomeKind::Keep);
        assert!(m.conflict_advisories.is_empty());
        assert_eq!(reparses_to_ids(&m.merged_doc, "queue"), vec!["a"]);
    }

    #[test]
    fn row_present_agent_edited_operator_unchanged() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task EDITED\n");
        let theirs = base.clone();
        let m = document_cell_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::AppliedAgentEdit
        );
        assert!(m.merged_doc.contains("EDITED"));
        assert!(m.conflict_advisories.is_empty());
    }

    #[test]
    fn smqstrike_struck_head_survives_concurrent_operator_queue_add() {
        // #smqstrike (document_cell_merge Phase 3): an answered/consumed queue head is
        // struck by the agent (ours) while the operator (theirs) concurrently adds
        // a new queue item. The node-keyed merge must keep BOTH — the head struck
        // on the merged structure AND the operator's add — with no advisory
        // (node-disjoint), so a drifted operator queue edit never aborts or loses
        // the head strike. This is the merged-tree contract behind the shipped
        // queue_consume node-keyed strike + divergence reconcile.
        let base = q("- do [#a] task\n");
        let ours = q("- ~~do [#a] task~~\n"); // agent struck the consumed head
        let theirs = q("- do [#a] task\n- do [#b] operator added\n"); // operator add
        let m = document_cell_merge(&base, &ours, &theirs);
        assert!(
            m.merged_doc.contains("~~do [#a] task~~"),
            "head must stay struck on the merged tree:\n{}",
            m.merged_doc
        );
        assert!(
            m.merged_doc.contains("do [#b] operator added"),
            "concurrent operator queue add must survive:\n{}",
            m.merged_doc
        );
        assert_eq!(reparses_to_ids(&m.merged_doc, "queue"), vec!["a", "b"]);
        assert!(
            m.conflict_advisories.is_empty(),
            "node-disjoint strike+add needs no advisory: {:?}",
            m.conflict_advisories
        );
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::AppliedAgentEdit,
            "the strike is an applied agent flag-edit"
        );
    }

    #[test]
    fn row_present_agent_unchanged_operator_edited() {
        let base = q("- do [#a] task\n");
        let ours = base.clone();
        let theirs = q("- do [#a] task OPEDIT\n");
        let m = document_cell_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::AppliedOperatorEdit
        );
        assert!(m.merged_doc.contains("OPEDIT"));
        assert!(m.conflict_advisories.is_empty());
    }

    #[test]
    fn row_present_both_edited_same_text_convergent() {
        let base = q("- do [#a] task\n");
        let same = q("- do [#a] task SAME\n");
        let m = document_cell_merge(&base, &same, &same);
        assert_eq!(outcome_for(&m, "a").unwrap().kind, OutcomeKind::Convergent);
        assert!(m.merged_doc.contains("SAME"));
        // Applied once.
        assert_eq!(m.merged_doc.matches("[#a]").count(), 1);
        assert!(m.conflict_advisories.is_empty());
    }

    #[test]
    fn row_present_both_edited_different_operator_wins() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task AGENT\n");
        let theirs = q("- do [#a] task OPERATOR\n");
        let m = document_cell_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::OperatorWonConflict
        );
        assert!(m.merged_doc.contains("OPERATOR"));
        assert!(!m.merged_doc.contains("AGENT"));
        assert_eq!(
            m.conflict_advisories[0].reason,
            MergeConflictReason::SameNodeOperatorOverride
        );
    }

    // ----- #msn6 / #smturnactive: turn-active-area advisory gating --------------

    #[test]
    fn scoped_conflict_outside_active_area_drops_ack_but_keeps_operator_value() {
        // A same-node conflict in the `queue` component. Unscoped, it advisories.
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task AGENT\n");
        let theirs = q("- do [#a] task OPERATOR\n");

        let unscoped = document_cell_merge(&base, &ours, &theirs);
        assert_eq!(
            unscoped.conflict_advisories.len(),
            1,
            "unscoped advisories the conflict"
        );

        // Scope the active area to `exchange` only — the queue conflict is OUTSIDE
        // it, so the advisory is dropped while the merged doc + outcome are unchanged.
        let active = ActiveNodes::new().active_component("exchange");
        let scoped = document_cell_merge_scoped(&base, &ours, &theirs, &active);
        assert!(
            scoped.conflict_advisories.is_empty(),
            "out-of-active-area conflict auto-resolves with no advisory"
        );
        assert_eq!(
            scoped.merged_doc, unscoped.merged_doc,
            "merged value identical (operator still wins)"
        );
        assert_eq!(
            outcome_for(&scoped, "a").unwrap().kind,
            OutcomeKind::OperatorWonConflict,
            "outcome record is unchanged — only advisory emission is scoped"
        );
        assert!(scoped.merged_doc.contains("OPERATOR"));
        assert!(!scoped.merged_doc.contains("AGENT"));
    }

    #[test]
    fn scoped_conflict_inside_active_area_keeps_ack() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task AGENT\n");
        let theirs = q("- do [#a] task OPERATOR\n");

        // The conflict lives in `queue`; marking `queue` active keeps the advisory.
        let active = ActiveNodes::new().active_component("queue");
        let scoped = document_cell_merge_scoped(&base, &ours, &theirs, &active);
        assert_eq!(
            scoped.conflict_advisories.len(),
            1,
            "active-area collision advisories"
        );
        assert_eq!(
            scoped.conflict_advisories[0].reason,
            MergeConflictReason::SameNodeOperatorOverride
        );
    }

    #[test]
    fn scoped_active_set_is_node_granular() {
        // Two conflicting nodes; only one is marked active by exact id.
        let base = q("- do [#a] one\n- do [#b] two\n");
        let ours = q("- do [#a] one AGENT\n- do [#b] two AGENT\n");
        let theirs = q("- do [#a] one OPERATOR\n- do [#b] two OPERATOR\n");

        let active = ActiveNodes::new().with_node("queue", "a");
        let scoped = document_cell_merge_scoped(&base, &ours, &theirs, &active);
        assert_eq!(
            scoped.conflict_advisories.len(),
            1,
            "only the active node advisories"
        );
        assert_eq!(scoped.conflict_advisories[0].id, "a");
        // Both operator values still win regardless of advisory scoping.
        assert!(scoped.merged_doc.contains("one OPERATOR"));
        assert!(scoped.merged_doc.contains("two OPERATOR"));
    }

    #[test]
    fn scoped_empty_active_set_drops_all_acks() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task AGENT\n");
        let theirs = q("- do [#a] task OPERATOR\n");

        let active = ActiveNodes::new();
        assert!(active.is_empty());
        let scoped = document_cell_merge_scoped(&base, &ours, &theirs, &active);
        assert!(
            scoped.conflict_advisories.is_empty(),
            "an empty active set means nothing is active → every conflict auto-resolves"
        );
        assert!(scoped.merged_doc.contains("OPERATOR"));
    }

    #[test]
    fn row_agent_edited_operator_deleted_deletion_kept() {
        let base = q("- do [#a] task\n- do [#b] other\n");
        // agent edits a
        let ours = q("- do [#a] task AGENTEDIT\n- do [#b] other\n");
        // operator deletes a entirely
        let theirs = q("- do [#b] other\n");
        let m = document_cell_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::DeletionKept
        );
        assert!(!m.merged_doc.contains("[#a]"), "deleted node omitted");
        assert_eq!(
            m.conflict_advisories[0].reason,
            MergeConflictReason::OperatorDeletedAgentEditedNode
        );
    }

    #[test]
    fn row_agent_deleted_operator_edited_operator_revived() {
        let base = q("- do [#a] task\n- do [#b] other\n");
        // agent deletes a (absent)
        let ours = q("- do [#b] other\n");
        // operator edits a
        let theirs = q("- do [#a] task OPEDIT\n- do [#b] other\n");
        let m = document_cell_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::OperatorRevived
        );
        assert!(m.merged_doc.contains("OPEDIT"), "operator edit revived");
        assert_eq!(
            m.conflict_advisories[0].reason,
            MergeConflictReason::OperatorRevivedAgentDeletedNode
        );
    }

    #[test]
    fn row_absent_agent_added_operator_absent() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task\n- do [#new] fresh\n");
        let theirs = base.clone();
        let m = document_cell_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "new").unwrap().kind,
            OutcomeKind::AppliedAgentAdd
        );
        assert!(m.merged_doc.contains("[#new]"));
        assert!(m.conflict_advisories.is_empty());
    }

    #[test]
    fn row_absent_operator_added_agent_absent() {
        let base = q("- do [#a] task\n");
        let ours = base.clone();
        let theirs = q("- do [#a] task\n- do [#opnew] op-fresh\n");
        let m = document_cell_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "opnew").unwrap().kind,
            OutcomeKind::AppliedOperatorAdd
        );
        assert!(m.merged_doc.contains("[#opnew]"));
        assert!(m.conflict_advisories.is_empty());
    }

    #[test]
    fn row_both_added_different_ids_union() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task\n- do [#agentadd] x\n");
        let theirs = q("- do [#a] task\n- do [#opadd] y\n");
        let m = document_cell_merge(&base, &ours, &theirs);
        assert!(m.merged_doc.contains("[#agentadd]"));
        assert!(m.merged_doc.contains("[#opadd]"));
        assert_eq!(
            outcome_for(&m, "agentadd").unwrap().kind,
            OutcomeKind::AppliedAgentAdd
        );
        assert_eq!(
            outcome_for(&m, "opadd").unwrap().kind,
            OutcomeKind::AppliedOperatorAdd
        );
        assert!(m.conflict_advisories.is_empty());
        let ids = reparses_to_ids(&m.merged_doc, "queue");
        assert!(ids.contains(&"a".to_string()));
        assert!(ids.contains(&"agentadd".to_string()));
        assert!(ids.contains(&"opadd".to_string()));
    }

    #[test]
    fn row_both_added_same_id_different_text_operator_wins() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task\n- do [#dup] AGENT-VERSION\n");
        let theirs = q("- do [#a] task\n- do [#dup] OPERATOR-VERSION\n");
        let m = document_cell_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "dup").unwrap().kind,
            OutcomeKind::OperatorWonConflict
        );
        assert!(m.merged_doc.contains("OPERATOR-VERSION"));
        assert!(!m.merged_doc.contains("AGENT-VERSION"));
        assert_eq!(
            m.conflict_advisories[0].reason,
            MergeConflictReason::SameNodeOperatorOverride
        );
    }

    // ----- extra required tests -------------------------------------------

    #[test]
    fn firsthand_repro_node_disjoint_both_changesets_apply() {
        // base: queue with head [#cf-txn-email] + another head, frontmatter queue: start.
        let base = "\
---
queue: start
---
<!-- agent:queue -->
- do [#cf-txn-email] send the transactional email
- do [#other-head] unrelated head
<!-- /agent:queue -->

<!-- agent:exchange -->
- re [#cf-txn-email] prior turn
<!-- /agent:exchange -->
";
        // agent: strikes a DIFFERENT head (#other-head), appends a new ### Re: exchange
        // turn (new item in exchange), migrates an item (adds to review). Frontmatter
        // untouched by agent.
        let ours = "\
---
queue: start
---
<!-- agent:queue -->
- do [#cf-txn-email] send the transactional email
- ~~do [#other-head] unrelated head~~
<!-- /agent:queue -->

<!-- agent:exchange -->
- re [#cf-txn-email] prior turn
- re [#new-turn] appended exchange turn
<!-- /agent:exchange -->

<!-- agent:review -->
- [ ] [#migrated] migrated item
<!-- /agent:review -->
";
        // operator: flips frontmatter queue: stop + restructures queue block
        // (node-disjoint: operator only touches frontmatter + adds an op line).
        let theirs = "\
---
queue: stop
---
<!-- agent:queue -->
- do [#cf-txn-email] send the transactional email
- do [#other-head] unrelated head
- do [#op-restructure] operator added queue line
<!-- /agent:queue -->

<!-- agent:exchange -->
- re [#cf-txn-email] prior turn
<!-- /agent:exchange -->
";
        let m = document_cell_merge(base, ours, theirs);

        // Operator's queue: stop applied.
        assert!(
            m.merged_doc.contains("queue: stop"),
            "operator frontmatter flip applied: {}",
            m.merged_doc
        );
        // Agent's new exchange turn applied.
        assert!(
            m.merged_doc.contains("[#new-turn]"),
            "agent's appended exchange turn applied: {}",
            m.merged_doc
        );
        // Agent's strike of the different head applied.
        assert!(
            m.merged_doc.contains("~~do [#other-head] unrelated head~~"),
            "agent strike applied: {}",
            m.merged_doc
        );
        // Operator's queue restructure (added line) applied.
        assert!(m.merged_doc.contains("[#op-restructure]"));
        // Agent's review migration carried (agent-only component).
        assert!(m.merged_doc.contains("[#migrated]"));

        // Zero conflicts / advisories — fully node-disjoint.
        let conflicts: Vec<_> = m
            .outcomes
            .iter()
            .filter(|o| {
                matches!(
                    o.kind,
                    OutcomeKind::OperatorWonConflict
                        | OutcomeKind::DeletionKept
                        | OutcomeKind::OperatorRevived
                )
            })
            .collect();
        assert!(conflicts.is_empty(), "no conflicts: {conflicts:?}");
        assert!(
            m.conflict_advisories.is_empty(),
            "no advisories: {:?}",
            m.conflict_advisories
        );

        // Re-parses cleanly.
        let queue_ids = reparses_to_ids(&m.merged_doc, "queue");
        assert!(queue_ids.contains(&"cf-txn-email".to_string()));
        assert!(queue_ids.contains(&"other-head".to_string()));
        assert!(queue_ids.contains(&"op-restructure".to_string()));
        let exch_ids = reparses_to_ids(&m.merged_doc, "exchange");
        assert!(exch_ids.contains(&"new-turn".to_string()));
        assert!(reparses_to_ids(&m.merged_doc, "review").contains(&"migrated".to_string()));
    }

    #[test]
    fn operator_delete_vs_agent_edit_deletion_stands_with_ack() {
        let base = q("- do [#x] keep\n- do [#y] target\n");
        let ours = q("- do [#x] keep\n- do [#y] target AGENTEDIT\n");
        let theirs = q("- do [#x] keep\n"); // operator deleted y
        let m = document_cell_merge(&base, &ours, &theirs);
        assert!(!m.merged_doc.contains("[#y]"), "y deleted");
        assert_eq!(
            outcome_for(&m, "y").unwrap().kind,
            OutcomeKind::DeletionKept
        );
        assert_eq!(m.conflict_advisories.len(), 1);
        assert_eq!(
            m.conflict_advisories[0].reason,
            MergeConflictReason::OperatorDeletedAgentEditedNode
        );
        assert_eq!(m.conflict_advisories[0].id, "y");
    }

    #[test]
    fn same_node_different_edit_operator_wins_with_ack() {
        let base = q("- do [#z] orig\n");
        let ours = q("- do [#z] agent version\n");
        let theirs = q("- do [#z] operator version\n");
        let m = document_cell_merge(&base, &ours, &theirs);
        assert!(m.merged_doc.contains("operator version"));
        assert!(!m.merged_doc.contains("agent version"));
        assert_eq!(m.conflict_advisories.len(), 1);
        assert_eq!(
            m.conflict_advisories[0].reason,
            MergeConflictReason::SameNodeOperatorOverride
        );
        assert!(
            m.exchange_notes
                .iter()
                .any(|n| n.contains("operator version") && n.contains("agent version")),
            "same-node conflict note must carry both versions: {:?}",
            m.exchange_notes
        );
    }

    #[test]
    fn same_node_conflict_note_lands_inside_exchange_when_present() {
        let base = "\
<!-- agent:queue -->
- do [#z] orig
<!-- /agent:queue -->
<!-- agent:exchange -->
### Re: prior - gpt-5

Done.
<!-- /agent:exchange -->
";
        let ours = base.replace("orig", "agent version");
        let theirs = base.replace("orig", "operator version");
        let m = document_cell_merge(base, &ours, &theirs);

        let exchange_inner = component_inner_lines("exchange", &m.merged_doc).join("");
        assert!(
            exchange_inner.contains("operator version")
                && exchange_inner.contains("agent version")
                && exchange_inner.contains("agent-doc conflict"),
            "same-node conflict must be visible in exchange:\n{}",
            m.merged_doc
        );
    }

    #[test]
    fn node_disjoint_union_of_adds_both_present() {
        let base = q("- do [#base] b\n");
        let ours = q("- do [#base] b\n- do [#a1] agent one\n- do [#a2] agent two\n");
        let theirs = q("- do [#base] b\n- do [#o1] op one\n");
        let m = document_cell_merge(&base, &ours, &theirs);
        for id in ["base", "a1", "a2", "o1"] {
            assert!(
                m.merged_doc.contains(&format!("[#{id}]")),
                "{id} present in {}",
                m.merged_doc
            );
        }
        assert!(m.conflict_advisories.is_empty());
    }

    #[test]
    fn frontmatter_scalar_operator_only_edit_applied_no_ack() {
        let base = "---\nqueue: start\nmodel: opus\n---\n# body\n";
        let ours = base.to_string();
        let theirs = "---\nqueue: stop\nmodel: opus\n---\n# body\n";
        let m = document_cell_merge(base, &ours, theirs);
        assert!(m.merged_doc.contains("queue: stop"));
        assert!(m.merged_doc.contains("model: opus"));
        let fm_outcome = m
            .outcomes
            .iter()
            .find(|o| o.component == FRONTMATTER_COMPONENT && o.id == "queue")
            .unwrap();
        assert_eq!(fm_outcome.kind, OutcomeKind::AppliedOperatorEdit);
        assert!(m.conflict_advisories.is_empty());
    }

    #[test]
    fn frontmatter_both_edit_different_operator_wins_with_ack() {
        let base = "---\nqueue: start\n---\n# body\n";
        let ours = "---\nqueue: paused\n---\n# body\n";
        let theirs = "---\nqueue: stop\n---\n# body\n";
        let m = document_cell_merge(base, ours, theirs);
        assert!(m.merged_doc.contains("queue: stop"));
        assert_eq!(m.conflict_advisories.len(), 1);
        assert_eq!(
            m.conflict_advisories[0].reason,
            MergeConflictReason::SameNodeOperatorOverride
        );
        assert_eq!(m.conflict_advisories[0].component, FRONTMATTER_COMPONENT);
    }

    #[test]
    fn frontmatter_agent_only_edit_applied() {
        let base = "---\nqueue: start\nmodel: opus\n---\n# body\n";
        let ours = "---\nqueue: start\nmodel: sonnet\n---\n# body\n";
        let theirs = base.to_string();
        let m = document_cell_merge(base, ours, &theirs);
        assert!(m.merged_doc.contains("model: sonnet"));
        assert!(m.conflict_advisories.is_empty());
    }

    #[test]
    fn prose_skeleton_is_operator() {
        // Documented assumption: non-component prose comes from the operator buffer.
        let base = "intro\n\n<!-- agent:queue -->\n- do [#a] t\n<!-- /agent:queue -->\n";
        let ours = "AGENT-PROSE\n\n<!-- agent:queue -->\n- do [#a] t\n<!-- /agent:queue -->\n";
        let theirs = "OPERATOR-PROSE\n\n<!-- agent:queue -->\n- do [#a] t\n<!-- /agent:queue -->\n";
        let m = document_cell_merge(base, ours, theirs);
        assert!(m.merged_doc.contains("OPERATOR-PROSE"));
        assert!(!m.merged_doc.contains("AGENT-PROSE"));
    }

    #[test]
    fn freetext_fenced_queue_head_survives_per_node_merge() {
        // `#qdup-freetext`: a multi-line free-text queue head (a pasted-console bug
        // report — a `:pushpin:` line + a fenced block between `---` rules) is not a
        // `- ` list item, so the overlay never parses it as an Item. The
        // reconstruction must preserve it verbatim while still merging the bullet
        // items, otherwise the `dropped_queue_prompt_lines_after_content_ours`
        // anti-data-loss gate sees it dropped and declines the merge on every cycle
        // — the persistent live_prompt_drift churn this fixes.
        let head = "\
---
:pushpin: JB `Run Agent Doc` did not submit.

```
claude exited cleanly.
Press Enter to restart, or 'q' to exit.
[agent-doc] idle-queue watch: reconciled stale busy actor to ready
```
---
";
        let base = format!(
            "<!-- agent:queue -->\n{head}- :pushpin: [#a] one\n- :pushpin: [#b] two\n<!-- /agent:queue -->\n"
        );
        // Agent struck #a this cycle (an item merge alongside the free-text head).
        let ours = format!(
            "<!-- agent:queue -->\n{head}- ~~:pushpin: [#a] one~~\n- :pushpin: [#b] two\n<!-- /agent:queue -->\n"
        );
        // Operator buffer unchanged from base — still carries the free-text head.
        let theirs = base.clone();

        let m = document_cell_merge(&base, &ours, &theirs);

        // Every meaningful line of the free-text head survives the merge.
        for needle in [
            ":pushpin: JB `Run Agent Doc` did not submit.",
            "claude exited cleanly.",
            "Press Enter to restart, or 'q' to exit.",
            "[agent-doc] idle-queue watch: reconciled stale busy actor to ready",
        ] {
            assert!(
                m.merged_doc.contains(needle),
                "free-text head line must survive the merge: {needle:?}\n---\n{}",
                m.merged_doc
            );
        }
        // The bullet items still merge: #b kept, #a's strike applied.
        assert!(m.merged_doc.contains("[#b] two"));
        assert!(
            m.merged_doc.contains("~~:pushpin: [#a] one~~"),
            "agent strike must apply:\n{}",
            m.merged_doc
        );
        // The fenced separators survive too.
        assert!(m.merged_doc.contains("---"));
    }

    /// `#fencedmarkerscan`: the live 2026-10-26 `agent-doc-bugs.md` queue
    /// shape. A pasted report's fenced block holds a bare
    /// `<!-- agent:run priority subagents -->` line, a heading-anchored copy,
    /// and a bare `<!-- /agent:run -->` line. The overlay ignores all three;
    /// the body scan must too. Before, the fenced close line ended the
    /// operator's queue early: the merged item set (with the agent's strike)
    /// was emitted there, and the operator's remaining lines (the fence tail,
    /// the unstruck copy of the row, and the real close) were passed through
    /// again, so the struck row came back live beside its strike.
    #[test]
    fn fenced_component_markers_never_open_or_close_the_merge_scan() {
        let fenced = "\
---
Run Agent Doc on sdk-demo.md still wedged:
```
## Run
<!-- agent:run priority subagents -->
◇ markdown   [Copy]
<!-- /agent:run -->
## Run <!-- agent:run priority subagents -->
```
---
";
        let doc = |before: &str, after: &str| {
            format!(
                "# T\n\n## Queue: Main\n<!-- agent:run id=\"main\" subagents go -->\n\
                 - {before}\n{fenced}- {after}\n- 📥 still pending\n<!-- /agent:run -->\n\n\
                 ## Run: Issues\n<!-- agent:loop id=issues interval=1h -->\n- 📥 onboarding\n<!-- /agent:loop -->\n"
            )
        };
        // Rows carry no lifecycle/owner prefix: the overlay keys a row by its
        // first `[#...]` token, so an annotated row and its bare struck copy
        // are different nodes (reported separately; out of scope here).
        let base = doc(
            "tmux pane auto sync is not working.",
            "The agent:loop is getting items.",
        );
        // The agent struck the rows on both sides of the fence.
        let ours = doc(
            "~~tmux pane auto sync is not working.~~",
            "~~The agent:loop is getting items.~~",
        );
        // The operator's editor still holds the base.
        let theirs = base.clone();

        let m = document_cell_merge(&base, &ours, &theirs);

        assert_eq!(m.merged_doc, ours, "merged:\n{}", m.merged_doc);
        assert_eq!(
            components(&m.merged_doc)
                .iter()
                .filter(|c| c.name == "queue")
                .count(),
            2
        );
    }

    /// `#fencedmarkerscan`: a fenced example of a component outside every
    /// component (operator prose) never opens one in the merge scan, so it
    /// cannot swallow the real queue that follows it.
    #[test]
    fn fenced_component_example_in_prose_does_not_swallow_the_next_component() {
        let doc = |row: &str| {
            format!(
                "# T\n\nExample:\n\n```md\n<!-- agent:queue -->\n- example row\n```\n\n\
                 <!-- agent:queue -->\n- {row}\n<!-- /agent:queue -->\n"
            )
        };
        let base = doc("do [#a] task");
        let ours = doc("~~do [#a] task~~");
        let theirs = base.clone();

        let m = document_cell_merge(&base, &ours, &theirs);

        assert_eq!(m.merged_doc, ours, "merged:\n{}", m.merged_doc);
    }

    // ----- exchange heading-prose turns (`#semmerge` real format) ----------

    #[test]
    fn exchange_merge_converges_a_poisoned_duplicate_turn() {
        // `#exchangeconverge`: theirs carries a mirror-ordered DUPLICATE
        // `### Re: A` turn — the poison a prior failed 3-way merge leaves behind.
        // With `document_cell_merge_enabled` (default ON) the exchange merge collapses it
        // to a single occurrence instead of preserving a duplicate that would
        // accrete across cycles (the runaway-growth wedge).
        //
        // Serialize with the kill-switch tests (which mutate the process-global
        // flag env) and pin the flag ON, so this integration assertion never
        // races their env mutation under parallel test threads.
        let _guard = crate::document_cell::CELL_MERGE_ENV_LOCK.lock();
        // SAFETY: held under the shared env lock; absent var ⇒ default ON.
        unsafe {
            std::env::remove_var(crate::document_cell::CELL_MERGE_ENV);
        }
        let base = "\
---
queue: start
---
<!-- agent:queue -->
- do [#a] task
<!-- /agent:queue -->

<!-- agent:exchange -->
### Re: A — opus-4-8

Answer to A.
<!-- /agent:exchange -->
";
        let poisoned = "\
---
queue: start
---
<!-- agent:queue -->
- do [#a] task
<!-- /agent:queue -->

<!-- agent:exchange -->
### Re: A — opus-4-8

Answer to A.

### Re: A — opus-4-8

Answer to A.
<!-- /agent:exchange -->
";
        let m = document_cell_merge(base, poisoned, poisoned);
        assert_eq!(
            m.merged_doc.matches("### Re: A").count(),
            1,
            "the duplicate turn must converge to a single occurrence:\n{}",
            m.merged_doc
        );
        assert!(m.merged_doc.contains("Answer to A."));
    }

    #[test]
    fn exchange_keeps_fresh_same_heading_response_with_distinct_body() {
        // `#qcellmerge-response-body-id`: the finalize shape that dropped a
        // response. `base`/`theirs` (the committed doc + the operator's live
        // buffer) carry a prior `### Re: #preset — opus-4-8` turn. `ours` (agent)
        // appends a FRESH response that reuses the same heading — a same-preset /
        // same-topic follow-up drained from the queue — but with a different body.
        // Heading-only identity treated the fresh turn as a duplicate of the prior
        // one and dropped it ("a cell-merge chose the existing content"); body-aware
        // identity keeps it while still collapsing true byte-identical duplicates.
        let _guard = crate::document_cell::CELL_MERGE_ENV_LOCK.lock();
        // SAFETY: held under the shared env lock; absent var ⇒ default ON.
        unsafe {
            std::env::remove_var(crate::document_cell::CELL_MERGE_ENV);
        }
        let base = "\
<!-- agent:exchange -->
### Re: #preset — opus-4-8

Prior answer for the preset run.
<!-- /agent:exchange -->
";
        let theirs = base;
        let ours = "\
<!-- agent:exchange -->
### Re: #preset — opus-4-8

Prior answer for the preset run.

### Re: #preset — opus-4-8

Fresh answer for the next same-preset queue item.
<!-- /agent:exchange -->
";
        let m = document_cell_merge(base, ours, theirs);
        assert!(
            m.merged_doc
                .contains("Fresh answer for the next same-preset queue item."),
            "the fresh same-heading response body must survive the merge:\n{}",
            m.merged_doc
        );
        assert_eq!(
            m.merged_doc.matches("### Re: #preset").count(),
            2,
            "both the prior and the fresh same-heading turn are kept:\n{}",
            m.merged_doc
        );
    }

    #[test]
    fn exchange_appends_agent_new_heading_prose_turn() {
        // base + theirs: exchange has `### Re: A` and `### Re: B` heading-prose
        // turns. ours: A, B, plus a NEW `### Re: C — opus-4-8` turn with prose.
        // theirs additionally flips a frontmatter scalar and adds a queue item.
        let base = "\
---
queue: start
---
<!-- agent:queue -->
- do [#a] task
<!-- /agent:queue -->

<!-- agent:exchange -->
### Re: A — opus-4-8

Answer to A.

### Re: B — opus-4-8

Answer to B.
<!-- /agent:exchange -->
";
        let ours = "\
---
queue: start
---
<!-- agent:queue -->
- do [#a] task
<!-- /agent:queue -->

<!-- agent:exchange -->
### Re: A — opus-4-8

Answer to A.

### Re: B — opus-4-8

Answer to B.

### Re: C — opus-4-8

Brand new agent turn for C.
<!-- /agent:exchange -->
";
        let theirs = "\
---
queue: stop
---
<!-- agent:queue -->
- do [#a] task
- do [#opadd] operator queue item
<!-- /agent:queue -->

<!-- agent:exchange -->
### Re: A — opus-4-8

Answer to A.

### Re: B — opus-4-8

Answer to B.
<!-- /agent:exchange -->
";
        let m = document_cell_merge(base, ours, theirs);

        // Agent's new heading + prose appended.
        assert!(
            m.merged_doc.contains("### Re: C — opus-4-8"),
            "agent's new C heading present: {}",
            m.merged_doc
        );
        assert!(
            m.merged_doc.contains("Brand new agent turn for C."),
            "agent's new C prose present: {}",
            m.merged_doc
        );
        // Operator's disjoint changes applied.
        assert!(
            m.merged_doc.contains("queue: stop"),
            "operator fm flip: {}",
            m.merged_doc
        );
        assert!(
            m.merged_doc.contains("[#opadd]"),
            "operator queue add: {}",
            m.merged_doc
        );
        // Existing turns kept, not duplicated.
        assert_eq!(m.merged_doc.matches("### Re: A — opus-4-8").count(), 1);
        assert_eq!(m.merged_doc.matches("### Re: B — opus-4-8").count(), 1);

        // C yields an AppliedAgentAdd outcome on the exchange component.
        let c_outcome = m
            .outcomes
            .iter()
            .find(|o| o.component == "exchange" && o.id == "Re: C — opus-4-8")
            .expect("C turn outcome present");
        assert_eq!(c_outcome.kind, OutcomeKind::AppliedAgentAdd);

        // Re-parses cleanly (queue still recognizable).
        let queue_ids = reparses_to_ids(&m.merged_doc, "queue");
        assert!(queue_ids.contains(&"a".to_string()));
        assert!(queue_ids.contains(&"opadd".to_string()));
    }

    #[test]
    fn exchange_head_marker_does_not_split_turn_identity() {
        // ours has the `(HEAD)` boundary annotation; base/theirs do not. The turn
        // must be treated as the SAME (not re-appended / duplicated).
        let base = "\
<!-- agent:exchange -->
### Re: X — opus-4-8

Answer to X.
<!-- /agent:exchange -->
";
        let ours = "\
<!-- agent:exchange -->
### Re: X — opus-4-8 (HEAD)

Answer to X.
<!-- /agent:exchange -->
";
        let theirs = base.to_string();
        let m = document_cell_merge(base, ours, &theirs);

        // Exactly one X turn — the `(HEAD)` variant must NOT be appended as new.
        let count = m.merged_doc.matches("### Re: X — opus-4-8").count();
        assert_eq!(count, 1, "X not duplicated: {}", m.merged_doc);
        // No AppliedAgentAdd for X on exchange.
        assert!(
            !m.outcomes
                .iter()
                .any(|o| o.component == "exchange" && o.kind == OutcomeKind::AppliedAgentAdd),
            "no agent-add for the HEAD-annotated same turn: {:?}",
            m.outcomes
        );
    }

    #[test]
    fn exchange_boundary_marker_preserved_and_new_turn_before_it() {
        // theirs' exchange ends with a boundary marker; ours appends a new turn.
        let base = "\
<!-- agent:exchange -->
### Re: A — opus-4-8

Answer to A.
<!-- agent:boundary:abc123 -->
<!-- /agent:exchange -->
";
        let ours = "\
<!-- agent:exchange -->
### Re: A — opus-4-8

Answer to A.

### Re: B — opus-4-8

New turn B.
<!-- agent:boundary:abc123 -->
<!-- /agent:exchange -->
";
        let theirs = base.to_string();
        let m = document_cell_merge(base, ours, &theirs);

        // New turn appended.
        assert!(
            m.merged_doc.contains("### Re: B — opus-4-8"),
            "B present: {}",
            m.merged_doc
        );
        // Exactly one boundary marker.
        assert_eq!(
            m.merged_doc
                .matches("<!-- agent:boundary:abc123 -->")
                .count(),
            1,
            "single boundary marker: {}",
            m.merged_doc
        );
        // New turn lands BEFORE the boundary marker.
        let b_idx = m.merged_doc.find("### Re: B — opus-4-8").unwrap();
        let boundary_idx = m.merged_doc.find("<!-- agent:boundary:abc123 -->").unwrap();
        assert!(
            b_idx < boundary_idx,
            "new turn must precede the boundary marker: {}",
            m.merged_doc
        );
    }

    #[test]
    fn exchange_append_separates_new_turn_from_unterminated_retained_tail() {
        // A retained projection may hand the cell merge a final exchange fragment
        // without its line terminator. Appending the agent-new turn must restore
        // the Markdown node boundary instead of producing `tail.### Re:`.
        let theirs_inner = vec!["Retained operator tail.".to_string()];
        let ours = "\
<!-- agent:exchange -->
### Re: new turn — opus-4-8

Fresh response.
<!-- /agent:exchange -->
";
        let mut outcomes = Vec::new();
        let mut advisories = Vec::new();

        let merged = merge_exchange_inner(
            &theirs_inner,
            None,
            None,
            None,
            ours,
            &mut outcomes,
            &mut advisories,
        );

        assert_eq!(
            merged,
            "Retained operator tail.\n### Re: new turn — opus-4-8\n\nFresh response.\n"
        );
    }

    #[test]
    fn exchange_rebuild_separates_retained_chat_prompt_from_unterminated_tail() {
        // Regression for the observed stranded-steering shape: the document
        // model retained two distinct line records, but the preceding response
        // tail had lost its terminator. Rebuilding the exchange must not turn
        // those records into `compaction.> **Chat prompt`.
        let theirs_inner = vec![
            "### Re: prior turn — opus-4-8\n".to_string(),
            "\n".to_string(),
            "The exchange is oversized, but the active queue was not stalled for compaction."
                .to_string(),
            "> **Chat prompt (#chatprompt):** Fix the cause of the error\n".to_string(),
        ];
        let ours = "\
<!-- agent:exchange -->
### Re: prior turn — opus-4-8

The exchange is oversized, but the active queue was not stalled for compaction.
> **Chat prompt (#chatprompt):** Fix the cause of the error
<!-- /agent:exchange -->
";
        let mut outcomes = Vec::new();
        let mut advisories = Vec::new();

        let merged = merge_exchange_inner(
            &theirs_inner,
            None,
            None,
            None,
            ours,
            &mut outcomes,
            &mut advisories,
        );

        assert!(
            merged.contains(
                "compaction.\n> **Chat prompt (#chatprompt):** Fix the cause of the error"
            ),
            "retained Markdown nodes must keep a line boundary:\n{merged}"
        );
        assert!(!merged.contains("compaction.> **Chat prompt"));
    }

    #[test]
    fn exchange_append_separates_new_turn_before_boundary_from_unterminated_tail() {
        // The canonical response boundary is structural trailing content. Its
        // presence must not let an unterminated retained tail absorb the first
        // agent-new heading when the turn is inserted before that boundary.
        let theirs_inner = vec![
            "Retained operator tail.".to_string(),
            "<!-- agent:boundary:abc123 -->\n".to_string(),
        ];
        let ours = "\
<!-- agent:exchange -->
### Re: new turn — opus-4-8

Fresh response.
<!-- agent:boundary:abc123 -->
<!-- /agent:exchange -->
";
        let mut outcomes = Vec::new();
        let mut advisories = Vec::new();

        let merged = merge_exchange_inner(
            &theirs_inner,
            None,
            None,
            None,
            ours,
            &mut outcomes,
            &mut advisories,
        );

        assert_eq!(
            merged,
            "Retained operator tail.\n### Re: new turn — opus-4-8\n\nFresh response.\n<!-- agent:boundary:abc123 -->\n"
        );
    }

    // ----- #hap7 / #qdup: scoped-merge no-structural-duplication -------------

    #[test]
    fn qdup_operator_queue_add_during_exchange_turn_no_duplication() {
        // #hap7 / #qdup test 1 (the operator-reported corruption shape): the agent
        // writes a NEW exchange `### Re:` turn while the operator concurrently
        // inserts a queue item. The per-node merge must land both change-sets with
        // the operator's queue item appearing EXACTLY ONCE and no adjacent queue
        // node duplicated/reversed.
        let base = "\
<!-- agent:queue -->
- do [#a] first
- do [#b] second
<!-- /agent:queue -->
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.
<!-- /agent:exchange -->
";
        // Agent (ours): appends a new exchange turn, queue untouched.
        let ours = "\
<!-- agent:queue -->
- do [#a] first
- do [#b] second
<!-- /agent:queue -->
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.

### Re: new turn — opus-4-8

Fresh response.
<!-- /agent:exchange -->
";
        // Operator (theirs): inserts a new queue item #c, exchange untouched.
        let theirs = "\
<!-- agent:queue -->
- do [#a] first
- do [#c] operator full screen dialogs deploy
- do [#b] second
<!-- /agent:queue -->
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.
<!-- /agent:exchange -->
";
        let m = document_cell_merge(base, ours, theirs);

        // The operator's concurrent queue add appears EXACTLY ONCE.
        assert_eq!(
            m.merged_doc
                .matches("do [#c] operator full screen dialogs deploy")
                .count(),
            1,
            "operator queue add must appear exactly once (no duplication):\n{}",
            m.merged_doc
        );
        // No adjacent queue node duplicated or dropped — order preserved.
        assert_eq!(
            reparses_to_ids(&m.merged_doc, "queue"),
            vec!["a", "c", "b"],
            "queue order preserved, no adjacent duplication:\n{}",
            m.merged_doc
        );
        // Agent's new exchange turn survives, exactly once.
        assert_eq!(
            m.merged_doc.matches("### Re: new turn — opus-4-8").count(),
            1,
            "agent's new turn present exactly once:\n{}",
            m.merged_doc
        );
        // Node-disjoint change-sets: no advisory noise.
        assert!(
            m.conflict_advisories.is_empty(),
            "node-disjoint queue-add + exchange-turn need no advisory: {:?}",
            m.conflict_advisories
        );
    }

    #[test]
    fn exchange_response_ordered_list_preserves_order() {
        // #4pdp / #ocreverselist: an OpenCode-shaped child response whose new
        // `### Re:` turn contains an ordered (numeric) markdown list must keep the
        // list items in authored order (1, 2, 3) after the semantic merge. The
        // OpenCode harness has no IDE-plugin IPC and no streaming, so its response
        // round-trips through the disk-write CRDT/semantic merge path; an ordering
        // inversion there surfaces as reverse-ordered lists in the document.
        let base = "\
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.
<!-- /agent:exchange -->
";
        let ours = "\
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.

### Re: new turn — opus-4-8

Steps:

1. First step.
2. Second step.
3. Third step.
<!-- /agent:exchange -->
";
        // Operator (theirs): exchange untouched.
        let theirs = base;
        let m = document_cell_merge(base, ours, theirs);

        let s1 = m
            .merged_doc
            .find("1. First step.")
            .expect("ordered item 1 missing");
        let s2 = m
            .merged_doc
            .find("2. Second step.")
            .expect("ordered item 2 missing");
        let s3 = m
            .merged_doc
            .find("3. Third step.")
            .expect("ordered item 3 missing");
        assert!(
            s1 < s2 && s2 < s3,
            "ordered list reversed in merged exchange (s1={s1} s2={s2} s3={s3}):\n{}",
            m.merged_doc
        );
    }

    #[test]
    fn exchange_bare_ordered_list_without_heading_preserves_order() {
        // #4pdp / #ocreverselist: an OpenCode response that is a bare ordered list
        // (no `### Re:` heading) falls through the heading-turn path into the
        // bullet-item merge, because the overlay treats `N. ` as a list item.
        // The item-merge must keep the agent-added items in authored order (1,2,3),
        // not reversed.
        let base = "\
<!-- agent:exchange -->
<!-- /agent:exchange -->
";
        let ours = "\
<!-- agent:exchange -->
1. First step.
2. Second step.
3. Third step.
<!-- /agent:exchange -->
";
        let theirs = base;
        let m = document_cell_merge(base, ours, theirs);

        let s1 = m
            .merged_doc
            .find("First step.")
            .expect("ordered item 1 missing");
        let s2 = m
            .merged_doc
            .find("Second step.")
            .expect("ordered item 2 missing");
        let s3 = m
            .merged_doc
            .find("Third step.")
            .expect("ordered item 3 missing");
        assert!(
            s1 < s2 && s2 < s3,
            "bare ordered list reversed in bullet-path merge (s1={s1} s2={s2} s3={s3}):\n{}",
            m.merged_doc
        );
    }

    #[test]
    fn exchange_response_mixing_bullets_prose_and_ordered_list_keeps_authored_order() {
        // `#ocreverselist` live shape, observed 2026-09-12 in
        // `src/haiven-dev/tasks/docs.md`: a response block whose body is a bullet
        // run, then plain prose, then an ordered list came back with EVERY list
        // line reversed (`2.` above `1.`, the bullets bottom-to-top) and the
        // intervening prose dropped entirely. The two existing `#ocreverselist`
        // tests each hold one list shape alone; neither mixes bullets, prose and
        // an ordered list in one block, which is what real responses look like.
        let base = "\
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.
<!-- /agent:exchange -->
";
        let ours = "\
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.

### Re: new turn — opus-4-8

What changed:

- **Shape chosen:** alpha.
- **All nine applied:** bravo.
- **Routes moved:** charlie.
- **Launch condition:** delta.
- **Ripple:** echo.
- **Pending question:** foxtrot.

**Verification:** lint clean.

**Two things need you:**

1. First thing.
2. Second thing.
<!-- /agent:exchange -->
";
        let theirs = base;
        let m = document_cell_merge(base, ours, theirs);

        let at = |needle: &str| {
            m.merged_doc
                .find(needle)
                .unwrap_or_else(|| panic!("missing {needle} in merged doc:\n{}", m.merged_doc))
        };
        let order = [
            "**Shape chosen:**",
            "**All nine applied:**",
            "**Routes moved:**",
            "**Launch condition:**",
            "**Ripple:**",
            "**Pending question:**",
            "1. First thing.",
            "2. Second thing.",
        ];
        let offsets: Vec<usize> = order.iter().map(|n| at(n)).collect();
        assert!(
            offsets.windows(2).all(|w| w[0] < w[1]),
            "list lines reordered in merged exchange (offsets={offsets:?}):\n{}",
            m.merged_doc
        );
        // The prose between the bullet run and the ordered list is part of the
        // block, not a list item, and must survive the split/merge round trip.
        assert!(
            m.merged_doc.contains("**Verification:** lint clean."),
            "prose between the lists was dropped:\n{}",
            m.merged_doc
        );
        assert!(
            m.merged_doc.contains("**Two things need you:**"),
            "prose heading before the ordered list was dropped:\n{}",
            m.merged_doc
        );
    }

    #[test]
    fn qdup_one_changed_node_leaves_siblings_byte_identical() {
        // #hap7 / #qdup test 3 (per-node isolation): an agent edit to ONE queue
        // node must leave every sibling node's serialized line byte-identical — a
        // concurrent change to one structural part cannot reflow adjacent parts.
        let base = q("- do [#a] first\n- do [#b] second\n- do [#c] third\n");
        let ours = q("- do [#a] first\n- do [#b] SECOND EDITED\n- do [#c] third\n");
        let theirs = base.clone();
        let m = document_cell_merge(&base, &ours, &theirs);

        assert!(
            m.merged_doc.contains("- do [#a] first\n"),
            "sibling #a byte-identical:\n{}",
            m.merged_doc
        );
        assert!(
            m.merged_doc.contains("- do [#c] third\n"),
            "sibling #c byte-identical:\n{}",
            m.merged_doc
        );
        assert!(
            m.merged_doc.contains("- do [#b] SECOND EDITED\n"),
            "changed node #b applied:\n{}",
            m.merged_doc
        );
        assert_eq!(
            reparses_to_ids(&m.merged_doc, "queue"),
            vec!["a", "b", "c"],
            "order + cardinality unchanged:\n{}",
            m.merged_doc
        );
    }

    #[test]
    fn qdup_operator_deleted_agent_targeted_node_noted_in_exchange_not_resurrected() {
        // #hap7 / #qdup test 2 (deleted-structure rule): the operator deletes a
        // queue node that the agent's content this cycle targeted (edited). The
        // deletion must STAND (node not resurrected) and the fact must be surfaced
        // in `agent:exchange` so the operator sees the agent's dropped edit — the
        // in-editor document is the source of truth and we never merge it back.
        let base = "\
<!-- agent:queue -->
- do [#a] first
- do [#x] target node
<!-- /agent:queue -->
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.
<!-- /agent:exchange -->
";
        // Agent (ours): edited #x (its targeted node).
        let ours = "\
<!-- agent:queue -->
- do [#a] first
- do [#x] target node AGENT EDITED
<!-- /agent:queue -->
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.
<!-- /agent:exchange -->
";
        // Operator (theirs): deleted #x entirely.
        let theirs = "\
<!-- agent:queue -->
- do [#a] first
<!-- /agent:queue -->
<!-- agent:exchange -->
### Re: prior — opus-4-8

Prior answer.
<!-- /agent:exchange -->
";
        let m = document_cell_merge(base, ours, theirs);

        // Deletion stands — #x is NOT resurrected.
        assert_eq!(
            reparses_to_ids(&m.merged_doc, "queue"),
            vec!["a"],
            "operator deletion of #x must stand (not resurrected):\n{}",
            m.merged_doc
        );
        assert!(
            !m.merged_doc.contains("target node AGENT EDITED"),
            "agent's edit to the deleted node must NOT be merged back:\n{}",
            m.merged_doc
        );
        // The outcome is recorded as a kept deletion.
        assert_eq!(
            outcome_for(&m, "x").unwrap().kind,
            OutcomeKind::DeletionKept
        );
        // The fact is surfaced as an exchange note (so the operator sees it this
        // cycle, independent of the cycle-local conflict projection).
        assert!(
            !m.exchange_notes.is_empty(),
            "a deletion-of-agent-targeted-node fact must be surfaced as an exchange note"
        );
        assert!(
            m.exchange_notes.iter().any(|n| n.contains("#x")),
            "the exchange note must name the deleted node #x: {:?}",
            m.exchange_notes
        );
        // The note must actually appear inside the merged exchange component.
        let exchange_inner = component_inner_lines("exchange", &m.merged_doc).join("");
        assert!(
            exchange_inner.contains("#x"),
            "the deletion note must land inside agent:exchange:\n{}",
            m.merged_doc
        );
        // And the merged doc must remain structurally clean (no duplicate
        // components, no corruption) — re-parses to exactly one exchange component.
        assert_eq!(
            components(&m.merged_doc)
                .iter()
                .filter(|c| c.name == "exchange")
                .count(),
            1,
            "exactly one exchange component after noting:\n{}",
            m.merged_doc
        );
    }
}

//! Semantic, node-keyed three-way merge over the component overlay.
//!
//! Phase 4 (`#semmerge-owner`) of `tasks/agent-doc/plan-semantic-ast-merge.md`:
//! a **pure** decision function that merges a base snapshot, the agent result
//! (`ours`), and the operator editor buffer (`theirs`) by *node identity* rather
//! than by text lines. It is the merge-**policy** layer on top of the Phase 2
//! [`crate::overlay`] node-keyed substrate — not a second CRDT, and with **no
//! agent-doc coupling** (no IPC, no commit path, no git).
//!
//! The classic line-based merge sees two concurrent edits to *different* items as
//! a text conflict and fail-closes (dropping the agent's work). This function
//! instead keys every change on a stable [`crate::overlay::Item::id`] within its
//! component (component identity = `name`) so node-disjoint change-sets — the
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
//! | present | edited | edited (diff text) | [`OutcomeKind::OperatorWonConflict`] + ack |
//! | present | edited/struck | deleted | [`OutcomeKind::DeletionKept`] + ack |
//! | present | deleted (struck/absent) | edited | [`OutcomeKind::OperatorRevived`] + ack |
//! | absent | added | absent | [`OutcomeKind::AppliedAgentAdd`] |
//! | absent | absent | added | [`OutcomeKind::AppliedOperatorAdd`] |
//! | absent | added | added (diff ids) | [`OutcomeKind::AppliedBothAdd`] |
//! | absent | added | added (same id, diff text) | [`OutcomeKind::OperatorWonConflict`] + ack |
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
//! changed differently ⇒ operator wins + ack. This is intentionally *not* a full
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

use crate::overlay::{Component, Item, components};

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
    /// Both edited the node differently — operator text wins; an ack is emitted.
    OperatorWonConflict,
    /// Operator deleted a node the agent edited/struck — the deletion stands and
    /// the node is omitted from `merged_doc`; an ack is emitted.
    DeletionKept,
    /// Agent deleted/struck a node the operator edited — the operator edit wins
    /// (the node is un-struck / revived); an ack is emitted.
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

/// Why an acknowledgement is required for the next agent turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AckReason {
    /// Operator deleted a node the agent had edited or struck — the agent should
    /// acknowledge the deletion stands.
    OperatorDeletedAgentEditedNode,
    /// Operator and agent edited the same node differently — operator content won.
    SameNodeOperatorOverride,
    /// Agent deleted/struck a node the operator edited — operator revived it.
    OperatorRevivedAgentDeletedNode,
}

impl AckReason {
    /// Stable snake_case token for serialization, logging, and the Phase-4
    /// `#semmerge-ack-turn` carry-forward (`cycle_state` persists the token, not
    /// the enum, so the orchestration crate stays decoupled from this type's
    /// representation).
    pub fn token(&self) -> &'static str {
        match self {
            AckReason::OperatorDeletedAgentEditedNode => "operator_deleted_agent_edited_node",
            AckReason::SameNodeOperatorOverride => "same_node_operator_override",
            AckReason::OperatorRevivedAgentDeletedNode => "operator_revived_agent_deleted_node",
        }
    }
}

/// A request for the next agent turn to acknowledge a non-applied agent change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckRequest {
    /// Component name (`__frontmatter__` for frontmatter keys).
    pub component: String,
    /// The node's stable id (item id, or frontmatter key name).
    pub id: String,
    pub reason: AckReason,
    /// Human-readable detail the next agent turn will acknowledge.
    pub detail: String,
}

/// The result of a [`semantic_merge`]: the merged document plus the per-node
/// outcomes and any acknowledgement requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticMerge {
    pub merged_doc: String,
    pub outcomes: Vec<NodeOutcome>,
    pub requires_ack: Vec<AckRequest>,
}

/// Sentinel component name used for frontmatter scalar key-nodes in outcomes/acks.
pub const FRONTMATTER_COMPONENT: &str = "__frontmatter__";

/// Merge `base`, `ours_agent`, and `theirs_operator` by node identity.
///
/// Pure: no IO, no git, no agent-doc coupling. See the [module docs](self) for the
/// full transition table, the frontmatter handling, and the documented prose
/// reconstruction assumption.
pub fn semantic_merge(base: &str, ours_agent: &str, theirs_operator: &str) -> SemanticMerge {
    let mut outcomes = Vec::new();
    let mut requires_ack = Vec::new();

    // --- Frontmatter merge -------------------------------------------------
    let (base_fm, _base_body) = split_frontmatter(base);
    let (ours_fm, _ours_body) = split_frontmatter(ours_agent);
    let (theirs_fm, theirs_body) = split_frontmatter(theirs_operator);
    let merged_fm = merge_frontmatter_owned(
        base_fm,
        ours_fm,
        theirs_fm,
        &mut outcomes,
        &mut requires_ack,
    );

    // --- Component merge over the operator body (skeleton) -----------------
    let base_comps = components(base);
    let ours_comps = components(ours_agent);
    let theirs_comps = components(theirs_operator);

    let merged_body = merge_components_into_body(
        theirs_body,
        &theirs_comps,
        &base_comps,
        &ours_comps,
        ours_agent,
        &mut outcomes,
        &mut requires_ack,
    );

    let merged_doc = match merged_fm {
        Some(fm) => format!("{fm}{merged_body}"),
        None => merged_body,
    };

    SemanticMerge {
        merged_doc,
        outcomes,
        requires_ack,
    }
}

/// The set of structural nodes considered "active" in the current agent turn —
/// the in-flight prompt and its response area (the `exchange` tail). Used by
/// [`semantic_merge_scoped`] to gate which same-node conflicts raise an
/// [`AckRequest`].
///
/// `#msn6` / `#smturnactive` (semantic_merge Phase 6, turn-active-area merge
/// gating): a same-node operator↔agent collision OUTSIDE the turn-active area
/// auto-resolves to the operator value with no ack noise; only a collision
/// INSIDE the active area produces an ack. The merged document is identical
/// either way (the operator always wins a same-node conflict) — only ack
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
    /// An empty active set. In [`semantic_merge_scoped`] this means "nothing is
    /// active" — every out-of-area conflict auto-resolves and NO acks are emitted.
    /// Callers that want the legacy all-active behavior call [`semantic_merge`].
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

/// Like [`semantic_merge`], but scope ack emission to a turn-active node-set
/// (`#msn6` / `#smturnactive`, Phase 6). The merged document and per-node
/// outcomes are IDENTICAL to [`semantic_merge`] — the operator still wins every
/// same-node conflict, so no content is ever lost or changed. Only
/// [`SemanticMerge::requires_ack`] is filtered: a same-node conflict whose node
/// is NOT in `active` auto-resolves silently (no ack noise), while a conflict
/// inside the active area still raises its [`AckRequest`].
///
/// This lets the live-prompt-drift convergence path ack ONLY when the operator's
/// concurrent edit collided with the in-flight turn's own response area, instead
/// of acking every unrelated operator edit (queue strike, backlog tweak,
/// frontmatter flip) that happened to touch the same node the agent did.
pub fn semantic_merge_scoped(
    base: &str,
    ours_agent: &str,
    theirs_operator: &str,
    active: &ActiveNodes,
) -> SemanticMerge {
    let mut sm = semantic_merge(base, ours_agent, theirs_operator);
    sm.requires_ack
        .retain(|ack| active.is_active(&ack.component, &ack.id));
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
    acks: &mut Vec<AckRequest>,
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
        let chosen = if operator_changed { theirs } else { ours.or(theirs) };
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
                            // operator wins + ack
                            acks.push(AckRequest {
                                component: FRONTMATTER_COMPONENT.to_string(),
                                id: key.clone(),
                                reason: AckReason::SameNodeOperatorOverride,
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
                        (b.clone().or_else(|| t.clone()).or_else(|| o.clone()), Some(OutcomeKind::Keep))
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

fn find_comp<'a>(comps: &'a [Component], name: &str) -> Option<&'a Component> {
    comps.iter().find(|c| c.name == name)
}

/// The raw bullet line for an item, reconstructed from `Item.raw`. We re-emit a
/// `- ` bullet to keep a stable, parseable shape (the overlay strips the bullet
/// into `raw`, so we restore one). Backlog tasks already carry their `N.`/`[ ]`
/// in `raw`; we still prefix `- ` only when `raw` does not already begin with a
/// list/ordered bullet shape, to avoid double bullets.
fn item_line(item: &Item) -> String {
    let raw = item.raw.trim();
    format!("- {raw}")
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
    acks: &mut Vec<AckRequest>,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    // 1. Walk operator items in operator order — they are the layout skeleton.
    for t_item in &theirs_comp.items {
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
                        lines.push(item_line(t_item));
                        record(outcomes, name, id, OutcomeKind::Convergent);
                    } else {
                        // Same-node conflict — operator wins + ack.
                        lines.push(item_line(t_item));
                        record(outcomes, name, id, OutcomeKind::OperatorWonConflict);
                        acks.push(AckRequest {
                            component: name.to_string(),
                            id: id.clone(),
                            reason: AckReason::SameNodeOperatorOverride,
                            detail: format!(
                                "{name} `{id}`: operator content won over the agent's edit"
                            ),
                        });
                    }
                } else if agent_changed {
                    // Agent edited, operator unchanged.
                    // Detect agent-delete-via-strike vs operator-edit handled below;
                    // here operator is unchanged so just apply the agent edit.
                    lines.push(item_line(o));
                    record(outcomes, name, id, OutcomeKind::AppliedAgentEdit);
                } else if operator_changed {
                    // Operator edited, agent unchanged.
                    lines.push(item_line(t_item));
                    record(outcomes, name, id, OutcomeKind::AppliedOperatorEdit);
                } else {
                    // Unchanged on both.
                    lines.push(item_line(t_item));
                    record(outcomes, name, id, OutcomeKind::Keep);
                }
            }
            // Present in base, absent in agent → agent deleted it.
            (Some(b), None) => {
                if operator_changed {
                    // Agent deleted (absent), operator edited → operator revives.
                    let _ = b;
                    lines.push(item_line(t_item));
                    record(outcomes, name, id, OutcomeKind::OperatorRevived);
                    acks.push(AckRequest {
                        component: name.to_string(),
                        id: id.clone(),
                        reason: AckReason::OperatorRevivedAgentDeletedNode,
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
                    lines.push(item_line(t_item));
                    record(outcomes, name, id, OutcomeKind::AppliedBothAdd);
                } else {
                    // Same id, different text → operator wins + ack.
                    lines.push(item_line(t_item));
                    record(outcomes, name, id, OutcomeKind::OperatorWonConflict);
                    acks.push(AckRequest {
                        component: name.to_string(),
                        id: id.clone(),
                        reason: AckReason::SameNodeOperatorOverride,
                        detail: format!(
                            "{name} `{id}`: both added with different text — operator's version kept"
                        ),
                    });
                }
            }
            // Absent in base, absent in agent → operator-only add.
            (None, None) => {
                lines.push(item_line(t_item));
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
                    lines.push(item_line(o_item));
                    record(outcomes, name, id, OutcomeKind::AppliedAgentAdd);
                }
                Some(b) => {
                    // Present in base, absent in operator → operator deleted it.
                    let agent_edited = !item_unchanged(o_item, b);
                    if agent_edited {
                        // Deletion stands + ack (operator deleted an agent-edited node).
                        record(outcomes, name, id, OutcomeKind::DeletionKept);
                        acks.push(AckRequest {
                            component: name.to_string(),
                            id: id.clone(),
                            reason: AckReason::OperatorDeletedAgentEditedNode,
                            detail: format!(
                                "{name} `{id}`: operator deleted the node the agent edited — deletion stands"
                            ),
                        });
                    } else {
                        // Operator deleted an unchanged node → deletion stands, no ack.
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
    acks: &mut Vec<AckRequest>,
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
                acks,
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
    let theirs_keys: std::collections::HashSet<&str> =
        theirs_blocks.iter().map(|b| b.key.as_str()).collect();

    // Determine the insertion point: theirs' inner is `leading` + each block's
    // lines. A trailing `<!-- agent:boundary:… -->` marker line lives at the very
    // end (in theirs' leading lines if there are no blocks, or in the last
    // block's trailing lines). We rebuild the inner explicitly to control where
    // the boundary marker sits relative to the appended blocks.
    let agent_new: Vec<&HeadingBlock> = ours_blocks
        .iter()
        .filter(|b| !theirs_keys.contains(b.key.as_str()))
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
    let boundary_idx = flat.iter().rposition(|l| {
        is_boundary_marker(l.trim_end_matches(['\n', '\r']).trim())
    });

    // Build the appended-turns text once, recording an `AppliedAgentAdd` per turn.
    // A trailing boundary-marker line that the block scan swept into the agent's
    // last turn belongs to the structural skeleton (theirs already carries it),
    // so it is stripped here to avoid emitting a duplicate boundary marker.
    let mut appended = String::new();
    for b in &agent_new {
        record(outcomes, EXCHANGE_COMPONENT, &b.key, OutcomeKind::AppliedAgentAdd);
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
                out.push_str(l);
            }
            out.push_str(&appended);
            for l in &flat[idx..] {
                out.push_str(l);
            }
        }
        None => {
            for l in &flat {
                out.push_str(l);
            }
            out.push_str(&appended);
        }
    }
    out
}

/// Recognize an `<!-- agent:boundary:HASH -->` marker line.
fn is_boundary_marker(trimmed: &str) -> bool {
    let Some(inner) = trimmed.strip_prefix("<!--").and_then(|s| s.strip_suffix("-->")) else {
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
/// recovers the verbatim inner content the heading-block append needs. Markers
/// inside fenced code are not a concern for the exchange (responses are prose),
/// and the open/close grammar mirrors the overlay's marker recognizers.
fn component_inner_lines(name: &str, source: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut inside = false;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']).trim();
        if !inside {
            if open_marker_name(trimmed).as_deref() == Some(name) {
                inside = true;
            }
            continue;
        }
        if close_marker_name(trimmed).as_deref() == Some(name) {
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

/// Recognize an `<!-- agent:name attrs -->` open marker line → name.
/// Mirrors [`crate::overlay`] marker grammar without depending on its byte spans
/// (the overlay's `Component::end_byte` is only reliable for the `agent:/name`
/// close spelling, so this merge does its own line scan).
fn open_marker_name(trimmed: &str) -> Option<String> {
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let rest = inner.strip_prefix("agent:")?;
    if rest.starts_with('/') {
        return None; // close marker
    }
    let name = rest.split_whitespace().next()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// Recognize a close marker line in either spelling (`<!-- /agent:name -->` or
/// `<!-- agent:/name -->`) → name.
fn close_marker_name(trimmed: &str) -> Option<String> {
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let rest = inner
        .strip_prefix("/agent:")
        .or_else(|| inner.strip_prefix("agent:/"))?;
    let name = rest.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Rebuild the operator body, replacing each component's inner items with the
/// merged item set, and appending any agent-only components at the end.
///
/// Line-scanning: the operator body is the layout skeleton. Non-component lines
/// pass through verbatim. When an open marker is seen, all lines up to the close
/// marker are dropped and replaced by the merged item lines for that component.
#[allow(clippy::too_many_arguments)]
fn merge_components_into_body(
    theirs_body: &str,
    theirs_comps: &[Component],
    base_comps: &[Component],
    ours_comps: &[Component],
    ours_source: &str,
    outcomes: &mut Vec<NodeOutcome>,
    acks: &mut Vec<AckRequest>,
) -> String {
    let mut out = String::new();
    let mut open: Option<String> = None; // currently-open component name
    // While inside the `exchange` component, the inner lines are buffered verbatim
    // (heading-prose `### Re:` turns are not list items, so the item merge cannot
    // reconstruct them; they must be preserved as-is). At the close marker the
    // buffer is rewritten by [`merge_exchange_inner`] to append agent-new turns.
    let mut exchange_inner: Vec<String> = Vec::new();

    for line in theirs_body.split_inclusive('\n') {
        let content = line.trim_end_matches('\n');
        let trimmed = content.trim();

        if let Some(name) = open.as_ref() {
            // Inside a component: look for its close marker.
            if let Some(close) = close_marker_name(trimmed)
                && &close == name
            {
                let theirs_comp = find_comp(theirs_comps, name);
                let base_comp = find_comp(base_comps, name);
                let ours_comp = find_comp(ours_comps, name);
                if name == EXCHANGE_COMPONENT {
                    // Exchange: preserve theirs' inner lines verbatim, then append
                    // any `### Re:` heading-blocks that exist only in the agent body.
                    let merged_inner = merge_exchange_inner(
                        &exchange_inner,
                        base_comp,
                        ours_comp,
                        theirs_comp,
                        ours_source,
                        outcomes,
                        acks,
                    );
                    out.push_str(&merged_inner);
                    exchange_inner.clear();
                } else if let Some(tc) = theirs_comp {
                    let merged = merge_component_items(
                        name, base_comp, ours_comp, tc, outcomes, acks,
                    );
                    for ml in &merged {
                        out.push_str(ml);
                        out.push('\n');
                    }
                }
                out.push_str(line); // close marker line, verbatim
                open = None;
            } else if name == EXCHANGE_COMPONENT {
                // Buffer the exchange inner line verbatim for the append pass.
                exchange_inner.push(line.to_string());
            }
            // else: drop the original inner line (replaced by merged items above)
            continue;
        }

        // Not inside a component.
        if let Some(name) = open_marker_name(trimmed) {
            out.push_str(line); // open marker line, verbatim
            open = Some(name);
            continue;
        }
        // Plain prose line — pass through.
        out.push_str(line);
    }

    // An unterminated operator component (no recognized close) — flush its items.
    if let Some(name) = open.take() {
        let base_comp = find_comp(base_comps, &name);
        let ours_comp = find_comp(ours_comps, &name);
        if name == EXCHANGE_COMPONENT {
            let theirs_comp = find_comp(theirs_comps, &name);
            let merged_inner = merge_exchange_inner(
                &exchange_inner,
                base_comp,
                ours_comp,
                theirs_comp,
                ours_source,
                outcomes,
                acks,
            );
            if !out.ends_with('\n') && !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&merged_inner);
        } else if let Some(tc) = find_comp(theirs_comps, &name) {
            let merged = merge_component_items(&name, base_comp, ours_comp, tc, outcomes, acks);
            if !out.ends_with('\n') && !out.is_empty() {
                out.push('\n');
            }
            for ml in &merged {
                out.push_str(ml);
                out.push('\n');
            }
        }
    }

    // Append agent-only components (present in agent, absent in operator).
    for oc in ours_comps {
        if find_comp(theirs_comps, &oc.name).is_some() {
            continue;
        }
        // Every item in an agent-only component is an agent add (or revival of a
        // base node the operator dropped along with the component). Classify per id.
        let base_comp = find_comp(base_comps, &oc.name);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_reason_tokens_are_stable() {
        // #semmerge-ack-turn (Phase 4): these tokens are the wire format
        // cycle_state persists and preflight surfaces — keep them stable.
        assert_eq!(
            AckReason::OperatorDeletedAgentEditedNode.token(),
            "operator_deleted_agent_edited_node"
        );
        assert_eq!(
            AckReason::SameNodeOperatorOverride.token(),
            "same_node_operator_override"
        );
        assert_eq!(
            AckReason::OperatorRevivedAgentDeletedNode.token(),
            "operator_revived_agent_deleted_node"
        );
    }

    // ----- helpers ---------------------------------------------------------

    fn q(items: &str) -> String {
        format!("<!-- agent:queue -->\n{items}<!-- /agent:queue -->\n")
    }

    fn outcome_for<'a>(m: &'a SemanticMerge, id: &str) -> Option<&'a NodeOutcome> {
        m.outcomes.iter().find(|o| o.id == id)
    }

    fn reparses_to_ids(doc: &str, comp: &str) -> Vec<String> {
        components(doc)
            .into_iter()
            .find(|c| c.name == comp)
            .map(|c| c.items.into_iter().map(|i| i.id).collect())
            .unwrap_or_default()
    }

    // ----- transition table: one test per row ------------------------------

    #[test]
    fn row_present_unchanged_unchanged_keep() {
        let base = q("- do [#a] task\n");
        let m = semantic_merge(&base, &base, &base);
        assert_eq!(outcome_for(&m, "a").unwrap().kind, OutcomeKind::Keep);
        assert!(m.requires_ack.is_empty());
        assert_eq!(reparses_to_ids(&m.merged_doc, "queue"), vec!["a"]);
    }

    #[test]
    fn row_present_agent_edited_operator_unchanged() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task EDITED\n");
        let theirs = base.clone();
        let m = semantic_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::AppliedAgentEdit
        );
        assert!(m.merged_doc.contains("EDITED"));
        assert!(m.requires_ack.is_empty());
    }

    #[test]
    fn row_present_agent_unchanged_operator_edited() {
        let base = q("- do [#a] task\n");
        let ours = base.clone();
        let theirs = q("- do [#a] task OPEDIT\n");
        let m = semantic_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::AppliedOperatorEdit
        );
        assert!(m.merged_doc.contains("OPEDIT"));
        assert!(m.requires_ack.is_empty());
    }

    #[test]
    fn row_present_both_edited_same_text_convergent() {
        let base = q("- do [#a] task\n");
        let same = q("- do [#a] task SAME\n");
        let m = semantic_merge(&base, &same, &same);
        assert_eq!(outcome_for(&m, "a").unwrap().kind, OutcomeKind::Convergent);
        assert!(m.merged_doc.contains("SAME"));
        // Applied once.
        assert_eq!(m.merged_doc.matches("[#a]").count(), 1);
        assert!(m.requires_ack.is_empty());
    }

    #[test]
    fn row_present_both_edited_different_operator_wins() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task AGENT\n");
        let theirs = q("- do [#a] task OPERATOR\n");
        let m = semantic_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::OperatorWonConflict
        );
        assert!(m.merged_doc.contains("OPERATOR"));
        assert!(!m.merged_doc.contains("AGENT"));
        assert_eq!(
            m.requires_ack[0].reason,
            AckReason::SameNodeOperatorOverride
        );
    }

    // ----- #msn6 / #smturnactive: turn-active-area ack gating --------------

    #[test]
    fn scoped_conflict_outside_active_area_drops_ack_but_keeps_operator_value() {
        // A same-node conflict in the `queue` component. Unscoped, it acks.
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task AGENT\n");
        let theirs = q("- do [#a] task OPERATOR\n");

        let unscoped = semantic_merge(&base, &ours, &theirs);
        assert_eq!(unscoped.requires_ack.len(), 1, "unscoped acks the conflict");

        // Scope the active area to `exchange` only — the queue conflict is OUTSIDE
        // it, so the ack is dropped while the merged doc + outcome are unchanged.
        let active = ActiveNodes::new().active_component("exchange");
        let scoped = semantic_merge_scoped(&base, &ours, &theirs, &active);
        assert!(
            scoped.requires_ack.is_empty(),
            "out-of-active-area conflict auto-resolves with no ack"
        );
        assert_eq!(
            scoped.merged_doc, unscoped.merged_doc,
            "merged value identical (operator still wins)"
        );
        assert_eq!(
            outcome_for(&scoped, "a").unwrap().kind,
            OutcomeKind::OperatorWonConflict,
            "outcome record is unchanged — only ack emission is scoped"
        );
        assert!(scoped.merged_doc.contains("OPERATOR"));
        assert!(!scoped.merged_doc.contains("AGENT"));
    }

    #[test]
    fn scoped_conflict_inside_active_area_keeps_ack() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task AGENT\n");
        let theirs = q("- do [#a] task OPERATOR\n");

        // The conflict lives in `queue`; marking `queue` active keeps the ack.
        let active = ActiveNodes::new().active_component("queue");
        let scoped = semantic_merge_scoped(&base, &ours, &theirs, &active);
        assert_eq!(scoped.requires_ack.len(), 1, "active-area collision acks");
        assert_eq!(
            scoped.requires_ack[0].reason,
            AckReason::SameNodeOperatorOverride
        );
    }

    #[test]
    fn scoped_active_set_is_node_granular() {
        // Two conflicting nodes; only one is marked active by exact id.
        let base = q("- do [#a] one\n- do [#b] two\n");
        let ours = q("- do [#a] one AGENT\n- do [#b] two AGENT\n");
        let theirs = q("- do [#a] one OPERATOR\n- do [#b] two OPERATOR\n");

        let active = ActiveNodes::new().with_node("queue", "a");
        let scoped = semantic_merge_scoped(&base, &ours, &theirs, &active);
        assert_eq!(scoped.requires_ack.len(), 1, "only the active node acks");
        assert_eq!(scoped.requires_ack[0].id, "a");
        // Both operator values still win regardless of ack scoping.
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
        let scoped = semantic_merge_scoped(&base, &ours, &theirs, &active);
        assert!(
            scoped.requires_ack.is_empty(),
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
        let m = semantic_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::DeletionKept
        );
        assert!(!m.merged_doc.contains("[#a]"), "deleted node omitted");
        assert_eq!(
            m.requires_ack[0].reason,
            AckReason::OperatorDeletedAgentEditedNode
        );
    }

    #[test]
    fn row_agent_deleted_operator_edited_operator_revived() {
        let base = q("- do [#a] task\n- do [#b] other\n");
        // agent deletes a (absent)
        let ours = q("- do [#b] other\n");
        // operator edits a
        let theirs = q("- do [#a] task OPEDIT\n- do [#b] other\n");
        let m = semantic_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "a").unwrap().kind,
            OutcomeKind::OperatorRevived
        );
        assert!(m.merged_doc.contains("OPEDIT"), "operator edit revived");
        assert_eq!(
            m.requires_ack[0].reason,
            AckReason::OperatorRevivedAgentDeletedNode
        );
    }

    #[test]
    fn row_absent_agent_added_operator_absent() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task\n- do [#new] fresh\n");
        let theirs = base.clone();
        let m = semantic_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "new").unwrap().kind,
            OutcomeKind::AppliedAgentAdd
        );
        assert!(m.merged_doc.contains("[#new]"));
        assert!(m.requires_ack.is_empty());
    }

    #[test]
    fn row_absent_operator_added_agent_absent() {
        let base = q("- do [#a] task\n");
        let ours = base.clone();
        let theirs = q("- do [#a] task\n- do [#opnew] op-fresh\n");
        let m = semantic_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "opnew").unwrap().kind,
            OutcomeKind::AppliedOperatorAdd
        );
        assert!(m.merged_doc.contains("[#opnew]"));
        assert!(m.requires_ack.is_empty());
    }

    #[test]
    fn row_both_added_different_ids_union() {
        let base = q("- do [#a] task\n");
        let ours = q("- do [#a] task\n- do [#agentadd] x\n");
        let theirs = q("- do [#a] task\n- do [#opadd] y\n");
        let m = semantic_merge(&base, &ours, &theirs);
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
        assert!(m.requires_ack.is_empty());
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
        let m = semantic_merge(&base, &ours, &theirs);
        assert_eq!(
            outcome_for(&m, "dup").unwrap().kind,
            OutcomeKind::OperatorWonConflict
        );
        assert!(m.merged_doc.contains("OPERATOR-VERSION"));
        assert!(!m.merged_doc.contains("AGENT-VERSION"));
        assert_eq!(
            m.requires_ack[0].reason,
            AckReason::SameNodeOperatorOverride
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
        let m = semantic_merge(base, ours, theirs);

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

        // Zero conflicts / acks — fully node-disjoint.
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
        assert!(m.requires_ack.is_empty(), "no acks: {:?}", m.requires_ack);

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
        let m = semantic_merge(&base, &ours, &theirs);
        assert!(!m.merged_doc.contains("[#y]"), "y deleted");
        assert_eq!(
            outcome_for(&m, "y").unwrap().kind,
            OutcomeKind::DeletionKept
        );
        assert_eq!(m.requires_ack.len(), 1);
        assert_eq!(
            m.requires_ack[0].reason,
            AckReason::OperatorDeletedAgentEditedNode
        );
        assert_eq!(m.requires_ack[0].id, "y");
    }

    #[test]
    fn same_node_different_edit_operator_wins_with_ack() {
        let base = q("- do [#z] orig\n");
        let ours = q("- do [#z] agent version\n");
        let theirs = q("- do [#z] operator version\n");
        let m = semantic_merge(&base, &ours, &theirs);
        assert!(m.merged_doc.contains("operator version"));
        assert!(!m.merged_doc.contains("agent version"));
        assert_eq!(m.requires_ack.len(), 1);
        assert_eq!(
            m.requires_ack[0].reason,
            AckReason::SameNodeOperatorOverride
        );
    }

    #[test]
    fn node_disjoint_union_of_adds_both_present() {
        let base = q("- do [#base] b\n");
        let ours = q("- do [#base] b\n- do [#a1] agent one\n- do [#a2] agent two\n");
        let theirs = q("- do [#base] b\n- do [#o1] op one\n");
        let m = semantic_merge(&base, &ours, &theirs);
        for id in ["base", "a1", "a2", "o1"] {
            assert!(
                m.merged_doc.contains(&format!("[#{id}]")),
                "{id} present in {}",
                m.merged_doc
            );
        }
        assert!(m.requires_ack.is_empty());
    }

    #[test]
    fn frontmatter_scalar_operator_only_edit_applied_no_ack() {
        let base = "---\nqueue: start\nmodel: opus\n---\n# body\n";
        let ours = base.to_string();
        let theirs = "---\nqueue: stop\nmodel: opus\n---\n# body\n";
        let m = semantic_merge(base, &ours, theirs);
        assert!(m.merged_doc.contains("queue: stop"));
        assert!(m.merged_doc.contains("model: opus"));
        let fm_outcome = m
            .outcomes
            .iter()
            .find(|o| o.component == FRONTMATTER_COMPONENT && o.id == "queue")
            .unwrap();
        assert_eq!(fm_outcome.kind, OutcomeKind::AppliedOperatorEdit);
        assert!(m.requires_ack.is_empty());
    }

    #[test]
    fn frontmatter_both_edit_different_operator_wins_with_ack() {
        let base = "---\nqueue: start\n---\n# body\n";
        let ours = "---\nqueue: paused\n---\n# body\n";
        let theirs = "---\nqueue: stop\n---\n# body\n";
        let m = semantic_merge(base, ours, theirs);
        assert!(m.merged_doc.contains("queue: stop"));
        assert_eq!(m.requires_ack.len(), 1);
        assert_eq!(
            m.requires_ack[0].reason,
            AckReason::SameNodeOperatorOverride
        );
        assert_eq!(m.requires_ack[0].component, FRONTMATTER_COMPONENT);
    }

    #[test]
    fn frontmatter_agent_only_edit_applied() {
        let base = "---\nqueue: start\nmodel: opus\n---\n# body\n";
        let ours = "---\nqueue: start\nmodel: sonnet\n---\n# body\n";
        let theirs = base.to_string();
        let m = semantic_merge(base, ours, &theirs);
        assert!(m.merged_doc.contains("model: sonnet"));
        assert!(m.requires_ack.is_empty());
    }

    #[test]
    fn prose_skeleton_is_operator() {
        // Documented assumption: non-component prose comes from the operator buffer.
        let base = "intro\n\n<!-- agent:queue -->\n- do [#a] t\n<!-- /agent:queue -->\n";
        let ours = "AGENT-PROSE\n\n<!-- agent:queue -->\n- do [#a] t\n<!-- /agent:queue -->\n";
        let theirs = "OPERATOR-PROSE\n\n<!-- agent:queue -->\n- do [#a] t\n<!-- /agent:queue -->\n";
        let m = semantic_merge(base, ours, theirs);
        assert!(m.merged_doc.contains("OPERATOR-PROSE"));
        assert!(!m.merged_doc.contains("AGENT-PROSE"));
    }

    // ----- exchange heading-prose turns (`#semmerge` real format) ----------

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
        let m = semantic_merge(base, ours, theirs);

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
        assert!(m.merged_doc.contains("queue: stop"), "operator fm flip: {}", m.merged_doc);
        assert!(m.merged_doc.contains("[#opadd]"), "operator queue add: {}", m.merged_doc);
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
        let m = semantic_merge(base, ours, &theirs);

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
        let m = semantic_merge(base, ours, &theirs);

        // New turn appended.
        assert!(m.merged_doc.contains("### Re: B — opus-4-8"), "B present: {}", m.merged_doc);
        // Exactly one boundary marker.
        assert_eq!(
            m.merged_doc.matches("<!-- agent:boundary:abc123 -->").count(),
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
}

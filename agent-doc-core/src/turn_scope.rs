//! # Module: turn_scope
//!
//! TurnScope manifest — phase 2 of the operation-scoped drift model
//! (`#op-scoped-drift-2`, `tasks/agent-doc/plan-operation-scoped-drift.md`).
//!
//! ## Spec
//! - An `Address` names an addressable location in the document: a component
//!   occurrence, optionally narrowed to a specific node key. A `node_key` of
//!   `None` is a whole-component address.
//! - A `TurnScope` is the operation manifest for the current turn: the `driver`
//!   node it is answering, the `read_set` it depends on, and the `write_set` it
//!   will mutate. It is emitted at turn start from preflight `prompt_targets`.
//! - `TurnScope::for_driver` builds the canonical scope: read `{driver,
//!   exchange tail}`; write `{exchange append, driver strike, backlog, status,
//!   review}`. The driver also appears in the write set because the turn
//!   strikes the queue item it consumes.
//!
//! ## Agentic Contracts
//! - The manifest is the substrate the phase-3 affectedness classifier reads;
//!   phase 2 only emits it. It never blocks a cycle.
//! - When no driver node resolves (a non-queue prompt), `driver` is `None` and
//!   the sets cover only the output components every turn touches.

use serde::{Deserialize, Serialize};

/// An addressable location in the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    /// Component name (`queue`, `exchange`, `backlog`, …).
    pub component: String,
    /// Zero-based occurrence of the component in the document.
    pub occurrence: usize,
    /// Stable node key when the address is narrowed to a single node; `None`
    /// addresses the whole component occurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_key: Option<String>,
}

impl Address {
    /// A whole-component address.
    pub fn component(name: &str, occurrence: usize) -> Self {
        Address {
            component: name.to_string(),
            occurrence,
            node_key: None,
        }
    }

    /// A node-level address inside a component.
    pub fn node(component: &str, occurrence: usize, node_key: &str) -> Self {
        Address {
            component: component.to_string(),
            occurrence,
            node_key: Some(node_key.to_string()),
        }
    }

    /// Build a node address from a component name and a stable node key,
    /// parsing the component occurrence out of the `comp:occurrence:id:n` node
    /// key. This is the single source of occurrence parsing shared by
    /// [`op_address`] and the finalize-path drift gate, so an op's address and
    /// the scope's driver address agree for the same node key.
    pub fn from_component_node_key(component: &str, node_key: &str) -> Self {
        let occurrence = node_key
            .split(':')
            .nth(1)
            .and_then(|field| field.parse().ok())
            .unwrap_or(0);
        Address::node(component, occurrence, node_key)
    }

    /// True when this address targets a whole component rather than one node.
    pub fn is_component_level(&self) -> bool {
        self.node_key.is_none()
    }

    /// True when two addresses refer to overlapping document regions: the same
    /// component, and either side is component-level (whole component) or both
    /// name the same node key. This is the `overlaps` primitive behind the
    /// affectedness `conflict` relation (`#op-scoped-drift-3`).
    ///
    /// Overlap is keyed on the component name plus the node key. The `occurrence`
    /// field is informational and is intentionally *not* part of the match: a
    /// node key already encodes the component index (`comp:index:id:n`), and the
    /// canonical write-set members built by [`TurnScope::for_driver`] are
    /// whole-component addresses (`occurrence: 0`) that must match the component
    /// wherever it sits in the document (`#nm1x`).
    pub fn overlaps(&self, other: &Address) -> bool {
        if self.component != other.component {
            return false;
        }
        match (&self.node_key, &other.node_key) {
            (Some(a), Some(b)) => a == b,
            // A component-level address overlaps any node in that component.
            _ => true,
        }
    }
}

/// The operation manifest for the current turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TurnScope {
    /// The driver node the turn is answering, when one resolves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<Address>,
    /// Addresses the turn depends on (input).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_set: Vec<Address>,
    /// Addresses the turn will mutate (output).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write_set: Vec<Address>,
    /// Exchange node-index floor for the turn's active tail
    /// (`#loop-guard-exchange-node-granularity`). The `exchange` component is
    /// whole-component in `read_set`/`write_set`, but only its *tail* — nodes at
    /// or after this index (the active turn's appends and edits) — affects the
    /// turn. An edit to an OLD exchange block (index below the floor) is
    /// `Independent` and must not preempt the auto-loop drain. `None` keeps the
    /// coarse whole-component behavior (no exchange nodes / unknown tail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange_tail_floor: Option<usize>,
}

impl TurnScope {
    /// Build the canonical turn scope for an optional driver queue node, with no
    /// exchange-tail narrowing (whole-component exchange affectedness).
    pub fn for_driver(driver: Option<Address>) -> Self {
        Self::for_driver_with_exchange_tail(driver, None)
    }

    /// Build the canonical turn scope for an optional driver queue node, narrowing
    /// the `exchange` component to its active tail at `exchange_tail_floor`.
    ///
    /// read_set = `{driver, exchange tail}`; write_set = `{exchange append,
    /// driver strike, backlog, status, review}`. Addresses are deduped while
    /// preserving first-seen order so the manifest is stable. `exchange_tail_floor`
    /// is the count of exchange nodes present at turn start: a later op at an index
    /// at or above it is a tail append/edit (affects the turn); an op below it
    /// edits committed history and is independent.
    pub fn for_driver_with_exchange_tail(
        driver: Option<Address>,
        exchange_tail_floor: Option<usize>,
    ) -> Self {
        let exchange = Address::component("exchange", 0);
        let mut read_set = Vec::new();
        if let Some(driver) = &driver {
            read_set.push(driver.clone());
        }
        read_set.push(exchange.clone());

        let mut write_set = vec![
            exchange,
            Address::component("backlog", 0),
            Address::component("status", 0),
            Address::component("review", 0),
        ];
        if let Some(driver) = &driver {
            // The turn strikes the queue item it consumes.
            write_set.push(driver.clone());
        }

        TurnScope {
            driver,
            read_set: dedupe_addresses(read_set),
            write_set: dedupe_addresses(write_set),
            exchange_tail_floor,
        }
    }
}

/// The effect class an incoming operation has on the current turn
/// (`#op-scoped-drift-3`). Mirrors the 5-class taxonomy in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AffectednessClass {
    /// Class 1 — target ∉ scope: integrate + persist, no drift.
    Independent,
    /// Class 2 — target ∈ read_set: re-read the affected input.
    InputAffecting,
    /// Class 3 — target ∈ write_set with a concurrent writer: CRDT merge.
    OutputContended,
    /// Class 4 — remove/move of a depended node: invalidate/adapt.
    StructuralDependency,
    /// Class 5 — non-user actor (lagging live-buffer) misread as a user edit: suppress.
    ProvenanceSpoofed,
}

impl AffectednessClass {
    /// Classes 2/3/4 affect the current turn; 1 (independent) and 5 (spoofed)
    /// do not — they integrate/persist or are suppressed without disturbing it.
    pub fn affects_turn(self) -> bool {
        matches!(
            self,
            AffectednessClass::InputAffecting
                | AffectednessClass::OutputContended
                | AffectednessClass::StructuralDependency
        )
    }

    /// Stable lowercase string form.
    pub fn as_str(self) -> &'static str {
        match self {
            AffectednessClass::Independent => "independent",
            AffectednessClass::InputAffecting => "input_affecting",
            AffectednessClass::OutputContended => "output_contended",
            AffectednessClass::StructuralDependency => "structural_dependency",
            AffectednessClass::ProvenanceSpoofed => "provenance_spoofed",
        }
    }
}

/// Resolve `(in_read, in_write)` membership for an op against the scope.
///
/// `#loop-guard-exchange-node-granularity`: when the scope carries an
/// `exchange_tail_floor`, an `exchange` op is in scope only if it targets the
/// active tail — its node index is at or above the floor (a tail append or tail
/// edit). An op below the floor edits committed exchange history and is out of
/// scope (→ `Independent`), so it never preempts the auto-loop drain. The
/// `exchange` component is canonically in both the read set (tail) and the write
/// set (append), so a tail op is in both. Without a floor, or for non-`exchange`
/// components, fall back to whole-component / node-key overlap.
fn scope_membership(
    op_address: &Address,
    op_index: Option<usize>,
    scope: &TurnScope,
) -> (bool, bool) {
    if op_address.component == "exchange"
        && let Some(floor) = scope.exchange_tail_floor
    {
        let is_tail = op_index.is_some_and(|index| index >= floor);
        return (is_tail, is_tail);
    }
    (
        scope.read_set.iter().any(|a| op_address.overlaps(a)),
        scope.write_set.iter().any(|a| op_address.overlaps(a)),
    )
}

/// Classify a single operation against the current turn scope.
///
/// `affects(O,S) = conflict(O.target, read_set) ∨ conflict(O.target, write_set)`.
/// An op that intersects no part of the scope is `Independent`. Within the
/// scope, a `live_buffer` actor is `ProvenanceSpoofed` (a lagging sidecar, not a
/// real edit), a `remove`/`move` is `StructuralDependency`, a read-set
/// intersection is `InputAffecting`, and a write-set-only intersection is
/// `OutputContended`.
///
/// `op_index` is the op's within-component node index (after-index for
/// inserts/replaces, before-index for removes); it narrows `exchange`
/// affectedness to the active tail (`#loop-guard-exchange-node-granularity`).
pub fn classify_op(
    actor: crate::op_log::OpActor,
    op_kind: &str,
    op_address: &Address,
    op_index: Option<usize>,
    scope: &TurnScope,
) -> AffectednessClass {
    let (in_read, in_write) = scope_membership(op_address, op_index, scope);
    if !in_read && !in_write {
        return AffectednessClass::Independent;
    }
    if actor == crate::op_log::OpActor::LiveBuffer {
        return AffectednessClass::ProvenanceSpoofed;
    }
    if matches!(op_kind, "remove" | "move") {
        return AffectednessClass::StructuralDependency;
    }
    if in_read {
        return AffectednessClass::InputAffecting;
    }
    AffectednessClass::OutputContended
}

/// One classified operation in a cycle's affectedness summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedOp {
    pub component: String,
    pub node_key: String,
    pub op_kind: String,
    pub actor: crate::op_log::OpActor,
    pub class: AffectednessClass,
}

/// Aggregate affectedness of a cycle's operations against the turn scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CycleAffectedness {
    /// True when any op falls in a turn-affecting class (2/3/4).
    pub turn_affected: bool,
    /// Per-op classification, in input order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classified: Vec<ClassifiedOp>,
}

/// The component-occurrence-narrowed address of a durable op.
pub fn op_address(op: &crate::op_log::DocumentOp) -> Address {
    Address::from_component_node_key(&op.component, &op.node_key)
}

/// Classify every op in a cycle against the turn scope, routing the coarse
/// all-drift-is-affecting assumption into the 5-class taxonomy. Independent and
/// provenance-spoofed ops do not affect the turn.
pub fn classify_cycle(ops: &[crate::op_log::DocumentOp], scope: &TurnScope) -> CycleAffectedness {
    let classified: Vec<ClassifiedOp> = ops
        .iter()
        .map(|op| {
            let address = op_address(op);
            let class = classify_op(op.actor, &op.op_kind, &address, op.node_index, scope);
            ClassifiedOp {
                component: op.component.clone(),
                node_key: op.node_key.clone(),
                op_kind: op.op_kind.clone(),
                actor: op.actor,
                class,
            }
        })
        .collect();
    let turn_affected = classified.iter().any(|op| op.class.affects_turn());
    CycleAffectedness {
        turn_affected,
        classified,
    }
}

fn dedupe_addresses(addresses: Vec<Address>) -> Vec<Address> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for address in addresses {
        if seen.contains(&address) {
            continue;
        }
        seen.push(address.clone());
        out.push(address);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_driver_with_queue_node_covers_input_and_output() {
        let driver = Address::node("queue", 0, "queue:0:op-scoped-drift-2:0");
        let scope = TurnScope::for_driver(Some(driver.clone()));
        assert_eq!(scope.driver.as_ref(), Some(&driver));
        // read set: driver first, then exchange tail.
        assert_eq!(scope.read_set[0], driver);
        assert!(scope.read_set.contains(&Address::component("exchange", 0)));
        // write set: exchange append, backlog, status, review, plus the strike.
        assert!(scope.write_set.contains(&Address::component("exchange", 0)));
        assert!(scope.write_set.contains(&Address::component("backlog", 0)));
        assert!(scope.write_set.contains(&Address::component("status", 0)));
        assert!(scope.write_set.contains(&Address::component("review", 0)));
        assert!(scope.write_set.contains(&driver));
    }

    #[test]
    fn for_driver_without_node_only_lists_output_components() {
        let scope = TurnScope::for_driver(None);
        assert!(scope.driver.is_none());
        assert_eq!(scope.read_set, vec![Address::component("exchange", 0)]);
        assert!(scope.write_set.contains(&Address::component("backlog", 0)));
        // No queue node, so nothing addresses the queue component.
        assert!(scope.write_set.iter().all(|a| a.component != "queue"));
    }

    #[test]
    fn addresses_are_deduped_preserving_order() {
        let exchange = Address::component("exchange", 0);
        let deduped = dedupe_addresses(vec![
            exchange.clone(),
            Address::component("backlog", 0),
            exchange.clone(),
        ]);
        assert_eq!(deduped, vec![exchange, Address::component("backlog", 0)]);
    }

    #[test]
    fn turn_scope_serde_round_trips() {
        let scope = TurnScope::for_driver(Some(Address::node("queue", 0, "queue:0:alpha:0")));
        let json = serde_json::to_string(&scope).unwrap();
        let parsed: TurnScope = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, scope);
    }

    #[test]
    fn overlaps_matches_component_level_and_same_node() {
        let comp = Address::component("queue", 0);
        let node_a = Address::node("queue", 0, "queue:0:a:0");
        let node_b = Address::node("queue", 0, "queue:0:b:0");
        // component-level overlaps any node in the same occurrence.
        assert!(comp.overlaps(&node_a));
        assert!(node_a.overlaps(&comp));
        // distinct nodes don't overlap.
        assert!(!node_a.overlaps(&node_b));
        // occurrence is informational: a whole-component address matches the
        // component regardless of the occurrence field (#nm1x).
        assert!(comp.overlaps(&Address::component("queue", 1)));
        assert!(Address::component("queue", 1).overlaps(&node_a));
        // different component never overlaps.
        assert!(!comp.overlaps(&Address::component("backlog", 0)));
    }

    fn scope_for(node_key: &str) -> TurnScope {
        TurnScope::for_driver(Some(Address::node("queue", 0, node_key)))
    }

    #[test]
    fn classify_independent_when_outside_scope() {
        use crate::op_log::OpActor;
        let scope = scope_for("queue:0:driver:0");
        // An insert into the icebox (not in any scope set) is independent.
        let addr = Address::node("icebox", 0, "icebox:0:note:0");
        assert_eq!(
            classify_op(OpActor::User, "insert", &addr, None, &scope),
            AffectednessClass::Independent
        );
        // A *different* queue node beside the running one is also independent.
        let sibling = Address::node("queue", 0, "queue:0:other:0");
        assert_eq!(
            classify_op(OpActor::User, "insert", &sibling, None, &scope),
            AffectednessClass::Independent
        );
    }

    #[test]
    fn classify_input_affecting_on_driver_edit() {
        use crate::op_log::OpActor;
        let scope = scope_for("queue:0:driver:0");
        let driver = Address::node("queue", 0, "queue:0:driver:0");
        assert_eq!(
            classify_op(OpActor::User, "replace", &driver, None, &scope),
            AffectednessClass::InputAffecting
        );
    }

    #[test]
    fn classify_structural_on_driver_removal() {
        use crate::op_log::OpActor;
        let scope = scope_for("queue:0:driver:0");
        let driver = Address::node("queue", 0, "queue:0:driver:0");
        assert_eq!(
            classify_op(OpActor::User, "remove", &driver, None, &scope),
            AffectednessClass::StructuralDependency
        );
    }

    #[test]
    fn classify_output_contended_on_write_set_only() {
        use crate::op_log::OpActor;
        let scope = scope_for("queue:0:driver:0");
        // backlog is in the write set but not the read set.
        let backlog = Address::node("backlog", 0, "backlog:0:item:0");
        assert_eq!(
            classify_op(
                OpActor::ForeignSupervisor,
                "replace",
                &backlog,
                None,
                &scope
            ),
            AffectednessClass::OutputContended
        );
    }

    #[test]
    fn classify_provenance_spoofed_for_live_buffer_in_scope() {
        use crate::op_log::OpActor;
        let scope = scope_for("queue:0:driver:0");
        // A live-buffer actor touching an in-scope address is spoofed, not contended.
        let exchange = Address::component("exchange", 0);
        assert_eq!(
            classify_op(OpActor::LiveBuffer, "replace", &exchange, None, &scope),
            AffectednessClass::ProvenanceSpoofed
        );
        // ...but a live-buffer op outside scope is still just independent.
        let icebox = Address::node("icebox", 0, "icebox:0:n:0");
        assert_eq!(
            classify_op(OpActor::LiveBuffer, "replace", &icebox, None, &scope),
            AffectednessClass::Independent
        );
    }

    fn exchange_tail_scope(floor: usize) -> TurnScope {
        TurnScope::for_driver_with_exchange_tail(
            Some(Address::node("queue", 0, "queue:0:driver:0")),
            Some(floor),
        )
    }

    #[test]
    fn exchange_tail_floor_makes_old_block_edit_independent() {
        // #loop-guard-exchange-node-granularity: with a tail floor of 2 (two
        // committed exchange nodes), an edit to an OLD exchange block (index 0)
        // must NOT affect the turn — it is committed history, not the active tail.
        use crate::op_log::OpActor;
        let scope = exchange_tail_scope(2);
        let old_block = Address::node("exchange", 0, "exchange:0:ft-old:0");
        assert_eq!(
            classify_op(OpActor::User, "replace", &old_block, Some(0), &scope),
            AffectednessClass::Independent
        );
        // The paired remove of the same old block is likewise independent.
        assert_eq!(
            classify_op(OpActor::User, "remove", &old_block, Some(0), &scope),
            AffectednessClass::Independent
        );
    }

    #[test]
    fn exchange_tail_floor_keeps_tail_append_affecting() {
        // A genuine mid-loop user prompt appended at the tail (index == floor)
        // still affects the turn and must preempt the drain — no false negative.
        use crate::op_log::OpActor;
        let scope = exchange_tail_scope(2);
        let tail_append = Address::node("exchange", 0, "exchange:0:ft-new:0");
        assert_eq!(
            classify_op(OpActor::User, "insert", &tail_append, Some(2), &scope),
            AffectednessClass::InputAffecting
        );
        // An index beyond the floor (a second appended node) is also tail.
        assert_eq!(
            classify_op(OpActor::User, "insert", &tail_append, Some(5), &scope),
            AffectednessClass::InputAffecting
        );
    }

    #[test]
    fn exchange_tail_floor_suppresses_live_buffer_tail_drift() {
        // A live-buffer actor touching the tail is still ProvenanceSpoofed (a
        // lagging sidecar), not a real contended edit.
        use crate::op_log::OpActor;
        let scope = exchange_tail_scope(2);
        let tail = Address::node("exchange", 0, "exchange:0:ft-x:0");
        assert_eq!(
            classify_op(OpActor::LiveBuffer, "insert", &tail, Some(2), &scope),
            AffectednessClass::ProvenanceSpoofed
        );
    }

    #[test]
    fn exchange_without_floor_keeps_whole_component_behavior() {
        // No floor (None): any exchange op overlaps the whole-component address,
        // preserving the conservative pre-narrowing behavior. Exchange is in the
        // read set, so an exchange edit is InputAffecting regardless of index.
        use crate::op_log::OpActor;
        let scope = scope_for("queue:0:driver:0");
        assert!(scope.exchange_tail_floor.is_none());
        let old_block = Address::node("exchange", 0, "exchange:0:ft-old:0");
        assert_eq!(
            classify_op(OpActor::User, "replace", &old_block, Some(0), &scope),
            AffectednessClass::InputAffecting
        );
    }

    #[test]
    fn classify_cycle_narrows_exchange_old_edit_but_keeps_tail() {
        // End-to-end through classify_cycle: an old-block exchange edit plus a
        // tail append. Only the tail append affects the turn.
        use crate::op_log::{CausalClock, DocumentOp, OpActor};
        let scope = exchange_tail_scope(2);
        let mk = |node_key: &str, op_kind: &str, idx: Option<usize>| DocumentOp {
            document_path: "plan.md".to_string(),
            component: "exchange".to_string(),
            node_key: node_key.to_string(),
            node_index: idx,
            item_id: String::new(),
            op_kind: op_kind.to_string(),
            actor: OpActor::User,
            clock: CausalClock::default(),
            before_preview: None,
            after_preview: None,
            recorded_at: None,
        };
        let old_only = vec![mk("exchange:0:ft-old:0", "replace", Some(0))];
        assert!(
            !classify_cycle(&old_only, &scope).turn_affected,
            "an old-block exchange edit alone must not affect the turn"
        );
        let with_tail = vec![
            mk("exchange:0:ft-old:0", "replace", Some(0)),
            mk("exchange:0:ft-new:0", "insert", Some(2)),
        ];
        assert!(
            classify_cycle(&with_tail, &scope).turn_affected,
            "a tail append must still affect the turn"
        );
    }

    #[test]
    fn classify_cycle_aggregates_turn_affected() {
        use crate::op_log::{CausalClock, DocumentOp, OpActor};
        let scope = scope_for("queue:0:driver:0");
        let mk = |component: &str, node_key: &str, op_kind: &str, actor: OpActor| DocumentOp {
            document_path: "plan.md".to_string(),
            component: component.to_string(),
            node_key: node_key.to_string(),
            node_index: None,
            item_id: String::new(),
            op_kind: op_kind.to_string(),
            actor,
            clock: CausalClock::default(),
            before_preview: None,
            after_preview: None,
            recorded_at: None,
        };
        // Independent op only -> turn not affected.
        let independent = vec![mk("queue", "queue:0:other:0", "insert", OpActor::User)];
        assert!(!classify_cycle(&independent, &scope).turn_affected);

        // Add an input-affecting driver edit -> turn affected.
        let mixed = vec![
            mk("queue", "queue:0:other:0", "insert", OpActor::User),
            mk("queue", "queue:0:driver:0", "replace", OpActor::User),
        ];
        let summary = classify_cycle(&mixed, &scope);
        assert!(summary.turn_affected);
        assert_eq!(summary.classified.len(), 2);
        assert_eq!(summary.classified[0].class, AffectednessClass::Independent);
        assert_eq!(
            summary.classified[1].class,
            AffectednessClass::InputAffecting
        );
    }

    #[test]
    fn affects_turn_matches_taxonomy() {
        assert!(!AffectednessClass::Independent.affects_turn());
        assert!(!AffectednessClass::ProvenanceSpoofed.affects_turn());
        assert!(AffectednessClass::InputAffecting.affects_turn());
        assert!(AffectednessClass::OutputContended.affects_turn());
        assert!(AffectednessClass::StructuralDependency.affects_turn());
    }
}

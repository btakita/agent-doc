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

    /// True when this address targets a whole component rather than one node.
    pub fn is_component_level(&self) -> bool {
        self.node_key.is_none()
    }

    /// True when two addresses refer to overlapping document regions: the same
    /// component occurrence, and either side is component-level or both name the
    /// same node key. This is the `overlaps` primitive behind the affectedness
    /// `conflict` relation (`#op-scoped-drift-3`).
    pub fn overlaps(&self, other: &Address) -> bool {
        if self.component != other.component || self.occurrence != other.occurrence {
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
}

impl TurnScope {
    /// Build the canonical turn scope for an optional driver queue node.
    ///
    /// read_set = `{driver, exchange tail}`; write_set = `{exchange append,
    /// driver strike, backlog, status, review}`. Addresses are deduped while
    /// preserving first-seen order so the manifest is stable.
    pub fn for_driver(driver: Option<Address>) -> Self {
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

/// Classify a single operation against the current turn scope.
///
/// `affects(O,S) = conflict(O.target, read_set) ∨ conflict(O.target, write_set)`.
/// An op that intersects no part of the scope is `Independent`. Within the
/// scope, a `live_buffer` actor is `ProvenanceSpoofed` (a lagging sidecar, not a
/// real edit), a `remove`/`move` is `StructuralDependency`, a read-set
/// intersection is `InputAffecting`, and a write-set-only intersection is
/// `OutputContended`.
pub fn classify_op(
    actor: crate::op_log::OpActor,
    op_kind: &str,
    op_address: &Address,
    scope: &TurnScope,
) -> AffectednessClass {
    let in_read = scope.read_set.iter().any(|a| op_address.overlaps(a));
    let in_write = scope.write_set.iter().any(|a| op_address.overlaps(a));
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
    let occurrence = op
        .node_key
        .split(':')
        .nth(1)
        .and_then(|field| field.parse().ok())
        .unwrap_or(0);
    Address::node(&op.component, occurrence, &op.node_key)
}

/// Classify every op in a cycle against the turn scope, routing the coarse
/// all-drift-is-affecting assumption into the 5-class taxonomy. Independent and
/// provenance-spoofed ops do not affect the turn.
pub fn classify_cycle(ops: &[crate::op_log::DocumentOp], scope: &TurnScope) -> CycleAffectedness {
    let classified: Vec<ClassifiedOp> = ops
        .iter()
        .map(|op| {
            let address = op_address(op);
            let class = classify_op(op.actor, &op.op_kind, &address, scope);
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
        assert_eq!(
            deduped,
            vec![exchange, Address::component("backlog", 0)]
        );
    }

    #[test]
    fn turn_scope_serde_round_trips() {
        let scope = TurnScope::for_driver(Some(Address::node(
            "queue",
            0,
            "queue:0:alpha:0",
        )));
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
        // different occurrence never overlaps.
        assert!(!comp.overlaps(&Address::component("queue", 1)));
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
            classify_op(OpActor::User, "insert", &addr, &scope),
            AffectednessClass::Independent
        );
        // A *different* queue node beside the running one is also independent.
        let sibling = Address::node("queue", 0, "queue:0:other:0");
        assert_eq!(
            classify_op(OpActor::User, "insert", &sibling, &scope),
            AffectednessClass::Independent
        );
    }

    #[test]
    fn classify_input_affecting_on_driver_edit() {
        use crate::op_log::OpActor;
        let scope = scope_for("queue:0:driver:0");
        let driver = Address::node("queue", 0, "queue:0:driver:0");
        assert_eq!(
            classify_op(OpActor::User, "replace", &driver, &scope),
            AffectednessClass::InputAffecting
        );
    }

    #[test]
    fn classify_structural_on_driver_removal() {
        use crate::op_log::OpActor;
        let scope = scope_for("queue:0:driver:0");
        let driver = Address::node("queue", 0, "queue:0:driver:0");
        assert_eq!(
            classify_op(OpActor::User, "remove", &driver, &scope),
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
            classify_op(OpActor::ForeignSupervisor, "replace", &backlog, &scope),
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
            classify_op(OpActor::LiveBuffer, "replace", &exchange, &scope),
            AffectednessClass::ProvenanceSpoofed
        );
        // ...but a live-buffer op outside scope is still just independent.
        let icebox = Address::node("icebox", 0, "icebox:0:n:0");
        assert_eq!(
            classify_op(OpActor::LiveBuffer, "replace", &icebox, &scope),
            AffectednessClass::Independent
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

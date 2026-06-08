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
}

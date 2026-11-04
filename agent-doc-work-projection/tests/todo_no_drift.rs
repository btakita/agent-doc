//! `#managedlistnodrift` property tests for `agent:todo`.
//!
//! Durable state: the todo items (operator records) plus the tracked-work
//! index. Resolution is monotone here: closing a todo is a lifecycle
//! transition recorded in the item's own checkbox, and only the operator
//! reopens one.

use std::collections::BTreeSet;

use agent_doc_element::todo::{TodoLinkIndex, project_todo};
use agent_doc_work_projection::managed_list::{laws, missing_operator_lines};
use proptest::prelude::*;

#[derive(Debug, Clone, PartialEq)]
struct State {
    doc: String,
    resolved: BTreeSet<u8>,
}

#[derive(Debug, Clone)]
enum Event {
    /// The operator types a new item (possibly reusing an id).
    Add(u8),
    /// The operator types a prose item with no id.
    AddProse(u8),
    /// The operator checks the first open item with this id.
    Check(u8),
    /// Tracked work with this id is durably resolved.
    Resolve(u8),
}

fn links(resolved: &BTreeSet<u8>) -> TodoLinkIndex {
    let mut links = TodoLinkIndex::default();
    for id in resolved {
        links.mark_resolved(&format!("t{id}"));
    }
    links
}

fn apply(state: &State, event: &Event) -> State {
    let mut next = state.clone();
    match event {
        Event::Add(id) => {
            next.doc = next.doc.replace(
                "<!-- /agent:todo -->",
                &format!("- [ ] [#t{id}] do thing {id}\n<!-- /agent:todo -->"),
            );
        }
        Event::AddProse(n) => {
            next.doc = next.doc.replace(
                "<!-- /agent:todo -->",
                &format!("- [ ] remember prose {n}\n<!-- /agent:todo -->"),
            );
        }
        Event::Check(id) => {
            let needle = format!("- [ ] [#t{id}] ");
            next.doc = next.doc.replacen(&needle, &format!("- [x] [#t{id}] "), 1);
        }
        Event::Resolve(id) => {
            next.resolved.insert(*id);
        }
    }
    next
}

fn render(state: &State) -> State {
    let projected = project_todo(&state.doc, &links(&state.resolved))
        .unwrap()
        .content
        .unwrap_or_else(|| state.doc.clone());
    State {
        doc: projected,
        resolved: state.resolved.clone(),
    }
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        (0u8..5).prop_map(Event::Add),
        (0u8..3).prop_map(Event::AddProse),
        (0u8..5).prop_map(Event::Check),
        (0u8..5).prop_map(Event::Resolve),
    ]
}

fn operator_text(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed
        .strip_prefix("- [ ] ")
        .or_else(|| trimmed.strip_prefix("- [x] "))?;
    Some(rest.to_string())
}

proptest! {
    #[test]
    fn todo_render_is_history_free_and_idempotent(events in proptest::collection::vec(event(), 0..24)) {
        let initial = State {
            doc: "## Todo <!-- agent:todo -->\n<!-- /agent:todo -->\n".to_string(),
            resolved: BTreeSet::new(),
        };
        let rendered = laws::incremental_equals_from_scratch(&initial, &events, apply, render)
            .map_err(TestCaseError::fail)?;
        // Every operator-typed item survives with its text unchanged.
        let raw = events.iter().fold(initial, |state, event| apply(&state, event));
        prop_assert!(missing_operator_lines(&raw.doc, &rendered.doc, operator_text).is_empty());
        // An open item never names resolved work by its own id.
        for id in &rendered.resolved {
            let open_marker = format!("- [ ] [#t{id}] ");
            prop_assert!(!rendered.doc.contains(&open_marker), "{}", rendered.doc);
        }
    }
}

//! `#managedlistnodrift` property tests for `agent:backlog`, `agent:review`,
//! `agent:icebox`, and `agent:done`.
//!
//! Durable state: the tracked-item records (id, checkbox state, operator
//! text). An item's list is derived from its state; the order of items within
//! a list is operator-owned, so states compare as per-list multisets.

use agent_doc_work_projection::managed_list::{
    ManagedList, ReconcileAction, component_bodies, laws, missing_operator_lines,
};
use agent_doc_work_projection::tracked_lists::project_tracked_lists;
use proptest::prelude::*;

#[derive(Debug, Clone)]
struct Doc(String);

impl Doc {
    fn lists(&self) -> Vec<Vec<String>> {
        [
            ManagedList::Backlog,
            ManagedList::Review,
            ManagedList::Icebox,
        ]
        .into_iter()
        .map(|list| {
            let mut lines: Vec<String> = component_bodies(&self.0, list)
                .unwrap()
                .concat()
                .lines()
                .map(str::to_string)
                .collect();
            lines.sort();
            lines
        })
        .collect()
    }
}

impl PartialEq for Doc {
    fn eq(&self, other: &Self) -> bool {
        self.lists() == other.lists()
    }
}

#[derive(Debug, Clone)]
enum Event {
    /// The operator types an open backlog item.
    AddBacklog(u8),
    /// The operator types a gated backlog item.
    AddGated(u8),
    /// The operator parks an item in the icebox.
    AddIcebox(u8),
    /// The operator gates the first open backlog item with this id.
    Gate(u8),
}

const INITIAL: &str = concat!(
    "## Backlog <!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
    "## Review <!-- agent:review -->\n<!-- /agent:review -->\n",
    "## Icebox <!-- agent:icebox -->\n<!-- /agent:icebox -->\n",
);

fn append(doc: &str, close: &str, line: &str) -> String {
    doc.replacen(close, &format!("{line}\n{close}"), 1)
}

fn apply(doc: &Doc, event: &Event) -> Doc {
    Doc(match event {
        Event::AddBacklog(id) => append(
            &doc.0,
            "<!-- /agent:backlog -->",
            &format!("- [ ] [#b{id}] build thing {id}"),
        ),
        Event::AddGated(id) => append(
            &doc.0,
            "<!-- /agent:backlog -->",
            &format!("- [/] [#g{id}] [blocked: vendor {id}] wait for it"),
        ),
        Event::AddIcebox(id) => append(
            &doc.0,
            "<!-- /agent:icebox -->",
            &format!("- [ ] [#i{id}] someday {id}"),
        ),
        Event::Gate(id) => {
            doc.0
                .replacen(&format!("- [ ] [#b{id}] "), &format!("- [/] [#b{id}] "), 1)
        }
    })
}

fn render(doc: &Doc) -> Doc {
    Doc(project_tracked_lists(&doc.0)
        .unwrap()
        .content
        .unwrap_or_else(|| doc.0.clone()))
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        (0u8..4).prop_map(Event::AddBacklog),
        (0u8..4).prop_map(Event::AddGated),
        (0u8..3).prop_map(Event::AddIcebox),
        (0u8..4).prop_map(Event::Gate),
    ]
}

fn operator_text(line: &str) -> Option<String> {
    let line = line.trim();
    line.starts_with("- [").then(|| line.to_string())
}

proptest! {
    #[test]
    fn tracked_render_is_history_free_and_idempotent(events in proptest::collection::vec(event(), 0..24)) {
        let initial = Doc(INITIAL.to_string());
        let rendered = laws::incremental_equals_from_scratch(&initial, &events, apply, render)
            .map_err(TestCaseError::fail)?;
        // Byte-level fixed point (stronger than the multiset comparison).
        prop_assert!(project_tracked_lists(&rendered.0).unwrap().content.is_none());
        // No gated item stays in the backlog; every operator line survives verbatim.
        let backlog = component_bodies(&rendered.0, ManagedList::Backlog).unwrap().concat();
        prop_assert!(!backlog.contains("- [/]"), "{backlog}");
        let raw = events.iter().fold(initial, |doc, event| apply(&doc, event));
        prop_assert!(missing_operator_lines(&raw.0, &rendered.0, operator_text).is_empty());
        // Duplicates are reported, not merged: the line count is unchanged.
        let count = |doc: &str| doc.lines().filter(|line| line.starts_with("- [")).count();
        prop_assert_eq!(count(&raw.0), count(&rendered.0));
        let duplicates = project_tracked_lists(&rendered.0)
            .unwrap()
            .actions
            .into_iter()
            .filter(|action| matches!(action, ReconcileAction::DuplicateId { .. }))
            .count();
        let mut ids: Vec<&str> = rendered.0.lines().filter_map(|line| line.split("[#").nth(1)?.split(']').next()).collect();
        ids.sort();
        let mut distinct = ids.clone();
        distinct.dedup();
        let duplicated = distinct.iter().filter(|id| ids.iter().filter(|seen| seen == id).count() > 1).count();
        prop_assert_eq!(duplicates, duplicated);
    }
}

//! `#managedlistnodrift`: render(durable state) for the tracked-work lists
//! `agent:backlog`, `agent:review`, `agent:icebox`, and `agent:done`.
//!
//! The durable state is the tracked-item records themselves: each item's id,
//! checkbox state (open `[ ]`, gated `[/...]`, done `[x]`), and the operator's
//! text. The list an item belongs in is derived from its state:
//!
//! | state                 | list            |
//! |-----------------------|-----------------|
//! | open `[ ]`            | where it is (backlog or icebox) |
//! | gated `[/]`, `[/type]`| `agent:review`  |
//! | done `[x]`            | `agent:done` (archived by pending maintenance) |
//!
//! This projection moves a gated backlog item, verbatim, into `agent:review`
//! and reports a stable id that names more than one open item. It never
//! deletes or rewrites operator text and never merges duplicates. Archiving
//! done items stays with pending maintenance (`archive_pending_done`), which
//! this module's [`done_drift`] observes for the invariant check.

use agent_doc_element::element;
use agent_doc_element_backlog::backlog::{self, PendingState};
use anyhow::{Context, Result};

use crate::managed_list::{ManagedList, ReconcileAction};

/// What [`project_tracked_lists`] derived.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackedListsProjection {
    /// The reconciled document, or `None` at a fixed point.
    pub content: Option<String>,
    /// Every move made and every duplicate found, in document order.
    pub actions: Vec<ReconcileAction>,
}

fn list_of(component: &str) -> Option<ManagedList> {
    if element::is_backlog_component(component) {
        Some(ManagedList::Backlog)
    } else if element::is_review_component(component) {
        Some(ManagedList::Review)
    } else if element::is_icebox_component(component) {
        Some(ManagedList::Icebox)
    } else {
        None
    }
}

/// `render(durable state)` for the tracked lists.
///
/// A gated item in the first `agent:backlog` moves, with its continuation
/// lines, to the end of the first `agent:review`. When the document has no
/// `agent:review`, nothing moves (preflight's `legacy_gated_in_backlog`
/// warning still names `agent-doc migrate`, which creates the component).
pub fn project_tracked_lists(content: &str) -> Result<TrackedListsProjection> {
    let mut actions = Vec::new();
    let mut current = content.to_string();
    let components = element::parse(&current).context("tracked lists: parse components")?;
    let has_review = components
        .iter()
        .any(|component| element::is_review_component(&component.name));
    if has_review
        && let Some(backlog) = components
            .iter()
            .find(|component| element::is_backlog_component(&component.name))
    {
        let (new_backlog, gated) =
            backlog::op_take_items_by_state(backlog.content(&current), PendingState::Gated);
        if !gated.is_empty() {
            actions.extend(gated.iter().map(|item| ReconcileAction::Migrate {
                id: item.id.clone(),
                from: ManagedList::Backlog,
                to: ManagedList::Review,
            }));
            current = backlog.replace_content(&current, &new_backlog);
            let review = element::parse(&current)?
                .into_iter()
                .find(|component| element::is_review_component(&component.name))
                .context("tracked lists: agent:review vanished while moving gated items")?;
            let new_review = backlog::op_append_items(review.content(&current), &gated);
            current = review.replace_content(&current, &new_review);
        }
    }
    actions.extend(duplicate_open_ids(&current)?);
    Ok(TrackedListsProjection {
        content: (current != content).then_some(current),
        actions,
    })
}

/// Stable ids that name more than one open (not done) item across the
/// tracked lists, reported in first-seen order with the list that first holds
/// the id.
pub fn duplicate_open_ids(content: &str) -> Result<Vec<ReconcileAction>> {
    let components = element::parse(content).context("tracked lists: parse components")?;
    let mut order: Vec<(String, ManagedList)> = Vec::new();
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for component in &components {
        let Some(list) = list_of(&component.name) else {
            continue;
        };
        let (_, items, _) = backlog::parse_items(component.content(content));
        for item in items.into_iter().filter(|item| !item.is_done()) {
            let id = backlog::normalize_pending_id(&item.id);
            if id.is_empty() {
                continue;
            }
            let count = counts.entry(id.clone()).or_default();
            if *count == 0 {
                order.push((id, list));
            }
            *count += 1;
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|(id, list)| {
            let rows = counts[&id];
            (rows > 1).then_some(ReconcileAction::DuplicateId { list, id, rows })
        })
        .collect())
}

/// A done `[x]` item still living in backlog, review, or icebox: the first
/// one, as the `agent:done` invariant's mismatch. Pending maintenance archives
/// it into `agent:done` at the next preflight.
pub fn done_drift(content: &str) -> Result<Option<(ManagedList, String)>> {
    let components = element::parse(content).context("tracked lists: parse components")?;
    for component in &components {
        let Some(list) = list_of(&component.name) else {
            continue;
        };
        let (_, items, _) = backlog::parse_items(component.content(content));
        if let Some(item) = items.into_iter().find(|item| item.is_done()) {
            return Ok(Some((list, format!("[#{}] {}", item.id, item.text))));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = concat!(
        "## Backlog <!-- agent:backlog queue=\"main\" -->\n",
        "- [ ] [#open] Open work\n",
        "- [/] [#gate] [blocked: vendor] Waits on a vendor\n",
        "  - operator note under the gate\n",
        "- [/operator-verify] [#verify] Needs a human check\n",
        "- [ ] [#twice] also parked\n",
        "<!-- /agent:backlog -->\n",
        "## Review <!-- agent:review -->\n",
        "- [/] [#existing] Already here\n",
        "<!-- /agent:review -->\n",
        "## Icebox <!-- agent:icebox -->\n",
        "- [ ] [#twice] parked copy\n",
        "<!-- /agent:icebox -->\n",
    );

    #[test]
    fn gated_backlog_items_move_verbatim_and_duplicates_are_reported() {
        let projection = project_tracked_lists(DOC).unwrap();
        let projected = projection.content.clone().expect("gated items move");
        let backlog = crate::managed_list::component_bodies(&projected, ManagedList::Backlog)
            .unwrap()[0]
            .to_string();
        let review = crate::managed_list::component_bodies(&projected, ManagedList::Review)
            .unwrap()[0]
            .to_string();
        assert_eq!(
            backlog,
            "- [ ] [#open] Open work\n- [ ] [#twice] also parked\n"
        );
        assert!(
            review.starts_with("- [/] [#existing] Already here\n"),
            "{review}"
        );
        assert!(
            review.contains(
                "- [/] [#gate] [blocked: vendor] Waits on a vendor\n  - operator note under the gate\n"
            ),
            "{review}"
        );
        assert!(
            review.contains("- [/operator-verify] [#verify] Needs a human check\n"),
            "{review}"
        );
        assert_eq!(
            projection.actions,
            vec![
                ReconcileAction::Migrate {
                    id: "gate".into(),
                    from: ManagedList::Backlog,
                    to: ManagedList::Review,
                },
                ReconcileAction::Migrate {
                    id: "verify".into(),
                    from: ManagedList::Backlog,
                    to: ManagedList::Review,
                },
                ReconcileAction::DuplicateId {
                    list: ManagedList::Backlog,
                    id: "twice".into(),
                    rows: 2,
                },
            ]
        );
        // Every operator line survives, and a second render is a no-op.
        let operator = |line: &str| {
            let line = line.trim();
            (!line.is_empty() && !line.starts_with('#') && !line.starts_with("<!--"))
                .then(|| line.to_string())
        };
        assert!(crate::managed_list::missing_operator_lines(DOC, &projected, operator).is_empty());
        let again = project_tracked_lists(&projected).unwrap();
        assert!(again.content.is_none());
        assert_eq!(again.actions.len(), 1, "the duplicate stays reported");
    }

    #[test]
    fn without_a_review_component_nothing_moves() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "- [/] [#gate] Waits\n",
            "<!-- /agent:backlog -->\n",
        );
        let projection = project_tracked_lists(doc).unwrap();
        assert!(projection.content.is_none());
        assert!(projection.actions.is_empty());
    }

    #[test]
    fn done_drift_names_the_first_unarchived_done_item() {
        assert!(done_drift(DOC).unwrap().is_none());
        let doc = DOC.replace("- [ ] [#open] Open work", "- [x] [#open] Open work");
        assert_eq!(
            done_drift(&doc).unwrap(),
            Some((ManagedList::Backlog, "[#open] Open work".to_string()))
        );
    }
}

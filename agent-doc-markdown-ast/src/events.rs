//! Node-keyed document events for realtime watchers.
//!
//! Phase 6 of `#md-ast-document-model`: derive a small event stream from two
//! markdown projections using the same stable node keys as mutation callers.

use crate::mutations::{self, MutationItemNode};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentNodeEventKind {
    Insert,
    Remove,
    Replace,
    Move,
    Strike,
    Unstrike,
}

impl DocumentNodeEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentNodeEventKind::Insert => "insert",
            DocumentNodeEventKind::Remove => "remove",
            DocumentNodeEventKind::Replace => "replace",
            DocumentNodeEventKind::Move => "move",
            DocumentNodeEventKind::Strike => "strike",
            DocumentNodeEventKind::Unstrike => "unstrike",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentNodeEvent {
    pub component: String,
    pub node_key: String,
    pub kind: DocumentNodeEventKind,
    pub item_id: String,
    pub before_index: Option<usize>,
    pub after_index: Option<usize>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub previous_node_key: Option<String>,
    pub next_node_key: Option<String>,
}

/// Diff two markdown projections into node-keyed item events.
pub fn diff_node_events(before: &str, after: &str) -> Vec<DocumentNodeEvent> {
    if before == after {
        return Vec::new();
    }

    let before_nodes = mutations::all_item_nodes(before);
    let after_nodes = mutations::all_item_nodes(after);
    if before_nodes.is_empty() && after_nodes.is_empty() {
        return Vec::new();
    }

    let before_by_key = nodes_by_key(&before_nodes);
    let after_by_key = nodes_by_key(&after_nodes);
    let mut events = Vec::new();

    for node in &before_nodes {
        if !after_by_key.contains_key(node.node_key.as_str()) {
            events.push(DocumentNodeEvent {
                component: node.component.clone(),
                node_key: node.node_key.clone(),
                kind: DocumentNodeEventKind::Remove,
                item_id: node.item.id.clone(),
                before_index: Some(node.index),
                after_index: None,
                before: Some(node_source(before, node)),
                after: None,
                previous_node_key: None,
                next_node_key: None,
            });
        }
    }

    for (absolute_after_index, node) in after_nodes.iter().enumerate() {
        if before_by_key.contains_key(node.node_key.as_str()) {
            continue;
        }
        let (previous_node_key, next_node_key) =
            insert_anchors(&after_nodes, absolute_after_index, &before_by_key);
        events.push(DocumentNodeEvent {
            component: node.component.clone(),
            node_key: node.node_key.clone(),
            kind: DocumentNodeEventKind::Insert,
            item_id: node.item.id.clone(),
            before_index: None,
            after_index: Some(node.index),
            before: None,
            after: Some(node_source(after, node)),
            previous_node_key,
            next_node_key,
        });
    }

    for node in &before_nodes {
        let Some(after_node) = after_by_key.get(node.node_key.as_str()) else {
            continue;
        };
        let before_source = node_source(before, node);
        let after_source = node_source(after, after_node);
        if before_source == after_source {
            continue;
        }
        let kind = if !node.item.struck && after_node.item.struck {
            DocumentNodeEventKind::Strike
        } else if node.item.struck && !after_node.item.struck {
            DocumentNodeEventKind::Unstrike
        } else {
            DocumentNodeEventKind::Replace
        };
        events.push(DocumentNodeEvent {
            component: node.component.clone(),
            node_key: node.node_key.clone(),
            kind,
            item_id: node.item.id.clone(),
            before_index: Some(node.index),
            after_index: Some(after_node.index),
            before: Some(before_source),
            after: Some(after_source),
            previous_node_key: None,
            next_node_key: None,
        });
    }

    for scope in shared_component_scopes(&before_nodes, &after_by_key) {
        let before_shared = before_nodes
            .iter()
            .filter(|node| {
                component_scope(&node.node_key) == scope
                    && after_by_key.contains_key(node.node_key.as_str())
            })
            .map(|node| node.node_key.as_str())
            .collect::<Vec<_>>();
        let after_shared = after_nodes
            .iter()
            .filter(|node| {
                component_scope(&node.node_key) == scope
                    && before_by_key.contains_key(node.node_key.as_str())
            })
            .map(|node| node.node_key.as_str())
            .collect::<Vec<_>>();
        if before_shared == after_shared {
            continue;
        }

        for (index, node_key) in after_shared.iter().enumerate() {
            if before_shared.get(index).copied() == Some(*node_key) {
                continue;
            }
            let Some(node) = after_by_key.get(*node_key) else {
                continue;
            };
            events.push(DocumentNodeEvent {
                component: node.component.clone(),
                node_key: (*node_key).to_string(),
                kind: DocumentNodeEventKind::Move,
                item_id: node.item.id.clone(),
                before_index: before_by_key
                    .get(*node_key)
                    .map(|before_node| before_node.index),
                after_index: Some(node.index),
                before: None,
                after: None,
                previous_node_key: after_shared[..index].last().map(|key| (*key).to_string()),
                next_node_key: after_shared.get(index + 1).map(|key| (*key).to_string()),
            });
        }
    }

    events
}

fn nodes_by_key(nodes: &[MutationItemNode]) -> HashMap<&str, &MutationItemNode> {
    nodes
        .iter()
        .map(|node| (node.node_key.as_str(), node))
        .collect()
}

fn node_source(source: &str, node: &MutationItemNode) -> String {
    source
        .get(node.item.start_byte..node.item.end_byte)
        .unwrap_or(&node.item.raw)
        .to_string()
}

fn insert_anchors(
    nodes: &[MutationItemNode],
    index: usize,
    existing: &HashMap<&str, &MutationItemNode>,
) -> (Option<String>, Option<String>) {
    let previous_node_key = nodes[..index]
        .iter()
        .rev()
        .find(|node| existing.contains_key(node.node_key.as_str()))
        .map(|node| node.node_key.clone());
    let next_node_key = nodes[index + 1..]
        .iter()
        .find(|node| existing.contains_key(node.node_key.as_str()))
        .map(|node| node.node_key.clone());
    (previous_node_key, next_node_key)
}

fn shared_component_scopes(
    before_nodes: &[MutationItemNode],
    after_by_key: &HashMap<&str, &MutationItemNode>,
) -> BTreeSet<String> {
    before_nodes
        .iter()
        .filter(|node| after_by_key.contains_key(node.node_key.as_str()))
        .map(|node| component_scope(&node.node_key))
        .collect()
}

fn component_scope(node_key: &str) -> String {
    let mut parts = node_key.splitn(3, ':');
    match (parts.next(), parts.next()) {
        (Some(component), Some(index)) => format!("{component}:{index}"),
        _ => node_key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEFORE: &str = "\
<!-- agent:queue -->
- do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";

    #[test]
    fn diff_node_events_reports_insert_with_anchors() {
        let after = "\
<!-- agent:queue -->
- do [#alpha]
- do [#beta]
- do [#gamma]
<!-- /agent:queue -->
";

        let events = diff_node_events(BEFORE, after);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, DocumentNodeEventKind::Insert);
        assert_eq!(events[0].component, "queue");
        assert_eq!(events[0].node_key, "queue:0:gamma:0");
        assert_eq!(
            events[0].previous_node_key.as_deref(),
            Some("queue:0:beta:0")
        );
        assert_eq!(events[0].next_node_key, None);
    }

    #[test]
    fn diff_node_events_reports_strike_by_stable_node_key() {
        let after = "\
<!-- agent:queue -->
- ~~do [#alpha]~~
- do [#beta]
<!-- /agent:queue -->
";

        let events = diff_node_events(BEFORE, after);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, DocumentNodeEventKind::Strike);
        assert_eq!(events[0].node_key, "queue:0:alpha:0");
        assert_eq!(events[0].before.as_deref(), Some("- do [#alpha]\n"));
        assert_eq!(events[0].after.as_deref(), Some("- ~~do [#alpha]~~\n"));
    }

    #[test]
    fn diff_node_events_reports_reorder_without_text_matching() {
        let after = "\
<!-- agent:queue -->
- do [#beta]
- do [#alpha]
<!-- /agent:queue -->
";

        let events = diff_node_events(BEFORE, after);
        let moves = events
            .iter()
            .filter(|event| event.kind == DocumentNodeEventKind::Move)
            .collect::<Vec<_>>();

        assert_eq!(moves.len(), 2);
        assert!(moves.iter().any(|event| {
            event.node_key == "queue:0:beta:0"
                && event.next_node_key.as_deref() == Some("queue:0:alpha:0")
        }));
        assert!(moves.iter().any(|event| {
            event.node_key == "queue:0:alpha:0"
                && event.previous_node_key.as_deref() == Some("queue:0:beta:0")
        }));
    }

    #[test]
    fn diff_node_events_keeps_same_id_replace_on_same_node_key() {
        let before = "\
<!-- agent:backlog -->
1. [ ] [#task] old wording
<!-- /agent:backlog -->
";
        let after = "\
<!-- agent:backlog -->
1. [ ] [#task] new wording
<!-- /agent:backlog -->
";

        let events = diff_node_events(before, after);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, DocumentNodeEventKind::Replace);
        assert_eq!(events[0].node_key, "backlog:0:task:0");
        assert_eq!(events[0].before_index, Some(0));
        assert_eq!(events[0].after_index, Some(0));
    }
}

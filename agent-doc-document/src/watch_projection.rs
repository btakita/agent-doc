//! Pure watch-daemon document projections.
//!
//! The orchestration daemon owns file reads, notify delivery, ops-log writes,
//! and controller routing. This module owns the deterministic text/event shapes
//! that can be computed from document content alone.

use agent_doc_markdown_ast::events::DocumentNodeEvent;

/// Strip transient boundary markers before watch convergence hashing.
///
/// Boundary marker IDs can shift during editor/IPC writes without changing the
/// meaningful document body. The watch daemon compares this normalized form so
/// boundary-only movement does not trigger reactive feedback.
pub fn strip_boundaries_for_watch_hash(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with(" -->"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Stable hash of the watch-comparable document projection.
pub fn watch_content_hash(content: &str) -> String {
    agent_doc_hash::content_hash(&strip_boundaries_for_watch_hash(content))
}

/// Stable state event id for a delivered file-watch change.
pub fn file_watch_event_id(doc_id: &str, generation: u64, content_hash: &str) -> String {
    format!("file-watch:{doc_id}:{generation}:{content_hash}")
}

/// Project a previous/current document pair into node-keyed watcher events.
pub fn project_watch_node_events(previous: Option<&str>, current: &str) -> Vec<DocumentNodeEvent> {
    previous
        .map(|before| agent_doc_markdown_ast::events::diff_node_events(before, current))
        .unwrap_or_default()
}

/// JSON shape emitted by the watch daemon for one node-keyed document event.
pub fn document_node_event_json(event: &DocumentNodeEvent) -> serde_json::Value {
    serde_json::json!({
        "component": &event.component,
        "node_key": &event.node_key,
        "op": event.kind.as_str(),
        "item_id": &event.item_id,
        "before_index": event.before_index,
        "after_index": event.after_index,
        "before": event.before.as_deref(),
        "after": event.after.as_deref(),
        "previous_node_key": event.previous_node_key.as_deref(),
        "next_node_key": event.next_node_key.as_deref(),
    })
}

/// JSON payload shape emitted to ops-log for node-keyed watcher events.
pub fn document_node_events_payload(file: &str, events: &[DocumentNodeEvent]) -> serde_json::Value {
    serde_json::json!({
        "event": "document_node_events",
        "file": file,
        "events": events.iter().map(document_node_event_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_markdown_ast::events::DocumentNodeEventKind;

    #[test]
    fn watch_content_hash_ignores_boundary_markers() {
        let without_boundary = "before\nbody\nafter";
        let with_boundary = "before\n<!-- agent:boundary:abc123 -->\nbody\n\
            <!-- agent:boundary:def456 -->\nafter";

        assert_eq!(
            watch_content_hash(with_boundary),
            watch_content_hash(without_boundary)
        );
    }

    #[test]
    fn watch_content_hash_changes_with_meaningful_content() {
        assert_ne!(
            watch_content_hash("version 1"),
            watch_content_hash("version 2")
        );
    }

    #[test]
    fn file_watch_event_id_includes_document_generation_and_content_hash() {
        assert_eq!(
            file_watch_event_id("doc-abc", 42, "hash-123"),
            "file-watch:doc-abc:42:hash-123"
        );
    }

    #[test]
    fn project_watch_node_events_is_empty_without_previous_projection() {
        let current = "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n";

        assert!(project_watch_node_events(None, current).is_empty());
    }

    #[test]
    fn project_watch_node_events_reports_node_keyed_insert_after_seed() {
        let before = "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n";
        let after = "<!-- agent:queue -->\n- do [#alpha]\n- do [#beta]\n<!-- /agent:queue -->\n";

        let events = project_watch_node_events(Some(before), after);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, DocumentNodeEventKind::Insert);
        assert!(events[0].node_key.contains(":beta:"));
        assert!(
            events[0]
                .previous_node_key
                .as_deref()
                .is_some_and(|key| key.contains(":alpha:"))
        );
    }

    #[test]
    fn document_node_events_payload_projects_ops_log_json() {
        let before =
            "<!-- agent:backlog -->\n1. [ ] [#task] old wording\n<!-- /agent:backlog -->\n";
        let after = "<!-- agent:backlog -->\n1. [ ] [#task] new wording\n<!-- /agent:backlog -->\n";
        let events = project_watch_node_events(Some(before), after);

        let payload = document_node_events_payload("session.md", &events);

        assert_eq!(payload["event"], "document_node_events");
        assert_eq!(payload["file"], "session.md");
        assert_eq!(payload["events"][0]["component"], "backlog");
        assert_eq!(payload["events"][0]["node_key"], "backlog:0:task:0");
        assert_eq!(payload["events"][0]["op"], "replace");
    }
}

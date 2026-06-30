//! Pure backlog/icebox/pending to queue-sync projection policy.
//!
//! This module owns content-only collection for backlog-backed queue sync,
//! priority ranks, and auto-DAG dependencies. Callers own file IO, snapshots,
//! done archives, tombstones, and queue body mutation.

use std::collections::HashMap;

use agent_doc_element::element;
use agent_doc_element_backlog::backlog;

use crate::document_queue::BacklogQueueSyncMode;

/// Effective backlog-to-queue sync request collected from tracked-work
/// components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogQueueSyncRequest {
    pub mode: BacklogQueueSyncMode,
    pub ids: Vec<String>,
    pub enqueue_ids: Vec<String>,
    pub priority: bool,
}

/// Collect queue sync source ids from tracked-work components.
///
/// `backlog` and the legacy `pending` alias can opt into component-level sync
/// with a `queue` attribute. `icebox` intentionally cannot opt into
/// component-level sync so parked work is not auto-promoted, but explicit
/// per-item enqueue markers are honored for all tracked-work components.
pub fn collect_backlog_queue_sync(
    components: &[element::Component],
    content: &str,
) -> Option<BacklogQueueSyncRequest> {
    let mut mode: Option<BacklogQueueSyncMode> = None;
    let mut ids: Vec<String> = Vec::new();
    let mut enqueue_ids: Vec<String> = Vec::new();
    let mut priority = false;
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        enqueue_ids.extend(backlog::active_enqueue_item_ids(body));
        if comp.name == "icebox" {
            continue;
        }
        let Some(value) = comp.attrs.get("queue") else {
            continue;
        };
        priority |= comp.attrs.contains_key("priority");
        let Some(comp_mode) = BacklogQueueSyncMode::parse(value) else {
            continue;
        };
        if mode.is_none() {
            mode = Some(comp_mode);
        }
        ids.extend(backlog::active_item_ids(body));
    }
    if mode.is_none() && !enqueue_ids.is_empty() {
        mode = Some(BacklogQueueSyncMode::Append);
    }
    ids.extend(enqueue_ids.iter().cloned());
    mode.map(|m| BacklogQueueSyncRequest {
        mode: m,
        ids,
        enqueue_ids,
        priority,
    })
}

/// Build an id->priority-rank map from active tracked-work items.
///
/// First-seen rank wins on duplicate ids across components.
pub fn collect_backlog_priority_ranks(
    components: &[element::Component],
    content: &str,
) -> HashMap<String, u8> {
    let mut rank = HashMap::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, r) in backlog::active_item_priorities(body) {
            rank.entry(id).or_insert(r);
        }
    }
    rank
}

/// Build an id->`after=#id` dependency map from active tracked-work items.
///
/// First-seen deps win on duplicate ids across components; items with no
/// dependency tokens are omitted.
pub fn collect_after_deps(
    components: &[element::Component],
    content: &str,
) -> HashMap<String, Vec<String>> {
    let mut deps = HashMap::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, d) in backlog::active_item_after_deps(body) {
            if !d.is_empty() {
                deps.entry(id).or_insert(d);
            }
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_backlog_queue_sync_reads_mode_and_active_ids() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#a] one\n",
            "- [/] [#g] gated\n",
            "- [ ] [#b] two\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = element::parse(content).unwrap();
        let request = collect_backlog_queue_sync(&components, content)
            .expect("backlog with queue attr should produce a sync request");
        assert_eq!(request.mode, BacklogQueueSyncMode::Sync);
        assert_eq!(request.ids, vec!["a".to_string(), "b".to_string()]);
        assert!(request.enqueue_ids.is_empty());
    }

    #[test]
    fn collect_backlog_queue_sync_reads_enqueue_markers_without_attr() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] :inbox_tray: one\n",
            "- [/] [#g] :inbox_tray: gated\n",
            "- [ ] [#b] unmarked\n",
            "- [ ] [#c] **enqueue** marked\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = element::parse(content).unwrap();
        let request = collect_backlog_queue_sync(&components, content)
            .expect("enqueue markers should produce an append request");
        assert_eq!(request.mode, BacklogQueueSyncMode::Append);
        assert_eq!(request.ids, vec!["a".to_string(), "c".to_string()]);
        assert_eq!(request.enqueue_ids, vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn collect_backlog_queue_sync_none_without_attr() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] one\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = element::parse(content).unwrap();
        assert!(collect_backlog_queue_sync(&components, content).is_none());
    }

    #[test]
    fn collect_backlog_priority_ranks_reads_tracked_work_components() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] priority=5 one\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#b] priority=1 parked\n",
            "<!-- /agent:icebox -->\n",
        );
        let components = element::parse(content).unwrap();
        let ranks = collect_backlog_priority_ranks(&components, content);
        assert_eq!(ranks.get("a"), Some(&5));
        assert_eq!(ranks.get("b"), Some(&1));
    }

    #[test]
    fn collect_after_deps_reads_tracked_work_components() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] after=#root one\n",
            "- [ ] [#b] no deps\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#c] after=#a parked\n",
            "<!-- /agent:icebox -->\n",
        );
        let components = element::parse(content).unwrap();
        let deps = collect_after_deps(&components, content);
        assert_eq!(deps.get("a"), Some(&vec!["root".to_string()]));
        assert_eq!(deps.get("c"), Some(&vec!["a".to_string()]));
        assert!(!deps.contains_key("b"));
    }
}

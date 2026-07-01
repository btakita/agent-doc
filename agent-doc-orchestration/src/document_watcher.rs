//! # Module: document_watcher
//!
//! ## Spec (`#pcpc4` — single controller-owned filesystem watcher, realizes `#pcp4`)
//! Replaces the per-process `notify` watchers (the `watch.rs` daemon watcher +
//! the editor plugin's `WatchService`) with **one controller-owned watcher per
//! document** whose change events feed the session actor (`#pcpc1`). Today two
//! independent watchers observe the same file and each can trigger a reconcile,
//! which is the R1 "two watchers / duplicate-watcher reconcile race". A single
//! owned event stream removes it.
//!
//! This module owns the controller-side gate logic:
//! - a [`WatcherRegistry`] that is **idempotent per document** — a second watch
//!   request for the same document reuses the one registration, so there is
//!   never a duplicate watcher;
//! - a [`DocumentWatchGate`] that coalesces event bursts into one logical change
//!   and suppresses agent-doc's own write echoes (via the existing
//!   `debounce` write-provenance), so the actor sees one settled change stream.
//!
//! The raw event source is abstracted ([`RawWatchEvent`]) so the gate logic is
//! deterministically testable without a live `notify` backend or the editor.
//! Wiring the production `notify` feed and demoting the plugin `WatchService` to
//! a read-only buffer reporter (`#pcp7`) — which needs a Kotlin change plus live
//! IntelliJ verification — is the `#pcpc4`/`#pcpc5` cutover step, kept separate
//! so this controller-side gate lands independently shippable and behind the
//! seam (the live `watch.rs` watcher is untouched).
//!
//! ## Evals
//! - `registry_is_idempotent_one_watcher_per_document`
//! - `registry_distinct_documents_get_distinct_gates`
//! - `gate_coalesces_event_burst_into_one_change`
//! - `gate_emits_change_on_distinct_content`
//! - `gate_suppresses_agent_self_write_echo`
//! - `gate_ignores_non_content_events`
//! - `watch_change_routes_serialized_through_session_actor`

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use agent_doc_document::watch_projection::file_watch_event_id;
use agent_doc_document_realtime::session_ops::SessionOpKind;
use agent_doc_document_realtime::watch_authority::{
    DocumentWatchGate, RawWatchEvent, WatchDelivery, WatchWriteProvenance,
};

use crate::session_actor::document_actor_in;

/// Controller-owned registry of one watch gate per document. Registration is
/// idempotent: the second `register` for a document returns the existing gate,
/// so a document is never watched twice (no duplicate-watcher reconcile race).
#[derive(Default)]
pub struct WatcherRegistry {
    gates: Mutex<HashMap<String, Arc<Mutex<DocumentWatchGate>>>>,
}

impl WatcherRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get-or-create the gate for `doc_id`. Returns the gate and whether it was
    /// newly created (`true` only on the first registration). `file` is the
    /// document path this gate accepts raw events for.
    pub fn register(&self, doc_id: &str, file: &str) -> (Arc<Mutex<DocumentWatchGate>>, bool) {
        let mut gates = self.gates.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(gate) = gates.get(doc_id) {
            return (Arc::clone(gate), false);
        }
        let gate = Arc::new(Mutex::new(DocumentWatchGate::new(file)));
        gates.insert(doc_id.to_string(), Arc::clone(&gate));
        (gate, true)
    }

    /// Whether `doc_id` is currently watched.
    pub fn is_watched(&self, doc_id: &str) -> bool {
        self.gates
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(doc_id)
    }

    /// Drop the watch for `doc_id`, returning whether one existed.
    pub fn unregister(&self, doc_id: &str) -> bool {
        self.gates
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(doc_id)
            .is_some()
    }

    /// Number of watched documents.
    pub fn len(&self) -> usize {
        self.gates.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The process-wide controller-owned watcher registry singleton.
pub fn registry() -> &'static WatcherRegistry {
    static REGISTRY: std::sync::OnceLock<WatcherRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(WatcherRegistry::new)
}

/// Route a settled watch change into the document's session actor (`#pcpc1`), so
/// a watcher-triggered reconcile is serialized against in-flight writes
/// (`#pcpc3`) on the one owner thread. Self-write echoes and coalesced bursts
/// are dropped before they reach the actor. Returns the delivery decision.
pub fn route_event(
    base_dir: &Path,
    doc_id: &str,
    file: &str,
    raw: &RawWatchEvent,
    current_content: &str,
    on_change: impl FnOnce() -> Result<()> + Send + 'static,
) -> Result<WatchDelivery> {
    let (gate, _new) = registry().register(doc_id, file);
    let content_hash = agent_doc_hash::content_hash(current_content);
    let provenance = agent_doc_debounce::write_provenance(file);
    let write_provenance = provenance
        .as_ref()
        .map(|prov| WatchWriteProvenance::new(prov.actor.as_str(), prov.hash.as_str()));
    let delivery = {
        let mut g = gate.lock().unwrap_or_else(|p| p.into_inner());
        g.observe(raw, &content_hash, write_provenance)
    };
    if let WatchDelivery::Change { generation } = delivery {
        let actor = document_actor_in(base_dir, file);
        let event = crate::state_backbone::StateEvent::new(
            file_watch_event_id(doc_id, generation, &content_hash),
            crate::state_backbone::StateFact::FileWatchChangeObserved {
                document_hash: doc_id.to_string(),
                path: file.to_string(),
                watch_generation: generation,
                content_hash,
            },
        );
        let base_dir = base_dir.to_path_buf();
        actor.submit(SessionOpKind::FileWatch, move |_ctx| -> Result<()> {
            crate::project_controller::append_state_event(&base_dir, &event)?;
            on_change()
        })??;
    }
    Ok(delivery)
}

#[cfg(test)]
mod tests {
    use super::{WatcherRegistry, registry, route_event};
    use agent_doc_document_realtime::watch_authority::{RawWatchEvent, WatchDelivery};
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    fn seed(dir: &tempfile::TempDir, rel: &str, content: &str) -> PathBuf {
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join(rel);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, content).unwrap();
        file
    }

    #[test]
    fn registry_is_idempotent_one_watcher_per_document() {
        let reg = WatcherRegistry::new();
        let (g1, new1) = reg.register("doc-A", "/tmp/doc-A.md");
        let (g2, new2) = reg.register("doc-A", "/tmp/doc-A.md");
        assert!(new1, "first registration is new");
        assert!(!new2, "second registration must reuse the one watcher");
        assert!(Arc::ptr_eq(&g1, &g2), "same gate for the same document");
        assert_eq!(reg.len(), 1, "exactly one watcher for the document");
        assert!(reg.is_watched("doc-A"));
        assert!(reg.unregister("doc-A"));
        assert!(!reg.is_watched("doc-A"));
    }

    #[test]
    fn registry_distinct_documents_get_distinct_gates() {
        let reg = WatcherRegistry::new();
        let (a, _) = reg.register("doc-A", "/tmp/a.md");
        let (b, _) = reg.register("doc-B", "/tmp/b.md");
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn watch_change_routes_serialized_through_session_actor() {
        // A settled change routes a reconcile op into the document's session
        // actor, so it serializes with writes on the one owner thread. Two
        // distinct changes deliver two ordered reconciles.
        let dir = tempfile::TempDir::new().unwrap();
        let file = seed(&dir, "doc.md", "");
        let file_str = file.to_string_lossy().to_string();
        let doc_id = format!("watch-route-{}", file_str.len());
        let raw = RawWatchEvent::modify(&file);

        let reconciles = Arc::new(AtomicU64::new(0));

        let r = reconciles.clone();
        let d1 = route_event(
            dir.path(),
            &doc_id,
            &file_str,
            &raw,
            "change one",
            move || {
                r.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(d1, WatchDelivery::Change { generation: 1 });

        // A coalesced burst event does NOT route to the actor.
        let r = reconciles.clone();
        let d2 = route_event(
            dir.path(),
            &doc_id,
            &file_str,
            &raw,
            "change one",
            move || {
                r.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(d2, WatchDelivery::Coalesced);

        let r = reconciles.clone();
        let d3 = route_event(
            dir.path(),
            &doc_id,
            &file_str,
            &raw,
            "change two",
            move || {
                r.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(d3, WatchDelivery::Change { generation: 2 });

        // Exactly two reconciles ran (the two real changes, not the coalesced one).
        assert_eq!(reconciles.load(Ordering::SeqCst), 2);
        let projection = crate::project_controller::load_state_backbone_projection(dir.path())
            .expect("ledger projection should reload from sqlite");
        let document = projection
            .document(&doc_id)
            .expect("watch event persisted document projection");
        assert_eq!(
            document
                .document
                .latest_file_watch_change
                .as_ref()
                .map(|change| change.watch_generation),
            Some(2)
        );
        registry().unregister(&doc_id);
    }
}

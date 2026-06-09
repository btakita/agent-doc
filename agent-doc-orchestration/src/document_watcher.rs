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
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::session_actor::{document_actor_in, SessionOpKind};

/// Minimal classification of a raw filesystem event, mirroring the
/// `notify::EventKind` subset `watch.rs` reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawKind {
    /// File content modified.
    Modify,
    /// File (re)created.
    Create,
    /// Anything else (access, metadata, remove) — ignored as a change source.
    Other,
}

/// A raw event delivered by the underlying watcher backend.
#[derive(Debug, Clone)]
pub struct RawWatchEvent {
    pub path: PathBuf,
    pub kind: RawKind,
}

impl RawWatchEvent {
    pub fn modify(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            kind: RawKind::Modify,
        }
    }
}

/// What the gate decided about a raw event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchDelivery {
    /// A new settled change the actor should reconcile. `generation` increments
    /// per distinct delivered change.
    Change { generation: u64 },
    /// Same content as the last delivered change — a coalesced burst event.
    Coalesced,
    /// The current content matches agent-doc's own most recent write — a
    /// self-write echo, suppressed so the agent never reconciles its own write.
    SelfWriteEcho,
    /// Not a content-bearing event (or a path mismatch).
    Ignored,
}

/// Per-document coalescing + self-write-suppression gate. One gate exists per
/// document in the [`WatcherRegistry`], so every raw event for that document —
/// regardless of which backend produced it — funnels through one place.
pub struct DocumentWatchGate {
    /// Provenance-lookup key (the document path string, as `debounce` keys it).
    file: String,
    last_delivered_hash: Option<String>,
    generation: u64,
}

impl DocumentWatchGate {
    fn new(file: String) -> Self {
        Self {
            file,
            last_delivered_hash: None,
            generation: 0,
        }
    }

    /// Number of distinct changes delivered so far.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Classify a raw event given the document's `current_content` (already read
    /// from disk by the caller). Pure decision logic — no I/O beyond the
    /// provenance lookup — so it is deterministically testable.
    pub fn observe(&mut self, raw: &RawWatchEvent, current_content: &str) -> WatchDelivery {
        if !matches!(raw.kind, RawKind::Modify | RawKind::Create) {
            return WatchDelivery::Ignored;
        }
        let hash = crate::debounce::content_hash(current_content);

        // Self-write echo: agent-doc just wrote exactly this content. Suppress so
        // the watcher never feeds our own write back as a user change.
        if let Some(prov) = crate::debounce::write_provenance(&self.file)
            && prov.actor == "agent"
            && prov.hash == hash
        {
            return WatchDelivery::SelfWriteEcho;
        }

        // Coalesce a burst: identical content to the last delivered change is not
        // a new change.
        if self.last_delivered_hash.as_deref() == Some(hash.as_str()) {
            return WatchDelivery::Coalesced;
        }

        self.last_delivered_hash = Some(hash);
        self.generation += 1;
        WatchDelivery::Change {
            generation: self.generation,
        }
    }
}

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
    /// provenance-lookup path key.
    pub fn register(&self, doc_id: &str, file: &str) -> (Arc<Mutex<DocumentWatchGate>>, bool) {
        let mut gates = self
            .gates
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(gate) = gates.get(doc_id) {
            return (Arc::clone(gate), false);
        }
        let gate = Arc::new(Mutex::new(DocumentWatchGate::new(file.to_string())));
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
    on_change: impl FnOnce() + Send + 'static,
) -> Result<WatchDelivery> {
    let (gate, _new) = registry().register(doc_id, file);
    let delivery = {
        let mut g = gate.lock().unwrap_or_else(|p| p.into_inner());
        g.observe(raw, current_content)
    };
    if let WatchDelivery::Change { .. } = delivery {
        let actor = document_actor_in(base_dir, file);
        actor.submit(SessionOpKind::QueueHead, move |_ctx| on_change())?;
    }
    Ok(delivery)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

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
    fn gate_coalesces_event_burst_into_one_change() {
        let mut gate = DocumentWatchGate::new("/tmp/none.md".into());
        let raw = RawWatchEvent::modify("/tmp/none.md");
        // A burst of identical-content events: only the first is a Change.
        assert_eq!(
            gate.observe(&raw, "version one"),
            WatchDelivery::Change { generation: 1 }
        );
        for _ in 0..5 {
            assert_eq!(gate.observe(&raw, "version one"), WatchDelivery::Coalesced);
        }
        assert_eq!(gate.generation(), 1, "burst delivered exactly one change");
    }

    #[test]
    fn gate_emits_change_on_distinct_content() {
        let mut gate = DocumentWatchGate::new("/tmp/none.md".into());
        let raw = RawWatchEvent::modify("/tmp/none.md");
        assert_eq!(
            gate.observe(&raw, "A"),
            WatchDelivery::Change { generation: 1 }
        );
        assert_eq!(
            gate.observe(&raw, "B"),
            WatchDelivery::Change { generation: 2 }
        );
        assert_eq!(gate.observe(&raw, "B"), WatchDelivery::Coalesced);
    }

    #[test]
    fn gate_suppresses_agent_self_write_echo() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = seed(&dir, "doc.md", "agent wrote this\n");
        let file_str = file.to_string_lossy().to_string();

        // Record agent-doc's own write of the current content.
        let content = "agent wrote this\n";
        let hash = crate::debounce::content_hash(content);
        crate::debounce::record_write_provenance(&file_str, content.len(), &hash, "wid-1", "agent")
            .unwrap();

        let mut gate = DocumentWatchGate::new(file_str.clone());
        let raw = RawWatchEvent::modify(&file);
        // The watcher fires for our own write → suppressed, no change delivered.
        assert_eq!(gate.observe(&raw, content), WatchDelivery::SelfWriteEcho);
        assert_eq!(gate.generation(), 0);

        // A genuine subsequent user edit (different content) is delivered.
        assert_eq!(
            gate.observe(&raw, "user edited this\n"),
            WatchDelivery::Change { generation: 1 }
        );
    }

    #[test]
    fn gate_ignores_non_content_events() {
        let mut gate = DocumentWatchGate::new("/tmp/none.md".into());
        let other = RawWatchEvent {
            path: "/tmp/none.md".into(),
            kind: RawKind::Other,
        };
        assert_eq!(gate.observe(&other, "anything"), WatchDelivery::Ignored);
        assert_eq!(gate.generation(), 0);
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
        let d1 = route_event(dir.path(), &doc_id, &file_str, &raw, "change one", move || {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        assert_eq!(d1, WatchDelivery::Change { generation: 1 });

        // A coalesced burst event does NOT route to the actor.
        let r = reconciles.clone();
        let d2 = route_event(dir.path(), &doc_id, &file_str, &raw, "change one", move || {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        assert_eq!(d2, WatchDelivery::Coalesced);

        let r = reconciles.clone();
        let d3 = route_event(dir.path(), &doc_id, &file_str, &raw, "change two", move || {
            r.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        assert_eq!(d3, WatchDelivery::Change { generation: 2 });

        // Exactly two reconciles ran (the two real changes, not the coalesced one).
        assert_eq!(reconciles.load(Ordering::SeqCst), 2);
        registry().unregister(&doc_id);
    }
}

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
//! `agent-doc-watch-io` owns the controller-side gate logic:
//! - a watcher registry that is **idempotent per document** — a second watch
//!   request for the same document reuses the one registration, so there is
//!   never a duplicate watcher;
//! - a document watch gate that coalesces event bursts into one logical change
//!   and suppresses agent-doc's own write echoes, so the actor sees one settled
//!   change stream.
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

use std::path::Path;

use anyhow::Result;

use agent_doc_document_realtime::session_ops::SessionOpKind;
use agent_doc_document_realtime::watch_authority::{RawWatchEvent, WatchDelivery};

use crate::session_actor::document_actor_in;

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
    let observation =
        agent_doc_watch_io::observe_document_event(doc_id, file, raw, current_content);
    if let Some(event) = observation.state_event {
        let actor = document_actor_in(base_dir, file);
        let base_dir = base_dir.to_path_buf();
        actor.submit(SessionOpKind::FileWatch, move |_ctx| -> Result<()> {
            crate::project_controller::append_state_event(&base_dir, &event)?;
            on_change()
        })??;
    }
    Ok(observation.delivery)
}

#[cfg(test)]
mod tests {
    use super::route_event;
    use agent_doc_document_realtime::watch_authority::{RawWatchEvent, WatchDelivery};
    use std::path::PathBuf;
    use std::sync::Arc;
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
        agent_doc_watch_io::unregister_document(&doc_id);
    }
}

//! Orchestration effects for the focused watch daemon.
//!
//! The daemon loop, PID handling, session discovery, debounce, capture polling,
//! node-event logging, and controller-watch gate live in `agent-doc-watch-io`.
//! This module supplies the two effects still owned by orchestration: stream
//! document writes and session-actor-routed file-watch persistence.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use agent_doc_config::Config;
use agent_doc_document_realtime::session_ops::SessionOpKind;
use agent_doc_document_realtime::watch_authority::{RawWatchEvent, WatchDelivery};
use agent_doc_watch_io::WatchDaemonEffects;
use anyhow::Result;

use crate::graph::ActorContext;
use crate::session_actor::document_actor_in;

#[derive(Default)]
struct OrchestrationWatchEffects {
    actor_contexts: HashMap<PathBuf, ActorContext>,
}

impl WatchDaemonEffects for OrchestrationWatchEffects {
    fn flush_stream_to_document(
        &mut self,
        file: &Path,
        text: &str,
        target: &str,
        baseline: &str,
    ) -> Result<()> {
        crate::stream::flush_to_document(file, text, target, baseline)
    }

    fn route_file_change(
        &mut self,
        base_dir: &Path,
        doc_id: &str,
        file: &str,
        raw: &RawWatchEvent,
        current_content: &str,
    ) -> Result<WatchDelivery> {
        let observation =
            agent_doc_watch_io::observe_document_event(doc_id, file, raw, current_content);
        if let Some(event) = observation.state_event {
            let actor = document_actor_in(base_dir, file);
            let base_dir = base_dir.to_path_buf();
            actor.submit(SessionOpKind::FileWatch, move |_ctx| -> Result<()> {
                crate::project_controller::append_state_event(&base_dir, &event)?;
                Ok(())
            })??;
        }
        Ok(observation.delivery)
    }

    fn on_file_change(&mut self, path: &Path) -> Result<()> {
        let ac = self
            .actor_contexts
            .entry(path.to_path_buf())
            .or_insert_with(|| ActorContext::new(path.to_path_buf()));
        ac.on_file_change(path.to_path_buf());
        Ok(())
    }

    fn on_config_change(&mut self) -> Result<usize> {
        for ac in self.actor_contexts.values() {
            ac.on_config_change();
        }
        Ok(self.actor_contexts.len())
    }

    fn on_stream_dead(&mut self, path: &Path) -> Result<()> {
        self.actor_contexts.remove(path);
        Ok(())
    }
}

pub fn start(config: &Config, watch_config: agent_doc_watch_io::WatchConfig) -> Result<()> {
    let mut effects = OrchestrationWatchEffects::default();
    agent_doc_watch_io::start(config, watch_config, &mut effects)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let dir = tempfile::TempDir::new().unwrap();
        let file = seed(&dir, "doc.md", "");
        let file_str = file.to_string_lossy().to_string();
        let doc_id = format!("watch-route-{}", file_str.len());
        let raw = RawWatchEvent::modify(&file);
        let reconciles = Arc::new(AtomicU64::new(0));
        let mut effects = OrchestrationWatchEffects::default();

        let d1 = effects
            .route_file_change(dir.path(), &doc_id, &file_str, &raw, "change one")
            .unwrap();
        assert_eq!(d1, WatchDelivery::Change { generation: 1 });
        reconciles.fetch_add(1, Ordering::SeqCst);

        let d2 = effects
            .route_file_change(dir.path(), &doc_id, &file_str, &raw, "change one")
            .unwrap();
        assert_eq!(d2, WatchDelivery::Coalesced);

        let d3 = effects
            .route_file_change(dir.path(), &doc_id, &file_str, &raw, "change two")
            .unwrap();
        assert_eq!(d3, WatchDelivery::Change { generation: 2 });
        reconciles.fetch_add(1, Ordering::SeqCst);

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

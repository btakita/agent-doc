use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use agent_doc_document::watch_projection::file_watch_event_id;
use agent_doc_document::watch_projection::{
    document_node_events_payload, project_watch_node_events,
};
use agent_doc_document_realtime::watch_authority::{
    RawWatchEvent, WatchDelivery, WatchWriteProvenance, WatcherRegistry,
};
use agent_doc_markdown_ast::events::DocumentNodeEvent;
use agent_doc_state_backbone::{StateEvent, StateFact};

pub mod daemon;

pub use daemon::{
    WatchConfig, WatchDaemonEffects, cycle_freshly_in_flight, ensure_running, start, status, stop,
};

pub const PID_FILE: &str = ".agent-doc/watch.pid";

/// A controller-watch observation after write-provenance filtering and burst
/// coalescing.
pub struct WatchObservation {
    pub delivery: WatchDelivery,
    pub state_event: Option<StateEvent>,
}

/// Refresh the cached markdown snapshot for `path` and return node-keyed events
/// against the previous projection.
pub fn update_node_snapshot(
    path: &Path,
    snapshots: &mut HashMap<PathBuf, String>,
) -> Result<Vec<DocumentNodeEvent>> {
    let content = std::fs::read_to_string(path)?;
    let previous = snapshots.insert(path.to_path_buf(), content.clone());
    Ok(project_watch_node_events(previous.as_deref(), &content))
}

/// Emit watch node-event evidence through the caller's ops-log boundary.
pub fn log_node_events(
    path: &Path,
    events: &[DocumentNodeEvent],
    mut ops_logger: impl FnMut(&Path, &str),
) {
    if events.is_empty() {
        return;
    }
    let payload = document_node_events_payload(&path.display().to_string(), events);
    let message = format!("document_node_events {payload}");
    ops_logger(path, &message);
}

/// The process-wide controller-owned watcher registry singleton.
pub fn registry() -> &'static WatcherRegistry {
    static REGISTRY: std::sync::OnceLock<WatcherRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(WatcherRegistry::new)
}

/// Remove a document from the process-wide watcher registry.
pub fn unregister_document(doc_id: &str) {
    registry().unregister(doc_id);
}

/// Observe a raw filesystem event through the controller-owned watch gate.
///
/// Self-write echoes and coalesced bursts are represented in `delivery` without
/// producing a state event. A real content change returns the state event that
/// callers should persist from their controller/session boundary.
pub fn observe_document_event(
    doc_id: &str,
    file: &str,
    raw: &RawWatchEvent,
    current_content: &str,
) -> WatchObservation {
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
    let state_event = match delivery {
        WatchDelivery::Change { generation } => Some(StateEvent::new(
            file_watch_event_id(doc_id, generation, &content_hash),
            StateFact::FileWatchChangeObserved {
                document_hash: doc_id.to_string(),
                path: file.to_string(),
                watch_generation: generation,
                content_hash,
            },
        )),
        _ => None,
    };
    WatchObservation {
        delivery,
        state_event,
    }
}

pub fn pid_path(base_dir: &Path) -> PathBuf {
    base_dir.join(PID_FILE)
}

/// Check whether a PID is alive using the same `/proc` probe as the watcher.
pub fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

pub fn read_pid() -> Option<u32> {
    read_pid_in(&std::env::current_dir().ok()?)
}

/// Check whether the watch daemon recorded in the current directory is alive.
pub fn is_running() -> bool {
    read_pid().is_some_and(pid_alive)
}

pub fn read_pid_in(base_dir: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(pid_path(base_dir)).ok()?;
    content.trim().parse().ok()
}

pub fn write_current_pid() -> Result<()> {
    write_current_pid_in(&std::env::current_dir()?)
}

pub fn write_current_pid_in(base_dir: &Path) -> Result<()> {
    let pid_path = pid_path(base_dir);
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(pid_path, std::process::id().to_string())?;
    Ok(())
}

pub fn remove_pid() {
    let _ = std::fs::remove_file(PID_FILE);
}

pub fn remove_pid_in(base_dir: &Path) {
    let _ = std::fs::remove_file(pid_path(base_dir));
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_document_realtime::watch_authority::RawWatchEvent;

    #[test]
    fn pid_file_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        write_current_pid_in(dir.path()).unwrap();
        let pid = read_pid_in(dir.path()).unwrap();
        assert_eq!(pid, std::process::id());

        remove_pid_in(dir.path());
        assert!(read_pid_in(dir.path()).is_none());
    }

    #[test]
    fn pid_alive_self() {
        assert!(pid_alive(std::process::id()));
    }

    #[test]
    fn pid_alive_nonexistent() {
        assert!(!pid_alive(4_294_967_295));
    }

    #[test]
    fn observe_document_event_builds_state_event_for_real_change() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        std::fs::write(&file, "one").unwrap();
        let file_str = file.to_string_lossy().to_string();
        let doc_id = "watch-io-event";
        let raw = RawWatchEvent::modify(&file);

        let first = observe_document_event(doc_id, &file_str, &raw, "one");
        assert_eq!(first.delivery, WatchDelivery::Change { generation: 1 });
        let event = first
            .state_event
            .expect("change should produce state event");
        assert!(event.event_id.contains(doc_id));

        let second = observe_document_event(doc_id, &file_str, &raw, "one");
        assert_eq!(second.delivery, WatchDelivery::Coalesced);
        assert!(
            second.state_event.is_none(),
            "coalesced events should not produce ledger events"
        );

        unregister_document(doc_id);
    }

    #[test]
    fn update_node_snapshot_emits_node_keyed_events_after_seed() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("session.md");
        std::fs::write(
            &path,
            "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let mut snapshots = HashMap::new();

        assert!(
            update_node_snapshot(&path, &mut snapshots)
                .unwrap()
                .is_empty()
        );

        std::fs::write(
            &path,
            "<!-- agent:queue -->\n- do [#alpha]\n- do [#beta]\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let events = update_node_snapshot(&path, &mut snapshots).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].kind,
            agent_doc_markdown_ast::events::DocumentNodeEventKind::Insert
        );
        assert!(events[0].node_key.contains(":beta:"));
        assert!(
            events[0]
                .previous_node_key
                .as_deref()
                .is_some_and(|key| key.contains(":alpha:"))
        );
    }

    #[test]
    fn log_node_events_emits_document_node_events_payload() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("session.md");
        let before =
            "<!-- agent:backlog -->\n1. [ ] [#task] old wording\n<!-- /agent:backlog -->\n";
        let after = "<!-- agent:backlog -->\n1. [ ] [#task] new wording\n<!-- /agent:backlog -->\n";
        std::fs::write(&path, after).unwrap();
        let events = agent_doc_markdown_ast::events::diff_node_events(before, after);
        let mut logged = Vec::new();

        log_node_events(&path, &events, |file, message| {
            logged.push((file.to_path_buf(), message.to_string()));
        });

        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].0, path);
        let message = &logged[0].1;
        assert!(message.contains("document_node_events"), "{message}");
        assert!(
            message.contains("\"event\":\"document_node_events\""),
            "{message}"
        );
        assert!(message.contains("\"component\":\"backlog\""), "{message}");
        assert!(
            message.contains("\"node_key\":\"backlog:0:task:0\""),
            "{message}"
        );
        assert!(message.contains("\"op\":\"replace\""), "{message}");
    }
}

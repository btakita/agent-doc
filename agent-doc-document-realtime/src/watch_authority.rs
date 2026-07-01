//! # Module: watch_authority
//!
//! ## Spec (`#dsqa` / `#pcp7` — 08b filesystem-watch authority, post-cutover end state)
//! Realizes the `specs/08b-single-process-control-plane.md` filesystem-watch
//! authority: the editor plugin's own NIO `WatchService` is **unconditionally**
//! demoted to read-only buffer reporting. The single controller-owned watcher
//! (adapted by orchestration's `document_watcher`, `#pcpc4`/`#pcp4`) plus the
//! socket IPC command channel are the sole writer to the live editor buffer.
//! This removes the
//! second-watcher race where the plugin mutated the live buffer between an agent
//! finalize's preflight and commit — the `live_prompt_drift_after_preflight` /
//! `ipc_socket_already_applied_live_buffer_diverged` drift family that the
//! `#dav9` in-process hosting swap alone did not fix (the host moved, the second
//! writer did not).
//!
//! ## History
//! This shipped through the 08b migration gate ladder behind the
//! `AGENT_DOC_PLUGIN_WATCH` rollback flag (`active → read-only`). The cutover is
//! now **complete**: the flag and the `active` (plugin-applies) path were removed
//! at the removal rung, so the plugin's WatchService file-apply path is always
//! read-only. The plugin queries this end state through the
//! `agent_doc_plugin_watch_readonly` FFI export in the binary crate, which now
//! always reports read-only and emits a structured
//! `plugin_watch_readonly` `ops.log` marker. The plugin's socket IPC apply path
//! (the controller's writer arm into the editor) stays active.

use std::path::PathBuf;

/// Minimal classification of a raw filesystem event, mirroring the
/// `notify::EventKind` subset the controller watcher reacts to.
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

/// Provenance for a recent document write, narrowed to the facts the watch gate
/// needs to decide whether an event is agent-doc's own write echo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchWriteProvenance<'a> {
    pub actor: &'a str,
    pub hash: &'a str,
}

impl<'a> WatchWriteProvenance<'a> {
    pub fn new(actor: &'a str, hash: &'a str) -> Self {
        Self { actor, hash }
    }

    fn is_agent_write_of(self, hash: &str) -> bool {
        self.actor == "agent" && self.hash == hash
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

/// Per-document coalescing + self-write-suppression gate. The orchestration
/// layer owns watcher IO and routing; this gate owns only the deterministic
/// delivery decision for a raw event and caller-provided content/provenance
/// facts.
#[derive(Debug, Clone)]
pub struct DocumentWatchGate {
    file: PathBuf,
    last_delivered_hash: Option<String>,
    generation: u64,
}

impl DocumentWatchGate {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            last_delivered_hash: None,
            generation: 0,
        }
    }

    /// Number of distinct changes delivered so far.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Classify a raw event for the document's current content hash.
    ///
    /// The caller supplies the current content hash and recent write provenance
    /// so this function stays pure: no filesystem, global debounce, or
    /// orchestration state is consulted here.
    pub fn observe(
        &mut self,
        raw: &RawWatchEvent,
        current_content_hash: &str,
        write_provenance: Option<WatchWriteProvenance<'_>>,
    ) -> WatchDelivery {
        if raw.path != self.file || !matches!(raw.kind, RawKind::Modify | RawKind::Create) {
            return WatchDelivery::Ignored;
        }

        if write_provenance.is_some_and(|prov| prov.is_agent_write_of(current_content_hash)) {
            return WatchDelivery::SelfWriteEcho;
        }

        if self.last_delivered_hash.as_deref() == Some(current_content_hash) {
            return WatchDelivery::Coalesced;
        }

        self.last_delivered_hash = Some(current_content_hash.to_string());
        self.generation += 1;
        WatchDelivery::Change {
            generation: self.generation,
        }
    }
}

/// Whether the editor plugin's own `WatchService` file-apply path is demoted to
/// read-only buffer reporting. Always `true` post-cutover — the plugin never
/// autonomously applies file-IPC patches it observes on disk; the
/// controller-owned watcher + socket IPC are the sole writer.
pub fn plugin_watch_is_readonly() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_watch_is_always_readonly_post_cutover() {
        assert!(plugin_watch_is_readonly());
    }

    #[test]
    fn gate_coalesces_event_burst_into_one_change() {
        let mut gate = DocumentWatchGate::new("/tmp/none.md");
        let raw = RawWatchEvent::modify("/tmp/none.md");
        assert_eq!(
            gate.observe(&raw, "hash-version-one", None),
            WatchDelivery::Change { generation: 1 }
        );
        for _ in 0..5 {
            assert_eq!(
                gate.observe(&raw, "hash-version-one", None),
                WatchDelivery::Coalesced
            );
        }
        assert_eq!(gate.generation(), 1, "burst delivered exactly one change");
    }

    #[test]
    fn gate_emits_change_on_distinct_content() {
        let mut gate = DocumentWatchGate::new("/tmp/none.md");
        let raw = RawWatchEvent::modify("/tmp/none.md");
        assert_eq!(
            gate.observe(&raw, "hash-a", None),
            WatchDelivery::Change { generation: 1 }
        );
        assert_eq!(
            gate.observe(&raw, "hash-b", None),
            WatchDelivery::Change { generation: 2 }
        );
        assert_eq!(gate.observe(&raw, "hash-b", None), WatchDelivery::Coalesced);
    }

    #[test]
    fn gate_suppresses_agent_self_write_echo() {
        let mut gate = DocumentWatchGate::new("/tmp/doc.md");
        let raw = RawWatchEvent::modify("/tmp/doc.md");
        let provenance = WatchWriteProvenance::new("agent", "hash-agent-wrote-this");
        assert_eq!(
            gate.observe(&raw, "hash-agent-wrote-this", Some(provenance)),
            WatchDelivery::SelfWriteEcho
        );
        assert_eq!(gate.generation(), 0);

        assert_eq!(
            gate.observe(&raw, "hash-user-edited-this", Some(provenance)),
            WatchDelivery::Change { generation: 1 }
        );
    }

    #[test]
    fn gate_ignores_non_agent_matching_write_provenance() {
        let mut gate = DocumentWatchGate::new("/tmp/doc.md");
        let raw = RawWatchEvent::modify("/tmp/doc.md");
        let provenance = WatchWriteProvenance::new("operator", "hash-operator-wrote-this");
        assert_eq!(
            gate.observe(&raw, "hash-operator-wrote-this", Some(provenance)),
            WatchDelivery::Change { generation: 1 }
        );
    }

    #[test]
    fn gate_ignores_non_content_events() {
        let mut gate = DocumentWatchGate::new("/tmp/none.md");
        let other = RawWatchEvent {
            path: "/tmp/none.md".into(),
            kind: RawKind::Other,
        };
        assert_eq!(
            gate.observe(&other, "hash-anything", None),
            WatchDelivery::Ignored
        );
        assert_eq!(gate.generation(), 0);
    }

    #[test]
    fn gate_ignores_path_mismatches() {
        let mut gate = DocumentWatchGate::new("/tmp/doc.md");
        let raw = RawWatchEvent::modify("/tmp/other.md");
        assert_eq!(
            gate.observe(&raw, "hash-anything", None),
            WatchDelivery::Ignored
        );
        assert_eq!(gate.generation(), 0);
    }
}

//! # Module: watch_authority
//!
//! ## Spec (`#dsqa` / `#pcp7` — 08b filesystem-watch authority, post-cutover end state)
//! Realizes the `specs/08b-single-process-control-plane.md` filesystem-watch
//! authority: the editor plugin's own NIO `WatchService` is **unconditionally**
//! demoted to read-only buffer reporting. The single controller-owned watcher
//! (adapted by orchestration's watch effects, `#pcpc4`/`#pcp4`) plus the
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

use crate::crdt_authority::CrdtAuthority;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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

/// Stable controller/state boundary projection of a disk-change watch delivery.
///
/// `WatchDelivery` is the edge watcher's local decision. `DiskChangeSignal` is
/// the small serializable fact that can cross the CP/state boundary and be
/// converted back when the controller routes the signal through the canonical
/// document model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiskChangeSignal {
    /// A new settled change the actor should reconcile. `generation` increments
    /// per distinct delivered change.
    Change { generation: u64 },
    /// Same content as the last delivered change — a coalesced burst event.
    Coalesced,
    /// The current content matches agent-doc's own most recent write.
    SelfWriteEcho,
    /// Not a content-bearing event (or a path mismatch).
    Ignored,
}

impl DiskChangeSignal {
    pub fn from_delivery(delivery: &WatchDelivery) -> Self {
        Self::from(delivery)
    }

    pub fn into_delivery(self) -> WatchDelivery {
        WatchDelivery::from(self)
    }
}

impl From<&WatchDelivery> for DiskChangeSignal {
    fn from(delivery: &WatchDelivery) -> Self {
        match delivery {
            WatchDelivery::Change { generation } => Self::Change {
                generation: *generation,
            },
            WatchDelivery::Coalesced => Self::Coalesced,
            WatchDelivery::SelfWriteEcho => Self::SelfWriteEcho,
            WatchDelivery::Ignored => Self::Ignored,
        }
    }
}

impl From<WatchDelivery> for DiskChangeSignal {
    fn from(delivery: WatchDelivery) -> Self {
        Self::from(&delivery)
    }
}

impl From<DiskChangeSignal> for WatchDelivery {
    fn from(signal: DiskChangeSignal) -> Self {
        match signal {
            DiskChangeSignal::Change { generation } => Self::Change { generation },
            DiskChangeSignal::Coalesced => Self::Coalesced,
            DiskChangeSignal::SelfWriteEcho => Self::SelfWriteEcho,
            DiskChangeSignal::Ignored => Self::Ignored,
        }
    }
}

impl From<&DiskChangeSignal> for WatchDelivery {
    fn from(signal: &DiskChangeSignal) -> Self {
        Self::from(*signal)
    }
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

/// Registry of one watch gate per document.
///
/// Registration is idempotent: a second registration for the same document id
/// reuses the existing gate, so callers cannot accidentally create duplicate
/// per-document watchers for the same realtime authority stream.
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

/// Whether the editor plugin's `WatchService` is limited to buffer reporting.
/// The CP document model and typed editor intents are the only mutation path.
pub fn plugin_watch_is_readonly() -> bool {
    true
}

/// What the controller-owned watcher should *do* with a settled change, once the
/// per-document gate ([`DocumentWatchGate::observe`]) has classified the raw
/// event. This is the missing consumer decision for a `FileWatchChangeObserved`
/// event: the gate answers "is this a real change vs. our own echo?", and this
/// answers "given the authority mode and whether an operator edit is in flight,
/// where does that change go?".
///
/// The action is *only* the routing decision; the IPC hop into the canonical
/// [`crate::crdt_relay`] replica and the fan-out to editor buffers are the
/// caller's job (`plan-crdt-scramble-and-disk-propagation.md` Phases C/D).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchAction {
    /// Nothing to do — the delivery was not a fresh content-bearing change
    /// (ignored, coalesced burst, or a suppressed self-write echo).
    None,
    /// A live editor holds an unsynced operator edit (`edit_epoch >
    /// last_synced_epoch`). Defer the reconcile for a **bounded** settle budget
    /// so the watcher never reads a half-applied buffer. This is *not* a held
    /// lock: on settle → re-decide (which yields [`Self::ReconcileIntoCanonical`]);
    /// on timeout → **fail open** and reconcile anyway (state-vector merge
    /// preserves the operator's text — never discard, never block forever).
    DeferForEditSettle,
    /// EditorAttached ([`CrdtAuthority::MultiReplica`]) with no operator edit in
    /// flight: integrate the disk change into the canonical replica via
    /// state-vector merge, then enqueue delivery to every live editor. Idempotent
    /// — a change the canonical already holds is a structural no-op (the
    /// "editor buffer already has it" reconcile case).
    ReconcileIntoCanonical,
    /// Detached ([`CrdtAuthority::GitAuthoritative`]) — no live editor. Disk is
    /// authoritative and the CRDT is ephemeral, so the change is applied as the
    /// new baseline directly; there is no editor buffer to reconcile against.
    ApplyAsDiskAuthority,
}

/// Decide what the controller watcher should do with a gate delivery.
///
/// Pure and total over the inputs: no filesystem, IPC, or clock. The bounded
/// defer + fail-open semantics of [`WatchAction::DeferForEditSettle`] are the
/// caller's to honor (see the variant docs) — this function only chooses the
/// action.
///
/// Design (matches the watcher-scoping guidance for goal 4): the watcher stays
/// **on** whether or not an editor is attached; what a live editor changes is the
/// *destination* of the change (reconcile into canonical vs. apply as disk
/// authority), not whether the change is observed. Self-write echoes are already
/// dropped upstream by the gate, so `decide` never re-applies agent-doc's own
/// write.
pub fn decide_watch_action(
    delivery: &WatchDelivery,
    authority: CrdtAuthority,
    editor_edit_in_flight: bool,
) -> WatchAction {
    match delivery {
        // Not a fresh change — the gate already coalesced bursts and suppressed
        // self-write echoes.
        WatchDelivery::Ignored | WatchDelivery::Coalesced | WatchDelivery::SelfWriteEcho => {
            WatchAction::None
        }
        WatchDelivery::Change { .. } => match authority {
            // No live editor: disk is the authority, nothing to reconcile against.
            CrdtAuthority::GitAuthoritative => WatchAction::ApplyAsDiskAuthority,
            // Live editor: reconcile into the canonical replica, but defer while
            // the operator's own edit is still settling (bounded, fail-open).
            CrdtAuthority::MultiReplica => {
                if editor_edit_in_flight {
                    WatchAction::DeferForEditSettle
                } else {
                    WatchAction::ReconcileIntoCanonical
                }
            }
        },
    }
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

    #[test]
    fn disk_change_signal_projects_watch_delivery_for_controller_boundary() {
        for (delivery, signal) in [
            (
                WatchDelivery::Change { generation: 7 },
                DiskChangeSignal::Change { generation: 7 },
            ),
            (WatchDelivery::Coalesced, DiskChangeSignal::Coalesced),
            (
                WatchDelivery::SelfWriteEcho,
                DiskChangeSignal::SelfWriteEcho,
            ),
            (WatchDelivery::Ignored, DiskChangeSignal::Ignored),
        ] {
            assert_eq!(DiskChangeSignal::from_delivery(&delivery), signal);
            assert_eq!(signal.into_delivery(), delivery);
        }
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

    // ---- decide_watch_action: the FileWatchChangeObserved consumer decision ----

    fn a_change() -> WatchDelivery {
        WatchDelivery::Change { generation: 1 }
    }

    #[test]
    fn non_change_deliveries_are_no_action_regardless_of_mode() {
        for delivery in [
            WatchDelivery::Ignored,
            WatchDelivery::Coalesced,
            WatchDelivery::SelfWriteEcho,
        ] {
            for authority in [CrdtAuthority::GitAuthoritative, CrdtAuthority::MultiReplica] {
                for in_flight in [false, true] {
                    assert_eq!(
                        decide_watch_action(&delivery, authority, in_flight),
                        WatchAction::None,
                        "delivery={delivery:?} authority={authority:?} in_flight={in_flight}"
                    );
                }
            }
        }
    }

    #[test]
    fn detached_change_applies_disk_as_authority() {
        // No live editor: disk is authoritative, nothing to reconcile against.
        // Edit-in-flight is meaningless here (no editor) and must not change it.
        assert_eq!(
            decide_watch_action(&a_change(), CrdtAuthority::GitAuthoritative, false),
            WatchAction::ApplyAsDiskAuthority
        );
        assert_eq!(
            decide_watch_action(&a_change(), CrdtAuthority::GitAuthoritative, true),
            WatchAction::ApplyAsDiskAuthority
        );
    }

    #[test]
    fn editor_attached_change_reconciles_into_canonical_when_settled() {
        assert_eq!(
            decide_watch_action(&a_change(), CrdtAuthority::MultiReplica, false),
            WatchAction::ReconcileIntoCanonical
        );
    }

    #[test]
    fn editor_attached_change_defers_while_operator_edit_in_flight() {
        // The bounded, fail-open defer — never a held lock (the no_ack trap).
        assert_eq!(
            decide_watch_action(&a_change(), CrdtAuthority::MultiReplica, true),
            WatchAction::DeferForEditSettle
        );
    }

    #[test]
    fn defer_falls_open_to_reconcile_when_the_edit_settles() {
        // Models the caller's fail-open path: an in-flight edit yields Defer; once
        // the edit settles (or the bounded budget expires and the caller clears
        // the in-flight flag), re-deciding yields a reconcile, never a wedge.
        let settling = decide_watch_action(&a_change(), CrdtAuthority::MultiReplica, true);
        assert_eq!(settling, WatchAction::DeferForEditSettle);
        let settled = decide_watch_action(&a_change(), CrdtAuthority::MultiReplica, false);
        assert_eq!(settled, WatchAction::ReconcileIntoCanonical);
    }

    #[test]
    fn watcher_stays_on_with_a_live_editor_it_only_changes_destination() {
        // Goal-4 invariant: a live editor never turns the watcher into a no-op;
        // it only routes a real disk change to canonical instead of to disk.
        let change = a_change();
        assert_ne!(
            decide_watch_action(&change, CrdtAuthority::MultiReplica, false),
            WatchAction::None,
            "an attached editor must not silence out-of-band disk changes"
        );
    }
}

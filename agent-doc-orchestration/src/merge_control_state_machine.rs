//! Plugin/binary merge-control write-ownership state machine (`#mergestatemachine`).
//!
//! The `File Cache Conflict` dialogs and the IPC `no_ack` /
//! `recovery=retry_without_disk_write` / forced `--force-disk` churn are the same
//! failure: a write race between the binary's disk-write path and the editor/FFI
//! plugin's buffer ownership. Today write ownership is implied by scattered
//! checks (`try_ipc`, `is_listener_active`, the commit-time boundary-skip guard)
//! rather than a single authoritative state — so the binary cannot tell
//! "editor owns buffer" from "stale listener" (`#6b5h`) and either wedges
//! (`no_ack`) or races (File Cache Conflict).
//!
//! This module is the pure FFI-owned ownership authority both the binary and
//! every editor plugin transition through. Phase 1 (`#mergestatemachine1`) ships
//! the transition table, the [`disk_write_permitted`] safety predicate, and the
//! [`ownership_probe`] liveness classifier that distinguishes a live editor from
//! a stale listener. Wiring the write path (`try_ipc` / `commit` / disk-write)
//! onto the SM is phase 2 (`#mergestatemachine2`).
//!
//! The durable JSON sidecar / lease remains the crash-recovery log; this module
//! is the pure transition authority, mirroring [`crate::cycle_state_machine`].

use std::time::Duration;

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

use crate::plugin_owner::{self, PluginOwnerLease};

/// Write-ownership phase for a document's editor buffer.
///
/// A disk write is gated on a proven ownership transition: the binary may write
/// to disk only in [`MergeOwnershipPhase::Detached`] (no editor) or after
/// [`MergeOwnershipPhase::IpcAckProven`] (editor applied the patch). It can
/// never write while [`MergeOwnershipPhase::EditorOwnsBuffer`] holds — that is
/// the `File Cache Conflict` the guard prevents. See [`disk_write_permitted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeOwnershipPhase {
    /// No editor plugin attached (CLI-only / no live buffer). The binary owns
    /// disk and may write directly.
    Detached,
    /// Editor plugin registered / IPC listener connected, but a live editor
    /// buffer is not yet proven. This is the ambiguous `#6b5h` state: the
    /// project controller can host a connectable socket with nothing behind it.
    /// Resolve via [`ownership_probe`] before any write.
    Attached,
    /// A live editor process provably owns the document buffer (fresh owner
    /// lease + live pid). The binary must NOT raw-write — it queues a patch and
    /// waits for an ACK.
    EditorOwnsBuffer,
    /// The binary has queued a patch for the editor to apply; awaiting ACK.
    /// Disk writes are forbidden (the editor may apply its own buffer state).
    BinaryWriteRequested,
    /// The editor applied the queued patch and ACKed. A disk sync is now safe
    /// (it lands behind the editor's own apply).
    IpcAckProven,
    /// Write cycle complete; terminal for this cycle.
    Committed,
}

/// Events driving the write-ownership transition.
///
/// Events are not serialized over the wire (only phases project, mirroring the
/// backbone convention); the editor plugins emit facts through the
/// `EventLedger` and the binary maps them to these events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOwnershipEvent {
    /// Editor plugin registered / IPC listener came up.
    EditorAttached,
    /// A live editor buffer was proven (fresh owner lease + live pid).
    EditorBufferObserved,
    /// Editor plugin detached / owner pid provably dead.
    EditorDetached,
    /// Liveness probe detected a stale listener (`#6b5h`): a registered
    /// listener with no proven live editor. Demotes [`MergeOwnershipPhase::Attached`]
    /// to [`MergeOwnershipPhase::Detached`] so the write routes to the
    /// controller-host disk path instead of wedging on `no_ack`.
    HeartbeatStale,
    /// The binary queued a patch for the editor to apply (editor path only).
    BinaryWriteRequested,
    /// The editor applied the queued patch and the ACK was observed.
    PatchAckObserved,
    /// The write cycle committed.
    Committed,
}

pub struct MergeOwnershipMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<MergeOwnershipPhase, MergeOwnershipEvent>,
}

impl MergeOwnershipMachine {
    pub fn new(initial: MergeOwnershipPhase) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_merge_ownership);
        Self { ctx, machine }
    }

    pub fn send(&self, event: MergeOwnershipEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> MergeOwnershipPhase {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: MergeOwnershipPhase,
        event: MergeOwnershipEvent,
    ) -> Option<MergeOwnershipPhase> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

impl MergeOwnershipPhase {
    /// Terminal phases accept no further ownership transitions.
    pub fn is_terminal(self) -> bool {
        matches!(self, MergeOwnershipPhase::Committed)
    }
}

/// Safety invariant: a disk write is permitted only when no live editor owns the
/// buffer ([`Detached`](MergeOwnershipPhase::Detached)) or after the editor
/// applied and ACKed the patch ([`IpcAckProven`](MergeOwnershipPhase::IpcAckProven)).
/// This is the single gate that eliminates `File Cache Conflict` (writing behind
/// a live editor) and the `#6b5h` no_ack wedge (waiting on a stale listener).
///
/// Phase 2 (`#mergestatemachine2`) routes `try_ipc` / `commit` / disk-write
/// through this predicate.
pub fn disk_write_permitted(phase: MergeOwnershipPhase) -> bool {
    matches!(
        phase,
        MergeOwnershipPhase::Detached | MergeOwnershipPhase::IpcAckProven
    )
}

/// Phase-2 write-path gate (`#mergestatemachine2`): resolve whether a disk write
/// is permitted for a document from its current editor-attachment facts,
/// expressed through the ownership SM. The converge/commit disk-write refusal
/// points ([`crate::write::converge`]) consult this instead of a raw
/// listener-active check, so a **stale listener** (`#6b5h`: a connectable
/// controller-hosted socket with no live editor behind it) routes to the
/// controller-host disk write instead of wedging on `no_ack` / `ack_mismatch`.
///
/// The write path observes ownership fresh each write (no persistent phase is
/// stored yet): it starts from the ambiguous [`MergeOwnershipPhase::Attached`]
/// state and lets [`ownership_probe`] resolve it from the lease — a live editor
/// (lease + live pid) → `EditorOwnsBuffer` → refuse; a stale listener / dead pid
/// / no lease → `Detached` → permit.
///
/// Decision-equivalent to `!plugin_owner::live_editor_endpoint_attached(file)`,
/// but routed through the ownership vocabulary so the gate and the SM share one
/// authority.
pub fn disk_write_permitted_for_file(file: &str) -> bool {
    let liveness = OwnershipLiveness::for_file(file);
    let resolved = ownership_probe(MergeOwnershipPhase::Attached, &liveness);
    let phase = match resolved {
        Some(MergeOwnershipEvent::EditorBufferObserved) => MergeOwnershipPhase::EditorOwnsBuffer,
        _ => MergeOwnershipPhase::Detached,
    };
    disk_write_permitted(phase)
}

/// Pure transition table for [`MergeOwnershipMachine`]. Returns `None` to reject
/// a transition (guard); `Some(next)` to accept.
///
/// Design rules:
/// - An in-flight write (`BinaryWriteRequested` / `IpcAckProven`) is never
///   disturbed by attach/detach churn — the handshake must drain.
/// - [`Committed`](MergeOwnershipEvent::Committed) is reachable only from a
///   proven-safe write phase (`Detached` direct-disk path, or `IpcAckProven`
///   editor path) so a half-applied editor write can never commit.
/// - [`BinaryWriteRequested`](MergeOwnershipEvent::BinaryWriteRequested) is
///   editor-path only: from `Detached` the binary writes disk directly and
///   commits, so there is no editor to "request."
pub fn transition_merge_ownership(
    current: &MergeOwnershipPhase,
    event: &MergeOwnershipEvent,
) -> Option<MergeOwnershipPhase> {
    match event {
        MergeOwnershipEvent::EditorAttached => match current {
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::EditorOwnsBuffer => Some(MergeOwnershipPhase::Attached),
            MergeOwnershipPhase::BinaryWriteRequested
            | MergeOwnershipPhase::IpcAckProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::EditorBufferObserved => match current {
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::EditorOwnsBuffer => Some(MergeOwnershipPhase::EditorOwnsBuffer),
            MergeOwnershipPhase::BinaryWriteRequested
            | MergeOwnershipPhase::IpcAckProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::EditorDetached => match current {
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::EditorOwnsBuffer => Some(MergeOwnershipPhase::Detached),
            MergeOwnershipPhase::BinaryWriteRequested
            | MergeOwnershipPhase::IpcAckProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::HeartbeatStale => match current {
            // #6b5h: a registered listener with no proven live editor demotes to
            // Detached so the write routes to disk instead of wedging on no_ack.
            // An idle real editor keys off pid liveness (see `ownership_probe`),
            // so staleness alone never demotes a proven EditorOwnsBuffer.
            MergeOwnershipPhase::Attached => Some(MergeOwnershipPhase::Detached),
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::EditorOwnsBuffer
            | MergeOwnershipPhase::BinaryWriteRequested
            | MergeOwnershipPhase::IpcAckProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::BinaryWriteRequested => match current {
            MergeOwnershipPhase::EditorOwnsBuffer
            | MergeOwnershipPhase::BinaryWriteRequested => {
                Some(MergeOwnershipPhase::BinaryWriteRequested)
            }
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::IpcAckProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::PatchAckObserved => match current {
            MergeOwnershipPhase::BinaryWriteRequested
            | MergeOwnershipPhase::IpcAckProven => Some(MergeOwnershipPhase::IpcAckProven),
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::EditorOwnsBuffer
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::Committed => match current {
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::IpcAckProven
            | MergeOwnershipPhase::Committed => Some(MergeOwnershipPhase::Committed),
            MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::EditorOwnsBuffer
            | MergeOwnershipPhase::BinaryWriteRequested => None,
        },
    }
}

/// Liveness facts the [`ownership_probe`] consults. Built from a
/// [`PluginOwnerLease`] so the probe composes the shipped owner-lease signal
/// rather than inventing a parallel heartbeat sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OwnershipLiveness {
    /// A plugin-owner lease exists for the document.
    pub lease_present: bool,
    /// The lease owner pid is a live process (implies a real editor is running).
    pub pid_live: bool,
    /// The lease heartbeat is within the freshness ttl.
    pub heartbeat_fresh: bool,
}

impl OwnershipLiveness {
    /// Build liveness facts from an optional owner lease, injecting the pid
    /// liveness predicate and clock so the decision is unit-testable without
    /// spawning a real IDE process. Mirrors the inject style of
    /// [`crate::plugin_owner::editor_endpoint_attached_for_lease`].
    pub fn from_lease(
        lease: Option<&PluginOwnerLease>,
        is_pid_live: impl Fn(u32) -> bool,
        now: u64,
        ttl: Duration,
    ) -> Self {
        match lease {
            Some(l) => Self {
                lease_present: true,
                pid_live: is_pid_live(l.pid),
                heartbeat_fresh: plugin_owner::plugin_owner_lease_is_fresh(
                    l.heartbeat_secs, now, ttl,
                ),
            },
            None => Self::default(),
        }
    }

    /// Read the live editor-attachment facts for a document from its plugin-owner
    /// lease sidecar. This is the write-path entry point: ownership is observed
    /// fresh each write (no persistent ownership phase is stored yet — phase 2
    /// resolves the ambiguous [`MergeOwnershipPhase::Attached`] state on demand).
    pub fn for_file(file: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self::from_lease(
            plugin_owner::read_plugin_owner_lease(file).as_ref(),
            plugin_owner::plugin_owner_pid_is_live,
            now,
            plugin_owner::plugin_owner_ttl(),
        )
    }
}

/// Editor-heartbeat liveness probe that distinguishes a **live editor** from a
/// **stale listener** (`#6b5h`), advancing an ambiguous
/// [`MergeOwnershipPhase::Attached`] state to the correct ownership phase.
///
/// Returns the event to fire, or `None` when the current phase is not
/// `Attached` (non-ambiguous phases advance via write-path events).
///
/// Decision (fail-safe — matches [`crate::plugin_owner::live_editor_endpoint_attached`]):
/// - lease present **and** pid live → [`EditorBufferObserved`](MergeOwnershipEvent::EditorBufferObserved):
///   a real editor is behind the listener; keys off **pid liveness**, not
///   heartbeat freshness, so an idle editor (stale heartbeat, live pid) is still
///   recognized and never mis-routed to a disk write that would hit its buffer.
/// - lease present but pid dead → [`EditorDetached`](MergeOwnershipEvent::EditorDetached):
///   the editor process is gone.
/// - no lease at all → [`HeartbeatStale`](MergeOwnershipEvent::HeartbeatStale):
///   the `#6b5h` case — a connectable socket with no editor buffer behind it —
///   demotes to `Detached` so the write routes to the controller-host disk path.
pub fn ownership_probe(
    current: MergeOwnershipPhase,
    liveness: &OwnershipLiveness,
) -> Option<MergeOwnershipEvent> {
    if current != MergeOwnershipPhase::Attached {
        return None;
    }
    if liveness.lease_present && liveness.pid_live {
        Some(MergeOwnershipEvent::EditorBufferObserved)
    } else if liveness.lease_present {
        Some(MergeOwnershipEvent::EditorDetached)
    } else {
        Some(MergeOwnershipEvent::HeartbeatStale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_PHASES: [MergeOwnershipPhase; 6] = [
        MergeOwnershipPhase::Detached,
        MergeOwnershipPhase::Attached,
        MergeOwnershipPhase::EditorOwnsBuffer,
        MergeOwnershipPhase::BinaryWriteRequested,
        MergeOwnershipPhase::IpcAckProven,
        MergeOwnershipPhase::Committed,
    ];

    const ALL_EVENTS: [MergeOwnershipEvent; 7] = [
        MergeOwnershipEvent::EditorAttached,
        MergeOwnershipEvent::EditorBufferObserved,
        MergeOwnershipEvent::EditorDetached,
        MergeOwnershipEvent::HeartbeatStale,
        MergeOwnershipEvent::BinaryWriteRequested,
        MergeOwnershipEvent::PatchAckObserved,
        MergeOwnershipEvent::Committed,
    ];

    #[test]
    fn editor_path_accepts_normal_closeout_order() {
        let machine = MergeOwnershipMachine::new(MergeOwnershipPhase::Detached);

        assert!(machine.send(MergeOwnershipEvent::EditorAttached));
        assert_eq!(machine.state(), MergeOwnershipPhase::Attached);
        assert!(machine.send(MergeOwnershipEvent::EditorBufferObserved));
        assert_eq!(machine.state(), MergeOwnershipPhase::EditorOwnsBuffer);
        assert!(machine.send(MergeOwnershipEvent::BinaryWriteRequested));
        assert_eq!(machine.state(), MergeOwnershipPhase::BinaryWriteRequested);
        assert!(machine.send(MergeOwnershipEvent::PatchAckObserved));
        assert_eq!(machine.state(), MergeOwnershipPhase::IpcAckProven);
        assert!(machine.send(MergeOwnershipEvent::Committed));
        assert_eq!(machine.state(), MergeOwnershipPhase::Committed);
    }

    #[test]
    fn detached_cli_only_path_commits_directly() {
        // No editor: the binary writes disk directly from Detached, then commits.
        // It must never enter the editor handshake states.
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::Detached,
                MergeOwnershipEvent::BinaryWriteRequested,
            ),
            None
        );
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::Detached,
                MergeOwnershipEvent::Committed,
            ),
            Some(MergeOwnershipPhase::Committed)
        );
    }

    #[test]
    fn heartbeat_stale_demotes_only_attached_stale_listener() {
        // #6b5h: a stale listener (Attached, no proven editor) demotes to Detached.
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::Attached,
                MergeOwnershipEvent::HeartbeatStale,
            ),
            Some(MergeOwnershipPhase::Detached)
        );
        // A proven editor buffer survives a stale heartbeat (idle editor, live pid).
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::EditorOwnsBuffer,
                MergeOwnershipEvent::HeartbeatStale,
            ),
            None
        );
        // Already detached is a no-op reject.
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::Detached,
                MergeOwnershipEvent::HeartbeatStale,
            ),
            None
        );
    }

    #[test]
    fn disk_write_permitted_only_for_detached_or_ack_proven() {
        for phase in ALL_PHASES {
            let permitted = disk_write_permitted(phase);
            match phase {
                MergeOwnershipPhase::Detached | MergeOwnershipPhase::IpcAckProven => {
                    assert!(permitted, "{phase:?} must permit disk write");
                }
                _ => {
                    assert!(
                        !permitted,
                        "{phase:?} must NOT permit disk write (File Cache Conflict guard)"
                    );
                }
            }
        }
    }

    #[test]
    fn commit_requires_proven_safe_write_phase() {
        // Editor path: cannot commit while the editor still owns the buffer or
        // the patch is unacked.
        for phase in [
            MergeOwnershipPhase::Attached,
            MergeOwnershipPhase::EditorOwnsBuffer,
            MergeOwnershipPhase::BinaryWriteRequested,
        ] {
            assert_eq!(
                MergeOwnershipMachine::transition(phase, MergeOwnershipEvent::Committed),
                None,
                "{phase:?} must not commit before a proven write"
            );
        }
        // Detached (direct disk) and IpcAckProven (editor applied) may commit.
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::Detached,
                MergeOwnershipEvent::Committed,
            ),
            Some(MergeOwnershipPhase::Committed)
        );
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::IpcAckProven,
                MergeOwnershipEvent::Committed,
            ),
            Some(MergeOwnershipPhase::Committed)
        );
    }

    #[test]
    fn in_flight_write_is_not_disturbed_by_attach_detach_churn() {
        for churn in [
            MergeOwnershipEvent::EditorAttached,
            MergeOwnershipEvent::EditorBufferObserved,
            MergeOwnershipEvent::EditorDetached,
            MergeOwnershipEvent::HeartbeatStale,
        ] {
            for phase in [
                MergeOwnershipPhase::BinaryWriteRequested,
                MergeOwnershipPhase::IpcAckProven,
            ] {
                assert_eq!(
                    MergeOwnershipMachine::transition(phase, churn),
                    None,
                    "{churn:?} must not disturb in-flight {phase:?}"
                );
            }
        }
    }

    #[test]
    fn committed_is_terminal() {
        assert!(MergeOwnershipPhase::Committed.is_terminal());
        let machine = MergeOwnershipMachine::new(MergeOwnershipPhase::Committed);
        for event in ALL_EVENTS {
            // Committed accepts only its own idempotent self-transition.
            let accepted = machine.send(event);
            match event {
                MergeOwnershipEvent::Committed => assert!(accepted),
                _ => assert!(!accepted, "{event:?} must not leave Committed"),
            }
            assert_eq!(machine.state(), MergeOwnershipPhase::Committed);
        }
    }

    #[test]
    fn patch_ack_requires_pending_binary_write_request() {
        // ACK is the response to a queued patch; it cannot arrive cold.
        for phase in [
            MergeOwnershipPhase::Detached,
            MergeOwnershipPhase::Attached,
            MergeOwnershipPhase::EditorOwnsBuffer,
        ] {
            assert_eq!(
                MergeOwnershipMachine::transition(phase, MergeOwnershipEvent::PatchAckObserved),
                None,
                "{phase:?} must not observe a patch ACK without a pending request"
            );
        }
        // Once requested (or already acked) the ACK is accepted/idempotent.
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::BinaryWriteRequested,
                MergeOwnershipEvent::PatchAckObserved,
            ),
            Some(MergeOwnershipPhase::IpcAckProven)
        );
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::IpcAckProven,
                MergeOwnershipEvent::PatchAckObserved,
            ),
            Some(MergeOwnershipPhase::IpcAckProven)
        );
    }

    #[test]
    fn transition_table_is_total_and_non_panicking() {
        // Every (phase, event) pair must resolve without panic.
        for phase in ALL_PHASES {
            for event in ALL_EVENTS {
                let _ = transition_merge_ownership(&phase, &event);
            }
        }
    }

    #[test]
    fn ownership_probe_only_resolves_attached() {
        // Non-Attached phases are not probe-resolvable.
        for phase in ALL_PHASES {
            if phase == MergeOwnershipPhase::Attached {
                continue;
            }
            assert_eq!(
                ownership_probe(
                    phase,
                    &OwnershipLiveness {
                        lease_present: true,
                        pid_live: true,
                        heartbeat_fresh: true,
                    },
                ),
                None,
                "{phase:?} should not be probe-resolvable"
            );
        }
    }

    #[test]
    fn ownership_probe_promotes_live_editor_regardless_of_heartbeat_freshness() {
        // Fail-safe: an idle editor (stale heartbeat) with a live pid is still a
        // real editor and must promote, not demote to a disk-write path.
        for heartbeat_fresh in [true, false] {
            let event = ownership_probe(
                MergeOwnershipPhase::Attached,
                &OwnershipLiveness {
                    lease_present: true,
                    pid_live: true,
                    heartbeat_fresh,
                },
            );
            assert_eq!(
                event,
                Some(MergeOwnershipEvent::EditorBufferObserved),
                "live pid must promote even when heartbeat_fresh={heartbeat_fresh}"
            );
        }
    }

    #[test]
    fn ownership_probe_demotes_dead_pid_editor() {
        let event = ownership_probe(
            MergeOwnershipPhase::Attached,
            &OwnershipLiveness {
                lease_present: true,
                pid_live: false,
                heartbeat_fresh: true,
            },
        );
        assert_eq!(event, Some(MergeOwnershipEvent::EditorDetached));
    }

    #[test]
    fn ownership_probe_demotes_stale_listener_no_lease() {
        // #6b5h: connectable socket but no editor buffer behind it.
        let event = ownership_probe(
            MergeOwnershipPhase::Attached,
            &OwnershipLiveness {
                lease_present: false,
                pid_live: false,
                heartbeat_fresh: false,
            },
        );
        assert_eq!(event, Some(MergeOwnershipEvent::HeartbeatStale));
    }

    #[test]
    fn ownership_liveness_from_lease_round_trips_signals() {
        let live = PluginOwnerLease {
            consumer_id: "jetbrains-42".to_string(),
            pid: 42,
            heartbeat_secs: 100,
        };
        let facts = OwnershipLiveness::from_lease(Some(&live), |pid| pid == 42, 100, Duration::from_secs(30));
        assert!(facts.lease_present);
        assert!(facts.pid_live);
        assert!(facts.heartbeat_fresh);

        // Stale heartbeat (age beyond ttl) still reports pid_live separately.
        let facts = OwnershipLiveness::from_lease(Some(&live), |pid| pid == 42, 200, Duration::from_secs(30));
        assert!(facts.pid_live);
        assert!(!facts.heartbeat_fresh);

        // Dead pid.
        let facts = OwnershipLiveness::from_lease(Some(&live), |_| false, 100, Duration::from_secs(30));
        assert!(facts.lease_present);
        assert!(!facts.pid_live);

        // Absent lease is the CLI-only / stale-listener case.
        let facts = OwnershipLiveness::from_lease(None, |_| true, 100, Duration::from_secs(30));
        assert_eq!(facts, OwnershipLiveness::default());
    }

    #[test]
    fn detach_during_attached_or_editor_owned_returns_to_detached() {
        for phase in [
            MergeOwnershipPhase::Detached,
            MergeOwnershipPhase::Attached,
            MergeOwnershipPhase::EditorOwnsBuffer,
        ] {
            let next = MergeOwnershipMachine::transition(phase, MergeOwnershipEvent::EditorDetached);
            assert_eq!(next, Some(MergeOwnershipPhase::Detached));
        }
    }

    #[test]
    fn disk_write_permitted_for_file_matches_live_editor_endpoint_attached() {
        // The SM write-path gate must be decision-equivalent to the inverted
        // live-editor check: a live editor → refuse; no editor → permit.
        use crate::plugin_owner::{
            self, editor_endpoint_attached_for_lease, PluginOwnerLease,
        };

        let cases = [
            // (lease_pid, pid_live_injected) -> editor attached, disk permitted
            (Some(42u32), true),
            (Some(42), false),
            (None, false),
        ];
        for (lease_pid, pid_live) in cases {
            let lease = lease_pid.map(|pid| PluginOwnerLease {
                consumer_id: format!("test-{pid}"),
                pid,
                heartbeat_secs: 0,
            });
            let attached = editor_endpoint_attached_for_lease(lease.clone(), |_| pid_live);
            // Build the SM liveness decision directly from the same facts.
            let liveness = OwnershipLiveness {
                lease_present: lease.is_some(),
                pid_live: attached, // mirrors editor_endpoint_attached_for_lease
                heartbeat_fresh: false,
            };
            let resolved = ownership_probe(MergeOwnershipPhase::Attached, &liveness);
            let phase = match resolved {
                Some(MergeOwnershipEvent::EditorBufferObserved) => {
                    MergeOwnershipPhase::EditorOwnsBuffer
                }
                _ => MergeOwnershipPhase::Detached,
            };
            assert_eq!(
                disk_write_permitted(phase),
                !attached,
                "lease_pid={lease_pid:?} pid_live={pid_live}: SM gate must invert live-editor check"
            );
        }
        // Quiet the unused-import warning when the helper isn't compiled in.
        let _ = plugin_owner::plugin_owner_pid_is_live(0);
    }

    // ---- #mergestatemachine3: end-to-end wedge-class repros ----
    //
    // The transition tests above prove individual edges; these walk the two
    // operator-reported failure classes as full live sequences and assert the
    // disk-write gate at *every* step, so a future edge regression that still
    // type-checks (e.g. permitting a write mid-handshake) is caught as the
    // concrete `File Cache Conflict` / `#6b5h` reproduction it would cause.

    #[test]
    fn file_cache_conflict_repro_never_writes_disk_behind_live_editor() {
        // Class: a live editor owns the buffer while the binary wants to write.
        // Writing disk in any phase between attach and ACK is exactly the
        // `File Cache Conflict` dialog. Walk the full editor closeout and prove
        // the gate forbids a disk write at every phase the editor could still
        // be mid-apply, and permits it only once the patch is ACK-proven.
        let machine = MergeOwnershipMachine::new(MergeOwnershipPhase::Detached);
        assert!(
            disk_write_permitted(machine.state()),
            "no editor yet: direct disk write is the CLI-only path"
        );

        assert!(machine.send(MergeOwnershipEvent::EditorAttached));
        assert_eq!(machine.state(), MergeOwnershipPhase::Attached);
        assert!(
            !disk_write_permitted(machine.state()),
            "Attached is ambiguous (#6b5h) — must NOT write until ownership is resolved"
        );

        assert!(machine.send(MergeOwnershipEvent::EditorBufferObserved));
        assert_eq!(machine.state(), MergeOwnershipPhase::EditorOwnsBuffer);
        assert!(
            !disk_write_permitted(machine.state()),
            "live editor owns the buffer — a disk write here IS the File Cache Conflict"
        );

        assert!(machine.send(MergeOwnershipEvent::BinaryWriteRequested));
        assert_eq!(machine.state(), MergeOwnershipPhase::BinaryWriteRequested);
        assert!(
            !disk_write_permitted(machine.state()),
            "patch queued but unacked — editor may still apply its own buffer state"
        );

        assert!(machine.send(MergeOwnershipEvent::PatchAckObserved));
        assert_eq!(machine.state(), MergeOwnershipPhase::IpcAckProven);
        assert!(
            disk_write_permitted(machine.state()),
            "editor applied + ACKed — a disk sync now lands behind the editor's apply"
        );

        assert!(machine.send(MergeOwnershipEvent::Committed));
        assert_eq!(machine.state(), MergeOwnershipPhase::Committed);
    }

    #[test]
    fn stale_listener_6b5h_wedge_repro_routes_to_disk_instead_of_no_ack() {
        // Class: the IPC listener is registered but no live editor sits behind
        // it (a controller-hosted connectable socket). The pre-SM behavior
        // queued a patch and wedged on `no_ack` / `retry_without_disk_write`
        // forever. The SM must demote the stale listener to Detached on a stale
        // heartbeat so the write routes to the controller-host disk path.
        let machine = MergeOwnershipMachine::new(MergeOwnershipPhase::Detached);

        assert!(machine.send(MergeOwnershipEvent::EditorAttached));
        assert_eq!(machine.state(), MergeOwnershipPhase::Attached);
        assert!(
            !disk_write_permitted(machine.state()),
            "ambiguous listener: writing now races; waiting forever is the #6b5h wedge"
        );

        // Liveness probe finds no proven editor behind the listener → stale.
        assert!(machine.send(MergeOwnershipEvent::HeartbeatStale));
        assert_eq!(
            machine.state(),
            MergeOwnershipPhase::Detached,
            "stale listener demotes to Detached (#6b5h resolution)"
        );
        assert!(
            disk_write_permitted(machine.state()),
            "no live editor — the write routes to disk instead of wedging on no_ack"
        );

        assert!(machine.send(MergeOwnershipEvent::Committed));
        assert_eq!(machine.state(), MergeOwnershipPhase::Committed);
    }

    #[test]
    fn idle_editor_survives_stale_heartbeat_no_false_disk_write() {
        // Inverse safety of the #6b5h repro: a *real* idle editor (live pid,
        // stale heartbeat) must NOT be demoted by a stale heartbeat, or the
        // binary would raw-write behind it and reintroduce File Cache Conflict.
        // Staleness alone never demotes a proven EditorOwnsBuffer.
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::EditorOwnsBuffer,
                MergeOwnershipEvent::HeartbeatStale,
            ),
            None,
            "proven editor buffer survives a stale heartbeat — keyed off pid liveness, not freshness"
        );
        assert!(
            !disk_write_permitted(MergeOwnershipPhase::EditorOwnsBuffer),
            "an idle but live editor still owns the buffer — no disk write"
        );
    }
}

//! Pure plugin/binary merge-control write-ownership state machine.
//!
//! This module owns the side-effect-free write ownership transition table. It
//! accepts plain liveness facts and never reads plugin sidecars, files, IPC, or
//! process state.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

/// Write-ownership phase for a document's editor buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeOwnershipPhase {
    Detached,
    Attached,
    EditorOwnsBuffer,
    BinaryWriteRequested,
    #[serde(rename = "lazily_patch_applied_proven")]
    LazilyPatchAppliedProven,
    Committed,
}

impl MergeOwnershipPhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, MergeOwnershipPhase::Committed)
    }
}

/// Events driving the write-ownership transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOwnershipEvent {
    EditorAttached,
    EditorBufferObserved,
    EditorDetached,
    HeartbeatStale,
    BinaryWriteRequested,
    LazilyPatchAppliedObserved,
    Committed,
}

pub struct MergeOwnershipMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<MergeOwnershipPhase, MergeOwnershipEvent>,
}

impl MergeOwnershipMachine {
    /// Join this machine to `scope`'s graph (`#stategraphjoin`).
    ///
    /// Merge ownership is a fact about one open document: who currently owns the buffer, and it dies with the document, not with a turn.
    ///
    /// The scope owns the context, so dropping the scope drops this machine's cells —
    /// teardown is the scope's lifetime, not a separate deregistration step.
    pub fn new_in(scope: &agent_doc_state_scope::DocumentScope, initial: MergeOwnershipPhase) -> Self {
        let ctx = scope.ctx().clone();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_merge_ownership);
        Self { ctx, machine }
    }

    /// Standalone machine in a private context — a pure-transition helper for unit
    /// tests only.
    ///
    /// A long-lived owner must use [`Self::new_in`] instead: nothing outside a private
    /// context can derive from its cells and invalidation never crosses it, so a
    /// `Computed` built over one is Computed in name only.
    pub fn new(initial: MergeOwnershipPhase) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
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

/// Safety invariant: a disk write is permitted only when no live editor owns the
/// buffer or after the editor has published the lazily patch-applied event.
pub fn disk_write_permitted(phase: MergeOwnershipPhase) -> bool {
    matches!(
        phase,
        MergeOwnershipPhase::Detached | MergeOwnershipPhase::LazilyPatchAppliedProven
    )
}

/// Pure transition table for [`MergeOwnershipMachine`].
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
            | MergeOwnershipPhase::LazilyPatchAppliedProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::EditorBufferObserved => match current {
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::EditorOwnsBuffer => Some(MergeOwnershipPhase::EditorOwnsBuffer),
            MergeOwnershipPhase::BinaryWriteRequested
            | MergeOwnershipPhase::LazilyPatchAppliedProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::EditorDetached => match current {
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::EditorOwnsBuffer => Some(MergeOwnershipPhase::Detached),
            MergeOwnershipPhase::BinaryWriteRequested
            | MergeOwnershipPhase::LazilyPatchAppliedProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::HeartbeatStale => match current {
            MergeOwnershipPhase::Attached => Some(MergeOwnershipPhase::Detached),
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::EditorOwnsBuffer
            | MergeOwnershipPhase::BinaryWriteRequested
            | MergeOwnershipPhase::LazilyPatchAppliedProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::BinaryWriteRequested => match current {
            MergeOwnershipPhase::EditorOwnsBuffer | MergeOwnershipPhase::BinaryWriteRequested => {
                Some(MergeOwnershipPhase::BinaryWriteRequested)
            }
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::LazilyPatchAppliedProven
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::LazilyPatchAppliedObserved => match current {
            MergeOwnershipPhase::BinaryWriteRequested
            | MergeOwnershipPhase::LazilyPatchAppliedProven => {
                Some(MergeOwnershipPhase::LazilyPatchAppliedProven)
            }
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::EditorOwnsBuffer
            | MergeOwnershipPhase::Committed => None,
        },
        MergeOwnershipEvent::Committed => match current {
            MergeOwnershipPhase::Detached
            | MergeOwnershipPhase::LazilyPatchAppliedProven
            | MergeOwnershipPhase::Committed => Some(MergeOwnershipPhase::Committed),
            MergeOwnershipPhase::Attached
            | MergeOwnershipPhase::EditorOwnsBuffer
            | MergeOwnershipPhase::BinaryWriteRequested => None,
        },
    }
}

/// Liveness facts the [`ownership_probe`] consults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OwnershipLiveness {
    pub lease_present: bool,
    pub pid_live: bool,
    pub heartbeat_fresh: bool,
}

/// Editor-heartbeat liveness probe that distinguishes a live editor from a stale
/// listener, advancing an ambiguous [`MergeOwnershipPhase::Attached`] state to
/// the correct event.
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
        MergeOwnershipPhase::LazilyPatchAppliedProven,
        MergeOwnershipPhase::Committed,
    ];

    const ALL_EVENTS: [MergeOwnershipEvent; 7] = [
        MergeOwnershipEvent::EditorAttached,
        MergeOwnershipEvent::EditorBufferObserved,
        MergeOwnershipEvent::EditorDetached,
        MergeOwnershipEvent::HeartbeatStale,
        MergeOwnershipEvent::BinaryWriteRequested,
        MergeOwnershipEvent::LazilyPatchAppliedObserved,
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
        assert!(machine.send(MergeOwnershipEvent::LazilyPatchAppliedObserved));
        assert_eq!(
            machine.state(),
            MergeOwnershipPhase::LazilyPatchAppliedProven
        );
        assert!(machine.send(MergeOwnershipEvent::Committed));
        assert_eq!(machine.state(), MergeOwnershipPhase::Committed);
    }

    #[test]
    fn detached_cli_only_path_commits_directly() {
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
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::Attached,
                MergeOwnershipEvent::HeartbeatStale,
            ),
            Some(MergeOwnershipPhase::Detached)
        );
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::EditorOwnsBuffer,
                MergeOwnershipEvent::HeartbeatStale,
            ),
            None
        );
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::Detached,
                MergeOwnershipEvent::HeartbeatStale,
            ),
            None
        );
    }

    #[test]
    fn disk_write_permitted_only_for_detached_or_patch_applied_event_proven() {
        for phase in ALL_PHASES {
            let permitted = disk_write_permitted(phase);
            match phase {
                MergeOwnershipPhase::Detached | MergeOwnershipPhase::LazilyPatchAppliedProven => {
                    assert!(permitted, "{phase:?} must permit disk write");
                }
                _ => assert!(!permitted, "{phase:?} must not permit disk write"),
            }
        }
    }

    #[test]
    fn commit_requires_proven_safe_write_phase() {
        for phase in [
            MergeOwnershipPhase::Attached,
            MergeOwnershipPhase::EditorOwnsBuffer,
            MergeOwnershipPhase::BinaryWriteRequested,
        ] {
            assert_eq!(
                MergeOwnershipMachine::transition(phase, MergeOwnershipEvent::Committed),
                None
            );
        }
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::Detached,
                MergeOwnershipEvent::Committed,
            ),
            Some(MergeOwnershipPhase::Committed)
        );
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::LazilyPatchAppliedProven,
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
                MergeOwnershipPhase::LazilyPatchAppliedProven,
            ] {
                assert_eq!(MergeOwnershipMachine::transition(phase, churn), None);
            }
        }
    }

    #[test]
    fn committed_is_terminal() {
        assert!(MergeOwnershipPhase::Committed.is_terminal());
        let machine = MergeOwnershipMachine::new(MergeOwnershipPhase::Committed);
        for event in ALL_EVENTS {
            let accepted = machine.send(event);
            match event {
                MergeOwnershipEvent::Committed => assert!(accepted),
                _ => assert!(!accepted),
            }
            assert_eq!(machine.state(), MergeOwnershipPhase::Committed);
        }
    }

    #[test]
    fn lazily_patch_applied_requires_pending_binary_write_request() {
        for phase in [
            MergeOwnershipPhase::Detached,
            MergeOwnershipPhase::Attached,
            MergeOwnershipPhase::EditorOwnsBuffer,
        ] {
            assert_eq!(
                MergeOwnershipMachine::transition(
                    phase,
                    MergeOwnershipEvent::LazilyPatchAppliedObserved
                ),
                None
            );
        }
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::BinaryWriteRequested,
                MergeOwnershipEvent::LazilyPatchAppliedObserved,
            ),
            Some(MergeOwnershipPhase::LazilyPatchAppliedProven)
        );
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::LazilyPatchAppliedProven,
                MergeOwnershipEvent::LazilyPatchAppliedObserved,
            ),
            Some(MergeOwnershipPhase::LazilyPatchAppliedProven)
        );
    }

    #[test]
    fn transition_table_is_total_and_non_panicking() {
        for phase in ALL_PHASES {
            for event in ALL_EVENTS {
                let _ = transition_merge_ownership(&phase, &event);
            }
        }
    }

    #[test]
    fn ownership_probe_only_resolves_attached() {
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
                None
            );
        }
    }

    #[test]
    fn ownership_probe_promotes_live_editor_regardless_of_heartbeat_freshness() {
        for heartbeat_fresh in [true, false] {
            let event = ownership_probe(
                MergeOwnershipPhase::Attached,
                &OwnershipLiveness {
                    lease_present: true,
                    pid_live: true,
                    heartbeat_fresh,
                },
            );
            assert_eq!(event, Some(MergeOwnershipEvent::EditorBufferObserved));
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
    fn detach_during_attached_or_editor_owned_returns_to_detached() {
        for phase in [
            MergeOwnershipPhase::Detached,
            MergeOwnershipPhase::Attached,
            MergeOwnershipPhase::EditorOwnsBuffer,
        ] {
            let next =
                MergeOwnershipMachine::transition(phase, MergeOwnershipEvent::EditorDetached);
            assert_eq!(next, Some(MergeOwnershipPhase::Detached));
        }
    }

    #[test]
    fn file_cache_conflict_repro_never_writes_disk_behind_live_editor() {
        let machine = MergeOwnershipMachine::new(MergeOwnershipPhase::Detached);
        assert!(disk_write_permitted(machine.state()));

        assert!(machine.send(MergeOwnershipEvent::EditorAttached));
        assert_eq!(machine.state(), MergeOwnershipPhase::Attached);
        assert!(!disk_write_permitted(machine.state()));

        assert!(machine.send(MergeOwnershipEvent::EditorBufferObserved));
        assert_eq!(machine.state(), MergeOwnershipPhase::EditorOwnsBuffer);
        assert!(!disk_write_permitted(machine.state()));

        assert!(machine.send(MergeOwnershipEvent::BinaryWriteRequested));
        assert_eq!(machine.state(), MergeOwnershipPhase::BinaryWriteRequested);
        assert!(!disk_write_permitted(machine.state()));

        assert!(machine.send(MergeOwnershipEvent::LazilyPatchAppliedObserved));
        assert_eq!(
            machine.state(),
            MergeOwnershipPhase::LazilyPatchAppliedProven
        );
        assert!(disk_write_permitted(machine.state()));

        assert!(machine.send(MergeOwnershipEvent::Committed));
        assert_eq!(machine.state(), MergeOwnershipPhase::Committed);
    }

    #[test]
    fn stale_listener_wedge_repro_routes_to_disk_instead_of_missing_lazily_event() {
        let machine = MergeOwnershipMachine::new(MergeOwnershipPhase::Detached);

        assert!(machine.send(MergeOwnershipEvent::EditorAttached));
        assert_eq!(machine.state(), MergeOwnershipPhase::Attached);
        assert!(!disk_write_permitted(machine.state()));

        assert!(machine.send(MergeOwnershipEvent::HeartbeatStale));
        assert_eq!(machine.state(), MergeOwnershipPhase::Detached);
        assert!(disk_write_permitted(machine.state()));

        assert!(machine.send(MergeOwnershipEvent::Committed));
        assert_eq!(machine.state(), MergeOwnershipPhase::Committed);
    }

    #[test]
    fn idle_editor_survives_stale_heartbeat_no_false_disk_write() {
        assert_eq!(
            MergeOwnershipMachine::transition(
                MergeOwnershipPhase::EditorOwnsBuffer,
                MergeOwnershipEvent::HeartbeatStale,
            ),
            None
        );
        assert!(!disk_write_permitted(MergeOwnershipPhase::EditorOwnsBuffer));
    }
}

//! Orchestration adapters for plugin/binary merge-control write ownership.
//!
//! The side-effect-free ownership state machine lives in
//! `agent-doc-merge::ownership`. Orchestration only translates runtime
//! plugin-owner lease facts into that pure model and exposes the current
//! file-based write gate used by converge/commit paths.

use std::time::Duration;

pub use agent_doc_merge::ownership::{
    MergeOwnershipEvent, MergeOwnershipMachine, MergeOwnershipPhase, OwnershipLiveness,
    disk_write_permitted, ownership_probe, transition_merge_ownership,
};

use crate::plugin_owner::{self, PluginOwnerLease};

/// Build ownership liveness facts from an optional plugin-owner lease, injecting
/// the pid-liveness predicate and clock so the adapter remains testable without
/// spawning a real editor process.
pub fn ownership_liveness_from_lease(
    lease: Option<&PluginOwnerLease>,
    is_pid_live: impl Fn(u32) -> bool,
    now: u64,
    ttl: Duration,
) -> OwnershipLiveness {
    match lease {
        Some(lease) => OwnershipLiveness {
            lease_present: true,
            pid_live: is_pid_live(lease.pid),
            heartbeat_fresh: plugin_owner::plugin_owner_lease_is_fresh(
                lease.heartbeat_secs,
                now,
                ttl,
            ),
        },
        None => OwnershipLiveness::default(),
    }
}

/// Read the live editor-attachment facts for a document from its plugin-owner
/// lease sidecar. Ownership is observed fresh each write; no persistent
/// ownership phase is stored in orchestration.
pub fn ownership_liveness_for_file(file: &str) -> OwnershipLiveness {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    ownership_liveness_from_lease(
        plugin_owner::read_plugin_owner_lease(file).as_ref(),
        plugin_owner::plugin_owner_pid_is_live,
        now,
        plugin_owner::plugin_owner_ttl(),
    )
}

/// Resolve whether a disk write is permitted for a document from its current
/// editor-attachment facts, expressed through the pure ownership state machine.
///
/// Decision-equivalent to `!plugin_owner::live_editor_endpoint_attached(file)`,
/// but routed through the ownership vocabulary so the write-path gate and state
/// machine share one authority.
pub fn disk_write_permitted_for_file(file: &str) -> bool {
    let liveness = ownership_liveness_for_file(file);
    let resolved = ownership_probe(MergeOwnershipPhase::Attached, &liveness);
    let phase = match resolved {
        Some(MergeOwnershipEvent::EditorBufferObserved) => MergeOwnershipPhase::EditorOwnsBuffer,
        _ => MergeOwnershipPhase::Detached,
    };
    disk_write_permitted(phase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_liveness_from_lease_round_trips_signals() {
        let live = PluginOwnerLease {
            consumer_id: "jetbrains-42".to_string(),
            pid: 42,
            heartbeat_secs: 100,
        };
        let facts = ownership_liveness_from_lease(
            Some(&live),
            |pid| pid == 42,
            100,
            Duration::from_secs(30),
        );
        assert!(facts.lease_present);
        assert!(facts.pid_live);
        assert!(facts.heartbeat_fresh);

        let facts = ownership_liveness_from_lease(
            Some(&live),
            |pid| pid == 42,
            200,
            Duration::from_secs(30),
        );
        assert!(facts.pid_live);
        assert!(!facts.heartbeat_fresh);

        let facts =
            ownership_liveness_from_lease(Some(&live), |_| false, 100, Duration::from_secs(30));
        assert!(facts.lease_present);
        assert!(!facts.pid_live);

        let facts = ownership_liveness_from_lease(None, |_| true, 100, Duration::from_secs(30));
        assert_eq!(facts, OwnershipLiveness::default());
    }

    #[test]
    fn disk_write_permitted_for_file_matches_live_editor_endpoint_attached() {
        use crate::plugin_owner::{PluginOwnerLease, editor_endpoint_attached_for_lease};

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
            let liveness = OwnershipLiveness {
                lease_present: lease.is_some(),
                pid_live: attached,
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
    }

    #[test]
    fn reexported_machine_still_accepts_editor_path() {
        let machine = MergeOwnershipMachine::new(MergeOwnershipPhase::Detached);
        assert!(machine.send(MergeOwnershipEvent::EditorAttached));
        assert!(machine.send(MergeOwnershipEvent::EditorBufferObserved));
        assert!(machine.send(MergeOwnershipEvent::BinaryWriteRequested));
        assert!(machine.send(MergeOwnershipEvent::PatchAckObserved));
        assert!(disk_write_permitted(machine.state()));
    }
}

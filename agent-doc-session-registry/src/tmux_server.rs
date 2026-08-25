//! Pure tmux-server identity reconciliation policy.

/// Stable identity for one tmux server lifetime.
///
/// Pane IDs are only unique inside a server lifetime. Persisting both tmux's
/// server PID and start time lets registry I/O reject rows carried across a
/// workspace restart even when the replacement server reuses a pane ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TmuxServerIdentity {
    pub pid: u32,
    pub start_time: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmuxServerIdentityAction {
    /// First observation after upgrading; preserve the current registry.
    Initialize,
    /// The durable rows belong to the currently running server.
    Keep,
    /// The server lifetime changed; every prior pane row is stale.
    Replace,
}

pub fn tmux_server_identity_action(
    previous: Option<TmuxServerIdentity>,
    current: TmuxServerIdentity,
) -> TmuxServerIdentityAction {
    match previous {
        None => TmuxServerIdentityAction::Initialize,
        Some(previous) if previous == current => TmuxServerIdentityAction::Keep,
        Some(_) => TmuxServerIdentityAction::Replace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TmuxRestartSimWorld {
        identity: Option<TmuxServerIdentity>,
        registry_rows: Vec<&'static str>,
        stale_sockets: Vec<&'static str>,
    }

    impl TmuxRestartSimWorld {
        fn first_command(&mut self, current: TmuxServerIdentity, live_session: &'static str) {
            if tmux_server_identity_action(self.identity, current)
                == TmuxServerIdentityAction::Replace
            {
                self.registry_rows.clear();
                self.stale_sockets.clear();
            }
            self.identity = Some(current);
            self.registry_rows.push(live_session);
        }
    }

    #[test]
    fn sim_world_replacement_server_reconciles_to_one_live_session() {
        let server_a = TmuxServerIdentity {
            pid: 100,
            start_time: 1_000,
        };
        let server_b = TmuxServerIdentity {
            pid: 200,
            start_time: 2_000,
        };
        let mut world = TmuxRestartSimWorld {
            identity: Some(server_a),
            registry_rows: vec!["old-a", "old-b"],
            stale_sockets: vec!["ipc-100.sock", "ipc-101.sock"],
        };

        world.first_command(server_b, "live-b");

        assert_eq!(world.identity, Some(server_b));
        assert_eq!(world.registry_rows, vec!["live-b"]);
        assert!(world.stale_sockets.is_empty());
    }

    #[test]
    fn first_identity_observation_is_migration_safe() {
        let current = TmuxServerIdentity {
            pid: 100,
            start_time: 1_000,
        };
        assert_eq!(
            tmux_server_identity_action(None, current),
            TmuxServerIdentityAction::Initialize
        );
        assert_eq!(
            tmux_server_identity_action(Some(current), current),
            TmuxServerIdentityAction::Keep
        );
    }
}

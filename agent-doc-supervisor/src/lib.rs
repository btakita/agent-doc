//! Pure supervisor realtime state model.
//!
//! This crate owns supervisor state and decisions. It does not spawn, kill,
//! unlink sockets, run tmux commands, mutate documents, or write commits.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

pub mod agent_change;
pub mod config;
pub mod crash_policy;
pub mod handoff;
pub mod idle_reconcile;
pub mod lifecycle;
pub mod route_owned;
pub mod run_loop;
pub mod selfkill;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorState {
    Starting,
    Ready,
    Busy,
    Blocked,
    Stale,
    Dead,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorEvent {
    Spawned,
    ReadyObserved,
    TurnStarted,
    TurnFinished,
    BlockedObserved,
    HeartbeatStale,
    ProcessDead,
    UserClosed,
    Restarted,
}

pub fn transition_supervisor(
    current: &SupervisorState,
    event: &SupervisorEvent,
) -> Option<SupervisorState> {
    use SupervisorEvent::*;
    use SupervisorState::*;

    match (*current, *event) {
        (Starting, ReadyObserved) | (Stale, Restarted) | (Dead, Restarted) => Some(Ready),
        (Ready, TurnStarted) => Some(Busy),
        (Busy, TurnFinished) => Some(Ready),
        (Ready | Busy | Starting | Stale, BlockedObserved) => Some(Blocked),
        (Ready | Busy | Starting | Blocked, HeartbeatStale) => Some(Stale),
        (Ready | Busy | Starting | Blocked | Stale, ProcessDead) => Some(Dead),
        (_, UserClosed) => Some(Closed),
        (Closed, _) => None,
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SupervisorBinding {
    pub pane_id: String,
    pub generation: u64,
    pub supervisor_instance_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorRecoveryAction {
    None,
    RestartSamePane,
    CloseStaleActor,
    RefuseDispatch,
}

pub fn recovery_action(state: SupervisorState, pane_alive: bool) -> SupervisorRecoveryAction {
    match (state, pane_alive) {
        (SupervisorState::Ready | SupervisorState::Busy, _) => SupervisorRecoveryAction::None,
        (SupervisorState::Stale | SupervisorState::Dead, true) => {
            SupervisorRecoveryAction::RestartSamePane
        }
        (SupervisorState::Stale | SupervisorState::Dead, false) => {
            SupervisorRecoveryAction::CloseStaleActor
        }
        (SupervisorState::Closed, _) => SupervisorRecoveryAction::CloseStaleActor,
        _ => SupervisorRecoveryAction::RefuseDispatch,
    }
}

pub struct SupervisorMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<SupervisorState, SupervisorEvent>,
}

impl SupervisorMachine {
    pub fn new(initial: SupervisorState) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_supervisor);
        Self { ctx, machine }
    }

    pub fn send(&self, event: SupervisorEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> SupervisorState {
        self.machine.state(&self.ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_supervisor_restarts_same_live_pane() {
        assert_eq!(
            recovery_action(SupervisorState::Stale, true),
            SupervisorRecoveryAction::RestartSamePane
        );
    }

    #[test]
    fn busy_turn_returns_to_ready() {
        let machine = SupervisorMachine::new(SupervisorState::Ready);
        assert!(machine.send(SupervisorEvent::TurnStarted));
        assert_eq!(machine.state(), SupervisorState::Busy);
        assert!(machine.send(SupervisorEvent::TurnFinished));
        assert_eq!(machine.state(), SupervisorState::Ready);
    }
}

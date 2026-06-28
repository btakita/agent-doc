//! Tmux turn-executor adapter for agent-doc.
//!
//! Shared tmux observations and model state live in `agent-doc-tmux`. This
//! crate maps those tmux facts into generic turn-executor readiness. It does
//! not execute `tmux` commands, merge document text, or commit document changes.

pub use agent_doc_tmux::{
    TmuxModelMachine, TmuxObservation, TmuxPaneActivity, TmuxRealtimeState, TmuxSupervisorHealth,
};
use agent_doc_turn_executor::{
    TurnExecutorAction, TurnExecutorActivity, TurnExecutorHealth, TurnExecutorKind,
    TurnExecutorRealtimeState, requested_action,
};

pub fn initial_state() -> TurnExecutorRealtimeState {
    executor_state_from_tmux(&TmuxRealtimeState::default())
}

pub fn executor_state_from_tmux(tmux: &TmuxRealtimeState) -> TurnExecutorRealtimeState {
    TurnExecutorRealtimeState {
        kind: TurnExecutorKind::Tmux,
        activity: match tmux.pane {
            TmuxPaneActivity::Unknown => TurnExecutorActivity::Unknown,
            TmuxPaneActivity::PromptReady => TurnExecutorActivity::Ready,
            TmuxPaneActivity::Busy => TurnExecutorActivity::Busy,
            TmuxPaneActivity::OutputActive => TurnExecutorActivity::OutputActive,
            TmuxPaneActivity::Exited => TurnExecutorActivity::Exited,
            TmuxPaneActivity::Missing => TurnExecutorActivity::Missing,
        },
        health: match tmux.supervisor {
            TmuxSupervisorHealth::Unknown => TurnExecutorHealth::Unknown,
            TmuxSupervisorHealth::Healthy => TurnExecutorHealth::Healthy,
            TmuxSupervisorHealth::Stale => TurnExecutorHealth::Stale,
            TmuxSupervisorHealth::Crashed => TurnExecutorHealth::Crashed,
        },
        last_output_watermark: tmux.last_output_watermark.clone(),
    }
}

pub fn transition_tmux(
    current: &TurnExecutorRealtimeState,
    event: &TmuxObservation,
) -> Option<TurnExecutorRealtimeState> {
    agent_doc_tmux::transition_tmux(&tmux_state_from_executor(current), event)
        .map(|state| executor_state_from_tmux(&state))
}

pub struct TmuxTurnExecutorMachine {
    tmux: TmuxModelMachine,
}

impl TmuxTurnExecutorMachine {
    pub fn new(initial: TurnExecutorRealtimeState) -> Self {
        Self::from_tmux_state(tmux_state_from_executor(&initial))
    }

    pub fn from_tmux_state(initial: TmuxRealtimeState) -> Self {
        Self {
            tmux: TmuxModelMachine::new(initial),
        }
    }

    pub fn default_tmux() -> Self {
        Self::from_tmux_state(TmuxRealtimeState::default())
    }

    pub fn send(&self, event: TmuxObservation) -> bool {
        self.tmux.send(event)
    }

    pub fn tmux_state(&self) -> TmuxRealtimeState {
        self.tmux.state()
    }

    pub fn state(&self) -> TurnExecutorRealtimeState {
        executor_state_from_tmux(&self.tmux_state())
    }

    pub fn action(&self) -> TurnExecutorAction {
        requested_action(&self.state())
    }
}

fn tmux_state_from_executor(executor: &TurnExecutorRealtimeState) -> TmuxRealtimeState {
    TmuxRealtimeState {
        pane: match executor.activity {
            TurnExecutorActivity::Unknown => TmuxPaneActivity::Unknown,
            TurnExecutorActivity::Ready => TmuxPaneActivity::PromptReady,
            TurnExecutorActivity::Busy => TmuxPaneActivity::Busy,
            TurnExecutorActivity::OutputActive => TmuxPaneActivity::OutputActive,
            TurnExecutorActivity::Exited => TmuxPaneActivity::Exited,
            TurnExecutorActivity::Missing => TmuxPaneActivity::Missing,
        },
        supervisor: match executor.health {
            TurnExecutorHealth::Unknown => TmuxSupervisorHealth::Unknown,
            TurnExecutorHealth::Healthy => TmuxSupervisorHealth::Healthy,
            TurnExecutorHealth::Degraded | TurnExecutorHealth::Stale => TmuxSupervisorHealth::Stale,
            TurnExecutorHealth::Crashed => TmuxSupervisorHealth::Crashed,
        },
        last_output_watermark: executor.last_output_watermark.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_pane_refuses_dispatch_until_prompt_ready() {
        let machine = TmuxTurnExecutorMachine::default_tmux();

        assert!(machine.send(TmuxObservation::Busy));
        assert_eq!(machine.action(), TurnExecutorAction::RefuseDispatch);

        assert!(machine.send(TmuxObservation::PromptReady));
        assert_eq!(machine.action(), TurnExecutorAction::None);
    }

    #[test]
    fn output_advanced_records_watermark() {
        let machine = TmuxTurnExecutorMachine::default_tmux();
        assert!(machine.send(TmuxObservation::OutputAdvanced {
            watermark: "42".to_string()
        }));

        let state = machine.state();
        assert_eq!(state.activity, TurnExecutorActivity::OutputActive);
        assert_eq!(state.last_output_watermark.as_deref(), Some("42"));
    }

    #[test]
    fn exposes_underlying_tmux_state() {
        let machine = TmuxTurnExecutorMachine::default_tmux();
        assert!(machine.send(TmuxObservation::PromptReady));

        assert_eq!(machine.tmux_state().pane, TmuxPaneActivity::PromptReady);
    }

    #[test]
    fn crashed_supervisor_requests_restart() {
        let state = transition_tmux(&initial_state(), &TmuxObservation::SupervisorCrashed).unwrap();
        assert_eq!(requested_action(&state), TurnExecutorAction::Restart);
    }
}

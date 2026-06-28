//! Shared pure tmux model state for agent-doc.
//!
//! This crate owns reusable tmux observations and realtime state. It does not
//! execute `tmux` commands, dispatch turns, merge document text, or commit
//! document changes.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TmuxPaneActivity {
    Unknown,
    PromptReady,
    Busy,
    OutputActive,
    Exited,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TmuxSupervisorHealth {
    Unknown,
    Healthy,
    Stale,
    Crashed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TmuxRealtimeState {
    pub pane: TmuxPaneActivity,
    pub supervisor: TmuxSupervisorHealth,
    pub last_output_watermark: Option<String>,
}

impl Default for TmuxRealtimeState {
    fn default() -> Self {
        Self {
            pane: TmuxPaneActivity::Unknown,
            supervisor: TmuxSupervisorHealth::Unknown,
            last_output_watermark: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TmuxObservation {
    PaneMissing,
    PaneExited,
    PromptReady,
    Busy,
    OutputAdvanced { watermark: String },
    SupervisorHeartbeat,
    SupervisorStale,
    SupervisorCrashed,
}

pub fn transition_tmux(
    current: &TmuxRealtimeState,
    event: &TmuxObservation,
) -> Option<TmuxRealtimeState> {
    let mut next = current.clone();
    match event {
        TmuxObservation::PaneMissing => next.pane = TmuxPaneActivity::Missing,
        TmuxObservation::PaneExited => next.pane = TmuxPaneActivity::Exited,
        TmuxObservation::PromptReady => next.pane = TmuxPaneActivity::PromptReady,
        TmuxObservation::Busy => next.pane = TmuxPaneActivity::Busy,
        TmuxObservation::OutputAdvanced { watermark } => {
            next.pane = TmuxPaneActivity::OutputActive;
            next.last_output_watermark = Some(watermark.clone());
        }
        TmuxObservation::SupervisorHeartbeat => next.supervisor = TmuxSupervisorHealth::Healthy,
        TmuxObservation::SupervisorStale => next.supervisor = TmuxSupervisorHealth::Stale,
        TmuxObservation::SupervisorCrashed => next.supervisor = TmuxSupervisorHealth::Crashed,
    }
    Some(next)
}

pub struct TmuxModelMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<TmuxRealtimeState, TmuxObservation>,
}

impl TmuxModelMachine {
    pub fn new(initial: TmuxRealtimeState) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_tmux);
        Self { ctx, machine }
    }

    pub fn default_tmux() -> Self {
        Self::new(TmuxRealtimeState::default())
    }

    pub fn send(&self, event: TmuxObservation) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> TmuxRealtimeState {
        self.machine.state(&self.ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_ready_updates_pane_activity() {
        let machine = TmuxModelMachine::default_tmux();

        assert!(machine.send(TmuxObservation::PromptReady));

        assert_eq!(machine.state().pane, TmuxPaneActivity::PromptReady);
    }

    #[test]
    fn output_advanced_records_watermark() {
        let machine = TmuxModelMachine::default_tmux();

        assert!(machine.send(TmuxObservation::OutputAdvanced {
            watermark: "42".to_string()
        }));

        let state = machine.state();
        assert_eq!(state.pane, TmuxPaneActivity::OutputActive);
        assert_eq!(state.last_output_watermark.as_deref(), Some("42"));
    }
}

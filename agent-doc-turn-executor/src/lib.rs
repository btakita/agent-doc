//! Shared turn executor vocabulary for agent-doc.
//!
//! A turn executor is where a turn is dispatched and observed: today tmux,
//! later other terminals or agent APIs. The session document is not an executor;
//! it is the realtime document subject/source/sink. This crate is pure model
//! vocabulary; concrete IO ports live in executor-specific adapter crates.

pub mod auto_trigger;
pub mod capability_proof;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum TurnExecutorKind {
    Tmux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnExecutorActivity {
    Unknown,
    Ready,
    Busy,
    OutputActive,
    Exited,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnExecutorHealth {
    Unknown,
    Healthy,
    Degraded,
    Stale,
    Crashed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnExecutorRealtimeState {
    pub kind: TurnExecutorKind,
    pub activity: TurnExecutorActivity,
    pub health: TurnExecutorHealth,
    pub last_output_watermark: Option<String>,
}

impl TurnExecutorRealtimeState {
    pub fn new(kind: TurnExecutorKind) -> Self {
        Self {
            kind,
            activity: TurnExecutorActivity::Unknown,
            health: TurnExecutorHealth::Unknown,
            last_output_watermark: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnExecutorAction {
    None,
    Probe,
    RefuseDispatch,
    Restart,
}

pub fn requested_action(state: &TurnExecutorRealtimeState) -> TurnExecutorAction {
    match (state.activity, state.health) {
        (TurnExecutorActivity::Missing | TurnExecutorActivity::Exited, _) => {
            TurnExecutorAction::Probe
        }
        (_, TurnExecutorHealth::Crashed) => TurnExecutorAction::Restart,
        (TurnExecutorActivity::Busy | TurnExecutorActivity::OutputActive, _) => {
            TurnExecutorAction::RefuseDispatch
        }
        _ => TurnExecutorAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_executors_refuse_dispatch() {
        let mut state = TurnExecutorRealtimeState::new(TurnExecutorKind::Tmux);
        state.activity = TurnExecutorActivity::Busy;

        assert_eq!(requested_action(&state), TurnExecutorAction::RefuseDispatch);
    }
}

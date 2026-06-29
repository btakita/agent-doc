//! Shared pure tmux model state for agent-doc.
//!
//! This crate owns reusable tmux observations and realtime state. It does not
//! execute `tmux` commands, dispatch turns, merge document text, or commit
//! document changes.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const TMUX_PANE_GEOMETRY_FORMAT: &str =
    "#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height}";

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

/// Outcome of reconciling a candidate pane against the document's provably-live
/// owner pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusPaneDecision {
    /// Keep the candidate pane resolved from projection or registry.
    UseCandidate,
    /// Focus the pane that currently owns the document instead of the stale
    /// projection/registry candidate.
    RepairToLiveOwner(String),
}

/// Decide whether focus should keep its candidate pane or swap to a provable
/// live owner pane.
///
/// The live owner wins only when it is a different, non-empty pane. When no live
/// owner is provable, or it matches the candidate, the existing selection is
/// preserved so the happy path is unchanged.
pub fn decide_focus_pane(candidate: &str, live_owner: Option<&str>) -> FocusPaneDecision {
    match live_owner {
        Some(owner) if !owner.is_empty() && owner != candidate => {
            FocusPaneDecision::RepairToLiveOwner(owner.to_string())
        }
        _ => FocusPaneDecision::UseCandidate,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanePosition {
    Left,
    Right,
    Top,
    Bottom,
}

impl PanePosition {
    pub fn parse(input: &str) -> Result<Self, PanePositionError> {
        match input {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            _ => Err(PanePositionError::InvalidPosition {
                position: input.to_string(),
            }),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TmuxPaneGeometry {
    pub id: String,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

impl TmuxPaneGeometry {
    fn right_edge(&self) -> u32 {
        self.left.saturating_add(self.width)
    }

    fn bottom_edge(&self) -> u32 {
        self.top.saturating_add(self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanePositionError {
    NoPanes { scope: String },
    InvalidPosition { position: String },
    Unresolved { position: String },
}

impl fmt::Display for PanePositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPanes { scope } => write!(f, "no panes found in {scope}"),
            Self::InvalidPosition { position } => write!(
                f,
                "invalid position '{position}' - use left, right, top, or bottom"
            ),
            Self::Unresolved { position } => {
                write!(f, "could not resolve pane for position '{position}'")
            }
        }
    }
}

impl std::error::Error for PanePositionError {}

pub fn parse_tmux_pane_geometry(text: &str) -> Vec<TmuxPaneGeometry> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let id = parts.next()?;
            let left = parts.next()?.parse().unwrap_or(0);
            let top = parts.next()?.parse().unwrap_or(0);
            let width = parts.next()?.parse().unwrap_or(0);
            let height = parts.next()?.parse().unwrap_or(0);
            Some(TmuxPaneGeometry {
                id: id.to_string(),
                left,
                top,
                width,
                height,
            })
        })
        .collect()
}

pub fn select_pane_by_position(
    text: &str,
    position: &str,
    scope: &str,
) -> Result<String, PanePositionError> {
    let panes = parse_tmux_pane_geometry(text);
    if panes.is_empty() {
        return Err(PanePositionError::NoPanes {
            scope: scope.to_string(),
        });
    }
    if panes.len() == 1 {
        return Ok(panes[0].id.clone());
    }

    let position = PanePosition::parse(position)?;
    let selected = match position {
        PanePosition::Left => panes.iter().min_by_key(|pane| pane.left),
        PanePosition::Right => panes.iter().max_by_key(|pane| pane.right_edge()),
        PanePosition::Top => panes.iter().min_by_key(|pane| pane.top),
        PanePosition::Bottom => panes.iter().max_by_key(|pane| pane.bottom_edge()),
    };

    selected
        .map(|pane| pane.id.clone())
        .ok_or_else(|| PanePositionError::Unresolved {
            position: position.as_str().to_string(),
        })
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

    #[test]
    fn focus_decision_keeps_candidate_when_no_live_owner() {
        assert_eq!(
            decide_focus_pane("%36", None),
            FocusPaneDecision::UseCandidate
        );
    }

    #[test]
    fn focus_decision_keeps_candidate_when_owner_matches() {
        assert_eq!(
            decide_focus_pane("%8", Some("%8")),
            FocusPaneDecision::UseCandidate
        );
    }

    #[test]
    fn focus_decision_repairs_to_live_owner_when_candidate_is_stale() {
        assert_eq!(
            decide_focus_pane("%36", Some("%8")),
            FocusPaneDecision::RepairToLiveOwner("%8".to_string())
        );
    }

    #[test]
    fn focus_decision_ignores_empty_live_owner() {
        assert_eq!(
            decide_focus_pane("%36", Some("")),
            FocusPaneDecision::UseCandidate
        );
    }

    #[test]
    fn pane_position_selection_uses_tmux_geometry() {
        let panes = "%left 0 0 80 24\n%right 120 0 80 24\n%bottom 0 24 160 24\n";

        assert_eq!(
            select_pane_by_position(panes, "left", "test").unwrap(),
            "%left"
        );
        assert_eq!(
            select_pane_by_position(panes, "right", "test").unwrap(),
            "%right"
        );
        assert_eq!(
            select_pane_by_position(panes, "bottom", "test").unwrap(),
            "%bottom"
        );
    }

    #[test]
    fn pane_position_selection_rejects_unknown_direction() {
        let err = select_pane_by_position("%0 0 0 80 24\n%1 80 0 80 24\n", "middle", "test")
            .unwrap_err()
            .to_string();

        assert!(err.contains("invalid position 'middle'"));
    }

    #[test]
    fn pane_geometry_parser_treats_bad_numbers_as_zero() {
        assert_eq!(
            parse_tmux_pane_geometry("%0 bad 3 80 nope\n"),
            vec![TmuxPaneGeometry {
                id: "%0".to_string(),
                left: 0,
                top: 3,
                width: 80,
                height: 0,
            }]
        );
    }
}

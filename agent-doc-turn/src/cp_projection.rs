//! Project Controller → plugin turn-state projection.
//!
//! The Project Controller owns the authoritative [`CyclePhase`] machine
//! (`transition_phase`). The editor plugin needs a coarse, stable view of the
//! turn so it can (a) render turn-in-flight UI and (b) decide whether an
//! operator prompt it is about to forward is a clean next-turn prompt or would
//! collide with an in-flight response — the `live_prompt_drift` double-append
//! wedge.
//!
//! This is a **pure projection** over `CyclePhase`: the plugin never owns turn
//! state; it observes this projection and *proposes* events. Each phase also
//! declares which side is the authority for the transition **into** it, encoding
//! the single-authority invariant — the two replicas never both drive the same
//! transition, which is the source of the queue-consume / supervisor self-race
//! wedges. Today the Project Controller is the authority for every transition;
//! the enum exists so the invariant is explicit and testable rather than
//! implied.
//!
//! Spec: `plan-crdt-scramble-and-disk-propagation.md` (turn-state chart, goal 1)
//! and the state-chart discussion that opened this thread.

use crate::CyclePhase;
use serde::{Deserialize, Serialize};

/// Coarse turn state the plugin renders and reasons about. Collapses the finer
/// [`CyclePhase`] persistence phases into the three the editor actually needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    /// No turn in flight — safe to start a fresh turn from an operator prompt.
    Idle,
    /// A prompt was dispatched; the agent response is being produced but is not
    /// yet captured.
    AwaitingResponse,
    /// The response is captured and being persisted (written / committed). The
    /// document is mid-write; an operator prompt now belongs to the next turn.
    Persisting,
}

/// Which replica is the authority for the transition **into** a phase. The
/// Project Controller owns turn state; the plugin observes and proposes. A
/// `Plugin` authority would mean the plugin may drive that transition directly
/// — reserved, not used today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionAuthority {
    /// The project controller drives this transition; the plugin only observes.
    ProjectController,
    /// Reserved: the plugin drives this transition directly. Unused today — the
    /// plugin always proposes through the Project Controller.
    Plugin,
}

/// The plugin-facing projection of the authoritative turn phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnSteeringState {
    #[default]
    None,
    PromptTarget,
    ContentEdit,
    PromptDeleted,
    PromptReduced,
}

fn turn_steering_state_is_none(state: &TurnSteeringState) -> bool {
    matches!(state, TurnSteeringState::None)
}

/// Realtime steering that changed the session document relative to the active
/// turn baseline while the Project Controller turn is still in flight.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TurnSteeringProjection {
    #[serde(default, skip_serializing_if = "turn_steering_state_is_none")]
    pub state: TurnSteeringState,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbatim: Option<String>,
}

const fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl TurnSteeringProjection {
    pub const fn none() -> Self {
        Self {
            state: TurnSteeringState::None,
            count: 0,
            preview: None,
            verbatim: None,
        }
    }

    pub fn observed(state: TurnSteeringState, preview: Option<String>) -> Self {
        Self {
            state,
            count: 1,
            preview,
            verbatim: None,
        }
    }

    pub fn observed_aggregate(
        state: TurnSteeringState,
        count: usize,
        preview: Option<String>,
        verbatim: Option<String>,
    ) -> Self {
        if count == 0 || matches!(state, TurnSteeringState::None) {
            return Self::none();
        }
        Self {
            state,
            count,
            preview,
            verbatim,
        }
    }

    pub fn is_present(&self) -> bool {
        !matches!(self.state, TurnSteeringState::None)
    }
}

fn turn_steering_projection_is_none(steering: &TurnSteeringProjection) -> bool {
    !steering.is_present()
}

/// The plugin-facing projection of the authoritative turn phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnProjection {
    /// Coarse state for UI + prompt-routing decisions.
    pub state: TurnState,
    /// Whether a turn is in flight (response not yet committed/abandoned).
    pub turn_in_flight: bool,
    /// Who owns the transition into the current phase.
    pub transition_authority: TransitionAuthority,
    /// Operator steering observed in the realtime document model during this
    /// in-flight turn. Hidden from JSON when absent so older plugins remain able
    /// to parse the projection.
    #[serde(default, skip_serializing_if = "turn_steering_projection_is_none")]
    pub realtime_steering: TurnSteeringProjection,
}

impl TurnProjection {
    /// Project the authoritative [`CyclePhase`] into the plugin-facing view.
    pub const fn from_phase(phase: CyclePhase) -> Self {
        let state = match phase {
            CyclePhase::PreflightStarted => TurnState::AwaitingResponse,
            CyclePhase::ResponseCaptured | CyclePhase::WriteApplied => TurnState::Persisting,
            CyclePhase::Committed | CyclePhase::Abandoned => TurnState::Idle,
        };
        Self {
            state,
            turn_in_flight: phase.is_open(),
            // The Project Controller is authoritative for every turn-state
            // transition today.
            transition_authority: TransitionAuthority::ProjectController,
            realtime_steering: TurnSteeringProjection::none(),
        }
    }

    pub fn with_realtime_steering(mut self, steering: TurnSteeringProjection) -> Self {
        self.realtime_steering = steering;
        self
    }

    /// Whether an operator prompt forwarded **right now** would start a clean
    /// fresh turn. True only at [`TurnState::Idle`]. While a turn is in flight,
    /// a forwarded prompt must be queued for the next turn, not fed into the
    /// current exchange — feeding it in is the `live_prompt_drift` double-append.
    pub const fn prompt_starts_fresh_turn(&self) -> bool {
        matches!(self.state, TurnState::Idle)
    }

    /// Whether forwarding an operator prompt as current-turn input would collide
    /// with an in-flight response (the double-append wedge). The inverse of
    /// [`Self::prompt_starts_fresh_turn`], named for the guard it drives.
    pub const fn would_collide_with_in_flight_response(&self) -> bool {
        self.turn_in_flight
    }
}

/// The single-authority invariant: the Project Controller owns every turn-state
/// transition and the plugin never drives one directly. Pure, so the invariant
/// is asserted in tests rather than left implicit.
pub const fn transition_authority(_phase: CyclePhase) -> TransitionAuthority {
    TransitionAuthority::ProjectController
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_PHASES: [CyclePhase; 5] = [
        CyclePhase::PreflightStarted,
        CyclePhase::ResponseCaptured,
        CyclePhase::WriteApplied,
        CyclePhase::Committed,
        CyclePhase::Abandoned,
    ];

    #[test]
    fn open_phases_are_in_flight_and_reject_fresh_prompt() {
        for phase in [
            CyclePhase::PreflightStarted,
            CyclePhase::ResponseCaptured,
            CyclePhase::WriteApplied,
        ] {
            let proj = TurnProjection::from_phase(phase);
            assert!(proj.turn_in_flight, "{phase:?} must be in flight");
            assert!(
                !proj.prompt_starts_fresh_turn(),
                "{phase:?} must not accept a fresh-turn prompt"
            );
            assert!(
                proj.would_collide_with_in_flight_response(),
                "{phase:?} must guard against the double-append"
            );
        }
    }

    #[test]
    fn closed_phases_are_idle_and_accept_a_fresh_prompt() {
        for phase in [CyclePhase::Committed, CyclePhase::Abandoned] {
            let proj = TurnProjection::from_phase(phase);
            assert_eq!(proj.state, TurnState::Idle);
            assert!(!proj.turn_in_flight, "{phase:?} must be idle");
            assert!(
                proj.prompt_starts_fresh_turn(),
                "{phase:?} must accept a fresh-turn prompt"
            );
            assert!(!proj.would_collide_with_in_flight_response());
        }
    }

    #[test]
    fn preflight_awaits_response_persist_phases_persist() {
        assert_eq!(
            TurnProjection::from_phase(CyclePhase::PreflightStarted).state,
            TurnState::AwaitingResponse
        );
        assert_eq!(
            TurnProjection::from_phase(CyclePhase::ResponseCaptured).state,
            TurnState::Persisting
        );
        assert_eq!(
            TurnProjection::from_phase(CyclePhase::WriteApplied).state,
            TurnState::Persisting
        );
    }

    #[test]
    fn project_controller_owns_every_turn_state_transition() {
        // The single-authority invariant: the plugin never drives a turn-state
        // transition. If a future phase hands a transition to the plugin, this
        // test must be updated deliberately — it is the wedge-prevention contract.
        for phase in ALL_PHASES {
            assert_eq!(
                transition_authority(phase),
                TransitionAuthority::ProjectController,
                "{phase:?} transition must be Project Controller-authoritative"
            );
            assert_eq!(
                TurnProjection::from_phase(phase).transition_authority,
                TransitionAuthority::ProjectController
            );
        }
    }

    #[test]
    fn projection_round_trips_through_serde() {
        // The plugin consumes this over IPC/FFI, so it must serialize stably.
        let proj = TurnProjection::from_phase(CyclePhase::ResponseCaptured);
        let json = serde_json::to_string(&proj).unwrap();
        let back: TurnProjection = serde_json::from_str(&json).unwrap();
        assert_eq!(proj, back);
        assert!(json.contains("persisting"));
    }

    #[test]
    fn steering_projection_is_optional_but_round_trips_when_present() {
        let proj = TurnProjection::from_phase(CyclePhase::PreflightStarted).with_realtime_steering(
            TurnSteeringProjection::observed(
                TurnSteeringState::PromptDeleted,
                Some("removed prompt".to_string()),
            ),
        );
        let json = serde_json::to_string(&proj).unwrap();
        assert!(json.contains("prompt_deleted"));
        assert!(json.contains("\"count\":1"));
        assert!(json.contains("removed prompt"));
        let back: TurnProjection = serde_json::from_str(&json).unwrap();
        assert_eq!(proj, back);
    }
}

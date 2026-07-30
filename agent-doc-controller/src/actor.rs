//! Storage-independent session actor model.
//!
//! Actor lifecycle and ownership are controller-domain facts. Persistence
//! adapters may serialize these values, but must not own their vocabulary.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorState {
    Starting,
    Ready,
    Busy,
    WaitingInput,
    Closed,
    Blocked,
}

impl ActorState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Busy => "busy",
            Self::WaitingInput => "waiting_input",
            Self::Closed => "closed",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "starting" => Some(Self::Starting),
            "ready" => Some(Self::Ready),
            "busy" => Some(Self::Busy),
            "waiting_input" => Some(Self::WaitingInput),
            "closed" => Some(Self::Closed),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorLastTransition {
    pub caller: String,
    pub reason: String,
    pub timestamp: u64,
    pub prior_generation: u64,
    pub new_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRecord {
    pub document_id: String,
    pub session_id: String,
    pub generation: u64,
    pub pane_id: String,
    pub window_id: String,
    pub harness: String,
    pub state: ActorState,
    pub last_transition: ActorLastTransition,
}

/// Accepted actor-store mutation emitted by the durable persistence boundary.
///
/// Consumers publish this value into the controller's reactive actor Source;
/// they do not reload SQLite to rediscover the transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorStoreWrite {
    pub record: ActorRecord,
    pub evicted_document_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_state_wire_names_are_model_owned() {
        for (state, wire) in [
            (ActorState::Starting, "starting"),
            (ActorState::Ready, "ready"),
            (ActorState::Busy, "busy"),
            (ActorState::WaitingInput, "waiting_input"),
            (ActorState::Closed, "closed"),
            (ActorState::Blocked, "blocked"),
        ] {
            assert_eq!(state.as_str(), wire);
            assert_eq!(ActorState::parse(wire), Some(state));
        }
    }
}

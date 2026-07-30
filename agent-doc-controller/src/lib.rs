//! Project controller model and CAS vocabulary.
//!
//! The controller is the project control plane. It stores bindings and applies
//! domain decisions from supervisor, turn, and realtime document crates; it does
//! not own those policies itself.

use agent_doc_supervisor::{SupervisorBinding, SupervisorState};
use serde::{Deserialize, Serialize};

pub mod claim;
pub mod command_line;
pub mod dispatch;
pub mod editor_route_error;
pub mod fleet;
pub mod operator_clear;
pub mod orphan_drain;
pub mod pane_layout;
pub mod paths;
pub mod recycle;
pub mod status;
pub mod supervisor_replacement;
pub mod timeout;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorRecord {
    pub document_path: String,
    pub harness: String,
    pub binding: SupervisorBinding,
    pub supervisor_state: SupervisorState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorBindingStatus {
    Bound,
    NotFound,
    Conflict,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorBindingDecision {
    pub status: ActorBindingStatus,
    pub record: Option<ActorRecord>,
}

impl ActorBindingDecision {
    pub fn bound(record: ActorRecord) -> Self {
        Self {
            status: ActorBindingStatus::Bound,
            record: Some(record),
        }
    }

    pub fn not_found() -> Self {
        Self {
            status: ActorBindingStatus::NotFound,
            record: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_decision_carries_typed_record() {
        let record = ActorRecord {
            document_path: "/tmp/doc.md".to_string(),
            harness: "codex".to_string(),
            binding: SupervisorBinding {
                pane_id: "%34".to_string(),
                generation: 9,
                supervisor_instance_id: None,
            },
            supervisor_state: SupervisorState::Ready,
        };

        let decision = ActorBindingDecision::bound(record.clone());

        assert_eq!(decision.status, ActorBindingStatus::Bound);
        assert_eq!(decision.record, Some(record));
    }
}
pub mod actor;

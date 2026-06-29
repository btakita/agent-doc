//! Compatibility re-export for cycle phase transitions.
//!
//! The pure phase graph is owned by `agent-doc-turn`; orchestration owns only
//! the durable sidecar IO that records accepted transitions.

pub use agent_doc_turn::{
    CycleBookkeepingEvent, CycleEvent, CyclePhase, CyclePhaseMachine, transition_phase,
};

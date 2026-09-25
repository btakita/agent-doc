//! Backlog element descriptor.

pub mod backlog;
pub mod gate_verify;
pub mod guard_policy;
pub mod ops_proof;

pub use guard_policy::dropped_from_history_report;

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementTurnRole, ElementWritePolicy,
};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "backlog",
    aliases: &["pending"],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::GranularTrackedWork,
    write_policy: ElementWritePolicy::GranularOnly,
    scheduling_role: ElementSchedulingRole::RunnableWorkSource,
    turn_role: ElementTurnRole::Trigger,
    realtime_model: ElementRealtimeModel::TrackedItems,
    composition_role: ElementCompositionRole::Producer,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backlog_accepts_legacy_pending_alias() {
        assert!(descriptor().matches_name("backlog"));
        assert!(descriptor().matches_name("pending"));
    }
}

//! Backlog element descriptor.

pub mod backlog;
pub mod gate_verify;

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "backlog",
    aliases: &["pending"],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::GranularTrackedWork,
    write_policy: ElementWritePolicy::GranularOnly,
    scheduling_role: ElementSchedulingRole::RunnableWorkSource,
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

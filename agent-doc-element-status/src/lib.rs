//! Status element descriptor.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementTurnRole, ElementWritePolicy,
};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "status",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::DerivedProjection,
    write_policy: ElementWritePolicy::ProjectionOnly,
    scheduling_role: ElementSchedulingRole::Status,
    turn_role: ElementTurnRole::Trigger,
    realtime_model: ElementRealtimeModel::Status,
    composition_role: ElementCompositionRole::Observer,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

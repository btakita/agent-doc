//! Boundary marker element descriptor.

pub mod boundary;

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementTurnRole, ElementWritePolicy,
};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "boundary",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::InlineMarker,
    authority: ElementAuthority::DerivedProjection,
    write_policy: ElementWritePolicy::ProjectionOnly,
    scheduling_role: ElementSchedulingRole::None,
    turn_role: ElementTurnRole::Trigger,
    realtime_model: ElementRealtimeModel::Boundary,
    composition_role: ElementCompositionRole::Consumer,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

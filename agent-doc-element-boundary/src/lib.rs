//! Boundary marker element descriptor.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};

pub mod id;
pub use id::{
    BOUNDARY_ID_LEN, boundary_id_from_seed_with_summary, format_boundary_marker, new_boundary_id,
    new_boundary_id_with_summary,
};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "boundary",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::InlineMarker,
    authority: ElementAuthority::DerivedProjection,
    write_policy: ElementWritePolicy::ProjectionOnly,
    scheduling_role: ElementSchedulingRole::None,
    realtime_model: ElementRealtimeModel::Boundary,
    composition_role: ElementCompositionRole::Consumer,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

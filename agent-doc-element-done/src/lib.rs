//! Done/archive element descriptor.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "done",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::Archive,
    write_policy: ElementWritePolicy::ArchiveOnly,
    scheduling_role: ElementSchedulingRole::CompletionArchive,
    realtime_model: ElementRealtimeModel::Archive,
    composition_role: ElementCompositionRole::ArchiveTarget,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

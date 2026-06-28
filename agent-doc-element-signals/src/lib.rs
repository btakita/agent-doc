//! Signals element descriptor.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "signals",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::Signals,
    write_policy: ElementWritePolicy::SignalReconcile,
    scheduling_role: ElementSchedulingRole::Signals,
    realtime_model: ElementRealtimeModel::Signals,
    composition_role: ElementCompositionRole::Observer,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

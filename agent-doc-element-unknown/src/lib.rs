//! Fallback descriptor for elements not defined by code or plugins.
//!
//! This descriptor is intentionally conservative. It lets a caller classify an
//! unregistered `agent:*` component as a known safety shape, but it does not
//! grant semantic mutation rights. Until a plugin registers a specific
//! descriptor, unknown element content is operator-authoritative and merge-only.
//!
//! `__unknown__` is an internal sentinel name, not a document marker. Do not
//! reserve `agent:unknown`; a plugin may define that component normally later.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};

pub const UNKNOWN_FALLBACK_NAME: &str = "__unknown__";

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: UNKNOWN_FALLBACK_NAME,
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::SharedOperatorAuthoritative,
    write_policy: ElementWritePolicy::MergeOnly,
    scheduling_role: ElementSchedulingRole::None,
    realtime_model: ElementRealtimeModel::Unknown,
    composition_role: ElementCompositionRole::LocalOnly,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

pub fn descriptor_for_unknown_name(_name: &str) -> ElementDescriptor {
    DESCRIPTOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fallback_is_operator_authoritative_merge_only() {
        let descriptor = descriptor_for_unknown_name("kanban");
        assert_eq!(descriptor.name, "__unknown__");
        assert_eq!(
            descriptor.authority,
            ElementAuthority::SharedOperatorAuthoritative
        );
        assert_eq!(descriptor.write_policy, ElementWritePolicy::MergeOnly);
        assert_eq!(descriptor.realtime_model, ElementRealtimeModel::Unknown);
    }
}

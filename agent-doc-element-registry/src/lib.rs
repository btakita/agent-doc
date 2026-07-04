//! Built-in element registry.
//!
//! This crate composes the pure built-in element descriptors. Dynamic plugin
//! loading belongs in a runtime crate; plugins should register additional
//! `ElementRegistration` values against this shape.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementRegistration, ElementSchedulingRole, ElementShape, ElementSource, ElementWritePolicy,
};

pub const NOTES_DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "notes",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::SharedOperatorAuthoritative,
    write_policy: ElementWritePolicy::MergeOnly,
    scheduling_role: ElementSchedulingRole::None,
    realtime_model: ElementRealtimeModel::None,
    composition_role: ElementCompositionRole::LocalOnly,
    realtime: true,
};

pub const BUILT_IN_ELEMENTS: &[ElementDescriptor] = &[
    agent_doc_element_exchange::DESCRIPTOR,
    agent_doc_element_boundary::DESCRIPTOR,
    agent_doc_element_queue::DESCRIPTOR,
    agent_doc_element_backlog::DESCRIPTOR,
    agent_doc_element_review::DESCRIPTOR,
    agent_doc_element_icebox::DESCRIPTOR,
    agent_doc_element_done::DESCRIPTOR,
    agent_doc_element_status::DESCRIPTOR,
    agent_doc_element_signals::DESCRIPTOR,
    NOTES_DESCRIPTOR,
];

pub fn built_in_elements() -> &'static [ElementDescriptor] {
    BUILT_IN_ELEMENTS
}

pub fn find_built_in(name: &str) -> Option<ElementDescriptor> {
    BUILT_IN_ELEMENTS
        .iter()
        .copied()
        .find(|descriptor| descriptor.matches_name(name))
}

pub fn descriptor_for(name: &str) -> ElementDescriptor {
    find_built_in(name)
        .unwrap_or_else(|| agent_doc_element_unknown::descriptor_for_unknown_name(name))
}

pub fn built_in_registrations() -> Vec<ElementRegistration> {
    BUILT_IN_ELEMENTS
        .iter()
        .map(|descriptor| descriptor.as_registration())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_element::{
        ElementAuthority, ElementCompositionRole, ElementRealtimeModel, ElementSchedulingRole,
        ElementWritePolicy,
    };

    #[test]
    fn registry_finds_canonical_and_legacy_names() {
        assert_eq!(find_built_in("backlog").unwrap().name, "backlog");
        assert_eq!(find_built_in("pending").unwrap().name, "backlog");
        assert_eq!(find_built_in("signals").unwrap().name, "signals");
        assert_eq!(find_built_in("boundary").unwrap().name, "boundary");
        assert_eq!(find_built_in("notes").unwrap().name, "notes");
        assert_eq!(find_built_in("unknown"), None);
    }

    #[test]
    fn descriptor_for_unknown_uses_unknown_fallback() {
        let descriptor = descriptor_for("operator-notes");
        assert_eq!(descriptor.name, "__unknown__");
        assert_eq!(descriptor.realtime_model, ElementRealtimeModel::Unknown);
        assert_eq!(
            descriptor.authority,
            ElementAuthority::SharedOperatorAuthoritative
        );
        assert_eq!(descriptor.write_policy, ElementWritePolicy::MergeOnly);
    }

    #[test]
    fn queue_and_backlog_composition_roles_are_explicit() {
        let backlog = find_built_in("backlog").unwrap();
        let queue = find_built_in("queue").unwrap();
        assert_eq!(backlog.composition_role, ElementCompositionRole::Producer);
        assert_eq!(backlog.realtime_model, ElementRealtimeModel::TrackedItems);
        assert_eq!(queue.composition_role, ElementCompositionRole::Consumer);
        assert_eq!(queue.realtime_model, ElementRealtimeModel::Queue);
    }

    #[test]
    fn icebox_is_parked_tracked_work_not_queue_producer() {
        let icebox = find_built_in("icebox").unwrap();
        assert_eq!(icebox.realtime_model, ElementRealtimeModel::TrackedItems);
        assert_eq!(
            icebox.scheduling_role,
            ElementSchedulingRole::ParkedWorkSource
        );
        assert_eq!(icebox.composition_role, ElementCompositionRole::LocalOnly);
    }

    #[test]
    fn notes_is_operator_owned_freeform_metadata() {
        let notes = find_built_in("notes").unwrap();
        assert_eq!(
            notes.authority,
            ElementAuthority::SharedOperatorAuthoritative
        );
        assert_eq!(notes.write_policy, ElementWritePolicy::MergeOnly);
        assert_eq!(notes.scheduling_role, ElementSchedulingRole::None);
        assert_eq!(notes.realtime_model, ElementRealtimeModel::None);
        assert_eq!(notes.composition_role, ElementCompositionRole::LocalOnly);
    }

    #[test]
    fn registry_manifest_stays_pure() {
        let manifest = include_str!("../Cargo.toml");
        for forbidden in [
            "agent-doc-orchestration",
            "interprocess",
            "notify",
            "rusqlite",
            "tmux-router",
        ] {
            assert!(
                !manifest.contains(forbidden),
                "agent-doc-element-registry must stay pure; found forbidden dependency {forbidden}"
            );
        }
    }
}

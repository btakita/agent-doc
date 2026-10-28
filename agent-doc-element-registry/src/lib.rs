//! Built-in element registry.
//!
//! This crate composes the pure built-in element descriptors. Dynamic plugin
//! loading belongs in a runtime crate; plugins should register additional
//! `ElementRegistration` values against this shape.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementRegistration, ElementSchedulingRole, ElementShape, ElementSource, ElementTurnRole,
    ElementWritePolicy,
};

pub const NOTES_DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "notes",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::SharedOperatorAuthoritative,
    write_policy: ElementWritePolicy::MergeOnly,
    scheduling_role: ElementSchedulingRole::None,
    turn_role: ElementTurnRole::Informational,
    realtime_model: ElementRealtimeModel::None,
    composition_role: ElementCompositionRole::LocalOnly,
    realtime: true,
};

pub const RULES_DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "rules",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::SharedOperatorAuthoritative,
    write_policy: ElementWritePolicy::MergeOnly,
    scheduling_role: ElementSchedulingRole::None,
    turn_role: ElementTurnRole::Informational,
    realtime_model: ElementRealtimeModel::None,
    composition_role: ElementCompositionRole::LocalOnly,
    realtime: true,
};

pub const TODO_DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "todo",
    aliases: &[],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::SharedOperatorAuthoritative,
    write_policy: ElementWritePolicy::MergeOnly,
    scheduling_role: ElementSchedulingRole::None,
    turn_role: ElementTurnRole::Informational,
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
    TODO_DESCRIPTOR,
    NOTES_DESCRIPTOR,
    RULES_DESCRIPTOR,
];

/// Binary-written components that only ever appear nested inside another
/// element and carry no element semantics of their own, so they have no
/// [`ElementDescriptor`]:
///
/// - `salient-response` — the cycle-keyed, non-final `Live response (not
///   final)` node that `salient-checkpoint` / `choice-checkpoint` upsert inside
///   `agent:exchange` and that final response insertion removes.
/// - `referral` — the pointer block `transfer --referral` inserts into the
///   target component.
///
/// The document still contains their open/close markers, so every marker-level
/// gate (the dialect lint above all) must accept them. A marker the binary
/// writes but no gate knows turns an in-flight cycle into a "malformed
/// directive" refusal.
pub const NESTED_BINARY_COMPONENTS: &[&str] = &["salient-response", "referral"];

/// Whether `name` (without the `agent:` prefix) is a nested binary-written
/// component from [`NESTED_BINARY_COMPONENTS`].
pub fn is_nested_binary_component(name: &str) -> bool {
    NESTED_BINARY_COMPONENTS.contains(&name)
}

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
        ElementTurnRole, ElementWritePolicy,
    };

    #[test]
    fn registry_finds_canonical_and_legacy_names() {
        assert_eq!(find_built_in("run").unwrap().name, "run");
        assert_eq!(find_built_in("queue").unwrap().name, "run");
        assert_eq!(find_built_in("backlog").unwrap().name, "backlog");
        assert_eq!(find_built_in("pending").unwrap().name, "backlog");
        assert_eq!(find_built_in("signals").unwrap().name, "signals");
        assert_eq!(find_built_in("boundary").unwrap().name, "boundary");
        assert_eq!(find_built_in("todo").unwrap().name, "todo");
        assert_eq!(find_built_in("notes").unwrap().name, "notes");
        assert_eq!(find_built_in("rules").unwrap().name, "rules");
        assert_eq!(find_built_in("unknown"), None);
    }

    #[test]
    fn nested_binary_components_are_known_but_not_descriptors() {
        for name in NESTED_BINARY_COMPONENTS {
            assert!(is_nested_binary_component(name));
            assert_eq!(
                find_built_in(name),
                None,
                "`{name}` is a nested marker, not a top-level element"
            );
        }
        assert!(!is_nested_binary_component("operator-notes"));
        assert!(!is_nested_binary_component("exchange"));
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
        let queue = find_built_in("run").unwrap();
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
        assert_eq!(notes.turn_role, ElementTurnRole::Informational);
        assert_eq!(
            notes.turn_role,
            agent_doc_element::turn_role_for_component_name("notes")
        );
        assert_eq!(notes.realtime_model, ElementRealtimeModel::None);
        assert_eq!(notes.composition_role, ElementCompositionRole::LocalOnly);
    }

    #[test]
    fn todo_is_operator_owned_informational_checklist() {
        let todo = find_built_in("todo").unwrap();
        assert_eq!(
            todo.authority,
            ElementAuthority::SharedOperatorAuthoritative
        );
        assert_eq!(todo.write_policy, ElementWritePolicy::MergeOnly);
        assert_eq!(todo.scheduling_role, ElementSchedulingRole::None);
        assert_eq!(todo.turn_role, ElementTurnRole::Informational);
        assert_eq!(
            todo.turn_role,
            agent_doc_element::turn_role_for_component_name("todo")
        );
        assert_eq!(todo.realtime_model, ElementRealtimeModel::None);
        assert_eq!(todo.composition_role, ElementCompositionRole::LocalOnly);
    }

    #[test]
    fn rules_are_operator_owned_turn_context() {
        let rules = find_built_in("rules").unwrap();
        assert_eq!(
            rules.authority,
            ElementAuthority::SharedOperatorAuthoritative
        );
        assert_eq!(rules.write_policy, ElementWritePolicy::MergeOnly);
        assert_eq!(rules.scheduling_role, ElementSchedulingRole::None);
        assert_eq!(rules.turn_role, ElementTurnRole::Informational);
        assert_eq!(
            rules.turn_role,
            agent_doc_element::turn_role_for_component_name("rules")
        );
        assert_eq!(rules.realtime_model, ElementRealtimeModel::None);
        assert_eq!(rules.composition_role, ElementCompositionRole::LocalOnly);
    }

    #[test]
    fn unknown_components_remain_turn_triggers() {
        assert_eq!(
            descriptor_for("operator-notes").turn_role,
            ElementTurnRole::Trigger
        );
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

//! Pure element descriptors for agent-doc document components.
//!
//! Element crates describe component semantics only: marker names, aliases,
//! ownership, realtime policy, and scheduling role. They do not read or write
//! files, schedule turns, open IPC, invoke tmux, mutate snapshots, or load
//! plugins. Runtime crates consume these descriptors.
//!
//! Architecture: every supported element may own a small pure realtime model
//! for its local state and reconciliation rules. The document model composes
//! those element models and owns cross-element invariants such as backlog to
//! queue projection, active-head marker placement, and signals that observe
//! queue/turn/supervisor state.

/// Where an element descriptor comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementSource {
    BuiltIn,
    Plugin,
}

/// Marker shape used by the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementShape {
    /// Bounded region: `<!-- agent:name -->...<!-- /agent:name -->`.
    Component,
    /// Single marker inside another component, such as an exchange boundary.
    InlineMarker,
}

/// Who owns conflict authority for this element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementAuthority {
    /// Shared user/agent surface; realtime merge must preserve operator text.
    SharedOperatorAuthoritative,
    /// User-visible tracked work mutated only through granular operations.
    GranularTrackedWork,
    /// Derived projection of runtime state; source state lives elsewhere.
    DerivedProjection,
    /// Durable completed-work archive.
    Archive,
    /// Realtime signals surface with user-definable signal declarations.
    Signals,
}

/// How writes to this element are allowed to happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementWritePolicy {
    /// Merge into the operator-visible buffer; never full-replace from stale state.
    MergeOnly,
    /// Use granular add/edit/done/reorder/clear-style operations.
    GranularOnly,
    /// Rewrite from current runtime state as a projection, preserving unrelated text.
    ProjectionOnly,
    /// Append/archive only; not a runnable work source.
    ArchiveOnly,
    /// Update signal definitions/readings through signal-specific reconciliation.
    SignalReconcile,
}

/// Scheduling relationship with the realtime queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementSchedulingRole {
    None,
    ActiveQueue,
    RunnableWorkSource,
    ParkedWorkSource,
    ReviewGate,
    CompletionArchive,
    Status,
    Signals,
}

/// Local realtime model owned by an element crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementRealtimeModel {
    /// No local realtime state beyond parsed component text.
    None,
    /// Conversation/prompt state inside `agent:exchange`.
    Exchange,
    /// Tracked work list state shared by backlog/review/icebox variants.
    TrackedItems,
    /// Active queue state: heads, pins, auto-DAG ordering, and projections.
    Queue,
    /// Runtime status projection.
    Status,
    /// Completed-work archive projection.
    Archive,
    /// Realtime signal definitions and readings.
    Signals,
    /// Inline response boundary marker inside an exchange.
    Boundary,
    /// Unknown or pluginless component content.
    Unknown,
}

/// Whether this element participates in document-level composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementCompositionRole {
    /// Independent element; local reconciliation is enough.
    LocalOnly,
    /// Feeds another element, e.g. backlog feeds queue.
    Producer,
    /// Projects or consumes state from another element, e.g. queue from backlog.
    Consumer,
    /// Observes multiple models, e.g. signals.
    Observer,
    /// Archive target for completed tracked work.
    ArchiveTarget,
}

/// Built-in descriptor shape. Plugin loaders can convert this to an owned
/// descriptor later without changing element crate semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementDescriptor {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub source: ElementSource,
    pub shape: ElementShape,
    pub authority: ElementAuthority,
    pub write_policy: ElementWritePolicy,
    pub scheduling_role: ElementSchedulingRole,
    pub realtime_model: ElementRealtimeModel,
    pub composition_role: ElementCompositionRole,
    /// True when realtime processing may update this element outside turn
    /// closeout. Commits remain owned by the turn lifecycle.
    pub realtime: bool,
}

impl ElementDescriptor {
    pub fn matches_name(self, name: &str) -> bool {
        self.name == name || self.aliases.contains(&name)
    }

    pub fn canonical_name(self, name: &str) -> Option<&'static str> {
        self.matches_name(name).then_some(self.name)
    }

    pub fn as_registration(self) -> ElementRegistration {
        ElementRegistration {
            name: self.name.to_string(),
            aliases: self
                .aliases
                .iter()
                .map(|alias| (*alias).to_string())
                .collect(),
            source: self.source,
            shape: self.shape,
            authority: self.authority,
            write_policy: self.write_policy,
            scheduling_role: self.scheduling_role,
            realtime_model: self.realtime_model,
            composition_role: self.composition_role,
            realtime: self.realtime,
        }
    }
}

/// Owned descriptor shape suitable for future plugin registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementRegistration {
    pub name: String,
    pub aliases: Vec<String>,
    pub source: ElementSource,
    pub shape: ElementShape,
    pub authority: ElementAuthority,
    pub write_policy: ElementWritePolicy,
    pub scheduling_role: ElementSchedulingRole,
    pub realtime_model: ElementRealtimeModel,
    pub composition_role: ElementCompositionRole,
    pub realtime: bool,
}

impl ElementRegistration {
    pub fn matches_name(&self, name: &str) -> bool {
        self.name == name || self.aliases.iter().any(|alias| alias == name)
    }
}

/// Future plugin boundary: loaders register descriptors, while element crates
/// remain pure and effect-free.
pub trait ElementPlugin {
    fn element_descriptors(&self) -> Vec<ElementRegistration>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKLOG: ElementDescriptor = ElementDescriptor {
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

    #[test]
    fn descriptor_matches_aliases_and_canonicalizes() {
        assert!(BACKLOG.matches_name("backlog"));
        assert!(BACKLOG.matches_name("pending"));
        assert_eq!(BACKLOG.canonical_name("pending"), Some("backlog"));
        assert_eq!(BACKLOG.canonical_name("queue"), None);
    }

    #[test]
    fn built_in_descriptor_converts_to_plugin_registration_shape() {
        let registration = BACKLOG.as_registration();
        assert_eq!(registration.name, "backlog");
        assert_eq!(registration.aliases, vec!["pending"]);
        assert!(registration.matches_name("pending"));
    }

    #[test]
    fn manifest_stays_pure() {
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
                "agent-doc-element must stay pure; found forbidden dependency {forbidden}"
            );
        }
    }
}

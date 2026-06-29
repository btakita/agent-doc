//! Pure merge policy functions for agent-doc documents.
//!
//! This crate is the realtime merge boundary: it accepts document strings and
//! returns merge plans. It does not read or write files, open IPC sockets, inspect
//! editor state, mutate snapshots, or commit changes. Turn lifecycle and realtime
//! scheduling crates own those responsibilities.

pub mod ownership;

pub use agent_doc_core::cell_doc::{
    CellConflict, CellMergeOutcome, ConflictKind, ConflictPolicy, component_conflict_policy,
    merge_3way as cell_merge_3way,
};
pub use agent_doc_markdown_ast::semantic_merge::{
    AckReason, AckRequest, NodeOutcome, OutcomeKind, SemanticMerge, semantic_merge,
};

/// Merge implementation to use for a pure three-way merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeEngine {
    /// Node-keyed semantic merge over the component overlay.
    Semantic,
    /// Per-cell merge over parsed document components.
    Cell,
}

/// Inputs for a pure agent-doc three-way merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeRequest<'a> {
    /// Common ancestor text.
    pub base: &'a str,
    /// Agent-authored candidate text.
    pub agent: &'a str,
    /// Operator-visible text from the realtime document.
    pub operator: &'a str,
    pub engine: MergeEngine,
}

impl<'a> MergeRequest<'a> {
    /// Build a semantic merge request.
    pub fn semantic(base: &'a str, agent: &'a str, operator: &'a str) -> Self {
        Self {
            base,
            agent,
            operator,
            engine: MergeEngine::Semantic,
        }
    }

    /// Build a per-cell merge request.
    pub fn cell(base: &'a str, agent: &'a str, operator: &'a str) -> Self {
        Self {
            base,
            agent,
            operator,
            engine: MergeEngine::Cell,
        }
    }
}

/// Result of a pure merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergePlan {
    /// The document text produced by the selected merge engine.
    pub merged_doc: String,
    /// Ack requests to carry into the next turn lifecycle.
    pub acknowledgements: Vec<AckRequest>,
    /// Cell-level conflicts recorded by the per-cell engine.
    pub conflicts: Vec<CellConflict>,
    pub engine: MergeEngine,
    /// True when the per-cell engine declined and the caller must choose another
    /// pure merge strategy before scheduling any realtime write.
    pub fell_back: bool,
}

/// Merge three document revisions without side effects.
pub fn merge(request: MergeRequest<'_>) -> MergePlan {
    match request.engine {
        MergeEngine::Semantic => {
            let outcome = semantic_merge(request.base, request.agent, request.operator);
            MergePlan {
                merged_doc: outcome.merged_doc,
                acknowledgements: outcome.requires_ack,
                conflicts: Vec::new(),
                engine: MergeEngine::Semantic,
                fell_back: false,
            }
        }
        MergeEngine::Cell => {
            let outcome = cell_merge_3way(request.base, request.agent, request.operator);
            MergePlan {
                merged_doc: outcome.merged_text,
                acknowledgements: Vec::new(),
                conflicts: outcome.conflicts,
                engine: MergeEngine::Cell,
                fell_back: outcome.fell_back,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"<!-- agent:queue -->
- [ ] [#task] old text
<!-- /agent:queue -->
"#;

    #[test]
    fn semantic_merge_keeps_operator_edit_on_same_queue_node() {
        let agent = r#"<!-- agent:queue -->
- [ ] [#task] agent text
<!-- /agent:queue -->
"#;
        let operator = r#"<!-- agent:queue -->
- [ ] [#task] operator typed text
<!-- /agent:queue -->
"#;

        let plan = merge(MergeRequest::semantic(BASE, agent, operator));

        assert_eq!(plan.engine, MergeEngine::Semantic);
        assert!(plan.merged_doc.contains("operator typed text"));
        assert!(!plan.merged_doc.contains("agent text"));
        assert_eq!(plan.acknowledgements.len(), 1);
        assert_eq!(
            plan.acknowledgements[0].reason,
            AckReason::SameNodeOperatorOverride
        );
    }

    #[test]
    fn custom_components_default_to_operator_authority() {
        assert_eq!(
            component_conflict_policy("signals"),
            ConflictPolicy::TheirsWins
        );
        assert_eq!(
            component_conflict_policy("custom-plugin"),
            ConflictPolicy::TheirsWins
        );
    }

    #[test]
    fn cell_merge_is_exposed_as_a_pure_plan() {
        let agent = r#"<!-- agent:queue -->
- [ ] [#task] agent text
<!-- /agent:queue -->
"#;
        let operator = r#"<!-- agent:queue -->
- [ ] [#task] operator typed text
<!-- /agent:queue -->
"#;

        let plan = merge(MergeRequest::cell(BASE, agent, operator));

        assert_eq!(plan.engine, MergeEngine::Cell);
        assert!(!plan.fell_back);
        assert!(plan.merged_doc.contains("operator typed text"));
        assert!(!plan.merged_doc.contains("agent text"));
    }

    #[test]
    fn crate_manifest_excludes_realtime_and_turn_dependencies() {
        let manifest = include_str!("../Cargo.toml");
        for forbidden in [
            "agent-doc-orchestration",
            "git2",
            "interprocess",
            "notify",
            "rusqlite",
            "tmux-router",
        ] {
            assert!(
                !manifest.contains(forbidden),
                "agent-doc-merge must stay pure; found forbidden dependency {forbidden}"
            );
        }
    }
}

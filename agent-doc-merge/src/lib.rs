//! Pure merge policy functions for agent-doc documents.
//!
//! This crate is the realtime merge boundary: it accepts document strings and
//! returns merge plans. It does not read or write files, open IPC sockets, inspect
//! editor state, mutate snapshots, or commit changes. Turn lifecycle and realtime
//! scheduling crates own those responsibilities.

pub mod ownership;

pub mod crdt;
pub mod crdt_sync;
pub mod document_cell;
pub mod document_cell_merge;
pub mod exchange_node_merge;
pub mod frontmatter_crdt;

pub use document_cell::{
    CellConflict, CellMergeOutcome, ConflictKind, ConflictPolicy, component_conflict_policy,
    merge_3way as cell_merge_3way,
};
pub use frontmatter_crdt::merge_contents_crdt;

/// Re-exports of the lossless-tree durable projection surface (`#lzlosstree`), so
/// the root crate's FFI layer — which already depends on `agent-doc-merge` but not
/// on `agent-doc-markdown-lossless` directly — can expose tree-frame exchange to
/// editor plugins without a new direct dependency.
pub mod lossless_tree {
    pub use agent_doc_markdown_lossless::{
        LOSSLESS_FRAME_DIR, LosslessProjection, apply_text_edit, apply_text_edits, project,
        projection_from_bytes, projection_to_bytes, read_frame_render, restore, write_frame,
    };
}

use document_cell_merge::AckRequest;

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
            let outcome = document_cell_merge::document_cell_merge(
                request.base,
                request.agent,
                request.operator,
            );
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

/// Resolve append-only conflicts in `git merge-file --diff3` output.
///
/// When the `||||||| original` section is empty or whitespace-only, both sides
/// added at the same insertion point without modifying existing content. Resolve
/// those blocks by concatenating ours first, then theirs. True conflicts keep the
/// original conflict block.
pub fn resolve_append_conflicts(merged: &str) -> (String, bool) {
    let mut result = String::new();
    let mut has_remaining = false;
    let lines: Vec<&str> = merged.lines().collect();
    let len = lines.len();
    let mut i = 0;

    while i < len {
        if !lines[i].starts_with("<<<<<<< ") {
            result.push_str(lines[i]);
            result.push('\n');
            i += 1;
            continue;
        }

        let conflict_start = i;
        i += 1;

        let mut ours_lines: Vec<&str> = Vec::new();
        while i < len && !lines[i].starts_with("||||||| ") && !lines[i].starts_with("=======") {
            ours_lines.push(lines[i]);
            i += 1;
        }

        let mut original_lines: Vec<&str> = Vec::new();
        if i < len && lines[i].starts_with("||||||| ") {
            i += 1;
            while i < len && !lines[i].starts_with("=======") {
                original_lines.push(lines[i]);
                i += 1;
            }
        }

        if i < len && lines[i].starts_with("=======") {
            i += 1;
        }

        let mut theirs_lines: Vec<&str> = Vec::new();
        while i < len && !lines[i].starts_with(">>>>>>> ") {
            theirs_lines.push(lines[i]);
            i += 1;
        }

        if i < len && lines[i].starts_with(">>>>>>> ") {
            i += 1;
        }

        let is_append_only = original_lines.iter().all(|l| l.trim().is_empty());

        if is_append_only {
            for line in &ours_lines {
                result.push_str(line);
                result.push('\n');
            }
            for line in &theirs_lines {
                result.push_str(line);
                result.push('\n');
            }
        } else {
            has_remaining = true;
            result.push_str(lines[conflict_start]);
            result.push('\n');
            for line in &ours_lines {
                result.push_str(line);
                result.push('\n');
            }
            if !original_lines.is_empty() {
                result.push_str("||||||| original\n");
                for line in &original_lines {
                    result.push_str(line);
                    result.push('\n');
                }
            }
            result.push_str("=======\n");
            for line in &theirs_lines {
                result.push_str(line);
                result.push('\n');
            }
            result.push_str(">>>>>>> your-edits\n");
        }
    }

    if !merged.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    (result, has_remaining)
}

/// Document-model classification of a merge result (operator directives 2026-07-03:
/// "the document model should account for all 5 merge_3way cases"; "corrupted
/// document should have a special designation"; "bad markers / frontmatter could
/// be repairable"). The merge reports WHAT it produced structurally, so callers
/// react deliberately instead of silently committing a scrambled or corrupt doc.
///
/// The `Corrupted` variant carries a `repairable` hint derived from the corruption
/// class: duplicate-singleton / orphaned-marker corruption is usually fixable by a
/// structural-normalize pass, whereas a hard parse error is not. The actual repair
/// (`normalize_template_structure` for markers, frontmatter repair) lives at the
/// orchestration layer that owns those passes — this crate only classifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeClassification {
    /// A structurally-sound merged document.
    Clean { text: String, state: Vec<u8> },
    /// The merged document has bad element markers or frontmatter. `reason` is the
    /// [`agent_doc_element::element::structural_corruption_reason`] tag; `repairable`
    /// hints whether a structural-normalize pass is likely to recover it (the
    /// caller performs the repair and re-classifies).
    Corrupted {
        text: String,
        state: Vec<u8>,
        reason: String,
        repairable: bool,
    },
}

/// Whether a [`structural_corruption_reason`](agent_doc_element::element::structural_corruption_reason)
/// tag names a corruption a structural-normalize pass is likely to repair
/// (duplicate singleton blocks, orphaned/unbalanced markers) vs. a hard parse
/// failure or malformed attribute that needs operator/agent intervention.
pub fn corruption_is_repairable(reason: &str) -> bool {
    reason.starts_with("duplicate_singleton_component")
        || reason.starts_with("orphan")
        || reason.starts_with("unbalanced")
}

/// Classify a CRDT merge (`merge_contents_crdt`) as [`MergeClassification::Clean`]
/// or [`MergeClassification::Corrupted`] via the shared element structural
/// validator — the document-model outcome callers consult before adopting merged
/// text. A corrupt result must never be committed as-is; the caller repairs
/// (when `repairable`) or fails closed.
pub fn merge_contents_crdt_classified(
    base_state: Option<&[u8]>,
    ours: &str,
    theirs: &str,
) -> anyhow::Result<MergeClassification> {
    let (text, state) = merge_contents_crdt(base_state, ours, theirs)?;
    match agent_doc_element::element::structural_corruption_reason(&text) {
        Some(reason) => {
            let repairable = corruption_is_repairable(&reason);
            Ok(MergeClassification::Corrupted {
                text,
                state,
                reason,
                repairable,
            })
        }
        None => Ok(MergeClassification::Clean { text, state }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_cell_merge::AckReason;

    #[test]
    fn classified_sound_merge_is_clean() {
        let base = "<!-- agent:exchange -->\nQ.\n<!-- /agent:exchange -->\n";
        let bs = crate::crdt::CrdtDoc::from_text(base).encode_state();
        let ours = "<!-- agent:exchange -->\nQ.\n\n### Re: Q\n\nA.\n<!-- /agent:exchange -->\n";
        let out = merge_contents_crdt_classified(Some(&bs), ours, base).unwrap();
        assert!(matches!(out, MergeClassification::Clean { .. }), "{out:?}");
    }

    #[test]
    fn classified_designates_repairable_marker_corruption() {
        // A document that already carries a DUPLICATE singleton exchange block
        // (ours == theirs so the merge is identity) is designated Corrupted, and the
        // marker corruption class is flagged repairable (a normalize pass recovers).
        let corrupt = "<!-- agent:exchange -->\nA\n<!-- /agent:exchange -->\n\
<!-- agent:exchange -->\nB\n<!-- /agent:exchange -->\n";
        match merge_contents_crdt_classified(None, corrupt, corrupt).unwrap() {
            MergeClassification::Corrupted {
                reason, repairable, ..
            } => {
                assert!(
                    repairable,
                    "marker corruption should be repairable: {reason}"
                );
            }
            other => panic!("expected Corrupted, got {other:?}"),
        }
    }

    const BASE: &str = r#"<!-- agent:queue -->
- [ ] [#task] old text
<!-- /agent:queue -->
"#;

    #[test]
    fn document_cell_merge_keeps_operator_edit_on_same_queue_node() {
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
    fn resolve_append_only_conflict() {
        let merged = concat!(
            "Before conflict\n",
            "<<<<<<< agent-response\n",
            "Agent added this line.\n",
            "||||||| original\n",
            "=======\n",
            "User added this line.\n",
            ">>>>>>> your-edits\n",
            "After conflict\n"
        );
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(!has_remaining);
        assert!(resolved.contains("Agent added this line."));
        assert!(resolved.contains("User added this line."));
        assert!(!resolved.contains("<<<<<<<"));
        assert!(!resolved.contains(">>>>>>>"));
        let agent_pos = resolved.find("Agent added this line.").unwrap();
        let user_pos = resolved.find("User added this line.").unwrap();
        assert!(agent_pos < user_pos);
    }

    #[test]
    fn preserve_true_conflict() {
        let merged = concat!(
            "<<<<<<< agent-response\n",
            "Agent changed this.\n",
            "||||||| original\n",
            "Original line that both sides modified.\n",
            "=======\n",
            "User changed this differently.\n",
            ">>>>>>> your-edits\n"
        );
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(has_remaining);
        assert!(resolved.contains("<<<<<<<"));
        assert!(resolved.contains(">>>>>>>"));
        assert!(resolved.contains("Original line that both sides modified."));
    }

    #[test]
    fn mixed_append_and_true_conflicts() {
        let merged = concat!(
            "Clean line.\n",
            "<<<<<<< agent-response\n",
            "Agent appended here.\n",
            "||||||| original\n",
            "=======\n",
            "User appended here.\n",
            ">>>>>>> your-edits\n",
            "Middle line.\n",
            "<<<<<<< agent-response\n",
            "Agent rewrote this.\n",
            "||||||| original\n",
            "Was originally this.\n",
            "=======\n",
            "User rewrote this differently.\n",
            ">>>>>>> your-edits\n",
            "End line.\n"
        );
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(has_remaining);
        assert!(resolved.contains("Agent appended here."));
        assert!(resolved.contains("User appended here."));
        assert!(resolved.contains("<<<<<<<"));
        assert!(resolved.contains("Was originally this."));
    }

    #[test]
    fn no_conflicts_passthrough() {
        let merged = "Line one.\nLine two.\nLine three.\n";
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(!has_remaining);
        assert_eq!(resolved, merged);
    }

    #[test]
    fn multiline_append_conflict() {
        let merged = concat!(
            "<<<<<<< agent-response\n",
            "Agent line 1.\n",
            "Agent line 2.\n",
            "Agent line 3.\n",
            "||||||| original\n",
            "=======\n",
            "User line 1.\n",
            "User line 2.\n",
            ">>>>>>> your-edits\n"
        );
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(!has_remaining);
        assert!(resolved.contains("Agent line 1.\nAgent line 2.\nAgent line 3.\n"));
        assert!(resolved.contains("User line 1.\nUser line 2.\n"));
        assert!(resolved.find("Agent line 1.").unwrap() < resolved.find("User line 1.").unwrap());
    }

    #[test]
    fn cell_merge_is_exposed_as_a_pure_plan() {
        let _guard = crate::document_cell::CELL_MERGE_ENV_LOCK.lock().unwrap();
        let prior_conflict_markers =
            std::env::var(crate::document_cell::CELL_MERGE_CONFLICT_MARKERS_ENV).ok();
        struct RestoreConflictMarkers(Option<String>);
        impl Drop for RestoreConflictMarkers {
            fn drop(&mut self) {
                unsafe {
                    match self.0.take() {
                        Some(value) => std::env::set_var(
                            crate::document_cell::CELL_MERGE_CONFLICT_MARKERS_ENV,
                            value,
                        ),
                        None => std::env::remove_var(
                            crate::document_cell::CELL_MERGE_CONFLICT_MARKERS_ENV,
                        ),
                    }
                }
            }
        }
        let _restore_conflict_markers = RestoreConflictMarkers(prior_conflict_markers);
        unsafe {
            std::env::set_var(crate::document_cell::CELL_MERGE_CONFLICT_MARKERS_ENV, "0");
        }

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
            "agent-doc-core",
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

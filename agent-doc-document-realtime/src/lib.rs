//! Realtime document authority state machine.
//!
//! This crate owns document-specific realtime state: editor/disk source
//! authority, merge/apply progress, and verified handoff state. It does not
//! commit, dispatch turns, spawn processes, or run git.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub mod baseline_comparison;
pub mod broadcast;
pub mod convergence_gate;
pub mod crdt_authority;
pub mod crdt_merge_base;
pub mod crdt_relay;
pub mod editor_identity;
pub mod ipc_corruption;
pub mod read_authority;
pub mod replica_sync;
pub mod session_ops;
pub mod watch_authority;
pub mod write_authority;
pub mod write_policy;

pub use read_authority::{
    BufferState, DocAuthority, Reconciliation, buffer_supersedes, current_doc,
    reconcile_current_doc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentKey {
    path: PathBuf,
}

impl DocumentKey {
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.path.display()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentDocument {
    key: DocumentKey,
    reconciliation: Reconciliation,
}

impl CurrentDocument {
    pub fn new(path: impl Into<PathBuf>, reconciliation: Reconciliation) -> Self {
        Self {
            key: DocumentKey::from_path(path),
            reconciliation,
        }
    }

    pub fn key(&self) -> &DocumentKey {
        &self.key
    }

    pub fn content(&self) -> &str {
        self.reconciliation.authoritative_content()
    }

    pub fn reconciliation(&self) -> &Reconciliation {
        &self.reconciliation
    }

    pub fn authority(&self) -> DocAuthority {
        self.reconciliation.authority
    }

    pub fn into_content(self) -> String {
        self.reconciliation.content
    }

    /// Project the authoritative document content into the existing lazily-backed
    /// semantic cell tree.
    pub fn document_cell_tree(
        &self,
        ctx: &lazily::Context,
    ) -> agent_doc_merge::document_cell::DocumentCellTree {
        agent_doc_merge::document_cell::DocumentCellTree::from_document(ctx, self.content())
    }

    /// Project the authoritative document content into keyed component cells.
    pub fn document_cell_components(
        &self,
    ) -> Vec<agent_doc_merge::document_cell::ComponentOccurrence> {
        agent_doc_merge::document_cell::project_document(self.content())
    }

    /// Diff a candidate visible-document projection against this current
    /// document's keyed semantic cells.
    pub fn document_cell_diff_to(
        &self,
        projected_content: &str,
    ) -> Vec<agent_doc_merge::document_cell::ComponentDiff> {
        agent_doc_merge::document_cell::diff_document(self.content(), projected_content)
    }

    /// Apply one focused semantic-cell mutation to this document's visible
    /// projection.
    pub fn apply_document_cell_mutation(
        &self,
        mutation: &agent_doc_merge::document_cell::DocumentCellMutation,
    ) -> Result<String, agent_doc_merge::document_cell::DocumentCellMutationError> {
        agent_doc_merge::document_cell::apply_document_cell_mutation(self.content(), mutation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentRealtimeState {
    DiskAuthoritative,
    EditorDirty,
    EditorQuiescent,
    DiskDriftObserved,
    AgentDeltaReady,
    MergePlanned,
    ApplyInFlight,
    AppliedVerified,
    ConflictBlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentRealtimeEvent {
    EditorAttached,
    EditorSettled,
    OperatorEdited,
    EditorDetachedClean,
    PluginlessDiskSave,
    OperatorSourcesReconciled,
    OperatorSourcesConflict,
    AgentDeltaCaptured,
    MergeSucceeded,
    MergeConflicted,
    DeliveryStarted,
    DeliveryVerified,
    DeliveryStale,
    DeliveryUnproven,
    HandoffCompleteToEditor,
    HandoffCompleteToDisk,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiveEditorDeliveryCandidate {
    pub editor_id: Option<String>,
    pub timestamp_ms: u128,
    pub is_live: bool,
    pub has_operator_text_authority: bool,
}

/// Select the live editor that should receive an agent-doc delivery.
///
/// Realtime document delivery is editor-targeted when a live editor sidecar
/// exists. Prefer an editor that proved operator-text authority, then the
/// newest live sidecar. The caller owns sidecar IO and liveness probing; this
/// crate only owns the deterministic selection rule.
pub fn select_live_editor_delivery_target(
    candidates: impl IntoIterator<Item = LiveEditorDeliveryCandidate>,
) -> Option<String> {
    let mut selected: Option<(bool, u128, String)> = None;

    for candidate in candidates {
        if !candidate.is_live {
            continue;
        }
        let Some(editor_id) = candidate
            .editor_id
            .map(|editor_id| editor_id.trim().to_string())
            .filter(|editor_id| !editor_id.is_empty())
        else {
            continue;
        };

        let key = (
            candidate.has_operator_text_authority,
            candidate.timestamp_ms,
        );
        if selected
            .as_ref()
            .is_none_or(|(selected_authority, selected_timestamp, _)| {
                key > (*selected_authority, *selected_timestamp)
            })
        {
            selected = Some((
                candidate.has_operator_text_authority,
                candidate.timestamp_ms,
                editor_id,
            ));
        }
    }

    selected.map(|(_, _, editor_id)| editor_id)
}

pub fn transition_document_realtime(
    current: &DocumentRealtimeState,
    event: &DocumentRealtimeEvent,
) -> Option<DocumentRealtimeState> {
    use DocumentRealtimeEvent::*;
    use DocumentRealtimeState::*;

    match (*current, *event) {
        (DiskAuthoritative, EditorAttached) => Some(EditorDirty),
        (EditorDirty, EditorSettled) => Some(EditorQuiescent),
        (EditorQuiescent, OperatorEdited) => Some(EditorDirty),
        (EditorDirty | EditorQuiescent, EditorDetachedClean) => Some(DiskAuthoritative),
        (DiskAuthoritative, PluginlessDiskSave) => Some(DiskAuthoritative),
        (EditorDirty | EditorQuiescent | ApplyInFlight, PluginlessDiskSave) => {
            Some(DiskDriftObserved)
        }
        (DiskDriftObserved, OperatorSourcesReconciled) => Some(EditorDirty),
        (DiskDriftObserved, OperatorSourcesConflict) => Some(ConflictBlocked),
        (DiskAuthoritative | EditorQuiescent, AgentDeltaCaptured) => Some(AgentDeltaReady),
        (EditorDirty | DiskDriftObserved, AgentDeltaCaptured) => Some(ConflictBlocked),
        (AgentDeltaReady, MergeSucceeded) => Some(MergePlanned),
        (AgentDeltaReady, MergeConflicted) => Some(ConflictBlocked),
        (MergePlanned, DeliveryStarted) => Some(ApplyInFlight),
        (ApplyInFlight, DeliveryVerified) => Some(AppliedVerified),
        (ApplyInFlight, DeliveryStale) => Some(EditorDirty),
        (ApplyInFlight, DeliveryUnproven) => Some(ConflictBlocked),
        (AppliedVerified, HandoffCompleteToEditor) => Some(EditorQuiescent),
        (AppliedVerified, HandoffCompleteToDisk) => Some(DiskAuthoritative),
        _ => None,
    }
}

pub struct DocumentRealtimeMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<DocumentRealtimeState, DocumentRealtimeEvent>,
}

impl DocumentRealtimeMachine {
    pub fn new(initial: DocumentRealtimeState) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_document_realtime);
        Self { ctx, machine }
    }

    pub fn send(&self, event: DocumentRealtimeEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> DocumentRealtimeState {
        self.machine.state(&self.ctx)
    }

    pub fn transition(
        initial: DocumentRealtimeState,
        event: DocumentRealtimeEvent,
    ) -> Option<DocumentRealtimeState> {
        let machine = Self::new(initial);
        machine.send(event).then(|| machine.state())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_editor_apply_requires_merge_and_verified_delivery() {
        let machine = DocumentRealtimeMachine::new(DocumentRealtimeState::DiskAuthoritative);
        for event in [
            DocumentRealtimeEvent::EditorAttached,
            DocumentRealtimeEvent::EditorSettled,
            DocumentRealtimeEvent::AgentDeltaCaptured,
            DocumentRealtimeEvent::MergeSucceeded,
            DocumentRealtimeEvent::DeliveryStarted,
            DocumentRealtimeEvent::DeliveryVerified,
            DocumentRealtimeEvent::HandoffCompleteToEditor,
        ] {
            assert!(machine.send(event), "{event:?}");
        }
        assert_eq!(machine.state(), DocumentRealtimeState::EditorQuiescent);
    }

    #[test]
    fn dirty_editor_blocks_agent_delta_until_settled() {
        assert_eq!(
            DocumentRealtimeMachine::transition(
                DocumentRealtimeState::EditorDirty,
                DocumentRealtimeEvent::AgentDeltaCaptured
            ),
            Some(DocumentRealtimeState::ConflictBlocked)
        );
    }

    #[test]
    fn merge_cannot_skip_delivery_verification() {
        assert_eq!(
            DocumentRealtimeMachine::transition(
                DocumentRealtimeState::MergePlanned,
                DocumentRealtimeEvent::DeliveryVerified
            ),
            None
        );
    }

    #[test]
    fn delivery_target_prefers_live_operator_authority() {
        let target = select_live_editor_delivery_target([
            LiveEditorDeliveryCandidate {
                editor_id: Some("jetbrains-newer-without-authority".to_string()),
                timestamp_ms: 30,
                is_live: true,
                has_operator_text_authority: false,
            },
            LiveEditorDeliveryCandidate {
                editor_id: Some("jetbrains-authoritative".to_string()),
                timestamp_ms: 20,
                is_live: true,
                has_operator_text_authority: true,
            },
        ]);

        assert_eq!(target.as_deref(), Some("jetbrains-authoritative"));
    }

    #[test]
    fn delivery_target_ignores_dead_and_empty_editor_ids() {
        let target = select_live_editor_delivery_target([
            LiveEditorDeliveryCandidate {
                editor_id: Some("jetbrains-dead".to_string()),
                timestamp_ms: 100,
                is_live: false,
                has_operator_text_authority: true,
            },
            LiveEditorDeliveryCandidate {
                editor_id: Some("  ".to_string()),
                timestamp_ms: 101,
                is_live: true,
                has_operator_text_authority: true,
            },
            LiveEditorDeliveryCandidate {
                editor_id: Some("vscode-live".to_string()),
                timestamp_ms: 90,
                is_live: true,
                has_operator_text_authority: false,
            },
        ]);

        assert_eq!(target.as_deref(), Some("vscode-live"));
    }

    #[test]
    fn current_document_projects_to_document_cells() {
        let doc = "\
<!-- agent:queue -->
- do first [#alpha]
<!-- /agent:queue -->
";
        let current = CurrentDocument::new("task.md", reconcile_current_doc(doc, None));

        let components = current.document_cell_components();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].component, "queue");

        let ctx = lazily::Context::new();
        let tree = current.document_cell_tree(&ctx);
        let ids = tree.item_ids(&ctx, "queue", 0);
        assert_eq!(ids, vec!["queue:0:id:alpha"]);
        assert_eq!(
            tree.item_value(&ctx, "queue", 0, "queue:0:id:alpha")
                .as_deref(),
            Some("- do first [#alpha]\n")
        );

        let updated = current
            .apply_document_cell_mutation(
                &agent_doc_merge::document_cell::DocumentCellMutation::Set {
                    component: "queue".into(),
                    occurrence: 0,
                    identity: "queue:0:id:alpha".into(),
                    value: "- do second [#alpha]\n".into(),
                },
            )
            .unwrap();
        assert!(updated.contains("- do second [#alpha]\n"));
        assert!(!updated.contains("- do first [#alpha]\n"));
    }
}

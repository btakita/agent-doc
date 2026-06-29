//! Realtime document authority state machine.
//!
//! This crate owns document-specific realtime state: editor/disk source
//! authority, merge/apply progress, and verified handoff state. It does not
//! commit, dispatch turns, spawn processes, or run git.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

pub mod convergence_gate;

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
}

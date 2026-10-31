//! Project-aware queue synchronization scan shared by preflight and terminal gates.

use std::path::{Path, PathBuf};

use agent_doc_queue::queue_sync_payload::{
    QueueSyncDocumentScope, QueueSyncOperation, QueueSyncPayload, QueueSyncPlacementDebt,
    QueueSyncScanError, normalize_claims_for_queue_sync, project_document_queue_sync,
};
use anyhow::Result;

/// Inspect the settled current-document cut plus every sibling session
/// document in the same project. Failures are retained as explicit reconcile
/// debt rather than being mistaken for a drained queue.
///
/// Preflight calls this after its deterministic repair pass. Terminal gates can
/// call the same lower-level API with their authoritative current-document cut;
/// it intentionally lives below preflight-command/session-check to avoid a
/// dependency cycle.
pub fn build_project_queue_sync_payload(
    file: &Path,
    current_content: &str,
    previous_content: Option<&str>,
) -> Result<QueueSyncPayload> {
    let current_key = canonical_key(file);
    let mut documents = vec![file.to_path_buf()];
    if let Some(root) = agent_doc_fs::find_project_root(file) {
        documents.extend(agent_doc_fs::project_session_documents(&root));
    }
    documents.sort_by_key(|path| canonical_key(path));
    documents.dedup_by(|left, right| canonical_key(left) == canonical_key(right));
    // Current first keeps its actions ahead of related-document cleanup while
    // lifecycle partitions remain stable inside the aggregate payload.
    documents.sort_by_key(|path| canonical_key(path) != current_key);

    let mut payload = QueueSyncPayload::default();
    let now_secs = crate::queue_claim::claim_clock_now_secs();
    for document in documents {
        let key = canonical_key(&document);
        let is_current = key == current_key;
        let scope = if is_current {
            QueueSyncDocumentScope::Current
        } else {
            QueueSyncDocumentScope::Related
        };
        let content = if is_current {
            current_content.to_string()
        } else {
            match std::fs::read_to_string(&document) {
                Ok(content) => content,
                Err(error) => {
                    payload.scan_errors.push(scan_error(&document, error));
                    continue;
                }
            }
        };
        let previous = if is_current {
            previous_content.map(str::to_string)
        } else {
            match agent_doc_snapshot_io::load_document_baseline(&document) {
                Ok(previous) => previous,
                Err(error) => {
                    payload.scan_errors.push(scan_error(&document, error));
                    None
                }
            }
        };
        let mut claims = match crate::queue_claim::load_ledger_following(&document, &content) {
            Ok(ledger) => ledger.claims,
            Err(error) => {
                payload.scan_errors.push(scan_error(&document, error));
                Vec::new()
            }
        };
        normalize_claims_for_queue_sync(&mut claims, now_secs);
        match project_document_queue_sync(
            key.display().to_string(),
            scope,
            &content,
            previous.as_deref(),
            &claims,
        ) {
            Ok(document_payload) => {
                payload.extend(document_payload);
                match collect_placement_debt(&key, scope, &content) {
                    Ok(debt) => payload.placement_reconcile.extend(debt),
                    Err(error) => payload.scan_errors.push(scan_error(&document, error)),
                }
            }
            Err(error) => payload.scan_errors.push(scan_error(&document, error)),
        }
    }
    Ok(payload)
}

fn collect_placement_debt(
    document: &Path,
    scope: QueueSyncDocumentScope,
    content: &str,
) -> Result<Vec<QueueSyncPlacementDebt>> {
    let components = agent_doc_element::element::parse(content)?;
    Ok(
        agent_doc_queue::backlog_sync::collect_backlog_queue_placement_debt(&components, content)?
            .into_iter()
            .map(|debt| QueueSyncPlacementDebt {
                document_path: document.display().to_string(),
                document_scope: scope,
                item_id: debt.item_id,
                item_text: debt.item_text,
                requested_queue_id: debt.requested_queue_id,
                eligible_queue_ids: debt.eligible_queue_ids,
                operation: QueueSyncOperation::Reconcile,
            })
            .collect(),
    )
}

fn canonical_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn scan_error(document: &Path, error: impl std::fmt::Display) -> QueueSyncScanError {
    QueueSyncScanError {
        document_path: canonical_key(document).display().to_string(),
        detail: error.to_string(),
        operation: QueueSyncOperation::Reconcile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_incomplete_items_in_sibling_session_documents() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".agent-doc")).unwrap();
        let current = root.path().join("current.md");
        let sibling = root.path().join("sibling.md");
        let current_content = concat!(
            "---\nagent_doc_session: current-session\n---\n",
            "<!-- agent:run id=current -->\n- current work\n<!-- /agent:run -->\n",
        );
        let sibling_content = concat!(
            "---\nagent_doc_session: sibling-session\n---\n",
            "<!-- agent:run id=inactive subagents -->\n- sibling work\n<!-- /agent:run -->\n",
        );
        std::fs::write(&current, current_content).unwrap();
        std::fs::write(&sibling, sibling_content).unwrap();

        let payload = build_project_queue_sync_payload(&current, current_content, None).unwrap();

        assert!(payload.scan_errors.is_empty(), "{:?}", payload.scan_errors);
        assert_eq!(payload.pending.len(), 2);
        let sibling_item = payload
            .pending
            .iter()
            .find(|item| item.item == "sibling work")
            .unwrap();
        assert_eq!(sibling_item.document_scope, QueueSyncDocumentScope::Related);
        assert_eq!(
            sibling_item.execution_mode,
            agent_doc_queue::queue_sync_payload::QueueExecutionMode::SubagentDispatch
        );
        assert_eq!(sibling_item.operation, QueueSyncOperation::Dispatch);
    }

    #[test]
    fn retains_ambiguous_backlog_queue_placement_as_reconcile_debt() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".agent-doc")).unwrap();
        let current = root.path().join("current.md");
        let current_content = concat!(
            "---\nagent_doc_session: current-session\n---\n",
            "<!-- agent:backlog queue -->\n- [ ] [#choose] choose a queue\n<!-- /agent:backlog -->\n",
            "<!-- agent:run id=one -->\n<!-- /agent:run -->\n",
            "<!-- agent:run id=two -->\n<!-- /agent:run -->\n",
        );
        std::fs::write(&current, current_content).unwrap();

        let payload = build_project_queue_sync_payload(&current, current_content, None).unwrap();

        assert!(payload.scan_errors.is_empty(), "{:?}", payload.scan_errors);
        assert_eq!(payload.placement_reconcile.len(), 1);
        let debt = &payload.placement_reconcile[0];
        assert_eq!(debt.item_id, "choose");
        assert_eq!(debt.item_text, "choose a queue");
        assert_eq!(debt.requested_queue_id, None);
        assert_eq!(debt.eligible_queue_ids, ["one", "two"]);
        assert_eq!(debt.operation, QueueSyncOperation::Reconcile);
        assert_eq!(debt.document_scope, QueueSyncDocumentScope::Current);
    }
}

//! Queue-consumption proof ledger and state-event recording.
//!
//! Pure queue-consumption planning lives in `agent-doc-queue`. This module owns
//! the queue I/O side of closeout proof recording while callers inject the
//! project-controller state-event sink.

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_queue::queue_consume::{QueueConsumptionPlan, next_queue_head_selection};
use agent_doc_queue::queue_response::queue_prompt_done_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueConsumptionProofStage {
    BeforeMutation,
    AfterMutation,
}

pub trait QueueConsumptionProofEffects {
    fn append_state_event(
        &self,
        project_root: &Path,
        event: &agent_doc_state_backbone::StateEvent,
    ) -> Result<bool>;

    fn log_op(&self, file: &Path, message: &str);

    fn now_millis(&self) -> u64;
}

pub fn record_queue_consumption_proofs<E: QueueConsumptionProofEffects + ?Sized>(
    effects: &E,
    file: &Path,
    plan: &QueueConsumptionPlan,
    stage: QueueConsumptionProofStage,
) -> Result<()> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("queue consume: failed to canonicalize {}", file.display()))?;
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        eprintln!(
            "[queue] warning: proof ledger unavailable for {}: project root not found",
            file.display()
        );
        return Ok(());
    };
    let document_hash = queue_state_document_hash(&canonical);
    for (index, consumed_text) in plan.consumed_texts.iter().enumerate() {
        let content_hash = agent_doc_hash::content_hash(consumed_text);
        let node_id = plan
            .node_ops
            .get(index)
            .map(|op| op.node_id.as_str())
            .unwrap_or("<missing-node>");
        let operation_id = format!("queue_head:{node_id}:{index}");
        let (outcome, proof_kind, proof) = match stage {
            QueueConsumptionProofStage::BeforeMutation => (
                agent_doc_workflow_io::proof_ledger::ProofOutcome::Recorded,
                agent_doc_workflow_io::proof_ledger::ProofEvidenceKind::QueueHeadIdentity,
                format!(
                    "phase=before_mutation node_id={} index={} consumed_count={} text_hash={} text={:?}",
                    node_id,
                    index,
                    plan.consumed_texts.len(),
                    content_hash,
                    consumed_text
                ),
            ),
            QueueConsumptionProofStage::AfterMutation => (
                agent_doc_workflow_io::proof_ledger::ProofOutcome::Consumed,
                agent_doc_workflow_io::proof_ledger::ProofEvidenceKind::WriteResult,
                format!(
                    "phase=after_mutation node_id={} index={} remaining={} drained={} auto={} save_snapshot={}",
                    node_id, index, plan.remaining, plan.drained, plan.auto, plan.save_snapshot
                ),
            ),
        };
        let record = agent_doc_workflow_io::proof_ledger::OperationProofRecord::new(
            agent_doc_workflow_io::proof_ledger::OperationProofInput {
                operation_id,
                operation_kind: agent_doc_workflow_io::proof_ledger::ProofOperationKind::QueueHead,
                outcome,
                subject_id: Some(node_id.to_string()),
                content_hash,
                proof_kind,
                proof,
                recorded_at_ms: effects.now_millis(),
            },
        )?;
        let path = agent_doc_workflow_io::proof_ledger::append_operation_proof(
            &project_root,
            &canonical,
            &record,
        )?;
        effects.log_op(
            file,
            &format!(
                "queue_consume_proof_recorded file={} stage={:?} operation_id={} ledger={}",
                file.display(),
                stage,
                record.operation_id,
                path.display()
            ),
        );
        record_queue_consumption_state_event(
            effects,
            QueueConsumptionStateEvent {
                file,
                project_root: &project_root,
                document_hash: &document_hash,
                node_id,
                index,
                consumed_text,
                content_hash: &record.content_hash,
                stage,
            },
        )?;
    }
    if stage == QueueConsumptionProofStage::AfterMutation && !plan.drained {
        record_next_queue_head_selected_state(
            effects,
            file,
            &project_root,
            &document_hash,
            &plan.new_document,
        )?;
    }
    Ok(())
}

pub fn queue_state_document_hash(file: &Path) -> String {
    agent_doc_hash::document_id_for_path(file)
}

pub fn record_authoritative_queue_completion_state<E: QueueConsumptionProofEffects + ?Sized>(
    effects: &E,
    file: &Path,
    content: &str,
) -> Result<usize> {
    let canonical = file.canonicalize().with_context(|| {
        format!(
            "queue completion projection: failed to canonicalize {}",
            file.display()
        )
    })?;
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical) else {
        eprintln!(
            "[queue] warning: completion state unavailable for {}: project root not found",
            file.display()
        );
        return Ok(0);
    };
    let document_hash = queue_state_document_hash(&canonical);
    let completed = agent_doc_queue::queue_projection::completed_queue_head_projections(content)?;
    for head in &completed {
        let content_hash = agent_doc_hash::content_hash(&head.text);
        let event = agent_doc_state_backbone::StateEvent::new(
            format!(
                "queue-head-completed:{document_hash}:{}:{}:{content_hash}",
                head.node_key, head.index
            ),
            agent_doc_state_backbone::StateFact::QueueHeadCompleted {
                document_hash: document_hash.clone(),
                node_key: head.node_key.clone(),
                backlog_id: head.backlog_id.clone(),
                hosting_epoch: None,
            },
        );
        let inserted = effects.append_state_event(&project_root, &event)?;
        effects.log_op(
            file,
            &format!(
                "queue_authoritative_completed_state_event_recorded file={} event_id={} inserted={} document_hash={} node_id={}",
                file.display(),
                event.event_id,
                inserted,
                document_hash,
                head.node_key
            ),
        );
    }
    Ok(completed.len())
}

struct QueueConsumptionStateEvent<'a> {
    file: &'a Path,
    project_root: &'a Path,
    document_hash: &'a str,
    node_id: &'a str,
    index: usize,
    consumed_text: &'a str,
    content_hash: &'a str,
    stage: QueueConsumptionProofStage,
}

fn record_queue_consumption_state_event<E: QueueConsumptionProofEffects + ?Sized>(
    effects: &E,
    args: QueueConsumptionStateEvent<'_>,
) -> Result<()> {
    let QueueConsumptionStateEvent {
        file,
        project_root,
        document_hash,
        node_id,
        index,
        consumed_text,
        content_hash,
        stage,
    } = args;
    let backlog_id = queue_prompt_done_id(consumed_text);
    let (event_id, fact) = match stage {
        QueueConsumptionProofStage::BeforeMutation => (
            format!("queue-head-selected:{document_hash}:{node_id}:{index}:{content_hash}"),
            agent_doc_state_backbone::StateFact::QueueHeadSelected {
                document_hash: document_hash.to_string(),
                node_key: node_id.to_string(),
                backlog_id,
                prompt_text: Some(consumed_text.to_string()),
                drainable: true,
                hosting_epoch: None,
            },
        ),
        QueueConsumptionProofStage::AfterMutation => (
            format!("queue-head-completed:{document_hash}:{node_id}:{index}:{content_hash}"),
            agent_doc_state_backbone::StateFact::QueueHeadCompleted {
                document_hash: document_hash.to_string(),
                node_key: node_id.to_string(),
                backlog_id,
                hosting_epoch: None,
            },
        ),
    };
    let event = agent_doc_state_backbone::StateEvent::new(event_id, fact);
    let inserted = effects.append_state_event(project_root, &event)?;
    effects.log_op(
        file,
        &format!(
            "queue_consume_state_event_recorded file={} stage={:?} event_id={} inserted={} document_hash={} node_id={}",
            file.display(),
            stage,
            event.event_id,
            inserted,
            document_hash,
            node_id
        ),
    );
    Ok(())
}

fn record_next_queue_head_selected_state<E: QueueConsumptionProofEffects + ?Sized>(
    effects: &E,
    file: &Path,
    project_root: &Path,
    document_hash: &str,
    content: &str,
) -> Result<()> {
    let Some(selection) = next_queue_head_selection(content)? else {
        return Ok(());
    };
    let node_key = selection.node_key;
    let head_text = selection.head_text;
    let stop_fence_at_head = selection.stop_fence_at_head;
    let content_hash = agent_doc_hash::content_hash(&head_text);
    let drainable = !stop_fence_at_head
        && agent_doc_queue::queue_continuation::live_drainable_continuation_head(
            content,
            agent_doc_queue::queue_continuation::DrainScope::Supervisor,
        )
        .is_some();
    let selected_event = agent_doc_state_backbone::StateEvent::new(
        format!("queue-head-selected:{document_hash}:{node_key}:0:{content_hash}"),
        agent_doc_state_backbone::StateFact::QueueHeadSelected {
            document_hash: document_hash.to_string(),
            node_key: node_key.clone(),
            backlog_id: queue_prompt_done_id(&head_text),
            prompt_text: Some(head_text.clone()),
            drainable,
            hosting_epoch: None,
        },
    );
    let inserted = effects.append_state_event(project_root, &selected_event)?;
    effects.log_op(
        file,
        &format!(
            "queue_next_selected_state_event_recorded file={} event_id={} inserted={} document_hash={} node_id={} drainable={}",
            file.display(),
            selected_event.event_id,
            inserted,
            document_hash,
            node_key,
            drainable
        ),
    );
    if stop_fence_at_head {
        let reason = "stop_fence";
        let reason_hash = agent_doc_hash::content_hash(reason);
        let deferred_event = agent_doc_state_backbone::StateEvent::new(
            format!(
                "queue-head-deferred:{document_hash}:{node_key}:0:{reason_hash}:{content_hash}"
            ),
            agent_doc_state_backbone::StateFact::QueueHeadDeferred {
                document_hash: document_hash.to_string(),
                node_key: node_key.clone(),
                reason: reason.to_string(),
                hosting_epoch: None,
            },
        );
        let deferred_inserted = effects.append_state_event(project_root, &deferred_event)?;
        effects.log_op(
            file,
            &format!(
                "queue_next_deferred_state_event_recorded file={} event_id={} inserted={} document_hash={} node_id={} reason={}",
                file.display(),
                deferred_event.event_id,
                deferred_inserted,
                document_hash,
                node_key,
                reason
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::queue_consume::{IpcNodeOp, QueueConsumptionPlan};
    use std::cell::RefCell;

    struct TestEffects {
        events: RefCell<Vec<agent_doc_state_backbone::StateEvent>>,
        logs: RefCell<Vec<String>>,
    }

    impl TestEffects {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
                logs: RefCell::new(Vec::new()),
            }
        }
    }

    impl QueueConsumptionProofEffects for TestEffects {
        fn append_state_event(
            &self,
            _project_root: &Path,
            event: &agent_doc_state_backbone::StateEvent,
        ) -> Result<bool> {
            self.events.borrow_mut().push(event.clone());
            Ok(true)
        }

        fn log_op(&self, _file: &Path, message: &str) {
            self.logs.borrow_mut().push(message.to_string());
        }

        fn now_millis(&self) -> u64 {
            42
        }
    }

    fn plan(content: &str) -> QueueConsumptionPlan {
        QueueConsumptionPlan {
            consumed_text: "Run queued thing".to_string(),
            consumed_texts: vec!["Run queued thing".to_string()],
            node_ops: vec![IpcNodeOp {
                component: "queue".to_string(),
                node_id: "queue:0".to_string(),
                op: "consume".to_string(),
                content: None,
            }],
            remaining: 0,
            drained: true,
            auto: true,
            new_document: content.to_string(),
            new_snapshot: content.to_string(),
            save_snapshot: true,
        }
    }

    #[test]
    fn proof_recorder_appends_ledger_and_state_event() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("s.md");
        let content = "<!-- agent:queue auto -->\n- Run queued thing\n<!-- /agent:queue -->\n";
        std::fs::write(&doc, content).unwrap();
        let canonical = doc.canonicalize().unwrap();
        let effects = TestEffects::new();

        record_queue_consumption_proofs(
            &effects,
            &doc,
            &plan(content),
            QueueConsumptionProofStage::BeforeMutation,
        )
        .unwrap();

        let ledger_path =
            agent_doc_workflow_io::proof_ledger::proof_ledger_path(dir.path(), &canonical);
        let records =
            agent_doc_workflow_io::proof_ledger::read_operation_proofs(&ledger_path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].outcome,
            agent_doc_workflow_io::proof_ledger::ProofOutcome::Recorded
        );
        assert_eq!(effects.events.borrow().len(), 1);
        assert!(matches!(
            effects.events.borrow()[0].fact,
            agent_doc_state_backbone::StateFact::QueueHeadSelected { .. }
        ));
        assert!(
            effects
                .logs
                .borrow()
                .iter()
                .any(|entry| entry.contains("queue_consume_proof_recorded"))
        );
    }

    #[test]
    fn authoritative_struck_rows_record_terminal_state_without_consumption_plan() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- ~~do [#completed-work]~~\n",
            "- do [#ready-work]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let effects = TestEffects::new();

        let projected =
            record_authoritative_queue_completion_state(&effects, &doc, content).unwrap();

        assert_eq!(projected, 1);
        let events = effects.events.borrow();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].fact,
            agent_doc_state_backbone::StateFact::QueueHeadCompleted {
                node_key,
                backlog_id,
                ..
            } if node_key == "queue:0:completed-work:0"
                && backlog_id.as_deref() == Some("completed-work")
        ));
        assert!(
            effects
                .logs
                .borrow()
                .iter()
                .any(|entry| entry.contains("queue_authoritative_completed_state_event_recorded"))
        );
    }
}

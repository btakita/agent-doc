//! Project-aware queue synchronization scan shared by preflight and terminal gates.

use std::path::{Path, PathBuf};

use agent_doc_queue::queue_sync_payload::{
    QueueSyncDocumentScope, QueueSyncEffectEvidence, QueueSyncExecutionEvidence,
    QueueSyncOperation, QueueSyncPayload, QueueSyncPlacementDebt, QueueSyncResponseEvidence,
    QueueSyncScanError, apply_attempt_evidence, finalize_lost_evidence_audit,
    normalize_claims_for_queue_sync, project_document_queue_sync,
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
        let claim_ledger = crate::queue_claim::load_ledger_following(&document, &content);
        let cancellations = claim_ledger
            .as_ref()
            .map(|ledger| ledger.cancellations.clone())
            .unwrap_or_default();
        let mut claims = match claim_ledger {
            Ok(ledger) => ledger.claims,
            Err(error) => {
                payload.scan_errors.push(scan_error(&document, error));
                Vec::new()
            }
        };
        match crate::claim_release_receipt::load(&document) {
            Ok(receipts) => {
                for receipt in receipts.receipts {
                    let cancelled = cancellations.iter().any(|cancelled| {
                        cancelled.cancels_attempt(&receipt.identity, receipt.generation)
                    });
                    if !cancelled
                        && !claims
                            .iter()
                            .any(|claim| claim.identity == receipt.identity)
                    {
                        claims.push(receipt.as_reported_claim());
                    }
                }
            }
            Err(error) => {
                payload.scan_errors.push(scan_error(&document, error));
            }
        }
        normalize_claims_for_queue_sync(&mut claims, now_secs);
        let lineage = if claims.is_empty() {
            None
        } else {
            match crate::queue_lineage::load(&document) {
                Ok(lineage) => Some(lineage),
                Err(error) => {
                    payload.scan_errors.push(scan_error(&document, error));
                    None
                }
            }
        };
        match project_document_queue_sync(
            key.display().to_string(),
            scope,
            &content,
            previous.as_deref(),
            &claims,
        ) {
            Ok(mut document_payload) => {
                apply_response_evidence(&mut document_payload, &content, &claims);
                if let Some(lineage) = lineage.as_ref() {
                    apply_lineage_evidence(&mut document_payload, lineage);
                    apply_owner_session_evidence(&document, &claims, &mut document_payload);
                    // Reaching this point proves every deterministic store was
                    // readable: claim/lineage, completion cells, release
                    // receipts, exchange/retained history, and owner-session
                    // evidence. Recovered Git effects from lineage remain
                    // actionable; only an evidence-empty attempt terminalizes
                    // as the last-resort `lost` outcome.
                    for item in &mut document_payload.lost {
                        let identity = agent_doc_queue::queue_claim::claim_identity(&item.item);
                        let explicit_requeue = claims.iter().any(|claim| {
                            claim.identity == identity
                                && claim.state
                                    == agent_doc_queue::queue_claim::QueueClaimState::Lost
                                && !claim.is_expired(now_secs)
                        }) && item.execution_evidence
                            == QueueSyncExecutionEvidence::NeverStarted;
                        if explicit_requeue {
                            item.operation = QueueSyncOperation::RecoverRequeue;
                        } else {
                            finalize_lost_evidence_audit(item);
                        }
                    }
                }
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

#[derive(Debug, PartialEq, Eq)]
enum HistoryResponse {
    Recovered(String),
    Missing,
    Ambiguous,
    Unsupported,
}

fn apply_response_evidence(
    payload: &mut QueueSyncPayload,
    content: &str,
    claims: &[agent_doc_queue::queue_claim::QueueClaim],
) {
    let claims = claims
        .iter()
        .map(|claim| (claim.identity.clone(), claim))
        .collect::<std::collections::HashMap<_, _>>();
    for item in &mut payload.lost_reported {
        let identity = agent_doc_queue::queue_claim::claim_identity(&item.item);
        let Some(claim) = claims.get(&identity).copied() else {
            item.response_evidence = QueueSyncResponseEvidence::Missing;
            item.missing_response_debt = true;
            item.operation = QueueSyncOperation::Reconcile;
            continue;
        };
        let key = crate::queue_claim::completion_feedback_key(claim).ok();
        if key
            .as_deref()
            .is_some_and(|key| completion_cell_has_response(content, key))
        {
            item.response_evidence = QueueSyncResponseEvidence::ExchangeOutbox;
            item.missing_response_debt = false;
            continue;
        }
        match recover_agent_history_response(claim) {
            HistoryResponse::Recovered(response) => {
                item.response_evidence = QueueSyncResponseEvidence::AgentHistory;
                item.recovered_response = Some(response);
                item.missing_response_debt = false;
            }
            HistoryResponse::Missing => {
                item.response_evidence = QueueSyncResponseEvidence::Missing;
                item.missing_response_debt = true;
                item.operation = QueueSyncOperation::Reconcile;
            }
            HistoryResponse::Ambiguous => {
                item.response_evidence = QueueSyncResponseEvidence::AmbiguousHistory;
                item.missing_response_debt = true;
                item.operation = QueueSyncOperation::Reconcile;
            }
            HistoryResponse::Unsupported => {
                item.response_evidence = QueueSyncResponseEvidence::UnsupportedHistory;
                item.missing_response_debt = true;
                item.operation = QueueSyncOperation::Reconcile;
            }
        }
    }

    // A plain lost attempt can still have an exact retained worker response
    // even when no completion cell/report receipt survived. Recover it before
    // loss can become terminal; never infer completion from a worker id alone.
    for item in &mut payload.lost {
        let identity = agent_doc_queue::queue_claim::claim_identity(&item.item);
        let Some(claim) = claims.get(&identity).copied() else {
            continue;
        };
        match recover_agent_history_response(claim) {
            HistoryResponse::Recovered(response) => {
                item.response_evidence = QueueSyncResponseEvidence::AgentHistory;
                item.recovered_response = Some(response);
                item.missing_response_debt = false;
                item.operation = QueueSyncOperation::IntegrateClose;
            }
            HistoryResponse::Missing => {
                item.response_evidence = QueueSyncResponseEvidence::Missing;
            }
            HistoryResponse::Ambiguous => {
                item.response_evidence = QueueSyncResponseEvidence::AmbiguousHistory;
            }
            HistoryResponse::Unsupported => {
                item.response_evidence = QueueSyncResponseEvidence::UnsupportedHistory;
            }
        }
    }
}

fn apply_owner_session_evidence(
    file: &Path,
    claims: &[agent_doc_queue::queue_claim::QueueClaim],
    payload: &mut QueueSyncPayload,
) {
    use agent_doc_queue::queue_claim::ClaimOwnerLiveness;

    let liveness = claims
        .iter()
        .map(|claim| {
            (
                claim.identity.clone(),
                (
                    crate::queue_claim::claim_owner_liveness(file, claim),
                    claim.worker.is_some(),
                ),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    for item in payload
        .claimed_unstarted
        .iter_mut()
        .chain(payload.running.iter_mut())
    {
        let identity = agent_doc_queue::queue_claim::claim_identity(&item.item);
        if liveness.get(&identity).is_some_and(|(state, _)| {
            matches!(
                *state,
                ClaimOwnerLiveness::Ended(_) | ClaimOwnerLiveness::Unrecorded
            )
        }) {
            // An ended coordinator session or an attempt with no recorded
            // owner session is not verifiably live. Reconcile/recover its exact
            // attempt before terminalizing it; never dispatch a second worker
            // merely because the visible row still says claimed/running.
            item.operation = QueueSyncOperation::Reconcile;
        }
    }
    for item in &mut payload.lost {
        let identity = agent_doc_queue::queue_claim::claim_identity(&item.item);
        if liveness.get(&identity).is_some_and(|(state, has_worker)| {
            *has_worker && matches!(*state, ClaimOwnerLiveness::Live)
        }) {
            // A still-live owner bound to a concrete worker is recoverable
            // state. Keep the attempt open for adoption/response recovery;
            // last-resort loss is allowed only after that liveness ends.
            item.missing_response_debt = true;
            item.operation = QueueSyncOperation::Reconcile;
        }
    }
}

fn completion_cell_has_response(content: &str, key: &str) -> bool {
    let lines = content.lines().collect::<Vec<_>>();
    let marker = agent_doc_document::subagent_completion::marker(key);
    agent_doc_document::subagent_completion::completion_cell_line_ranges(content)
        .into_iter()
        .any(|range| {
            lines
                .get(range.end.saturating_sub(1))
                .is_some_and(|line| line.trim() == marker)
                && lines[range].iter().any(|line| {
                    line.trim().starts_with(
                        agent_doc_document::subagent_completion::RESPONSE_DIGEST_PREFIX,
                    )
                })
        })
}

fn recover_agent_history_response(
    claim: &agent_doc_queue::queue_claim::QueueClaim,
) -> HistoryResponse {
    let Some(session) = claim.owner_session.as_ref() else {
        return HistoryResponse::Unsupported;
    };
    let Some(harness) = session.harness.as_deref() else {
        return HistoryResponse::Unsupported;
    };
    let (Some(session_id), Some(worker_id)) = (
        session.session_id.as_deref(),
        claim
            .worker
            .as_ref()
            .map(|worker| worker.worker_id.as_str()),
    ) else {
        return HistoryResponse::Missing;
    };
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return HistoryResponse::Missing;
    };
    match harness {
        "claude" | "claude-code" => recover_claude_history_response(&home, session_id, worker_id),
        "codex" => {
            if claim.generation == 0 {
                return HistoryResponse::Unsupported;
            }
            let Ok(attempt_completion_key) = crate::queue_claim::completion_feedback_key(claim)
            else {
                return HistoryResponse::Unsupported;
            };
            recover_codex_history_response(
                &home,
                CodexHistoryLocator {
                    coordinator_thread_id: session_id,
                    native_subagent_id: worker_id,
                    attempt_completion_key: &attempt_completion_key,
                },
            )
        }
        _ => HistoryResponse::Unsupported,
    }
}

/// Exact durable coordinates for one Codex queue attempt. The completion key
/// binds the recorded worker to the queue claim generation; the transcript is
/// then selected only by the two native thread identities.
struct CodexHistoryLocator<'a> {
    coordinator_thread_id: &'a str,
    native_subagent_id: &'a str,
    attempt_completion_key: &'a str,
}

fn recover_codex_history_response(
    home: &Path,
    locator: CodexHistoryLocator<'_>,
) -> HistoryResponse {
    if locator.attempt_completion_key.is_empty()
        || !codex_thread_id_is_safe(locator.coordinator_thread_id)
        || !codex_thread_id_is_safe(locator.native_subagent_id)
    {
        return HistoryResponse::Unsupported;
    }
    let sessions = home.join(".codex/sessions");
    let mut candidates = Vec::new();
    collect_codex_rollout_candidates(&sessions, locator.native_subagent_id, &mut candidates);
    let mut matches = candidates
        .into_iter()
        .filter(|path| codex_rollout_matches(path, &locator))
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [] => HistoryResponse::Missing,
        [path] => parse_codex_subagent_final_response(path),
        _ => HistoryResponse::Ambiguous,
    }
}

fn codex_thread_id_is_safe(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn collect_codex_rollout_candidates(root: &Path, worker_id: &str, matches: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let filename_suffix = format!("-{worker_id}.jsonl");
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_codex_rollout_candidates(&path, worker_id, matches);
        } else if file_type.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(&filename_suffix))
        {
            matches.push(path);
        }
    }
}

fn codex_rollout_matches(path: &Path, locator: &CodexHistoryLocator<'_>) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let mut matching_metadata = 0;
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        if codex_session_metadata_matches(&value, locator) {
            matching_metadata += 1;
        }
    }
    matching_metadata == 1
}

fn codex_session_metadata_matches(
    value: &serde_json::Value,
    locator: &CodexHistoryLocator<'_>,
) -> bool {
    if value.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
        return false;
    }
    let Some(payload) = value.get("payload") else {
        return false;
    };
    payload.get("id").and_then(serde_json::Value::as_str) == Some(locator.native_subagent_id)
        && payload
            .get("parent_thread_id")
            .and_then(serde_json::Value::as_str)
            == Some(locator.coordinator_thread_id)
        && payload
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            == Some(locator.coordinator_thread_id)
        && payload
            .pointer("/source/subagent/thread_spawn/parent_thread_id")
            .and_then(serde_json::Value::as_str)
            == Some(locator.coordinator_thread_id)
        && payload
            .get("thread_source")
            .and_then(serde_json::Value::as_str)
            == Some("subagent")
}

fn parse_codex_subagent_final_response(path: &Path) -> HistoryResponse {
    let Ok(content) = std::fs::read_to_string(path) else {
        return HistoryResponse::Missing;
    };
    let values = match content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(values) => values,
        Err(_) => return HistoryResponse::Missing,
    };
    let started_turns = values
        .iter()
        .filter_map(|value| {
            (value.get("type").and_then(serde_json::Value::as_str) == Some("event_msg")
                && value
                    .pointer("/payload/type")
                    .and_then(serde_json::Value::as_str)
                    == Some("task_started"))
            .then(|| {
                value
                    .pointer("/payload/turn_id")
                    .and_then(serde_json::Value::as_str)
            })
            .flatten()
        })
        .collect::<Vec<_>>();
    let completed = values
        .iter()
        .filter(|value| {
            value.get("type").and_then(serde_json::Value::as_str) == Some("event_msg")
                && value
                    .pointer("/payload/type")
                    .and_then(serde_json::Value::as_str)
                    == Some("task_complete")
        })
        .collect::<Vec<_>>();
    if started_turns.is_empty() || completed.is_empty() {
        return HistoryResponse::Missing;
    }
    if started_turns.len() != 1 || completed.len() != 1 {
        return HistoryResponse::Ambiguous;
    }
    let Some(completed_turn) = completed[0]
        .pointer("/payload/turn_id")
        .and_then(serde_json::Value::as_str)
    else {
        return HistoryResponse::Missing;
    };
    if started_turns[0] != completed_turn {
        return HistoryResponse::Ambiguous;
    }
    let Some(completed_response) = completed[0]
        .pointer("/payload/last_agent_message")
        .and_then(serde_json::Value::as_str)
        .filter(|response| !response.trim().is_empty())
    else {
        return HistoryResponse::Missing;
    };
    let responses = values
        .iter()
        .filter(|value| {
            value.get("type").and_then(serde_json::Value::as_str) == Some("response_item")
                && value
                    .pointer("/payload/type")
                    .and_then(serde_json::Value::as_str)
                    == Some("message")
                && value
                    .pointer("/payload/role")
                    .and_then(serde_json::Value::as_str)
                    == Some("assistant")
                && value
                    .pointer("/payload/phase")
                    .and_then(serde_json::Value::as_str)
                    == Some("final_answer")
                && value
                    .pointer("/payload/internal_chat_message_metadata_passthrough/turn_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(completed_turn)
        })
        .filter_map(|value| {
            let response = value
                .pointer("/payload/content")
                .and_then(serde_json::Value::as_array)?
                .iter()
                .filter(|block| {
                    block.get("type").and_then(serde_json::Value::as_str) == Some("output_text")
                })
                .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!response.trim().is_empty()).then_some(response)
        })
        .collect::<Vec<_>>();
    match responses.as_slice() {
        [] => HistoryResponse::Missing,
        [response] if response == completed_response => {
            HistoryResponse::Recovered(response.clone())
        }
        [_] => HistoryResponse::Ambiguous,
        _ => HistoryResponse::Ambiguous,
    }
}

fn recover_claude_history_response(
    home: &Path,
    session_id: &str,
    worker_id: &str,
) -> HistoryResponse {
    let worker_id = worker_id.strip_prefix("agent-").unwrap_or(worker_id);
    if worker_id.is_empty()
        || !worker_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return HistoryResponse::Unsupported;
    }
    let projects = home.join(".claude/projects");
    let Ok(entries) = std::fs::read_dir(projects) else {
        return HistoryResponse::Missing;
    };
    let filename = format!("agent-{worker_id}.jsonl");
    let mut matches = entries
        .flatten()
        .map(|entry| {
            entry
                .path()
                .join(session_id)
                .join("subagents")
                .join(&filename)
        })
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [] => HistoryResponse::Missing,
        [path] => parse_claude_subagent_final_response(path, worker_id),
        _ => HistoryResponse::Ambiguous,
    }
}

fn parse_claude_subagent_final_response(path: &Path, worker_id: &str) -> HistoryResponse {
    let Ok(content) = std::fs::read_to_string(path) else {
        return HistoryResponse::Missing;
    };
    let mut final_response = None;
    let mut last_role = None;
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value
            .get("agentId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|observed| observed != worker_id)
        {
            return HistoryResponse::Ambiguous;
        }
        let role = value
            .get("message")
            .and_then(|message| message.get("role"))
            .and_then(serde_json::Value::as_str);
        match role {
            Some("user") => last_role = Some("user"),
            Some("assistant") => {
                last_role = Some("assistant");
                let text = value
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|block| {
                        block.get("type").and_then(serde_json::Value::as_str) == Some("text")
                    })
                    .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();
                final_response = (!text.is_empty()).then_some(text);
            }
            _ => {}
        }
    }
    match (last_role, final_response) {
        (Some("assistant"), Some(response)) => HistoryResponse::Recovered(response),
        _ => HistoryResponse::Missing,
    }
}

fn apply_lineage_evidence(
    payload: &mut QueueSyncPayload,
    lineage: &crate::queue_lineage::QueueLineageLedger,
) {
    use crate::queue_lineage::QueueLineageEventKind;

    for item in payload
        .pending
        .iter_mut()
        .chain(&mut payload.claimed_unstarted)
        .chain(&mut payload.running)
        .chain(&mut payload.reported_checkmark)
        .chain(&mut payload.lost_reported)
        .chain(&mut payload.lost)
        .chain(&mut payload.blocked)
        .chain(&mut payload.reconcile_unknown)
        .chain(&mut payload.operator_gated)
    {
        let Some(generation) = item.generation else {
            continue;
        };
        let identity = agent_doc_queue::queue_claim::claim_identity(&item.item);
        let Some(attempt) = lineage.attempts.iter().find(|attempt| {
            attempt.generation == generation
                && (attempt.identity == identity || attempt.identity_history.contains(&identity))
        }) else {
            continue;
        };
        let started = attempt.events.iter().any(|event| {
            matches!(
                event.kind,
                QueueLineageEventKind::Attached
                    | QueueLineageEventKind::Dirty
                    | QueueLineageEventKind::Committed
                    | QueueLineageEventKind::Pushed
                    | QueueLineageEventKind::Integrated
                    | QueueLineageEventKind::Released
            )
        });
        let created_only = !attempt.events.is_empty()
            && attempt
                .events
                .iter()
                .all(|event| event.kind == QueueLineageEventKind::Created);
        let execution = if item.completion_outbox_recovered {
            QueueSyncExecutionEvidence::CompletedOutbox
        } else if started {
            QueueSyncExecutionEvidence::Started
        } else if created_only {
            QueueSyncExecutionEvidence::NeverStarted
        } else {
            QueueSyncExecutionEvidence::Unknown
        };
        let effects = attempt
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    QueueLineageEventKind::Committed
                        | QueueLineageEventKind::Pushed
                        | QueueLineageEventKind::Integrated
                        | QueueLineageEventKind::Released
                )
            })
            .map(|event| QueueSyncEffectEvidence {
                kind: match event.kind {
                    QueueLineageEventKind::Committed => "committed",
                    QueueLineageEventKind::Pushed => "pushed",
                    QueueLineageEventKind::Integrated => "integrated",
                    QueueLineageEventKind::Released => "released",
                    _ => unreachable!("filtered effect event"),
                }
                .to_string(),
                source_sha: event.source_sha.clone(),
                pushed_ref: event.pushed_ref.clone(),
                integration_sha: event.integration_sha.clone(),
                release_tag: event.release_tag.clone(),
                release_state: event.release_state.map(|state| {
                    match state {
                        crate::queue_lineage::ReleaseState::Planned => "planned",
                        crate::queue_lineage::ReleaseState::Tagged => "tagged",
                        crate::queue_lineage::ReleaseState::Published => "published",
                        crate::queue_lineage::ReleaseState::Failed => "failed",
                    }
                    .to_string()
                }),
            })
            .collect();
        apply_attempt_evidence(item, execution, effects);
    }
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

    fn lost_payload() -> QueueSyncPayload {
        let mut ledger = agent_doc_queue::queue_claim::QueueClaimLedger::default();
        ledger.claim("lost work", "subagent:test", 10, 60);
        ledger.claims[0].state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
        agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            "/tmp/lost.md",
            QueueSyncDocumentScope::Current,
            "<!-- agent:run id=run subagents -->\n- ⚠️ lost work\n<!-- /agent:run -->\n",
            None,
            &ledger.claims,
        )
        .unwrap()
    }

    fn lineage_with(
        events: Vec<crate::queue_lineage::QueueLineageEvent>,
    ) -> crate::queue_lineage::QueueLineageLedger {
        crate::queue_lineage::QueueLineageLedger {
            contract_version: crate::queue_lineage::QUEUE_LINEAGE_CONTRACT_VERSION.to_string(),
            attempts: vec![crate::queue_lineage::QueueLineageAttempt {
                identity: agent_doc_queue::queue_claim::claim_identity("lost work"),
                identity_history: Vec::new(),
                item_text: "lost work".to_string(),
                item_text_history: Vec::new(),
                generation: 1,
                owner: "subagent:test".to_string(),
                events,
            }],
        }
    }

    #[test]
    fn lineage_recovers_effects_and_never_turns_loss_into_implicit_retry() {
        let created = crate::queue_lineage::QueueLineageEvent {
            kind: crate::queue_lineage::QueueLineageEventKind::Created,
            ..Default::default()
        };
        let mut unstarted = lost_payload();
        apply_lineage_evidence(&mut unstarted, &lineage_with(vec![created.clone()]));
        finalize_lost_evidence_audit(&mut unstarted.lost[0]);
        assert!(!unstarted.lost[0].retry_safe);
        assert_eq!(
            unstarted.lost[0].operation,
            QueueSyncOperation::TerminalLost
        );

        let mut started = lost_payload();
        let attached = crate::queue_lineage::QueueLineageEvent {
            kind: crate::queue_lineage::QueueLineageEventKind::Attached,
            worker_id: Some("worker:test".to_string()),
            ..Default::default()
        };
        let committed = crate::queue_lineage::QueueLineageEvent {
            kind: crate::queue_lineage::QueueLineageEventKind::Committed,
            source_sha: Some("abc123".to_string()),
            ..Default::default()
        };
        apply_lineage_evidence(
            &mut started,
            &lineage_with(vec![created, attached, committed]),
        );
        assert!(!started.lost[0].retry_safe);
        assert_eq!(started.lost[0].operation, QueueSyncOperation::Reconcile);
        assert_eq!(
            started.lost[0].execution_evidence,
            QueueSyncExecutionEvidence::Started
        );
        assert_eq!(started.lost[0].recovered_effects[0].kind, "committed");
        assert_eq!(
            started.lost[0].recovered_effects[0].source_sha.as_deref(),
            Some("abc123")
        );
        finalize_lost_evidence_audit(&mut started.lost[0]);
        assert_eq!(started.lost[0].operation, QueueSyncOperation::Reconcile);
    }

    #[test]
    fn ended_owner_session_is_reconcile_debt_not_a_waiting_worker() {
        let root = tempfile::tempdir().unwrap();
        let doc = root.path().join("session.md");
        let content =
            "<!-- agent:run id=run subagents -->\n- 🚧 stale worker\n<!-- /agent:run -->\n";
        std::fs::write(&doc, content).unwrap();
        let mut ended_process = std::process::Command::new("true").spawn().unwrap();
        let ended_pid = ended_process.id();
        ended_process.wait().unwrap();
        let mut ledger = agent_doc_queue::queue_claim::QueueClaimLedger::default();
        ledger.claim("stale worker", "subagent:stale", 10, u64::MAX / 2);
        let claim = &mut ledger.claims[0];
        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Running;
        claim.worker = Some(agent_doc_queue::queue_claim::QueueWorkerBinding {
            worker_id: "worker-stale".to_string(),
            started_at_secs: 11,
        });
        claim.owner_session = Some(agent_doc_queue::queue_claim::ClaimOwnerSession {
            harness: Some("claude".to_string()),
            session_id: Some("ended-session".to_string()),
            pane: None,
            pid: Some(ended_pid),
        });
        let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            doc.display().to_string(),
            QueueSyncDocumentScope::Current,
            content,
            None,
            &ledger.claims,
        )
        .unwrap();
        assert_eq!(payload.running[0].operation, QueueSyncOperation::Wait);

        apply_owner_session_evidence(&doc, &ledger.claims, &mut payload);

        assert_eq!(payload.running[0].operation, QueueSyncOperation::Reconcile);
    }

    #[test]
    fn unrecorded_owner_session_is_reconcile_debt_not_a_waiting_worker() {
        let root = tempfile::tempdir().unwrap();
        let doc = root.path().join("session.md");
        let content =
            "<!-- agent:run id=run subagents -->\n- 🚧 unverified worker\n<!-- /agent:run -->\n";
        std::fs::write(&doc, content).unwrap();
        let mut ledger = agent_doc_queue::queue_claim::QueueClaimLedger::default();
        ledger.claim("unverified worker", "subagent:unverified", 10, u64::MAX / 2);
        let claim = &mut ledger.claims[0];
        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Running;
        claim.worker = Some(agent_doc_queue::queue_claim::QueueWorkerBinding {
            worker_id: "worker-unverified".to_string(),
            started_at_secs: 11,
        });
        claim.owner_session = None;
        let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            doc.display().to_string(),
            QueueSyncDocumentScope::Current,
            content,
            None,
            &ledger.claims,
        )
        .unwrap();
        assert_eq!(payload.running[0].operation, QueueSyncOperation::Wait);

        apply_owner_session_evidence(&doc, &ledger.claims, &mut payload);

        assert_eq!(payload.running[0].operation, QueueSyncOperation::Reconcile);
    }

    #[test]
    fn released_recovered_completion_keeps_lost_and_outbox_state_in_sync() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".agent-doc")).unwrap();
        let doc = root.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: recovered-session\n---\n",
            "<!-- agent:run id=run subagents -->\n",
            "- recovered worker task\n",
            "<!-- /agent:run -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let (_, claimed) =
            crate::queue_claim::claim(&doc, "recovered worker task", "subagent:recovered", 600)
                .unwrap();
        let running = crate::queue_claim::start_attempt(
            &doc,
            "recovered worker task",
            "subagent:recovered",
            "worker:recovered",
            600,
            claimed.generation,
        )
        .unwrap();
        crate::queue_claim::abandon_attempt(
            &doc,
            "recovered worker task",
            Some(claimed.generation),
        )
        .unwrap();
        let cell = agent_doc_document::subagent_completion::cell_with_response(
            &running.owner,
            &running.item_text,
            &crate::queue_claim::completion_feedback_key(&running).unwrap(),
            "Recovered exact worker response.",
        )
        .unwrap();
        assert_eq!(
            crate::queue_claim::reconcile_completion_evidence(&doc, &cell).unwrap(),
            1
        );
        crate::claim_release_receipt::release_attempt(
            &doc,
            "recovered worker task",
            crate::claim_release_receipt::ReleaseKind::Reported,
            Some(claimed.generation),
        )
        .unwrap();
        assert!(
            crate::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .is_empty()
        );
        let content = format!(
            "---\nagent_doc_session: recovered-session\n---\n<!-- agent:exchange -->\n{cell}\n<!-- /agent:exchange -->\n<!-- agent:run id=run subagents -->\n- recovered worker task\n<!-- /agent:run -->\n"
        );
        std::fs::write(&doc, &content).unwrap();

        let payload = build_project_queue_sync_payload(&doc, &content, None).unwrap();
        assert!(payload.scan_errors.is_empty(), "{:?}", payload.scan_errors);
        assert_eq!(payload.lost_reported.len(), 1);
        let item = &payload.lost_reported[0];
        assert!(item.completion_outbox_recovered);
        assert!(!item.retry_safe);
        assert_eq!(item.operation, QueueSyncOperation::IntegrateClose);
        assert_eq!(
            item.response_evidence,
            QueueSyncResponseEvidence::ExchangeOutbox
        );
        assert!(!item.missing_response_debt);
        assert_eq!(
            item.execution_evidence,
            QueueSyncExecutionEvidence::CompletedOutbox
        );
    }

    #[test]
    fn exact_claude_subagent_history_recovers_only_a_terminal_assistant_response() {
        let home = tempfile::tempdir().unwrap();
        let history = home
            .path()
            .join(".claude/projects/project/session-1/subagents");
        std::fs::create_dir_all(&history).unwrap();
        let transcript = history.join("agent-worker-1.jsonl");
        std::fs::write(
            &transcript,
            concat!(
                "{\"agentId\":\"worker-1\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"do it\"}]}}\n",
                "{\"agentId\":\"worker-1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Exact recovered response.\"}]}}\n",
            ),
        )
        .unwrap();
        assert_eq!(
            recover_claude_history_response(home.path(), "session-1", "worker-1"),
            HistoryResponse::Recovered("Exact recovered response.".to_string())
        );

        std::fs::write(
            &transcript,
            concat!(
                "{\"agentId\":\"worker-1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Progress only.\"}]}}\n",
                "{\"agentId\":\"worker-1\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\"}]}}\n",
            ),
        )
        .unwrap();
        assert_eq!(
            recover_claude_history_response(home.path(), "session-1", "worker-1"),
            HistoryResponse::Missing
        );
    }

    fn write_codex_rollout(
        home: &Path,
        directory: &str,
        parent: &str,
        worker: &str,
        turns: &[(&str, &str)],
    ) -> PathBuf {
        let history = home.join(".codex/sessions").join(directory);
        std::fs::create_dir_all(&history).unwrap();
        let transcript = history.join(format!("rollout-arbitrary-{worker}.jsonl"));
        let mut records = vec![serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": worker,
                "session_id": parent,
                "parent_thread_id": parent,
                "cwd": "/deliberately/not/a/lookup/key",
                "source": {"subagent": {"thread_spawn": {"parent_thread_id": parent}}},
                "thread_source": "subagent"
            }
        })];
        for (turn_id, response) in turns {
            records.push(serde_json::json!({
                "type": "event_msg",
                "payload": {"type": "task_started", "turn_id": turn_id}
            }));
            records.push(serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "phase": "final_answer",
                    "content": [{"type": "output_text", "text": response}],
                    "internal_chat_message_metadata_passthrough": {"turn_id": turn_id}
                }
            }));
            records.push(serde_json::json!({
                "type": "event_msg",
                "payload": {
                    "type": "task_complete",
                    "turn_id": turn_id,
                    "last_agent_message": response
                }
            }));
        }
        let content = records
            .into_iter()
            .map(|record| record.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&transcript, format!("{content}\n")).unwrap();
        transcript
    }

    #[test]
    fn exact_codex_subagent_history_uses_native_attempt_coordinates_only() {
        let home = tempfile::tempdir().unwrap();
        write_codex_rollout(
            home.path(),
            "not-a-date",
            "parent-thread-1",
            "worker-thread-1",
            &[("turn-1", "Exact Codex response.")],
        );
        let locator = CodexHistoryLocator {
            coordinator_thread_id: "parent-thread-1",
            native_subagent_id: "worker-thread-1",
            attempt_completion_key: "immutable-generation-key",
        };
        assert_eq!(
            recover_codex_history_response(home.path(), locator),
            HistoryResponse::Recovered("Exact Codex response.".to_string())
        );
    }

    #[test]
    fn codex_history_fails_closed_on_missing_attempt_key_or_multiple_matches() {
        let home = tempfile::tempdir().unwrap();
        write_codex_rollout(
            home.path(),
            "one",
            "parent-thread-1",
            "worker-thread-1",
            &[("turn-1", "First copy.")],
        );
        assert_eq!(
            recover_codex_history_response(
                home.path(),
                CodexHistoryLocator {
                    coordinator_thread_id: "parent-thread-1",
                    native_subagent_id: "worker-thread-1",
                    attempt_completion_key: "",
                },
            ),
            HistoryResponse::Unsupported
        );
        write_codex_rollout(
            home.path(),
            "two",
            "parent-thread-1",
            "worker-thread-1",
            &[("turn-1", "Second copy.")],
        );
        assert_eq!(
            recover_codex_history_response(
                home.path(),
                CodexHistoryLocator {
                    coordinator_thread_id: "parent-thread-1",
                    native_subagent_id: "worker-thread-1",
                    attempt_completion_key: "immutable-generation-key",
                },
            ),
            HistoryResponse::Ambiguous
        );
    }

    #[test]
    fn codex_history_fails_closed_on_multiple_terminal_turns_or_wrong_parent() {
        let home = tempfile::tempdir().unwrap();
        write_codex_rollout(
            home.path(),
            "one",
            "parent-thread-1",
            "worker-thread-1",
            &[("turn-1", "First."), ("turn-2", "Second.")],
        );
        assert_eq!(
            recover_codex_history_response(
                home.path(),
                CodexHistoryLocator {
                    coordinator_thread_id: "parent-thread-1",
                    native_subagent_id: "worker-thread-1",
                    attempt_completion_key: "immutable-generation-key",
                },
            ),
            HistoryResponse::Ambiguous
        );
        assert_eq!(
            recover_codex_history_response(
                home.path(),
                CodexHistoryLocator {
                    coordinator_thread_id: "different-parent",
                    native_subagent_id: "worker-thread-1",
                    attempt_completion_key: "immutable-generation-key",
                },
            ),
            HistoryResponse::Missing
        );
    }

    #[test]
    fn recovered_loss_without_response_is_explicit_nonterminal_debt() {
        let content =
            "<!-- agent:run id=run subagents -->\n- ⚠️☑️ recovered work\n<!-- /agent:run -->\n";
        let mut ledger = agent_doc_queue::queue_claim::QueueClaimLedger::default();
        ledger.claim("recovered work", "subagent:test", 10, 60);
        let claim = &mut ledger.claims[0];
        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Reported;
        claim.completion_recovered_after_loss = true;
        claim.worker = Some(agent_doc_queue::queue_claim::QueueWorkerBinding {
            worker_id: "worker:test".to_string(),
            started_at_secs: 10,
        });
        let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            "/tmp/lost.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &ledger.claims,
        )
        .unwrap();

        apply_response_evidence(&mut payload, content, &ledger.claims);
        let item = &payload.lost_reported[0];
        assert!(item.missing_response_debt);
        assert_eq!(item.operation, QueueSyncOperation::Reconcile);
        assert_eq!(
            item.response_evidence,
            QueueSyncResponseEvidence::UnsupportedHistory
        );
        assert!(!item.retry_safe);
    }

    #[test]
    fn evidence_exhausted_expired_attempt_is_terminal_and_clean() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".agent-doc")).unwrap();
        let doc = root.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: terminal-loss-session\n---\n",
            "<!-- agent:run id=run subagents -->\n",
            "- terminal loss\n",
            "<!-- /agent:run -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let _claimed_at = crate::queue_claim::pin_claim_clock_for_current_thread(1_000_000);
        crate::queue_claim::claim(&doc, "terminal loss", "subagent:old", 10).unwrap();
        let _expired = crate::queue_claim::pin_claim_clock_for_current_thread(1_000_010);

        let payload = build_project_queue_sync_payload(&doc, content, None).unwrap();

        assert!(payload.scan_errors.is_empty(), "{:?}", payload.scan_errors);
        assert_eq!(payload.lost.len(), 1);
        assert_eq!(payload.lost[0].operation, QueueSyncOperation::TerminalLost);
        assert!(!payload.lost[0].retry_safe);
        assert!(
            payload.is_empty(),
            "an audited terminal loss must not keep the turn open"
        );
    }

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

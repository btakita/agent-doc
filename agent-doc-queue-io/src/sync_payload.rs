//! Project-aware queue synchronization scan shared by preflight and terminal gates.

use std::path::{Path, PathBuf};

use agent_doc_queue::queue_sync_payload::{
    QueueSyncDocumentScope, QueueSyncEffectEvidence, QueueSyncExecutionEvidence,
    QueueSyncOperation, QueueSyncPayload, QueueSyncPlacementDebt, QueueSyncResponseEvidence,
    QueueSyncScanError, apply_attempt_evidence, normalize_claims_for_queue_sync,
    project_document_queue_sync,
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
        let mut durable_response_receipts = Vec::new();
        match crate::claim_release_receipt::load(&document) {
            Ok(receipts) => {
                durable_response_receipts = receipts.receipts.clone();
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
        let committed_content = match agent_doc_git_io::revision::show_head(&document) {
            Ok(content) => content,
            Err(error) => {
                payload.scan_errors.push(scan_error(&document, error));
                None
            }
        };
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
                let retained_capture_targets = agent_doc_cycle_state_io::load_document_projection(
                    &document,
                )
                .ok()
                .flatten()
                .map(|projection| {
                    if projection.document.pending_write_journal.is_empty() {
                        projection
                            .document
                            .pending_write
                            .into_iter()
                            .map(|intent| intent.target_content)
                            .collect::<Vec<_>>()
                    } else {
                        projection
                            .document
                            .pending_write_journal
                            .into_iter()
                            .map(|intent| intent.target_content)
                            .collect::<Vec<_>>()
                    }
                })
                .unwrap_or_default();
                apply_response_evidence(
                    &mut document_payload,
                    &content,
                    committed_content.as_deref(),
                    &claims,
                    &durable_response_receipts,
                    &retained_capture_targets,
                );
                if let Some(lineage) = lineage.as_ref() {
                    apply_lineage_evidence(&mut document_payload, lineage);
                }
                // A claim whose coordinator session ended is durable reconcile
                // evidence, not proof that a worker is still in flight. Keep
                // the row fenced by the claim, but do not advertise it as a
                // fresh pending/claimed/running execution obligation. This is
                // especially important after derived active maintenance:
                // the selected-row projection must not resurrect the same
                // cleared-session claim as runnable work.
                reclassify_ended_owner_claims(&document, &claims, &mut document_payload);
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

fn reclassify_ended_owner_claims(
    document: &Path,
    claims: &[agent_doc_queue::queue_claim::QueueClaim],
    payload: &mut QueueSyncPayload,
) {
    let ended = claims
        .iter()
        .filter(|claim| {
            matches!(
                crate::queue_claim::claim_owner_liveness(document, claim),
                agent_doc_queue::queue_claim::ClaimOwnerLiveness::Ended(_)
            )
        })
        .map(|claim| claim.identity.clone())
        .collect::<std::collections::HashSet<_>>();
    if ended.is_empty() {
        return;
    }

    let mut stale = Vec::new();
    for partition in [&mut payload.claimed_unstarted, &mut payload.running] {
        let mut retained = Vec::with_capacity(partition.len());
        for mut item in std::mem::take(partition) {
            let identity = agent_doc_queue::queue_claim::claim_identity(&item.item);
            if ended.contains(&identity) {
                item.lifecycle =
                    agent_doc_queue::queue_sync_payload::QueueSyncLifecycle::ReconcileUnknown;
                item.operation = QueueSyncOperation::Reconcile;
                item.execution_evidence = QueueSyncExecutionEvidence::Unknown;
                item.retry_safe = false;
                stale.push(item);
            } else {
                retained.push(item);
            }
        }
        *partition = retained;
    }
    payload.reconcile_unknown.extend(stale);
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
    committed_content: Option<&str>,
    claims: &[agent_doc_queue::queue_claim::QueueClaim],
    durable_receipts: &[agent_doc_queue::claim_release_receipt::ClaimReleaseReceipt],
    retained_capture_targets: &[String],
) {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    apply_response_evidence_with(
        payload,
        content,
        committed_content,
        claims,
        durable_receipts,
        retained_capture_targets,
        |claim| {
            home.as_deref().map_or(HistoryResponse::Missing, |home| {
                recover_agent_history_response_from(home, claim)
            })
        },
    );
}

fn apply_response_evidence_with(
    payload: &mut QueueSyncPayload,
    content: &str,
    committed_content: Option<&str>,
    claims: &[agent_doc_queue::queue_claim::QueueClaim],
    durable_receipts: &[agent_doc_queue::claim_release_receipt::ClaimReleaseReceipt],
    retained_capture_targets: &[String],
    mut recover: impl FnMut(&agent_doc_queue::queue_claim::QueueClaim) -> HistoryResponse,
) {
    let claims = claims
        .iter()
        .map(|claim| (claim.identity.clone(), claim))
        .collect::<std::collections::HashMap<_, _>>();
    let root_exchange = root_authored_exchange(content);
    for item in payload
        .reported_checkmark
        .iter_mut()
        .chain(&mut payload.lost_reported)
    {
        let identity = agent_doc_queue::queue_claim::claim_identity(&item.item);
        let Some(claim) = claims.get(&identity).copied() else {
            item.response_evidence = QueueSyncResponseEvidence::Missing;
            item.missing_response_debt = true;
            item.operation = QueueSyncOperation::Reconcile;
            continue;
        };
        let key = crate::queue_claim::completion_feedback_key(claim).ok();
        let outbox_response = key.as_deref().and_then(|key| {
            durable_completion_response(
                claim,
                key,
                content,
                durable_receipts,
                retained_capture_targets,
            )
        });
        item.response_id = key.clone();
        item.response_capture_sha256 =
            matching_response_capture(claim, key.as_deref(), durable_receipts);
        let has_outbox_receipt = outbox_response.is_some()
            || key.as_deref().is_some_and(|key| {
                completion_cell_has_response(content, key)
                    || committed_content.is_some_and(|head| completion_cell_has_response(head, key))
            });
        item.completion_response_receipt = has_outbox_receipt;
        let committed_queue_response = key.as_deref().and_then(|key| {
            committed_content.and_then(|head| committed_queue_response_for_claim(head, key, claim))
        });
        apply_completed_response_evidence(
            item,
            &root_exchange,
            outbox_response,
            recover(claim),
            committed_queue_response,
        );
        // Keep any recovery failure as the terminal blocker. The separate
        // receipt bit preserves the valid completion evidence without letting
        // its possibly abbreviated body stand in for the full worker result.
    }

    // A child can finish and persist its terminal rollout after the root sees
    // the mailbox notification but before the root records report-complete.
    // Stable terminal history is positive completion evidence, so promote that
    // stale Running projection into coordinator integration debt. A merely
    // missing/unsupported history source does not prove a still-running child
    // completed and therefore remains a normal wait.
    let mut waiting = Vec::new();
    for mut item in std::mem::take(&mut payload.running) {
        let identity = agent_doc_queue::queue_claim::claim_identity(&item.item);
        let Some(claim) = claims.get(&identity).copied() else {
            waiting.push(item);
            continue;
        };
        let key = crate::queue_claim::completion_feedback_key(claim).ok();
        let outbox_response = key.as_deref().and_then(|key| {
            durable_completion_response(
                claim,
                key,
                content,
                durable_receipts,
                retained_capture_targets,
            )
        });
        item.response_id = key.clone();
        item.response_capture_sha256 =
            matching_response_capture(claim, key.as_deref(), durable_receipts);
        let has_outbox_receipt = outbox_response.is_some()
            || key.as_deref().is_some_and(|key| {
                completion_cell_has_response(content, key)
                    || committed_content.is_some_and(|head| completion_cell_has_response(head, key))
            });
        let history = recover(claim);
        let completed = has_outbox_receipt
            || matches!(
                &history,
                HistoryResponse::Recovered(_) | HistoryResponse::Ambiguous
            );
        if !completed {
            waiting.push(item);
            continue;
        }
        item.lifecycle = agent_doc_queue::queue_sync_payload::QueueSyncLifecycle::ReportedCheckmark;
        item.operation = QueueSyncOperation::IntegrateClose;
        item.completion_response_receipt = has_outbox_receipt;
        let committed_queue_response = key.as_deref().and_then(|key| {
            committed_content.and_then(|head| committed_queue_response_for_claim(head, key, claim))
        });
        apply_completed_response_evidence(
            &mut item,
            &root_exchange,
            outbox_response,
            history,
            committed_queue_response,
        );
        payload.reported_checkmark.push(item);
    }
    payload.running = waiting;
}

fn apply_completed_response_evidence(
    item: &mut agent_doc_queue::queue_sync_payload::QueueSyncItem,
    root_exchange: &str,
    outbox_response: Option<String>,
    history: HistoryResponse,
    committed_queue_response: Option<CommittedQueueResponse>,
) {
    match committed_queue_response {
        Some(CommittedQueueResponse::Terminal) => {
            item.response_evidence = QueueSyncResponseEvidence::CommittedQueuePrompt;
            item.recovered_response = None;
            item.missing_response_debt = false;
            item.operation = QueueSyncOperation::IntegrateClose;
            return;
        }
        Some(CommittedQueueResponse::Blocked) => {
            item.response_evidence = QueueSyncResponseEvidence::CommittedQueuePromptBlocked;
            item.recovered_response = None;
            item.missing_response_debt = false;
            item.lifecycle = agent_doc_queue::queue_sync_payload::QueueSyncLifecycle::OperatorGated;
            item.operation = QueueSyncOperation::OperatorGate;
            return;
        }
        Some(CommittedQueueResponse::Ambiguous) | None => {}
    }
    if let Some(response) = outbox_response {
        if response_is_consolidated(root_exchange, &response) {
            item.response_evidence = QueueSyncResponseEvidence::ConsolidatedExchange;
            item.recovered_response = None;
        } else {
            item.response_evidence = QueueSyncResponseEvidence::ExchangeOutbox;
            item.recovered_response = Some(response);
        }
        item.missing_response_debt = false;
        return;
    }
    match history {
        HistoryResponse::Recovered(response) => {
            if response_is_consolidated(root_exchange, &response) {
                item.response_evidence = QueueSyncResponseEvidence::ConsolidatedExchange;
                item.recovered_response = None;
            } else {
                item.response_evidence = QueueSyncResponseEvidence::AgentHistory;
                item.recovered_response = Some(response);
            }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommittedQueueResponse {
    Terminal,
    Blocked,
    Ambiguous,
}

fn committed_queue_response_for_claim(
    committed_content: &str,
    completion_key: &str,
    claim: &agent_doc_queue::queue_claim::QueueClaim,
) -> Option<CommittedQueueResponse> {
    let marker = agent_doc_document::subagent_completion::marker(completion_key);
    let marker_end = committed_content.rfind(&marker)? + marker.len();
    let responses =
        committed_queue_prompt_responses_after_completion(&committed_content[marker_end..]);
    responses
        .get(&normalize_response_comparison(&claim.item_text))
        .copied()
}

/// Derive exact prompt -> coordinator outcome evidence after the current
/// attempt's generation-specific completion marker in committed exchange
/// order. The marker is both the generation fence and the lower time bound;
/// this parser supplies only the missing root-delivery association after a
/// retained-state/controller timeout.
fn committed_queue_prompt_responses_after_completion(
    content: &str,
) -> std::collections::HashMap<String, CommittedQueueResponse> {
    let exchange = agent_doc_document::subagent_completion::strip_completion_cells(content);
    committed_queue_prompt_responses_from_exchange(&exchange)
}

fn committed_queue_prompt_responses_from_exchange(
    exchange: &str,
) -> std::collections::HashMap<String, CommittedQueueResponse> {
    let mut responses = std::collections::HashMap::new();
    for node in agent_doc_markdown_ast::exchange_tree::parse_exchange_nodes(exchange) {
        if !matches!(
            node.kind,
            agent_doc_markdown_ast::exchange_tree::ExchangeNodeKind::Response { .. }
        ) {
            continue;
        }
        let lines = node.lines;
        for (index, line) in lines.iter().enumerate() {
            let Some(prompt) = queue_prompt_echo(line) else {
                continue;
            };
            let end = lines[index + 1..]
                .iter()
                .position(|candidate| queue_prompt_echo(candidate).is_some())
                .map_or(lines.len(), |offset| index + 1 + offset);
            let Some(outcome) = classify_committed_queue_response(&lines[index + 1..end]) else {
                continue;
            };
            let key = normalize_response_comparison(prompt);
            if !key.is_empty() {
                merge_committed_queue_response(&mut responses, key, outcome);
            }
        }
    }
    responses
}

fn merge_committed_queue_response(
    responses: &mut std::collections::HashMap<String, CommittedQueueResponse>,
    key: String,
    outcome: CommittedQueueResponse,
) {
    responses
        .entry(key)
        .and_modify(|existing| {
            if *existing != outcome {
                *existing = CommittedQueueResponse::Ambiguous;
            }
        })
        .or_insert(outcome);
}

fn queue_prompt_echo(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let line = line.strip_prefix('>').unwrap_or(line).trim_start();
    line.strip_prefix("**Queue prompt:** ").map(str::trim)
}

fn classify_committed_queue_response(lines: &[String]) -> Option<CommittedQueueResponse> {
    let first = lines
        .iter()
        .map(|line| {
            let line = line.trim_start();
            line.strip_prefix('>').unwrap_or(line).trim()
        })
        .find(|line| !line.is_empty() && !line.starts_with("<!--"))?;
    let lower = first.to_ascii_lowercase();
    if lower.starts_with("**deferred:**")
        || lower.starts_with("**blocked:**")
        || lower.starts_with("**pending:**")
        || lower.starts_with("**waiting:**")
    {
        return Some(CommittedQueueResponse::Blocked);
    }
    let outcome = lower.strip_prefix("**outcome:**")?;
    if ["blocked", "deferred", "pending", "waiting", "incomplete"]
        .iter()
        .any(|marker| outcome.trim_start().starts_with(marker))
    {
        Some(CommittedQueueResponse::Blocked)
    } else if outcome.trim().is_empty() {
        None
    } else {
        Some(CommittedQueueResponse::Terminal)
    }
}

fn durable_completion_response(
    claim: &agent_doc_queue::queue_claim::QueueClaim,
    key: &str,
    _content: &str,
    durable_receipts: &[agent_doc_queue::claim_release_receipt::ClaimReleaseReceipt],
    _retained_capture_targets: &[String],
) -> Option<String> {
    durable_receipts
        .iter()
        .find(|receipt| {
            receipt.identity == claim.identity
                && receipt.generation == claim.generation
                && receipt.worker_id
                    == claim.worker.as_ref().map(|worker| worker.worker_id.clone())
                && receipt.completion_key.as_deref() == Some(key)
        })
        .and_then(|receipt| receipt.response.as_ref())
        .filter(|response| response.is_valid() && response.response_id == key)
        .map(|response| response.body.clone())
}

fn matching_response_capture(
    claim: &agent_doc_queue::queue_claim::QueueClaim,
    key: Option<&str>,
    durable_receipts: &[agent_doc_queue::claim_release_receipt::ClaimReleaseReceipt],
) -> Option<String> {
    let key = key?;
    durable_receipts
        .iter()
        .find(|receipt| {
            receipt.identity == claim.identity
                && receipt.generation == claim.generation
                && receipt.worker_id
                    == claim.worker.as_ref().map(|worker| worker.worker_id.clone())
                && receipt.completion_key.as_deref() == Some(key)
        })
        .and_then(|receipt| receipt.response.as_ref())
        .filter(|response| response.is_valid() && response.response_id == key)
        .map(|response| response.response_sha256.clone())
}

fn root_authored_exchange(content: &str) -> String {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return String::new();
    };
    components
        .iter()
        .filter(|component| component.name == "exchange")
        .flat_map(|component| {
            let exchange = agent_doc_document::subagent_completion::strip_completion_cells(
                component.content(content),
            );
            agent_doc_markdown_ast::exchange_tree::parse_exchange_nodes(&exchange)
                .into_iter()
                .filter_map(|node| {
                    matches!(
                        node.kind,
                        agent_doc_markdown_ast::exchange_tree::ExchangeNodeKind::Response { .. }
                    )
                    .then(|| node.render())
                })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn response_is_consolidated(root_exchange: &str, response: &str) -> bool {
    let response = normalize_response_comparison(response);
    !response.is_empty() && normalize_response_comparison(root_exchange).contains(&response)
}

fn normalize_response_comparison(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.trim_start()
                .strip_prefix('>')
                .unwrap_or(line.trim_start())
                .trim_start()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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

fn recover_agent_history_response_from(
    home: &Path,
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
    match harness {
        "claude" | "claude-code" => recover_claude_history_response(home, session_id, worker_id),
        "codex" => {
            if claim.generation == 0 {
                return HistoryResponse::Unsupported;
            }
            let Ok(attempt_completion_key) = crate::queue_claim::completion_feedback_key(claim)
            else {
                return HistoryResponse::Unsupported;
            };
            recover_codex_history_response(
                home,
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
            .enumerate()
            .filter(|(event_index, event)| match event.kind {
                QueueLineageEventKind::Committed | QueueLineageEventKind::Pushed => true,
                QueueLineageEventKind::Integrated => {
                    crate::queue_lineage::stored_integration_is_verified(attempt, *event_index)
                }
                QueueLineageEventKind::Released => (0..*event_index).any(|index| {
                    crate::queue_lineage::stored_integration_is_verified(attempt, index)
                }),
                _ => false,
            })
            .map(|(_, event)| QueueSyncEffectEvidence {
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
                sink_ref: event.sink_ref.clone(),
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
    fn lineage_is_the_retry_fence_and_recovers_effect_receipts() {
        let created = crate::queue_lineage::QueueLineageEvent {
            kind: crate::queue_lineage::QueueLineageEventKind::Created,
            ..Default::default()
        };
        let mut unstarted = lost_payload();
        apply_lineage_evidence(&mut unstarted, &lineage_with(vec![created.clone()]));
        assert!(unstarted.lost[0].retry_safe);
        assert_eq!(
            unstarted.lost[0].operation,
            QueueSyncOperation::RecoverRequeue
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
    }

    #[test]
    fn legacy_unverifiable_integration_is_not_recovered_as_effect() {
        let mut payload = lost_payload();
        let created = crate::queue_lineage::QueueLineageEvent {
            kind: crate::queue_lineage::QueueLineageEventKind::Created,
            ..Default::default()
        };
        let integrated = crate::queue_lineage::QueueLineageEvent {
            kind: crate::queue_lineage::QueueLineageEventKind::Integrated,
            source_sha: Some("legacy-source".to_string()),
            integration_sha: Some("legacy-integration".to_string()),
            integration_method: Some(crate::queue_lineage::IntegrationMethod::Other),
            ..Default::default()
        };
        let released = crate::queue_lineage::QueueLineageEvent {
            kind: crate::queue_lineage::QueueLineageEventKind::Released,
            release_tag: Some("v0.0.0-legacy".to_string()),
            release_state: Some(crate::queue_lineage::ReleaseState::Published),
            ..Default::default()
        };

        apply_lineage_evidence(
            &mut payload,
            &lineage_with(vec![created, integrated, released]),
        );

        assert_eq!(
            payload.lost[0].execution_evidence,
            QueueSyncExecutionEvidence::Started
        );
        assert!(payload.lost[0].recovered_effects.is_empty());
    }

    #[test]
    fn released_recovered_completion_preserves_loss_and_receipt_without_claiming_delivery() {
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
        crate::claim_release_receipt::capture_response_for_attempt(
            &doc,
            "recovered worker task",
            claimed.generation,
            "Recovered exact worker response.",
        )
        .unwrap();
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
        assert!(item.completion_response_receipt);
        assert_eq!(
            item.recovered_response.as_deref(),
            Some("Recovered exact worker response.")
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

    fn reported_claim(
        item: &str,
        owner: &str,
        worker: &str,
        parent: &str,
    ) -> agent_doc_queue::queue_claim::QueueClaim {
        let mut ledger = agent_doc_queue::queue_claim::QueueClaimLedger::default();
        ledger.claim(item, owner, 10, 600);
        let mut claim = ledger.claims.remove(0);
        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Reported;
        claim.worker = Some(agent_doc_queue::queue_claim::QueueWorkerBinding {
            worker_id: worker.to_string(),
            started_at_secs: 11,
        });
        claim.owner_session = Some(agent_doc_queue::queue_claim::ClaimOwnerSession {
            harness: Some("codex".to_string()),
            session_id: Some(parent.to_string()),
            ..Default::default()
        });
        claim
    }

    #[test]
    fn replacement_coordinator_recovers_multiple_results_and_replays_each_exactly_once() {
        let home = tempfile::tempdir().unwrap();
        let first = reported_claim("first child", "subagent:first", "worker-first", "parent");
        let second = reported_claim("second child", "subagent:second", "worker-second", "parent");
        write_codex_rollout(
            home.path(),
            "one",
            "parent",
            "worker-first",
            &[(
                "turn-first",
                "Full first child result with implementation details.",
            )],
        );
        write_codex_rollout(
            home.path(),
            "two",
            "parent",
            "worker-second",
            &[(
                "turn-second",
                "Full second child result with verification details.",
            )],
        );
        let first_receipt = agent_doc_document::subagent_completion::cell_with_response(
            &first.owner,
            &first.item_text,
            &crate::queue_claim::completion_feedback_key(&first).unwrap(),
            "Completed first worker.",
        )
        .unwrap();
        let second_receipt = agent_doc_document::subagent_completion::cell_with_response(
            &second.owner,
            &second.item_text,
            &crate::queue_claim::completion_feedback_key(&second).unwrap(),
            "Completed second worker.",
        )
        .unwrap();
        let content = format!(
            "<!-- agent:exchange -->\n❯ Full first child result with implementation details.\n\n### Re: retained parent synthesis — agent\nFull second child result with verification details.\n\n{first_receipt}\n\n{second_receipt}\n<!-- /agent:exchange -->\n<!-- agent:run id=run subagents -->\n- ☑️ first child\n- ☑️ second child\n<!-- /agent:run -->\n"
        );
        let claims = vec![first, second];
        let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            "/tmp/replacement.md",
            QueueSyncDocumentScope::Current,
            &content,
            None,
            &claims,
        )
        .unwrap();

        apply_response_evidence_with(
            &mut payload,
            &content,
            Some(&content),
            &claims,
            &[],
            &[],
            |claim| recover_agent_history_response_from(home.path(), claim),
        );

        assert_eq!(payload.reported_checkmark.len(), 2);
        let first = payload
            .reported_checkmark
            .iter()
            .find(|item| item.item.contains("first child"))
            .unwrap();
        assert!(first.completion_response_receipt);
        assert_eq!(
            first.response_evidence,
            QueueSyncResponseEvidence::AgentHistory
        );
        assert_eq!(
            first.recovered_response.as_deref(),
            Some("Full first child result with implementation details.")
        );
        assert!(!first.missing_response_debt);

        let second = payload
            .reported_checkmark
            .iter()
            .find(|item| item.item.contains("second child"))
            .unwrap();
        assert!(second.completion_response_receipt);
        assert_eq!(
            second.response_evidence,
            QueueSyncResponseEvidence::ConsolidatedExchange
        );
        assert_eq!(second.recovered_response, None);
        assert!(!second.missing_response_debt);
    }

    #[test]
    fn terminal_child_history_recovers_interruption_before_parent_report_receipt() {
        let home = tempfile::tempdir().unwrap();
        let mut claim = reported_claim(
            "finished before receipt",
            "subagent:interrupted",
            "worker-interrupted",
            "parent",
        );
        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Running;
        write_codex_rollout(
            home.path(),
            "interrupted",
            "parent",
            "worker-interrupted",
            &[("turn", "Terminal result retained only in child history.")],
        );
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: parent status — agent\nThe worker completed.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:run id=run subagents -->\n",
            "- 🚧 finished before receipt\n",
            "<!-- /agent:run -->\n",
        );
        let claims = vec![claim];
        let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            "/tmp/interrupted.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &claims,
        )
        .unwrap();
        assert_eq!(payload.running.len(), 1);

        apply_response_evidence_with(
            &mut payload,
            content,
            Some(content),
            &claims,
            &[],
            &[],
            |claim| recover_agent_history_response_from(home.path(), claim),
        );

        assert!(payload.running.is_empty());
        assert_eq!(payload.reported_checkmark.len(), 1);
        let item = &payload.reported_checkmark[0];
        assert_eq!(
            item.lifecycle,
            agent_doc_queue::queue_sync_payload::QueueSyncLifecycle::ReportedCheckmark
        );
        assert_eq!(item.operation, QueueSyncOperation::IntegrateClose);
        assert_eq!(
            item.recovered_response.as_deref(),
            Some("Terminal result retained only in child history.")
        );
        assert!(!item.completion_response_receipt);
    }

    #[test]
    fn durable_capture_recovers_reported_projection_interruption_without_history() {
        let claim = reported_claim(
            "captured before projection",
            "subagent:captured",
            "worker-captured",
            "parent",
        );
        let key = crate::queue_claim::completion_feedback_key(&claim).unwrap();
        let mut receipts =
            agent_doc_queue::claim_release_receipt::ClaimReleaseReceiptLedger::default();
        receipts
            .record_response_capture(
                &claim,
                key.clone(),
                "Exact full response retained before editor projection.",
                20,
                600,
            )
            .unwrap();
        let content = concat!(
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
            "<!-- agent:run id=run subagents -->\n",
            "- ☑️ captured before projection\n",
            "<!-- /agent:run -->\n",
        );
        let claims = vec![claim];
        let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            "/tmp/captured.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &claims,
        )
        .unwrap();

        apply_response_evidence_with(
            &mut payload,
            content,
            Some(content),
            &claims,
            &receipts.receipts,
            &[],
            |_| HistoryResponse::Missing,
        );

        let item = &payload.reported_checkmark[0];
        assert_eq!(item.response_id.as_deref(), Some(key.as_str()));
        assert!(item.response_capture_sha256.is_some());
        assert_eq!(
            item.recovered_response.as_deref(),
            Some("Exact full response retained before editor projection.")
        );
        assert_eq!(
            item.response_evidence,
            QueueSyncResponseEvidence::ExchangeOutbox
        );
        assert!(!item.missing_response_debt);
    }

    #[test]
    fn abbreviated_receipt_does_not_hide_unsupported_or_ambiguous_history_debt() {
        let claim = reported_claim("receipt only", "subagent:receipt", "worker", "parent");
        let receipt = agent_doc_document::subagent_completion::cell_with_response(
            &claim.owner,
            &claim.item_text,
            &crate::queue_claim::completion_feedback_key(&claim).unwrap(),
            "Done.",
        )
        .unwrap();
        let content = format!(
            "<!-- agent:exchange -->\n{receipt}\n<!-- /agent:exchange -->\n<!-- agent:run id=run subagents -->\n- ☑️ receipt only\n<!-- /agent:run -->\n"
        );
        let claims = vec![claim];
        for (history, expected) in [
            (
                HistoryResponse::Unsupported,
                QueueSyncResponseEvidence::UnsupportedHistory,
            ),
            (
                HistoryResponse::Ambiguous,
                QueueSyncResponseEvidence::AmbiguousHistory,
            ),
        ] {
            let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
                "/tmp/debt.md",
                QueueSyncDocumentScope::Current,
                &content,
                None,
                &claims,
            )
            .unwrap();
            apply_response_evidence_with(
                &mut payload,
                &content,
                Some(&content),
                &claims,
                &[],
                &[],
                |_| match &history {
                    HistoryResponse::Unsupported => HistoryResponse::Unsupported,
                    HistoryResponse::Ambiguous => HistoryResponse::Ambiguous,
                    _ => unreachable!(),
                },
            );
            let item = &payload.reported_checkmark[0];
            assert!(item.completion_response_receipt);
            assert!(item.missing_response_debt);
            assert_eq!(item.operation, QueueSyncOperation::Reconcile);
            assert_eq!(item.response_evidence, expected);
        }
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

        apply_response_evidence(&mut payload, content, None, &ledger.claims, &[], &[]);
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
    fn committed_queue_prompt_receipts_close_terminal_outcomes_but_keep_deferred_live() {
        let terminal_prompt = concat!(
            "Waiting on you — answer by adding a child line under an item ",
            "<!-- agent-doc:waiting-on-you -->: #done: Complete the remaining work for the parent item."
        );
        let blocked_prompt = concat!(
            "Waiting on you — answer by adding a child line under an item ",
            "<!-- agent-doc:waiting-on-you -->: #blocked: Complete the remaining work for the parent item."
        );
        let terminal = reported_claim(terminal_prompt, "subagent:done", "worker-done", "parent");
        let blocked = reported_claim(
            blocked_prompt,
            "subagent:blocked",
            "worker-blocked",
            "parent",
        );
        let terminal_cell = agent_doc_document::subagent_completion::cell_with_response(
            &terminal.owner,
            &terminal.item_text,
            &crate::queue_claim::completion_feedback_key(&terminal).unwrap(),
            "Done.",
        )
        .unwrap();
        let blocked_cell = agent_doc_document::subagent_completion::cell_with_response(
            &blocked.owner,
            &blocked.item_text,
            &crate::queue_claim::completion_feedback_key(&blocked).unwrap(),
            "Blocked.",
        )
        .unwrap();
        let content = format!(
            concat!(
                "<!-- agent:exchange -->\n",
                "{terminal_cell}\n\n{blocked_cell}\n\n",
                "### Re: recovered coordinator closeout — agent\n\n",
                "> **Queue prompt:** {terminal_prompt}\n\n",
                "**Outcome:** Complete and integrated. No operator decision remains.\n\n",
                "> **Queue prompt:** {blocked_prompt}\n\n",
                "**Deferred:** A live operator verification is still required.\n\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:run id=run subagents -->\n",
                "- ☑️ {terminal_prompt}\n",
                "- ☑️ {blocked_prompt}\n",
                "<!-- /agent:run -->\n",
            ),
            terminal_prompt = terminal_prompt,
            blocked_prompt = blocked_prompt,
            terminal_cell = terminal_cell,
            blocked_cell = blocked_cell,
        );
        // Model a retained-state/controller timeout whose live projection lost
        // the completion cells even though committed HEAD kept both cells and
        // the coordinator's exact prompt/outcome associations.
        let current = agent_doc_document::subagent_completion::strip_completion_cells(&content);
        let claims = vec![terminal, blocked];
        let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            "/tmp/committed-closeout.md",
            QueueSyncDocumentScope::Current,
            &current,
            None,
            &claims,
        )
        .unwrap();

        apply_response_evidence_with(
            &mut payload,
            &current,
            Some(&content),
            &claims,
            &[],
            &[],
            |_| HistoryResponse::Missing,
        );

        let terminal = payload
            .reported_checkmark
            .iter()
            .find(|item| item.item.contains("#done"))
            .unwrap();
        assert_eq!(terminal.operation, QueueSyncOperation::IntegrateClose);
        assert_eq!(
            terminal.response_evidence,
            QueueSyncResponseEvidence::CommittedQueuePrompt
        );
        assert!(!terminal.missing_response_debt);

        let blocked = payload
            .reported_checkmark
            .iter()
            .find(|item| item.item.contains("#blocked"))
            .unwrap();
        assert_eq!(blocked.operation, QueueSyncOperation::OperatorGate);
        assert_eq!(
            blocked.lifecycle,
            agent_doc_queue::queue_sync_payload::QueueSyncLifecycle::OperatorGated
        );
        assert_eq!(
            blocked.response_evidence,
            QueueSyncResponseEvidence::CommittedQueuePromptBlocked
        );
        assert!(!blocked.missing_response_debt);
    }

    #[test]
    fn queue_prompt_echo_without_committed_or_completion_proof_never_closes() {
        let prompt = "reported work";
        let claim = reported_claim(prompt, "subagent:reported", "worker", "parent");
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: uncommitted synthesis — agent\n\n",
            "> **Queue prompt:** reported work\n\n",
            "**Outcome:** Complete.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:run id=run subagents -->\n",
            "- ☑️ reported work\n",
            "<!-- /agent:run -->\n",
        );
        let claims = vec![claim];
        let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            "/tmp/uncommitted-closeout.md",
            QueueSyncDocumentScope::Current,
            content,
            None,
            &claims,
        )
        .unwrap();

        apply_response_evidence_with(
            &mut payload,
            content,
            Some(content),
            &claims,
            &[],
            &[],
            |_| HistoryResponse::Missing,
        );

        let item = &payload.reported_checkmark[0];
        assert_eq!(item.operation, QueueSyncOperation::Reconcile);
        assert_eq!(item.response_evidence, QueueSyncResponseEvidence::Missing);
        assert!(item.missing_response_debt);
    }

    #[test]
    fn queue_prompt_echo_before_current_completion_marker_never_closes() {
        let prompt = "repeated reported work";
        let claim = reported_claim(prompt, "subagent:reported", "worker", "parent");
        let cell = agent_doc_document::subagent_completion::cell_with_response(
            &claim.owner,
            &claim.item_text,
            &crate::queue_claim::completion_feedback_key(&claim).unwrap(),
            "New generation returned a different result.",
        )
        .unwrap();
        let content = format!(
            concat!(
                "<!-- agent:exchange -->\n",
                "### Re: stale coordinator synthesis — agent\n\n",
                "> **Queue prompt:** {prompt}\n\n",
                "**Outcome:** Complete.\n\n",
                "{cell}\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:run id=run subagents -->\n",
                "- ☑️ {prompt}\n",
                "<!-- /agent:run -->\n",
            ),
            prompt = prompt,
            cell = cell,
        );
        let current = agent_doc_document::subagent_completion::strip_completion_cells(&content);
        let claims = vec![claim];
        let mut payload = agent_doc_queue::queue_sync_payload::project_document_queue_sync(
            "/tmp/stale-closeout.md",
            QueueSyncDocumentScope::Current,
            &current,
            None,
            &claims,
        )
        .unwrap();

        apply_response_evidence_with(
            &mut payload,
            &current,
            Some(&content),
            &claims,
            &[],
            &[],
            |_| HistoryResponse::Missing,
        );

        let item = &payload.reported_checkmark[0];
        assert_eq!(item.operation, QueueSyncOperation::Reconcile);
        assert_eq!(item.response_evidence, QueueSyncResponseEvidence::Missing);
        assert!(item.missing_response_debt);
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

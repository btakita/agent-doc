//! # Module: queue_cmd
//!
//! CLI subcommands for managing the `agent:queue` component.
//!
//! - `agent-doc queue sync <FILE>` — declaratively reconcile queue lifecycle,
//!   captured answered free-text residue, and configured backlog mirrors.
//! - `agent-doc queue consume <FILE> [--count N]` — explicitly strike the
//!   leading N free-text queue head(s) the agent has already answered.
//! - `agent-doc queue consume <FILE> --id <id>` — escape hatch (#orphanqhead)
//!   that strikes an orphaned id-backed head whose backing backlog item was
//!   already reaped (or is gone), so it stops re-firing the auto-loop.
//! - `agent-doc queue consume <FILE> --ack-id <id>` — explicit acknowledgement
//!   (#freshqueueauth) for one top-level bare id-backed correction head, or
//!   report-proven closeout of runnable work, while leaving the tracked item
//!   unresolved. Supplying the flag is never acknowledgement provenance.
//! - `agent-doc queue consume <FILE> --ack-text <text>` — exact acknowledgement
//!   for a completed free-text head, including a unique active nested lineage.
//! - `queue claim` is an admission transaction: the harness receives spawn
//!   permission only after the durable claim and visible `Claimed` projection
//!   agree. Projection refusal compensates the exact unstarted generation.
//!   Admission/start/release project only that work identity across its queue
//!   displays; global lifecycle/Active reconciliation belongs to explicit sync.

use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use crate::one_shot_sync::OneShotQueueSyncResult;
use agent_doc_element_backlog::backlog;
use agent_doc_queue::document_queue::QueueEntry;
use agent_doc_queue::queue_heads::active_queue_head_text;
use agent_doc_queue::queue_heads::{ActiveQueueHeadKind, classify_active_queue_head};
use agent_doc_queue::queue_sync_payload::{
    QUEUE_SYNC_GUIDANCE, QueueSyncPayload, QueueSyncRecoveryResponse,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct ConsumeOptions {
    pub force_disk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueCommandConsumeOutcome {
    pub consumed_text: String,
    pub consumed_count: usize,
    pub remaining: usize,
    pub drained: bool,
}

fn queue_sync_reconciliation_payload(
    file: &Path,
    content: &str,
    previous_content: Option<&str>,
) -> Result<QueueSyncPayload> {
    crate::sync_payload::build_project_queue_sync_payload(file, content, previous_content)
}

/// Document-generic declarative sync inventory. Project scanners can call this
/// same engine with the authoritative document cut they discovered.
#[derive(Debug, Clone, Serialize)]
pub struct QueueSyncReconciliationPlan {
    pub document_path: String,
    pub editor_authority: &'static str,
    pub retained_projection_pending: bool,
    pub payload: QueueSyncPayload,
    pub recovery_responses: Vec<QueueSyncRecoveryResponse>,
}

pub fn build_queue_sync_reconciliation_plan(
    file: &Path,
    content: &str,
    previous_content: Option<&str>,
) -> Result<QueueSyncReconciliationPlan> {
    let payload = queue_sync_reconciliation_payload(file, content, previous_content)?;
    let retained_projection_pending = agent_doc_cycle_state_io::load_document_projection(file)?
        .is_some_and(|projection| {
            projection.document.pending_write.is_some()
                || !projection.document.pending_write_journal.is_empty()
        });
    let recovery_responses = payload.recovery_responses();
    Ok(QueueSyncReconciliationPlan {
        document_path: file.display().to_string(),
        editor_authority: "current_editor_or_disk_authority_cut",
        retained_projection_pending,
        payload,
        recovery_responses,
    })
}

fn print_queue_sync_plan(plan: &QueueSyncReconciliationPlan) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(plan)?);
    for recovery in &plan.recovery_responses {
        println!("\n{}", recovery.render_markdown());
    }
    if !plan.payload.is_empty() {
        println!("{QUEUE_SYNC_GUIDANCE}");
    }
    Ok(())
}

pub trait QueueCommandEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<()>;

    fn converge_document_or_disk_for_generation(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
        generation: u64,
    ) -> Result<bool> {
        Ok(matches!(
            agent_doc_controller_io::project_controller::converge_queue_projection_for_generation(
                file,
                target_content,
                source_content,
                reason,
                generation,
                agent_doc_controller_io::project_controller::QueueProjectionWriter::LifecycleOwner,
            )?,
            agent_doc_controller_io::project_controller::QueueProjectionGenerationWriteOutcome::Applied
        ))
    }

    fn consume_free_text_queue_prompts_force_disk(
        &self,
        file: &Path,
        count: usize,
    ) -> Result<Option<QueueCommandConsumeOutcome>>;
    fn consume_free_text_queue_prompts_with_outcome(
        &self,
        file: &Path,
        count: usize,
    ) -> Result<Option<QueueCommandConsumeOutcome>>;
    fn strike_orphan_id_backed_queue_head(&self, file: &Path, id: &str) -> Result<bool>;
    fn acknowledge_open_id_backed_queue_head(&self, file: &Path, id: &str) -> Result<bool>;
    fn acknowledge_paused_free_text_queue_head(
        &self,
        file: &Path,
        expected_head: &str,
    ) -> Result<Option<QueueCommandConsumeOutcome>>;
    fn acknowledge_paused_free_text_queue_head_with_selector(
        &self,
        file: &Path,
        expected_head: &str,
        selector: Option<&crate::queue_consume::AckTextSelector>,
    ) -> Result<Option<QueueCommandConsumeOutcome>> {
        if selector.is_some() {
            anyhow::bail!("this queue command effect does not support --ack-text selectors");
        }
        self.acknowledge_paused_free_text_queue_head(file, expected_head)
    }
    fn prune_noise_queue_heads(&self, file: &Path) -> Result<usize>;
    fn converge_captured_answered_free_text_projection(&self, _file: &Path) -> Result<usize> {
        Ok(0)
    }

    /// See [`QueueRuntimeProjectionEffects::settle_before_projection_retry`].
    fn settle_before_projection_retry(&self, _file: &Path) {}
}

/// The narrow effect boundary for re-projecting derived queue runtime state.
///
/// Tracked-work mutations need this without also gaining authority to consume,
/// acknowledge, or prune queue heads. Keeping the projection effect separate
/// lets closeout repaint blockers from the newly authoritative document cut
/// while the queue command surface retains its stronger capabilities.
pub trait QueueRuntimeProjectionEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<()>;

    fn converge_document_or_disk_for_generation(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
        _generation: u64,
    ) -> Result<bool> {
        self.converge_document_or_disk(file, target_content, source_content, reason)?;
        Ok(true)
    }

    /// Let an in-flight CRDT delivery settle before a projection retries a
    /// `retry_crdt_merge` CAS refusal (`#claimprojectionretry`). The default is
    /// actorless: the retry's fresh authority read is the only observation.
    fn settle_before_projection_retry(&self, _file: &Path) {}
}

impl<T: QueueCommandEffects + ?Sized> QueueRuntimeProjectionEffects for T {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String> {
        QueueCommandEffects::current_document_content(self, file, source)
    }

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<()> {
        QueueCommandEffects::converge_document_or_disk(
            self,
            file,
            target_content,
            source_content,
            reason,
        )
    }

    fn converge_document_or_disk_for_generation(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
        generation: u64,
    ) -> Result<bool> {
        QueueCommandEffects::converge_document_or_disk_for_generation(
            self,
            file,
            target_content,
            source_content,
            reason,
            generation,
        )
    }

    fn settle_before_projection_retry(&self, file: &Path) {
        QueueCommandEffects::settle_before_projection_retry(self, file)
    }
}

/// Pure document successor derived from the durable claim-state ledger.
/// Callers that also publish another binary-owned document effect can compose
/// that effect onto `target_content` and land one CAS successor instead of
/// racing two retained editor writes.
#[derive(Debug, Clone)]
pub struct ClaimRuntimeProjection {
    pub target_content: String,
    pub persisted_heads: Vec<String>,
    pub changed: bool,
}

const CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS: usize = 8;

static QUEUE_INVARIANT_LOG_DEDUPE: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn log_queue_invariant_once(
    file: &Path,
    claim: &agent_doc_queue::queue_claim::QueueClaim,
    diagnosis: agent_doc_queue::queue_claim::QueueLifecycleDiagnosis,
) {
    let signature = format!(
        "{}:{:?}:{}:{:?}",
        agent_doc_hash::document_id_for_path(file),
        claim.identity,
        claim.generation,
        diagnosis.invariant,
    );
    if !QUEUE_INVARIANT_LOG_DEDUPE.lock().insert(signature.clone()) {
        return;
    }
    let key = format!("qinv{}", agent_doc_hash::short_content_hash(&signature));
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{} key={} invariant={:?} owner={} generation={} expected={} observed={} worker_liveness={:?} correction={:?}",
            agent_doc_turn::op_log::OpsLogEvent::QueueLifecycleInvariant,
            key,
            diagnosis.invariant,
            claim.owner,
            claim.generation,
            diagnosis.expected.prefix(),
            diagnosis
                .observed
                .map(|marker| marker.prefix())
                .unwrap_or("bare"),
            diagnosis.worker_liveness,
            diagnosis.correction,
        ),
    );
}

/// Which queues a claim runtime projection rewrites (`#queuemirrormarkers`).
#[derive(Debug, Clone, Copy)]
pub enum ClaimProjectionScope<'a> {
    /// Every queue in the document. In-session `🚧` selection comes from the
    /// persisted preflight cycle state. This is the claim command path.
    Document,
    /// Every queue except `projected_queue_id`, which the caller (preflight
    /// maintenance) already projected with pass-local evidence.
    /// `selected_heads` are that pass's in-session selected head texts, so a
    /// sibling copy of a selected head shows the same `🚧` instead of
    /// flip-flopping against the caller's own projection.
    SiblingsOf {
        projected_queue_id: &'a str,
        selected_heads: &'a [String],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeProjectionRows {
    All,
    /// A claim-ledger transition must not take ownership of a newly typed,
    /// unrelated bare row merely to add the cosmetic pending marker. The
    /// transition still repaints rows with durable/runtime evidence and rows
    /// that already carry a lifecycle marker.
    LifecycleEvidence,
    BlockedOrPreviouslyBlocked,
}

pub fn derive_claim_runtime_projection(
    file: &Path,
    current: &str,
    cleared_item: Option<&str>,
) -> Result<ClaimRuntimeProjection> {
    derive_claim_runtime_projection_scoped(
        file,
        current,
        cleared_item,
        ClaimProjectionScope::Document,
    )
}

/// Derive the visible lifecycle projection of the durable claim, report, and
/// completion state for every queue row in `scope`.
///
/// Claims are keyed by work identity, not by queue id, so every queue that
/// displays an identity shows the same authoritative state
/// (`#queuemirrormarkers`). Only the scheduler-selected queue may reopen a
/// struck row on durable live evidence; a sibling's struck row is its own
/// terminal or pass-owned evidence and is never resurrected by another
/// queue's claim.
pub fn derive_claim_runtime_projection_scoped(
    file: &Path,
    current: &str,
    cleared_item: Option<&str>,
    scope: ClaimProjectionScope<'_>,
) -> Result<ClaimRuntimeProjection> {
    derive_runtime_projection_scoped(
        file,
        current,
        cleared_item,
        scope,
        RuntimeProjectionRows::All,
        None,
    )
}

fn derive_blocker_runtime_projection(file: &Path, current: &str) -> Result<ClaimRuntimeProjection> {
    derive_runtime_projection_scoped(
        file,
        current,
        None,
        ClaimProjectionScope::Document,
        RuntimeProjectionRows::BlockedOrPreviouslyBlocked,
        None,
    )
}

fn derive_runtime_projection_scoped(
    file: &Path,
    current: &str,
    cleared_item: Option<&str>,
    scope: ClaimProjectionScope<'_>,
    rows: RuntimeProjectionRows,
    only_identity: Option<&agent_doc_element_queue::QueueItemIdentity>,
) -> Result<ClaimRuntimeProjection> {
    let input_current = current;
    let mut claim_ledger = crate::queue_claim::load_ledger(file);
    let repair_claims = claim_ledger
        .as_ref()
        .map(|ledger| {
            ledger
                .claims
                .iter()
                .filter(|claim| {
                    !ledger.cancellations.iter().any(|cancelled| {
                        cancelled.cancels_attempt(&claim.identity, claim.generation)
                    })
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let repaired_current =
        agent_doc_queue::queue_mirror_projection::restore_missing_nested_claim_rows_across_queues(
            current,
            &repair_claims,
        )?;
    let current = repaired_current.as_deref().unwrap_or(current);
    let components = agent_doc_element::element::parse(current)?;
    if !components.iter().any(|component| component.name == "queue") {
        return Ok(ClaimRuntimeProjection {
            target_content: input_current.to_string(),
            persisted_heads: Vec::new(),
            changed: false,
        });
    }
    if let Ok(ledger) = &mut claim_ledger
        && !ledger.claims.is_empty()
        && let Some(view) = agent_doc_queue::queue_claim::ClaimQueueView::from_content(current)
    {
        ledger.follow_edits_in_view(crate::queue_claim::claim_clock_now_secs(), &view);
    }
    let receipt_ledger = crate::claim_release_receipt::load(file);
    let state_degraded = claim_ledger.is_err() || receipt_ledger.is_err();
    if let Err(err) = &claim_ledger {
        eprintln!(
            "[queue] lifecycle projection degraded: cannot read claims for {}: {err:#}",
            file.display()
        );
    }
    if let Err(err) = &receipt_ledger {
        eprintln!(
            "[queue] lifecycle projection degraded: cannot read report outbox for {}: {err:#}",
            file.display()
        );
    }
    let now = crate::queue_claim::claim_clock_now_secs();
    let claim_ledger = claim_ledger.unwrap_or_default();
    let cancellations = claim_ledger.cancellations;
    let claim_states = claim_ledger
        .claims
        .into_iter()
        .map(|mut claim| {
            claim.project_expiry(now);
            (claim.identity.clone(), claim)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let receipts = receipt_ledger
        .unwrap_or_default()
        .receipts
        .into_iter()
        .filter(|receipt| {
            // `#todoeditrevivesrow`: a closed attempt is consumed work; it
            // never paints `☑️` onto a later row with its identity.
            !receipt.attempt_closed()
                && !cancellations.iter().any(|cancelled| {
                    cancelled.cancels_attempt(&receipt.identity, receipt.generation)
                })
        })
        .map(|receipt| (receipt.identity.clone(), receipt))
        .collect::<std::collections::HashMap<_, _>>();
    let completion_evidence = agent_doc_document::subagent_completion::completion_evidence(current)
        .into_iter()
        .filter(|evidence| {
            !cancellations
                .iter()
                .any(|cancelled| cancelled.cancels_completion_key(&evidence.key))
        })
        .map(|evidence| evidence.item)
        .collect::<Vec<_>>();
    let cleared_identity = cleared_item.map(agent_doc_queue::queue_claim::claim_identity);
    // `#queueblocked`: a row whose tracked item waits on open blockers paints
    // `⛔ ⟦blocked by ...⟧`, outranking a stale `⏭️`/`⏳` it may still carry.
    let blockers = agent_doc_queue::queue_blocked::queue_blockers(current);
    let (skip_queue_id, selected) = match scope {
        ClaimProjectionScope::Document => (
            None,
            agent_doc_cycle_state_io::load(file)
                .ok()
                .flatten()
                .filter(|state| {
                    state.queue_selection_authority
                        == agent_doc_cycle_state_io::QueueSelectionAuthority::Preflight
                        && !state.queue_selection_preempted
                })
                .map(|state| {
                    agent_doc_queue::queue_claim::ClaimedQueueItems::none()
                        .with_heads(&state.selected_free_text_queue_heads)
                })
                .unwrap_or_else(agent_doc_queue::queue_claim::ClaimedQueueItems::none),
        ),
        ClaimProjectionScope::SiblingsOf {
            projected_queue_id,
            selected_heads,
        } => (
            Some(projected_queue_id),
            agent_doc_queue::queue_claim::ClaimedQueueItems::none().with_heads(selected_heads),
        ),
    };

    // Build one target per queue occurrence. This retains marker cardinality
    // for duplicate heads while avoiding a union that could mark an unrelated
    // duplicate twice.
    let mut persisted_heads = Vec::new();
    let targets_for = |block: agent_doc_queue::queue_mirror_projection::QueueProjectionBlock<
        '_,
    >| {
        // `#strikeisterminal`, per queue: a struck row whose identity is also live in this
        // queue is an earlier attempt's record, never the claim's row.
        let targets = block
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| match entry {
                QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
                    if prompt.text.contains(
                        agent_doc_queue::queue_consume::WAITING_ON_YOU_ROOT_MARKER,
                    ) || agent_doc_queue::waiting_packet::is_agent_doc_reply_entry(
                        block.entries,
                        index,
                    ) {
                        return None;
                    }
                    // `#waitingitemclaimmarker`: a waiting-item head is never
                    // painted pending, selected, or blocked (`#waitingpacketowner`),
                    // but a real attempt on it (claim, reported receipt,
                    // completion evidence) still projects. Dropping the head
                    // here left a claimed answered item bare forever while
                    // `queue claims` expected `🟡`, so every dispatch was
                    // released and requeued (sdk-dev, 2026-11-04).
                    let waiting_item_head =
                        agent_doc_queue::waiting_packet::is_waiting_item_head(entry);
                    let struck = matches!(entry, QueueEntry::Completed(_));
                    // `#queuemirrormarkers`: a sibling queue's struck row is its
                    // own terminal (or `agent:loop` pass-owned) evidence.
                    // Another queue's live claim never reopens it.
                    if struck && !block.scheduler_selected {
                        return None;
                    }
                    let identity = agent_doc_queue::queue_claim::claim_identity_at(block.entries, index)
                        .unwrap_or_else(|| {
                            agent_doc_queue::queue_claim::claim_identity(&prompt.text)
                        });
                    let observed =
                        agent_doc_document::queue_projection::lifecycle_marker(&prompt.text);
                    if only_identity.is_some_and(|target| target != &identity) {
                        // The document projector owns every visible lifecycle prefix it
                        // encounters. Preserve unrelated projections explicitly so a
                        // target-scoped admission neither normalizes them away nor asks
                        // their historical durable state to converge first.
                        let marker = observed?;
                        return Some(agent_doc_queue::document_queue::QueueRowProjection {
                            text: agent_doc_document::queue_projection::strip_in_progress_marker(
                                &prompt.text,
                            ),
                            blockers: if marker
                                == agent_doc_document::queue_projection::QueueLifecycleMarker::Blocked
                            {
                                blockers
                                    .blockers_for_row(&prompt.text)
                                    .map(<[String]>::to_vec)
                                    .unwrap_or_default()
                            } else {
                                Vec::new()
                            },
                            marker,
                            owner: agent_doc_document::queue_projection::owner_projection(
                                &prompt.text,
                            ),
                        });
                    }
                    if let Some(claim) = claim_states.get(&identity)
                        && let Some(diagnosis) = agent_doc_queue::queue_claim::diagnose_lifecycle(
                            claim,
                            observed,
                            crate::queue_claim::claim_clock_now_secs(),
                            agent_doc_queue::queue_claim::DISPATCH_EVIDENCE_GRACE_SECS,
                        )
                    {
                        log_queue_invariant_once(file, claim, diagnosis);
                    }
                    // `#terminalitemstability`: runtime evidence never reopens
                    // a struck row. The operator removes the strike to reopen it.
                    if struck {
                        return None;
                    }
                    let claim = (cleared_identity.as_ref() != Some(&identity))
                        .then(|| claim_states.get(&identity))
                        .flatten();
                    let receipt = (cleared_identity.as_ref() != Some(&identity) && claim.is_none())
                        .then(|| receipts.get(&identity))
                        .flatten();
                    let has_completion_evidence = completion_evidence.iter().any(|item| {
                        agent_doc_queue::queue_claim::completion_evidence_matches_row(
                            item,
                            &prompt.text,
                        )
                    });
                    if waiting_item_head
                        && claim.is_none()
                        && receipt.is_none()
                        && !has_completion_evidence
                    {
                        return None;
                    }
                    let is_selected = selected.claims_at(block.entries, index);
                    let is_blocked = blockers.blocks_row(&prompt.text);
                    if rows == RuntimeProjectionRows::LifecycleEvidence
                        && observed.is_none()
                        && !state_degraded
                        && claim.is_none()
                        && receipt.is_none()
                        && !has_completion_evidence
                        && !is_selected
                        && !is_blocked
                    {
                        return None;
                    }
                    let marker = if state_degraded {
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Unknown
                    } else if let Some(claim) = claim {
                        claim.projection_marker()
                    } else if let Some(receipt) = receipt {
                        if receipt.completion_recovered_after_loss {
                            agent_doc_document::queue_projection::QueueLifecycleMarker::LostReported
                        } else {
                            agent_doc_document::queue_projection::QueueLifecycleMarker::Reported
                        }
                    } else if has_completion_evidence {
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Reconciling
                    } else if is_selected {
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Running
                    } else if is_blocked {
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Blocked
                    } else if matches!(
                        observed,
                        Some(
                            agent_doc_document::queue_projection::QueueLifecycleMarker::Skipped
                                | agent_doc_document::queue_projection::QueueLifecycleMarker::AwaitingOperator
                        )
                    ) {
                        observed.expect("matched Some lifecycle marker")
                    } else {
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Pending
                    };
                    if rows == RuntimeProjectionRows::BlockedOrPreviouslyBlocked
                        && observed
                            != Some(
                                agent_doc_document::queue_projection::QueueLifecycleMarker::Blocked,
                            )
                        && marker
                            != agent_doc_document::queue_projection::QueueLifecycleMarker::Blocked
                    {
                        return None;
                    }
                    let owner = claim
                        .map(|claim| agent_doc_document::queue_projection::QueueOwnerProjection {
                            owner: claim.owner.clone(),
                            generation: claim.generation,
                        })
                        .or_else(|| {
                            receipt.map(|receipt| {
                                agent_doc_document::queue_projection::QueueOwnerProjection {
                                    owner: receipt.owner.clone(),
                                    generation: receipt.generation,
                                }
                            })
                        });
                    Some(agent_doc_queue::document_queue::QueueRowProjection {
                        text: agent_doc_element_queue::render_queue_report_state_annotation(
                            &agent_doc_document::queue_projection::strip_in_progress_marker(
                                &prompt.text,
                            ),
                            claim
                                .and_then(|claim| claim.publication_state)
                                .or_else(|| receipt.and_then(|receipt| receipt.publication_state)),
                        ),
                        blockers: if marker
                            == agent_doc_document::queue_projection::QueueLifecycleMarker::Blocked
                        {
                            blockers
                                .blockers_for_row(&prompt.text)
                                .map(<[String]>::to_vec)
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        },
                        marker,
                        owner,
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        persisted_heads.extend(
            targets
                .iter()
                .filter(|target| {
                    !matches!(
                        target.marker,
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Pending
                            | agent_doc_document::queue_projection::QueueLifecycleMarker::Blocked
                            | agent_doc_document::queue_projection::QueueLifecycleMarker::Unknown
                    )
                })
                .map(|target| {
                    agent_doc_document::queue_projection::strip_priority_markers(&target.text)
                }),
        );
        targets
    };
    let target_content =
        agent_doc_queue::queue_mirror_projection::project_runtime_projections_across_queues(
            current,
            skip_queue_id,
            targets_for,
        )?
        .unwrap_or_else(|| current.to_string());
    Ok(ClaimRuntimeProjection {
        changed: target_content != input_current,
        target_content,
        persisted_heads,
    })
}

/// Record the non-document projections paired with a successfully accepted
/// claim-marker document successor.
pub fn record_claim_runtime_projection(
    file: &Path,
    projection: &ClaimRuntimeProjection,
    cleared_item: Option<&str>,
) -> Result<()> {
    record_claim_runtime_projection_scoped(file, projection, cleared_item, None)
}

fn record_claim_runtime_projection_scoped(
    file: &Path,
    projection: &ClaimRuntimeProjection,
    cleared_item: Option<&str>,
    only_identity: Option<&agent_doc_element_queue::QueueItemIdentity>,
) -> Result<()> {
    if projection.changed {
        // Keep the recovery baseline aligned with this binary-owned cosmetic
        // mutation. Otherwise a later exchange update can make the unrelated
        // queue diff block the document-only auto-commit boundary.
        if let Ok(Some(snapshot)) = agent_doc_snapshot_io::load_document_baseline(file) {
            let queue_synced =
                agent_doc_document::queue_projection::sync_in_progress_marker_regions(
                    &snapshot,
                    &projection.target_content,
                );
            // A target-scoped claim still changes the authoritative lifecycle
            // of that work identity. Keep the derived Active index on the same
            // accepted boundary; otherwise a reintroduced stable id can show a
            // fresh Running queue row beside its predecessor's stale Reported
            // status until an unrelated full-document repair happens.
            let synced = agent_doc_work_projection::reconcile_managed_active_items_for(file, &queue_synced)?
                .unwrap_or(queue_synced);
            if synced != snapshot
                && let Err(err) = agent_doc_snapshot_io::checkpoint_document_baseline(
                    file,
                    &synced,
                    agent_doc_ops_log_io::log_op,
                )
            {
                eprintln!("[queue] claim marker snapshot sync warning: {err:#}");
            }
        }
    }
    let projected_heads = if let Some(identity) = only_identity {
        let mut heads = agent_doc_cycle_state_io::load(file)?
            .map(|state| state.projected_in_progress_queue_heads)
            .unwrap_or_default();
        heads.retain(|head| agent_doc_queue::queue_claim::claim_identity(head) != *identity);
        heads.extend(projection.persisted_heads.iter().cloned());
        heads
    } else {
        projection.persisted_heads.clone()
    };
    agent_doc_cycle_state_io::set_projected_in_progress_queue_heads(file, &projected_heads)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_claim_runtime_projection markers={} cleared={} (#claimmarker)",
            projection.persisted_heads.len(),
            cleared_item.is_some()
        ),
    );
    Ok(())
}

/// Reconcile the visible typed queue projection immediately after a claim
/// lifecycle mutation (`#claimmarker`).
///
/// Claims live in `state.db`, but the queue marker is operator-facing runtime
/// state. Waiting for the next preflight left every just-dispatched worker
/// invisible for the duration of its run. This command-path projection reads
/// the editor/CRDT authority, changes only queue components (every queue that
/// displays a claimed identity, `#queuemirrormarkers`), and converges that
/// exact authoritative cut. Existing non-claim `🚧` markers are
/// preserved; `cleared_item` identifies an abandoned/requeued claim whose
/// marker must be removed. A reported release keeps its durable receipt visible
/// as `☑️`, so callers must not clear it.
pub fn project_claim_runtime_state_with_effects(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
    cleared_item: Option<&str>,
) -> Result<bool> {
    let repair_generation = queue_repair_generation(file)?;
    project_runtime_state_with_effects(
        effects,
        file,
        cleared_item,
        RuntimeProjectionRows::LifecycleEvidence,
        None,
        CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS,
        repair_generation,
    )
}

/// Reconcile only the exact attempt identity (across every duplicate display)
/// after a claim/start/release transition. Unrelated queue-lifecycle drift
/// remains for declarative sync and cannot veto this transition. The managed
/// Active index is re-derived on the same boundary because it is a binary-owned
/// view of those queue lifecycles, not independent operator state.
pub fn project_claim_item_runtime_state_with_effects(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
    item: &str,
    cleared: bool,
) -> Result<bool> {
    let identity = agent_doc_queue::queue_claim::claim_identity(item);
    let repair_generation = queue_repair_generation(file)?;
    project_runtime_state_with_effects(
        effects,
        file,
        cleared.then_some(item),
        RuntimeProjectionRows::LifecycleEvidence,
        Some(&identity),
        CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS,
        repair_generation,
    )
}

/// Named blocker for a claim admission the projector cannot make visible.
pub const CLAIM_PROJECTION_UNREPRESENTABLE: &str = "claim_projection_unrepresentable";

/// A claim admission refused for a deterministic reason: retrying the same
/// claim against the same binary and row would fail the same way.
#[derive(Debug)]
pub struct ClaimAdmissionRefused {
    pub blocker: &'static str,
    pub detail: String,
}

impl std::fmt::Display for ClaimAdmissionRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "claim admission refused (blocker: {}): {}", self.blocker, self.detail)
    }
}

impl std::error::Error for ClaimAdmissionRefused {}

/// Verify the admitted attempt through the SAME observer `queue claims` reads
/// (`#waitingitemclaimmarker`). The projector's own settle check derives its
/// targets with its own row filter; when that filter skipped a row, the
/// projector "settled" with nothing written and admission returned spawn
/// permission while `queue claims` reported `out_of_sync`. The coordinator
/// then released and requeued, and the next cycle claimed again.
fn verify_claim_admission_visible(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
    claim: &agent_doc_queue::queue_claim::QueueClaim,
) -> Result<()> {
    let current = effects.current_document_content(file, "queue_claim_admission_verify")?;
    let observed = crate::queue_claim::visible_lifecycle_markers_for_content(&current)?;
    let visible = crate::queue_claim::visible_claim_projection(&observed, claim);
    if visible.synchronized {
        return Ok(());
    }
    let detail = format!(
        "the claim projection settled but the visible row for {:?} shows {:?}, not the expected {}; no worker may be spawned. Subagent dispatch holds this item until the projection is fixed and an explicit `agent-doc queue claim {} --item {} --owner <owner>` succeeds",
        agent_doc_queue::queue_claim::claim_item_handle(&claim.item_text),
        visible.observed_visible_markers,
        visible.expected_visible_marker,
        crate::queue_claim::shell_single_quote(&file.display().to_string()),
        crate::queue_claim::shell_single_quote(&agent_doc_queue::queue_claim::claim_item_handle(
            &claim.item_text
        )),
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_claim_admission_refused blocker={CLAIM_PROJECTION_UNREPRESENTABLE} owner={} generation={} expected={} observed={:?} (#waitingitemclaimmarker)",
            claim.owner,
            claim.generation,
            visible.expected_visible_marker,
            visible.observed_visible_markers,
        ),
    );
    Err(ClaimAdmissionRefused {
        blocker: CLAIM_PROJECTION_UNREPRESENTABLE,
        detail,
    }
    .into())
}

/// Admit one dispatch claim only when its operator-visible `Claimed` projection
/// converges. The returned claim is the harness's permission to spawn. If the
/// projection fails, compensate the exact still-unstarted ledger transition
/// and make a best-effort projection repair so callers never receive a spawn
/// acknowledgement for an invisible reservation (`#claimbeforespawn`).
pub fn claim_with_runtime_projection(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
    item: &str,
    owner: &str,
    ttl_secs: u64,
) -> Result<(
    agent_doc_queue::queue_claim::ClaimOutcome,
    agent_doc_queue::queue_claim::QueueClaim,
)> {
    claim_with_runtime_projection_for_harness(effects, file, item, owner, ttl_secs, None)
}

/// [`claim_with_runtime_projection`] for a worker on `worker_harness`
/// (`queue claim --harness`, `#claimworkerharness`).
pub fn claim_with_runtime_projection_for_harness(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
    item: &str,
    owner: &str,
    ttl_secs: u64,
    worker_harness: Option<&str>,
) -> Result<(
    agent_doc_queue::queue_claim::ClaimOutcome,
    agent_doc_queue::queue_claim::QueueClaim,
)> {
    let before = crate::queue_claim::load_ledger(file)?;
    let (outcome, claim) =
        crate::queue_claim::claim_for_worker_harness(file, item, owner, ttl_secs, worker_harness)?;
    let previous = before
        .claims
        .iter()
        .find(|previous| previous.identity == claim.identity);
    let projection =
        project_claim_item_runtime_state_with_effects(effects, file, &claim.item_text, false)
            .and_then(|_| verify_claim_admission_visible(effects, file, &claim));
    if let Err(projection_error) = projection {
        let rollback = crate::queue_claim::rollback_unstarted_admission(file, &claim, previous);
        // `#waitingitemclaimmarker`: a deterministic blocker terminalizes this
        // exact generation so subagent dispatch stops re-offering the row; a
        // transient convergence failure stays retryable.
        let rollback = rollback.and_then(|()| {
            let Some(refused) = projection_error.downcast_ref::<ClaimAdmissionRefused>() else {
                return Ok(());
            };
            crate::queue_lineage::record_admission_refused(file, &claim, refused.blocker)
                .map(|_| ())
                .context("record the refused claim admission in queue lineage")
        });
        let repair = rollback.as_ref().ok().and_then(|_| {
            project_claim_item_runtime_state_with_effects(effects, file, &claim.item_text, true)
                .err()
        });
        return match (rollback, repair) {
            (Ok(()), None) => Err(projection_error).context(
                "queue claim admission was rolled back because its Claimed projection was not acknowledged; no worker may be spawned",
            ),
            (Ok(()), Some(repair_error)) => Err(projection_error).context(format!(
                "queue claim admission was rolled back because its Claimed projection was not acknowledged; marker repair also failed: {repair_error:#}; no worker may be spawned"
            )),
            (Err(rollback_error), _) => Err(projection_error).context(format!(
                "queue claim projection failed and admission rollback was refused: {rollback_error:#}; inspect the exact generation before dispatching any worker"
            )),
        };
    }
    Ok((outcome, claim))
}

/// Reconcile the complete queue lifecycle projection for an explicit
/// declarative sync.
///
/// Unlike a single claim transition, document sync owns every queue row and
/// therefore paints pending rows as well as rows carrying durable attempt,
/// report, completion, or blocker evidence. It also reconciles completion
/// evidence into the durable ledger before deriving the projection.
pub fn project_all_queue_runtime_state_with_effects(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
) -> Result<bool> {
    let repair_generation = queue_repair_generation(file)?;
    project_runtime_state_with_effects(
        effects,
        file,
        None,
        RuntimeProjectionRows::All,
        None,
        CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS,
        repair_generation,
    )
}

/// One CAS attempt for a background projection repair.
///
/// Foreground claim admission may retry because its caller owns a concrete
/// lifecycle transition and waits for the corresponding marker. Background
/// self-heal has no such right to contend with operator edits: a changed editor
/// revision supersedes its snapshot and schedules a later repair edge. One
/// attempt keeps operator typing authoritative and prevents eight full-document
/// retries from multiplying controller pressure.
pub fn project_all_queue_runtime_state_once_with_effects(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
) -> Result<QueueRuntimeProjectionOnceOutcome> {
    Ok(project_all_queue_runtime_state_once_run_with_effects(effects, file)?.outcome)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueRuntimeProjectionRun {
    pub outcome: QueueRuntimeProjectionOnceOutcome,
    pub generation: u64,
}

pub fn project_all_queue_runtime_state_once_run_with_effects(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
) -> Result<QueueRuntimeProjectionRun> {
    let generation = queue_repair_generation(file)?;
    let outcome = project_all_queue_runtime_state_once_for_generation_with_effects(
        effects, file, generation,
    )?;
    Ok(QueueRuntimeProjectionRun {
        outcome,
        generation,
    })
}

pub fn project_all_queue_runtime_state_once_for_generation_with_effects(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
    generation: u64,
) -> Result<QueueRuntimeProjectionOnceOutcome> {
    match project_runtime_state_with_effects(
        effects,
        file,
        None,
        RuntimeProjectionRows::All,
        None,
        1,
        generation,
    ) {
        Ok(changed) => Ok(QueueRuntimeProjectionOnceOutcome::Settled { changed }),
        Err(error)
            if error
                .downcast_ref::<QueueRuntimeProjectionSuperseded>()
                .is_some() =>
        {
            Ok(QueueRuntimeProjectionOnceOutcome::Superseded)
        }
        Err(error) => Err(error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueRuntimeProjectionOnceOutcome {
    Settled { changed: bool },
    Superseded,
}

/// Result of an explicit document-local queue clean-slate repair.
///
/// Historical lineage, operator-delete tombstones, queue schedules, and every
/// document byte are outside the two cleared state families and remain intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct QueueCleanSlateOutcome {
    pub cleared_claims: usize,
    pub cleared_release_receipts: usize,
    pub projection: QueueRuntimeProjectionOnceOutcome,
}

fn queue_projection_repair_state_identity(file: &Path) -> Result<(std::path::PathBuf, String)> {
    let project_root = agent_doc_fs::find_project_root(file).with_context(|| {
        format!(
            "queue projection repair request: project root not found for {}",
            file.display()
        )
    })?;
    let document_hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize queue repair document: {}", file.display()))?;
    Ok((project_root, document_hash))
}

pub fn queue_repair_generation(file: &Path) -> Result<u64> {
    let (project_root, document_hash) = queue_projection_repair_state_identity(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let record = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        &conn,
        &document_hash,
        agent_doc_queue::queue_repair_generation::QUEUE_REPAIR_GENERATION_STATE_KIND,
    )?;
    Ok(record
        .as_ref()
        .map(|record| {
            serde_json::from_str::<agent_doc_queue::queue_repair_generation::QueueRepairGeneration>(
                &record.payload_json,
            )
            .context("parse queue repair generation")
        })
        .transpose()?
        .unwrap_or_default()
        .generation)
}

pub fn ensure_queue_projection_repair_request(
    file: &Path,
    generation: u64,
    reason: &str,
) -> Result<agent_doc_queue::queue_repair_generation::QueueProjectionRepairRequest> {
    ensure_queue_projection_repair_request_with_wake(file, generation, reason, true, |file| {
        agent_doc_controller_io::project_controller::wake_queue_projection_repair_for_file(file)
    })
}

/// Persist or reuse the exact repair request without publishing a wake.
///
/// The idle supervisor uses this after consuming the document-delivery edge
/// that made repair due. Publishing another delivery wake from inside that
/// consumer would re-arm its own successful `changed=false` repair forever.
pub fn ensure_queue_projection_repair_request_without_wake(
    file: &Path,
    generation: u64,
    reason: &str,
) -> Result<agent_doc_queue::queue_repair_generation::QueueProjectionRepairRequest> {
    ensure_queue_projection_repair_request_with_wake(file, generation, reason, false, |_| Ok(()))
}

fn ensure_queue_projection_repair_request_with_wake(
    file: &Path,
    generation: u64,
    reason: &str,
    publish_wake: bool,
    wake: impl FnOnce(&Path) -> Result<()>,
) -> Result<agent_doc_queue::queue_repair_generation::QueueProjectionRepairRequest> {
    let (project_root, document_hash) = queue_projection_repair_state_identity(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin queue projection repair request transaction")?;
    let recorded = (|| {
        let generation_record = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
            &conn,
            &document_hash,
            agent_doc_queue::queue_repair_generation::QUEUE_REPAIR_GENERATION_STATE_KIND,
        )?;
        let current_generation = generation_record
            .as_ref()
            .map(|record| {
                serde_json::from_str::<
                    agent_doc_queue::queue_repair_generation::QueueRepairGeneration,
                >(&record.payload_json)
                .context("parse queue repair generation while recording request")
            })
            .transpose()?
            .unwrap_or_default()
            .generation;
        if current_generation != generation {
            anyhow::bail!(
                "queue projection repair request superseded: expected generation {generation}, current generation {current_generation}"
            );
        }

        let existing = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
            &conn,
            &document_hash,
            agent_doc_queue::queue_repair_generation::QUEUE_PROJECTION_REPAIR_REQUEST_STATE_KIND,
        )?
        .map(|record| {
            serde_json::from_str::<
                agent_doc_queue::queue_repair_generation::QueueProjectionRepairRequest,
            >(&record.payload_json)
            .context("parse existing queue projection repair request")
        })
        .transpose()?;
        if let Some(existing) = existing
            .filter(|request| request.generation == generation && request.request_generation > 0)
        {
            return Ok::<_, anyhow::Error>((existing, false));
        }

        let sequence_record = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
            &conn,
            &document_hash,
            agent_doc_queue::queue_repair_generation::QUEUE_PROJECTION_REPAIR_SEQUENCE_STATE_KIND,
        )?;
        let sequence = sequence_record
            .as_ref()
            .map(|record| {
                serde_json::from_str::<
                    agent_doc_queue::queue_repair_generation::QueueProjectionRepairSequence,
                >(&record.payload_json)
                .context("parse queue projection repair sequence")
            })
            .transpose()?
            .unwrap_or_default()
            .generation
            .saturating_add(1);
        let request = agent_doc_queue::queue_repair_generation::QueueProjectionRepairRequest {
            generation,
            request_generation: sequence,
            reason: reason.to_string(),
            requested_at_secs: crate::queue_claim::claim_clock_now_secs(),
        };
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash: document_hash.clone(),
                state_kind: agent_doc_queue::queue_repair_generation::QUEUE_PROJECTION_REPAIR_SEQUENCE_STATE_KIND.to_string(),
                canonical_path: file
                    .canonicalize()
                    .unwrap_or_else(|_| file.to_path_buf())
                    .display()
                    .to_string(),
                payload_json: serde_json::to_string(
                    &agent_doc_queue::queue_repair_generation::QueueProjectionRepairSequence {
                        generation: sequence,
                    },
                )?,
                updated_at_secs: request.requested_at_secs,
            },
        )?;
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash: document_hash.clone(),
                state_kind: agent_doc_queue::queue_repair_generation::QUEUE_PROJECTION_REPAIR_REQUEST_STATE_KIND.to_string(),
                canonical_path: file
                    .canonicalize()
                    .unwrap_or_else(|_| file.to_path_buf())
                    .display()
                    .to_string(),
                payload_json: serde_json::to_string(&request)?,
                updated_at_secs: request.requested_at_secs,
            },
        )?;
        Ok((request, true))
    })();
    let recorded = match recorded {
        Ok(recorded) => {
            conn.execute_batch("COMMIT")?;
            recorded
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    };
    let (request, created) = recorded;
    if !created || !publish_wake {
        return Ok(request);
    }
    if let Err(error) = wake(file) {
        // The request is already durable. An inactive document consumes it on
        // supervisor startup; an attached supervisor also has its periodic
        // repair cadence, so a transient wake failure must not erase the typed
        // deferred result.
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_projection_repair_wake_deferred file={} durable_request=true error={}",
                file.display(),
                format!("{error:#}").replace('\n', "\\n"),
            ),
        );
    }
    Ok(request)
}

pub fn clear_queue_projection_repair_request(
    file: &Path,
    expected: &agent_doc_queue::queue_repair_generation::QueueProjectionRepairRequest,
) -> Result<bool> {
    let (project_root, document_hash) = queue_projection_repair_state_identity(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let cleared = (|| {
        let record = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
            &conn,
            &document_hash,
            agent_doc_queue::queue_repair_generation::QUEUE_PROJECTION_REPAIR_REQUEST_STATE_KIND,
        )?;
        let Some(request) = record
            .as_ref()
            .map(|record| {
                serde_json::from_str::<
                    agent_doc_queue::queue_repair_generation::QueueProjectionRepairRequest,
                >(&record.payload_json)
                .context("parse queue projection repair request while clearing")
            })
            .transpose()?
        else {
            return Ok::<_, anyhow::Error>(false);
        };
        if request.generation != expected.generation
            || request.request_generation != expected.request_generation
        {
            return Ok(false);
        }
        agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
            &conn,
            &document_hash,
            agent_doc_queue::queue_repair_generation::QUEUE_PROJECTION_REPAIR_REQUEST_STATE_KIND,
        )?;
        Ok(true)
    })();
    match cleared {
        Ok(cleared) => {
            conn.execute_batch("COMMIT")?;
            Ok(cleared)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

pub fn load_queue_projection_repair_request(
    file: &Path,
) -> Result<Option<agent_doc_queue::queue_repair_generation::QueueProjectionRepairRequest>> {
    let (project_root, document_hash) = queue_projection_repair_state_identity(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        &conn,
        &document_hash,
        agent_doc_queue::queue_repair_generation::QUEUE_PROJECTION_REPAIR_REQUEST_STATE_KIND,
    )?
    .map(|record| serde_json::from_str(&record.payload_json).map_err(Into::into))
    .transpose()
}

impl Serialize for QueueRuntimeProjectionOnceOutcome {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Settled { changed } => serializer.serialize_str(if *changed {
                "settled_changed"
            } else {
                "settled_unchanged"
            }),
            Self::Superseded => serializer.serialize_str("superseded_deferred"),
        }
    }
}

/// Explicit, document-local recovery for queue/runtime projection drift.
///
/// The destructive reset is one controller-owned exclusive maintenance
/// operation, not an actorless preflight followed by a write. It is never a
/// hot-path claim transition and never participates in closeout. Durable
/// lineage, delete tombstones, schedules, continuation state, and document
/// text are separate families and remain untouched.
pub fn clean_slate_with_effects(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
) -> Result<QueueCleanSlateOutcome> {
    let reset =
        agent_doc_controller_io::project_controller::queue_clean_slate_reset_for_file(file)?;
    let projection = match project_all_queue_runtime_state_once_for_generation_with_effects(
        effects,
        file,
        reset.generation,
    ) {
        Ok(outcome @ QueueRuntimeProjectionOnceOutcome::Settled { .. }) => {
            if let Some(request) = load_queue_projection_repair_request(file)?
                && request.generation == reset.generation
            {
                clear_queue_projection_repair_request(file, &request)?;
            }
            outcome
        }
        Ok(QueueRuntimeProjectionOnceOutcome::Superseded) => {
            ensure_queue_projection_repair_request(
                file,
                reset.generation,
                "clean_slate_projection_superseded",
            )?;
            QueueRuntimeProjectionOnceOutcome::Superseded
        }
        Err(error) => {
            ensure_queue_projection_repair_request(
                file,
                reset.generation,
                "clean_slate_projection_error",
            )?;
            return Err(error).context(
                "queue clean-slate reset committed; projection repair was durably deferred",
            );
        }
    };
    Ok(QueueCleanSlateOutcome {
        cleared_claims: reset.cleared_claims,
        cleared_release_receipts: reset.cleared_release_receipts,
        projection,
    })
}

/// A projection CAS refusal from the owning CRDT actor: the relay current
/// advanced past the cut the successor was derived from
/// (`recovery=retry_crdt_merge`). Retrying from a fresh authority cut is safe
/// because the projection is a pure function of that cut and the ledger.
fn is_retryable_projection_cas_refusal(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("recovery=retry_crdt_merge")
}

#[derive(Debug)]
struct QueueRuntimeProjectionSuperseded;

impl std::fmt::Display for QueueRuntimeProjectionSuperseded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("queue runtime projection was superseded by a newer authority cut")
    }
}

impl std::error::Error for QueueRuntimeProjectionSuperseded {}

/// Repaint only rows that are blocked now or still carry a stale blocked
/// projection. Tracked-work closeout uses this narrower effect so blocker-list
/// changes converge immediately without painting unrelated pending rows.
pub fn project_blocker_runtime_state_with_effects(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
) -> Result<bool> {
    let repair_generation = queue_repair_generation(file)?;
    project_runtime_state_with_effects(
        effects,
        file,
        None,
        RuntimeProjectionRows::BlockedOrPreviouslyBlocked,
        None,
        CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS,
        repair_generation,
    )
}

fn project_runtime_state_with_effects(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
    cleared_item: Option<&str>,
    rows: RuntimeProjectionRows,
    only_identity: Option<&agent_doc_element_queue::QueueItemIdentity>,
    max_attempts: usize,
    repair_generation: u64,
) -> Result<bool> {
    let mut wrote_successor = false;
    for attempt in 1..=max_attempts {
        if queue_repair_generation(file)? != repair_generation {
            return Err(QueueRuntimeProjectionSuperseded.into());
        }
        // Start every attempt from the live authority. This first absorbs any
        // operator/editor normalization that raced the ledger transition; the
        // marker overlay below is then rederived by stable work identity.
        let current = effects.current_document_content(file, "queue_claim_runtime_projection")?;
        // A completion cell is durable attempt-scoped evidence. Reconcile it
        // into the ledger before deriving claim markers, including after
        // restart. Blocker-only refresh is a document projection and must not
        // acquire that ledger side effect.
        if rows == RuntimeProjectionRows::All {
            crate::queue_claim::reconcile_completion_evidence(file, &current)?;
        }
        let projection = match rows {
            RuntimeProjectionRows::All | RuntimeProjectionRows::LifecycleEvidence => {
                derive_runtime_projection_scoped(
                    file,
                    &current,
                    cleared_item,
                    ClaimProjectionScope::Document,
                    rows,
                    only_identity,
                )?
            }
            RuntimeProjectionRows::BlockedOrPreviouslyBlocked => {
                derive_blocker_runtime_projection(file, &current)?
            }
        };
        let target_content =
            agent_doc_work_projection::reconcile_managed_active_items_for(file, &projection.target_content)?
                .unwrap_or(projection.target_content);
        // `#sdkdocumentselfheal`: exact operator-request rows and their Todo
        // receipts are one lifecycle. Older/interrupted closeouts could strike
        // the queue row but leave its same-id Todo open until another foreground
        // preflight happened to repair it. A full runtime projection already
        // owns every queue row, so fold the same proof-complete reconciliation
        // into its CAS successor. The pure helper refuses cited ids and refuses
        // when any duplicate row is still live.
        let target_content = if rows == RuntimeProjectionRows::All && only_identity.is_none() {
            agent_doc_queue::queue_consume::reconcile_terminal_operator_request_todos(
                &target_content,
                &[],
            )?
            .content
        } else {
            target_content
        };
        if target_content != current {
            if queue_repair_generation(file)? != repair_generation {
                return Err(QueueRuntimeProjectionSuperseded.into());
            }
            match effects.converge_document_or_disk_for_generation(
                file,
                &target_content,
                &current,
                "queue_claim_runtime_projection",
                repair_generation,
            ) {
                Ok(true) => wrote_successor = true,
                Ok(false) => return Err(QueueRuntimeProjectionSuperseded.into()),
                // `#claimprojectionretry`: a CAS refusal means a concurrent
                // CRDT write moved the relay current past the cut this attempt
                // derived its successor from. The marker overlay is rederived
                // by stable work identity, so the same lifecycle transition
                // (and the same claim generation) retries its projection from
                // a fresh authority cut instead of surfacing a
                // `projection_failure` the caller can only clear by claiming
                // again. Every retry is earned by an explicit refusal and
                // shares the bounded attempt budget.
                Err(error)
                    if attempt < max_attempts && is_retryable_projection_cas_refusal(&error) =>
                {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "queue_claim_runtime_projection_cas_retry attempt={attempt} max={max_attempts} expected_hash={} attempted_hash={} recovery=retry_crdt_merge (#claimprojectionretry)",
                            agent_doc_hash::content_hash(&current),
                            agent_doc_hash::content_hash(&target_content),
                        ),
                    );
                    effects.settle_before_projection_retry(file);
                    continue;
                }
                Err(error) if max_attempts > 1 && is_retryable_projection_cas_refusal(&error) => {
                    return Err(error).context(format!(
                        "queue claim runtime projection CAS was refused on all {max_attempts} attempts; concurrent CRDT writes kept advancing the document (#claimprojectionretry)"
                    ));
                }
                Err(error) => return Err(error),
            }
        }

        // Convergence may legitimately preserve a concurrent editor cell over
        // this cosmetic successor. Success is therefore a property of a fresh
        // authoritative cut, not of the write call returning `Ok(())`.
        let authoritative =
            effects.current_document_content(file, "queue_claim_runtime_projection_verify")?;
        let settled = match rows {
            RuntimeProjectionRows::All | RuntimeProjectionRows::LifecycleEvidence => {
                derive_runtime_projection_scoped(
                    file,
                    &authoritative,
                    cleared_item,
                    ClaimProjectionScope::Document,
                    rows,
                    only_identity,
                )?
            }
            RuntimeProjectionRows::BlockedOrPreviouslyBlocked => {
                derive_blocker_runtime_projection(file, &authoritative)?
            }
        };
        let active_settled =
            agent_doc_work_projection::reconcile_managed_active_items_for(file, &authoritative)?.is_none();
        let terminal_todos_settled = rows != RuntimeProjectionRows::All
            || only_identity.is_some()
            || agent_doc_queue::queue_consume::reconcile_terminal_operator_request_todos(
                &authoritative,
                &[],
            )?
            .completed_ids
            .is_empty();
        if !settled.changed && active_settled && terminal_todos_settled {
            if queue_repair_generation(file)? != repair_generation {
                return Err(QueueRuntimeProjectionSuperseded.into());
            }
            let accepted = ClaimRuntimeProjection {
                target_content: authoritative,
                persisted_heads: settled.persisted_heads,
                changed: wrote_successor,
            };
            record_claim_runtime_projection_scoped(file, &accepted, cleared_item, only_identity)?;
            return Ok(wrote_successor);
        }
        let authority_superseded = authoritative != current;
        if max_attempts == 1 && authority_superseded {
            return Err(QueueRuntimeProjectionSuperseded.into());
        }

        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_runtime_projection_retry attempt={attempt} max={} (#queueclaimprojectionrace)",
                max_attempts
            ),
        );
    }

    bail!(
        "queue claim runtime projection did not converge after {} authoritative retries; the claim ledger transition is durable but its live queue marker is not yet verified",
        max_attempts
    )
}

/// Read claim reports from the same live document authority used by claim
/// mutation and marker projection. Disk is only the detached-document fallback
/// supplied by the effect adapter.
pub fn claim_reports_with_effects(
    effects: &impl QueueCommandEffects,
    file: &Path,
) -> Result<Vec<crate::queue_claim::ClaimReport>> {
    let current = effects.current_document_content(file, "queue_claims")?;
    crate::queue_claim::claim_reports_for_content(file, &current)
}

/// Explicitly strike the leading `count` free-text queue head(s) — the agent
/// asserting it has already answered them, the same contract `--done <id>` gives
/// an id-backed head (`#multi-head-consume-one-per-finalize`).
///
/// The free-text strike heuristic only consumes ONE head per finalize (the head
/// current at that cycle's preflight), so when several free-text heads are
/// answered across a single cycle the trailing ones stay queued and re-serve on
/// the next auto-loop, producing duplicate-response churn. This gives a
/// deterministic, non-heuristic way to drain those answered stragglers without a
/// fuzzy head↔response matcher that could delete a genuinely unanswered prompt.
/// Session-check still requires committed exchange proof for the struck
/// free-text head, normally through the queue-prompt echo.
///
/// Scoped to free-text heads: if a head in range is id-backed it bails with
/// guidance to use `--done`, so it can never silently desync a head from its
/// still-open backlog item. Writes document + snapshot like `sync`; the caller
/// closes out through the normal commit path.
pub fn consume_with_options(
    effects: &impl QueueCommandEffects,
    file: &Path,
    count: usize,
    options: ConsumeOptions,
) -> Result<()> {
    // #sqedit-race Phase 2: hold the queue-edit lease for the whole strike loop so
    // preflight queue maintenance and the supervisor idle-watch defer instead of
    // round-tripping a torn intermediate queue. Released on drop (incl. early
    // return / `bail!`).
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let target = count.max(1);
    let content = effects.current_document_content(file, "queue_consume_classify")?;
    match classify_active_queue_head(&content)? {
        ActiveQueueHeadKind::None => {}
        ActiveQueueHeadKind::Inactive => {
            let struck = effects.converge_captured_answered_free_text_projection(file)?;
            if struck > 0 {
                println!(
                    "{}: recovered torn inactive queue projection and consumed {} captured-response-proven head(s)",
                    file.display(),
                    struck
                );
                return Ok(());
            }
            bail!(
                "{}: queue rows exist but queue_active is false, and no durable captured response proves an answered head. Preserve the rows and repair the activation projection before consuming work.",
                file.display()
            );
        }
        ActiveQueueHeadKind::IdBacked => {
            let queue_active = agent_doc_frontmatter::frontmatter::parse(&content)?
                .0
                .queue_active;
            let classified_head = active_queue_head_text(&content)?;
            bail!(
                "{}: queue head is an id-backed directive, not a free-text prompt. \
                 classification evidence: queue_active={queue_active:?} active_head={classified_head:?}. \
                 If it represents completed or gated work, reap it through the normal closeout with \
                 `--done <id>` / `--pending-gate <id>` so the backlog item stays in sync. \
                 If it is only an acknowledgement/correction for still-open work, or a reported \
                 (☑️) dispatch row whose backlog/Todo item must stay open, use \
                 `agent-doc queue consume <FILE> --ack-id <id>` to strike the head without closing \
                 the tracked item. Otherwise leave it queued.",
                file.display()
            );
        }
        ActiveQueueHeadKind::FreeText => {}
    }
    let outcome = if matches!(
        classify_active_queue_head(&content)?,
        ActiveQueueHeadKind::FreeText
    ) {
        if options.force_disk {
            effects.consume_free_text_queue_prompts_force_disk(file, target)?
        } else {
            effects.consume_free_text_queue_prompts_with_outcome(file, target)?
        }
    } else {
        None
    };

    match outcome {
        None => println!(
            "{}: no free-text queue head to consume (queue inactive, empty, or id-backed head).",
            file.display()
        ),
        Some(outcome) => {
            println!(
                "{}: consumed {} free-text queue head(s) (remaining: {}){}",
                file.display(),
                outcome.consumed_count,
                outcome.remaining,
                if outcome.drained {
                    ", drained — cleared queue_active"
                } else {
                    ""
                }
            );
        }
    }
    Ok(())
}

/// Escape hatch (#orphanqhead): strike an orphaned id-backed queue head by id.
/// Delegates to the write-layer striker, which guards against desyncing live
/// open backlog work and keeps the document and snapshot in sync.
pub fn consume_orphan_id(effects: &impl QueueCommandEffects, file: &Path, id: &str) -> Result<()> {
    let normalized = backlog::normalize_pending_id(id);
    if effects.strike_orphan_id_backed_queue_head(file, id)? {
        println!(
            "{}: struck orphaned id-backed queue head [#{}] (#orphanqhead).",
            file.display(),
            normalized
        );
    } else {
        println!(
            "{}: no change — orphaned id-backed head [#{}] was already struck or drained.",
            file.display(),
            normalized
        );
    }
    Ok(())
}

pub fn acknowledge_open_id(
    effects: &impl QueueCommandEffects,
    file: &Path,
    id: &str,
) -> Result<()> {
    let normalized = backlog::normalize_pending_id(id);
    if effects.acknowledge_open_id_backed_queue_head(file, id)? {
        println!(
            "{}: acknowledged id-backed correction head [#{}] while preserving the open backlog item (#freshqueueauth).",
            file.display(),
            normalized
        );
    } else {
        println!(
            "{}: no change — id-backed correction head [#{}] was already acknowledged or drained.",
            file.display(),
            normalized
        );
    }
    Ok(())
}

pub fn acknowledge_paused_free_text(
    effects: &impl QueueCommandEffects,
    file: &Path,
    expected_head: &str,
    selector: Option<&crate::queue_consume::AckTextSelector>,
) -> Result<()> {
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    match effects.acknowledge_paused_free_text_queue_head_with_selector(
        file,
        expected_head,
        selector,
    )? {
        Some(outcome) => println!(
            "{}: acknowledged exact free-text queue head (remaining: {}); \
             queue activation unchanged (#acktextlineage).",
            file.display(),
            outcome.remaining
        ),
        None => println!(
            "{}: no exact free-text queue head to acknowledge.",
            file.display()
        ),
    }
    Ok(())
}

pub fn consume(effects: &impl QueueCommandEffects, file: &Path, count: usize) -> Result<()> {
    consume_with_options(effects, file, count, ConsumeOptions::default())
}

/// `agent-doc queue prune-noise <FILE>` — strike every non-drainable noise queue
/// head at any position (`#goqstall2`), clearing pasted console output / agent
/// response fragments that session-check surfaces as
/// `queue_stale_noise_lines=N`. Also strikes **orphan id-backed heads**
/// (`#orphanqhead`): a `do [#id]` / `[#id]` head whose id names no open
/// `agent:backlog` item, which is non-drainable yet blocks the leading-run
/// `queue consume` from reaching answered free-text heads behind it. Preserves
/// id-backed directives whose id is still open backlog work (including deferred
/// `[operator-verify]` / `[focused-cycle]` items) and drainable free-text/prose
/// heads. The prune set is predicate-proven; fresh operator prompts that remain
/// drainable are never removed by this command.
/// Supervisor-safe: routes through the same editor-IPC-converged write path the
/// closeout strikes use.
pub fn prune_noise(effects: &impl QueueCommandEffects, file: &Path) -> Result<()> {
    // #sqedit-race Phase 2: hold the queue-edit lease across the prune so the
    // supervisor idle-watch + preflight maintenance defer (single queue writer).
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let struck = effects.prune_noise_queue_heads(file)?;
    if struck == 0 {
        println!(
            "{}: no predicate-proven queue heads to prune (queue inactive, empty, or all heads are drainable/live).",
            file.display()
        );
    } else {
        println!(
            "{}: pruned {} predicate-proven queue head(s) (noise/orphan only).",
            file.display(),
            struck
        );
    }
    Ok(())
}

/// Read-only declarative sync. It performs no queue/document convergence and
/// emits the same inventory engine apply mode uses after its proof-complete
/// repairs.
pub fn plan_with_effects(effects: &impl QueueCommandEffects, file: &Path) -> Result<()> {
    let previous_content = agent_doc_snapshot_io::load_document_baseline(file)?;
    let content = effects.current_document_content(file, "queue_sync_plan")?;
    let plan = build_queue_sync_reconciliation_plan(file, &content, previous_content.as_deref())?;
    print_queue_sync_plan(&plan)
}

pub fn sync_with_effects(effects: &impl QueueCommandEffects, file: &Path) -> Result<()> {
    let previous_content = agent_doc_snapshot_io::load_document_baseline(file)?;
    let mut content = effects.current_document_content(file, "queue_sync")?;
    // A torn realtime merge can place the binary completion cell underneath
    // the queue row it reports instead of in `agent:exchange`. That corrupts
    // the queue grammar and makes every later strike fail the conversation
    // integrity gate, which in turn strands the reported row. Recover the
    // hash-attested (or keyed rolling-upgrade) cell losslessly into exchange
    // before any queue parsing or lifecycle projection.
    let completion_cell_repaired = if let Some(repaired) =
        agent_doc_template::repair_subagent_completion_cells_inside_queue(&content)?
    {
        effects.converge_document_or_disk(
            file,
            &repaired,
            &content,
            "queue_sync_completion_cell_repair",
        )?;
        content = effects.current_document_content(file, "queue_sync_after_completion_repair")?;
        if let Err(warning) = agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &content,
            agent_doc_ops_log_io::log_op,
        ) {
            eprintln!(
                "[queue sync] warning: failed to update snapshot after completion-cell repair: {warning}"
            );
        }
        true
    } else {
        false
    };
    // Captured-answer convergence may strike a response-proven free-text head,
    // and that write is guarded against conversation scaffolding in queue. Run
    // it only after the repair above has restored the completion cell to
    // exchange; otherwise the recovery path itself is blocked by the corruption
    // it is meant to make recoverable.
    let answered_count = effects.converge_captured_answered_free_text_projection(file)?;
    if answered_count > 0 {
        content = effects.current_document_content(file, "queue_sync_after_captured_answers")?;
    }
    // Older binaries could project `📥` onto a retained generated Response child
    // whose id differed from the terminal parent's latest response backlink.
    // Repair that binary-owned cosmetic corruption before parsing queue work so
    // the child cannot reopen its parent or enter the sync payload. The shared
    // normalizer recognizes only canonical response metadata and leaves ordinary
    // operator rows, including lookalikes with malformed ids, byte-for-byte.
    let normalized_responses =
        agent_doc_markdown_ast::overlay::normalize_generated_response_lifecycle_markers(&content);
    let response_metadata_repaired = normalized_responses != content;
    if response_metadata_repaired {
        effects.converge_document_or_disk(
            file,
            &normalized_responses,
            &content,
            "queue_sync_response_metadata_repair",
        )?;
        content = effects.current_document_content(file, "queue_sync_after_response_repair")?;
        if let Err(warning) = agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &content,
            agent_doc_ops_log_io::log_op,
        ) {
            eprintln!(
                "[queue sync] warning: failed to update snapshot after response metadata repair: {warning}"
            );
        }
    }
    // `#adwaitingrepair`: declarative sync owns binary-authored queue
    // projections even when there is no response-bearing closeout. Older
    // versions struck the Waiting-on-you owner rows themselves, making live
    // Todo-backed operator work look complete until another response happened
    // to refresh the tree. Re-derive the tree from the current authoritative
    // Todo state here; the empty response only means there is no new packet
    // context to add.
    let waiting_tree_repaired = if let Some(repaired) =
        agent_doc_queue::queue_consume::project_waiting_on_you_tree(&content, "")?
    {
        effects.converge_document_or_disk(
            file,
            &repaired,
            &content,
            "queue_sync_waiting_tree_repair",
        )?;
        content = effects.current_document_content(file, "queue_sync_after_waiting_tree_repair")?;
        if let Err(warning) = agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &content,
            agent_doc_ops_log_io::log_op,
        ) {
            eprintln!(
                "[queue sync] warning: failed to update snapshot after Waiting-on-you repair: {warning}"
            );
        }
        true
    } else {
        false
    };
    let explicit_routing = crate::one_shot_sync::sync_explicit_backlog_queue_routes_with_snapshot(
        file,
        &content,
        |path, current, target| {
            effects.converge_document_or_disk(path, target, current, "queue_sync_explicit_routes")
        },
        |path, content| {
            agent_doc_snapshot_io::checkpoint_document_baseline(
                path,
                content,
                agent_doc_ops_log_io::log_op,
            )
        },
    )?;
    if explicit_routing.changed {
        // Re-read the accepted authority cut before the legacy mirror pass.
        // The editor convergence boundary may normalize unrelated bytes.
        content = effects.current_document_content(file, "queue_sync_after_explicit_routes")?;
    }
    let routed_ids = explicit_routing.newly_materialized;
    let placement_debt = explicit_routing.placement_debt;
    let routing_snapshot_warning = explicit_routing.snapshot_warning;
    let backlog_sync = crate::one_shot_sync::sync_configured_backlog_queue_with_snapshot(
        file,
        &content,
        |path, current, target| {
            effects.converge_document_or_disk(path, target, current, "queue_sync")
        },
        |path, content| {
            agent_doc_snapshot_io::checkpoint_document_baseline(
                path,
                content,
                agent_doc_ops_log_io::log_op,
            )
        },
    )?;

    let lifecycle_changed = project_all_queue_runtime_state_with_effects(effects, file)?;
    let reconciled = effects.current_document_content(file, "queue_sync_debt")?;
    let plan =
        build_queue_sync_reconciliation_plan(file, &reconciled, previous_content.as_deref())?;
    let payload = &plan.payload;
    let action_debt_remains = !payload.is_empty() || !placement_debt.is_empty();

    if !routed_ids.is_empty() {
        println!(
            "{}: routed explicit backlog id(s) into their named queues: {}",
            file.display(),
            crate::one_shot_sync::format_queue_ids(&routed_ids),
        );
    }
    if let Some(warning) = routing_snapshot_warning {
        eprintln!("[queue sync] warning: failed to update explicit-route snapshot: {warning}");
    }
    for debt in &placement_debt {
        println!(
            "{}: nonterminal queue placement debt: item_id={} requested_queue_id={} eligible_queue_ids={}; operation=reconcile",
            file.display(),
            debt.item_id,
            debt.requested_queue_id
                .as_deref()
                .unwrap_or("<unspecified>"),
            debt.eligible_queue_ids.join(","),
        );
    }

    match backlog_sync {
        Some(OneShotQueueSyncResult::AlreadyInSync {
            requested_count,
            mode,
        }) => {
            if action_debt_remains {
                println!(
                    "{}: backlog mirror synchronized ({} active backlog id(s), {:?} mode); unresolved queue action debt remains.",
                    file.display(),
                    requested_count,
                    mode
                );
            } else {
                println!(
                    "{}: backlog mirror already synchronized ({} active backlog id(s), {:?} mode). No changes.",
                    file.display(),
                    requested_count,
                    mode
                );
            }
        }
        Some(OneShotQueueSyncResult::Synced(applied)) => {
            println!(
                "{}: synced {} backlog id(s) → {} queue prompt(s) ({:?} mode)",
                file.display(),
                applied.requested_count,
                applied.prompt_count,
                applied.mode
            );
            if !applied.already_present.is_empty() {
                println!(
                    "{}: skipped already represented backlog id(s): {} (reason: already_in_queue)",
                    file.display(),
                    crate::one_shot_sync::format_queue_ids(&applied.already_present)
                );
            }
            if !applied.newly_materialized.is_empty() {
                println!(
                    "{}: materialized backlog id(s): {}",
                    file.display(),
                    crate::one_shot_sync::format_queue_ids(&applied.newly_materialized)
                );
            }
            if let Some(warning) = applied.snapshot_warning {
                eprintln!("[queue sync] warning: failed to update snapshot: {warning}");
            }
        }
        None if completion_cell_repaired
            || response_metadata_repaired
            || waiting_tree_repaired
            || lifecycle_changed
            || answered_count > 0
            || action_debt_remains =>
        {
            println!(
                "{}: reconciled queue state (completion_cell_repaired={}, response_metadata_repaired={}, waiting_tree_repaired={}, lifecycle_projection_changed={}, captured_answered_free_text={}); no configured backlog mirror{}.",
                file.display(),
                completion_cell_repaired,
                response_metadata_repaired,
                waiting_tree_repaired,
                lifecycle_changed,
                answered_count,
                if action_debt_remains {
                    "; unresolved queue action debt remains"
                } else {
                    ""
                }
            );
        }
        None => {
            println!(
                "{}: queue projections already synchronized; no durable lifecycle change, captured answered free-text residue, or configured backlog mirror.",
                file.display()
            );
        }
    }

    if !payload.is_empty() {
        println!(
            "{}: nonterminal queue sync payload ({} unresolved item(s) or scan error(s)):",
            file.display(),
            payload.len(),
        );
        println!("{}", serde_json::to_string(&payload)?);
        println!("{QUEUE_SYNC_GUIDANCE}");
        for recovery in &plan.recovery_responses {
            println!("\n{}", recovery.render_markdown());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};

    #[test]
    fn orphaned_completion_never_regresses_to_bare() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let item = "#gh-fix https://github.com/example/project/issues/284";
        let completion =
            agent_doc_document::subagent_completion::cell("subagent:gh284", item, "orphan-key");
        let content = format!(
            "<!-- agent:exchange -->\n{completion}\n<!-- /agent:exchange -->\n\n<!-- agent:queue subagents -->\n- {item}\n- 🚧 worked but ledger lost\n- never started\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &content).unwrap();

        let projection = derive_claim_runtime_projection(&doc, &content, None).unwrap();
        assert!(
            projection
                .target_content
                .contains(&format!("- 🔄 {item}\n"))
        );
        assert!(
            projection
                .target_content
                .contains("- 📥 worked but ledger lost\n")
        );
        assert!(projection.target_content.contains("- 📥 never started\n"));
        assert!(!projection.target_content.contains("- 🔄 never started\n"));
        let unavailable = crate::queue_claim::claimed_items_for_content(&doc, &content);
        assert!(unavailable.claims(item));
        assert!(!unavailable.claims("worked but ledger lost"));
        assert!(!unavailable.claims("never started"));

        let closed = content.replace(&format!("- {item}\n"), &format!("- ~~{item}~~\n"));
        assert!(crate::queue_claim::completion_evidence_head_texts(&closed).is_empty());
    }

    /// A completion cell recorded as `do [#id] - <child>` shares the parent's
    /// tracked id and must not paint `🔄` on `do [#id]`, or retire when only
    /// the parent is struck. That parent is a nesting label, so projection
    /// leaves it unmarked rather than painting `📥`. The one-line row whose
    /// work is the cell item, and a child row with its own cell, still
    /// reconcile. Projecting the markers does not change the row count.
    #[test]
    fn completion_cell_for_a_longer_tracked_line_does_not_reconcile_the_parent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let parent = "do [#alpha]";
        let child = "unblock the sample";
        let composite = "do [#alpha] - unblock the sample";
        let cells = format!(
            "{}\n\n{}",
            agent_doc_document::subagent_completion::cell(
                "subagent:sample",
                composite,
                "composite-key"
            ),
            agent_doc_document::subagent_completion::cell("subagent:sample", child, "child-key"),
        );
        let content = format!(
            "<!-- agent:exchange -->\n{cells}\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- {parent}\n  - {child}\n- {composite}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        assert!(
            agent_doc_queue::queue_claim::completion_evidence_matches_row(
                parent,
                &format!("📥 {parent}\n  - {child}"),
            )
        );
        assert!(
            agent_doc_queue::queue_claim::completion_evidence_matches_row(
                "parent - child",
                "parent\n  - child",
            )
        );
        assert!(!agent_doc_queue::queue_claim::completion_evidence_matches_row(composite, parent,));

        let queue_len = |document: &str| {
            let components = agent_doc_element::element::parse(document).unwrap();
            let body = components
                .iter()
                .find(|component| component.name == "queue")
                .unwrap()
                .content(document);
            agent_doc_queue::document_queue::parse(body).unwrap().len()
        };
        assert_eq!(queue_len(&content), 3, "tracked parent keeps its child");

        let projection = derive_claim_runtime_projection(&doc, &content, None).unwrap();
        assert_eq!(queue_len(&projection.target_content), 3);
        assert!(
            projection.target_content.contains(&format!("- {parent}\n")),
            "{}",
            projection.target_content
        );
        assert!(
            !projection
                .target_content
                .contains(&format!("- 📥 {parent}\n")),
            "{}",
            projection.target_content
        );
        assert!(
            !projection
                .target_content
                .contains(&format!("- 🔄 {parent}\n")),
            "{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains(&format!("  - 🔄 {child}\n")),
            "{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains(&format!("- 🔄 {composite}\n")),
            "{}",
            projection.target_content
        );

        let held = crate::queue_claim::completion_evidence_head_texts(&content);
        let work = |head: &str| {
            agent_doc_element_queue::subprompts::head_without_subprompts(
                agent_doc_element_queue::strip_attempt_projection_prefix(head),
            )
            .trim()
            .to_string()
        };
        let held_work = held.iter().map(|head| work(head)).collect::<Vec<_>>();
        assert!(held_work.iter().any(|head| head == composite));
        assert!(held_work.iter().any(|head| head == child));
        assert!(!held_work.iter().any(|head| head == parent));

        let (parent_struck, parent_keys) =
            agent_doc_queue::queue_consume::retire_consumed_completion_feedback(
                &content,
                &[parent.to_string()],
            );
        assert!(parent_keys.is_empty(), "{parent_keys:?}");
        assert_eq!(
            agent_doc_document::subagent_completion::completion_evidence(&parent_struck).len(),
            2
        );
        let (line_struck, line_keys) =
            agent_doc_queue::queue_consume::retire_consumed_completion_feedback(
                &content,
                &[composite.to_string()],
            );
        assert!(line_keys.contains("composite-key"));
        assert!(!line_keys.contains("child-key"));
        let remaining = agent_doc_document::subagent_completion::completion_evidence(&line_struck);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].item, child);
    }

    #[derive(Default)]
    struct FakeEffects {
        current: RefCell<Option<String>>,
        captured_answered: Cell<usize>,
        reject_scaffold_during_capture: Cell<bool>,
        reject_convergence: Cell<bool>,
        required_preserved_line: RefCell<Option<String>>,
    }

    impl FakeEffects {
        fn with_current(content: impl Into<String>) -> Self {
            Self {
                current: RefCell::new(Some(content.into())),
                captured_answered: Cell::new(0),
                reject_scaffold_during_capture: Cell::new(false),
                reject_convergence: Cell::new(false),
                required_preserved_line: RefCell::new(None),
            }
        }

        fn strike_free_text_heads(
            file: &Path,
            count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            let mut updated = std::fs::read_to_string(file)?;
            let mut consumed = Vec::new();
            for _ in 0..count.max(1) {
                let Some(head) = active_queue_head_text(&updated)? else {
                    break;
                };
                if !matches!(
                    classify_active_queue_head(&updated)?,
                    ActiveQueueHeadKind::FreeText
                ) {
                    break;
                }
                let needle = format!("- {head}");
                let replacement = format!("- ~{head}~");
                updated = updated.replacen(&needle, &replacement, 1);
                consumed.push(head);
            }
            let Some(first) = consumed.first().cloned() else {
                return Ok(None);
            };
            // One write proves the batch is planned from one authoritative cut.
            std::fs::write(file, &updated)?;
            Ok(Some(QueueCommandConsumeOutcome {
                consumed_text: first,
                consumed_count: consumed.len(),
                remaining: usize::from(active_queue_head_text(&updated)?.is_some()),
                drained: false,
            }))
        }
    }

    impl QueueCommandEffects for FakeEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            match self.current.borrow().as_ref() {
                Some(content) => Ok(content.clone()),
                None => Ok(std::fs::read_to_string(file)?),
            }
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            if self.reject_convergence.get() {
                bail!("synthetic projection delivery refusal");
            }
            if let Some(line) = self.required_preserved_line.borrow().as_deref() {
                anyhow::ensure!(
                    target_content.lines().any(|candidate| candidate == line),
                    "target projection rewrote unrelated lifecycle drift: {target_content:?}"
                );
            }
            if let Some(current) = self.current.borrow().as_ref() {
                anyhow::ensure!(
                    current == source_content,
                    "projection must converge from the authoritative current cut"
                );
            }
            std::fs::write(file, target_content)?;
            if self.current.borrow().is_some() {
                *self.current.borrow_mut() = Some(target_content.to_string());
            }
            Ok(())
        }

        fn converge_document_or_disk_for_generation(
            &self,
            file: &Path,
            target_content: &str,
            source_content: &str,
            reason: &str,
            _generation: u64,
        ) -> Result<bool> {
            QueueCommandEffects::converge_document_or_disk(
                self,
                file,
                target_content,
                source_content,
                reason,
            )?;
            Ok(true)
        }

        fn consume_free_text_queue_prompts_force_disk(
            &self,
            file: &Path,
            count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            Self::strike_free_text_heads(file, count)
        }

        fn consume_free_text_queue_prompts_with_outcome(
            &self,
            file: &Path,
            count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            Self::strike_free_text_heads(file, count)
        }

        fn strike_orphan_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            Ok(true)
        }

        fn acknowledge_open_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            Ok(true)
        }

        fn acknowledge_paused_free_text_queue_head(
            &self,
            file: &Path,
            expected_head: &str,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            let mut content = std::fs::read_to_string(file)?;
            let needle = format!("- {}", expected_head.trim());
            if !content.lines().any(|line| line == needle) {
                bail!("exact queue head mismatch");
            }
            let replacement = format!("- ~~{}~~", expected_head.trim());
            content = content.replacen(&needle, &replacement, 1);
            std::fs::write(file, content)?;
            Ok(Some(QueueCommandConsumeOutcome {
                consumed_text: expected_head.trim().to_string(),
                consumed_count: 1,
                remaining: 0,
                drained: true,
            }))
        }

        fn prune_noise_queue_heads(&self, _file: &Path) -> Result<usize> {
            Ok(0)
        }

        fn converge_captured_answered_free_text_projection(&self, file: &Path) -> Result<usize> {
            if self.reject_scaffold_during_capture.get() {
                let content = self
                    .current
                    .borrow()
                    .clone()
                    .map_or_else(|| std::fs::read_to_string(file), Ok)?;
                agent_doc_template::guard_no_conversation_content_inside_tracked_components(
                    &content,
                )?;
            }
            Ok(self.captured_answered.get())
        }
    }

    struct RacingClaimEffects {
        current: Mutex<String>,
        discarded_convergences: AtomicUsize,
        first_convergence_round: Barrier,
    }

    impl RacingClaimEffects {
        fn new(current: String) -> Self {
            Self {
                current: Mutex::new(current),
                discarded_convergences: AtomicUsize::new(0),
                first_convergence_round: Barrier::new(2),
            }
        }

        fn current(&self) -> String {
            self.current.lock().unwrap().clone()
        }
    }

    impl QueueCommandEffects for RacingClaimEffects {
        fn current_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
            Ok(self.current())
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            let ordinal = self.discarded_convergences.fetch_add(1, Ordering::SeqCst);
            if ordinal < 2 {
                // Model two concurrent cell merges choosing the editor's
                // normalized text instead of either marker successor.
                self.first_convergence_round.wait();
                return Ok(());
            }
            *self.current.lock().unwrap() = target_content.to_string();
            std::fs::write(file, target_content)?;
            Ok(())
        }

        fn converge_document_or_disk_for_generation(
            &self,
            file: &Path,
            target_content: &str,
            source_content: &str,
            reason: &str,
            _generation: u64,
        ) -> Result<bool> {
            QueueCommandEffects::converge_document_or_disk(
                self,
                file,
                target_content,
                source_content,
                reason,
            )?;
            Ok(true)
        }

        fn consume_free_text_queue_prompts_force_disk(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            Ok(None)
        }

        fn consume_free_text_queue_prompts_with_outcome(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            Ok(None)
        }

        fn strike_orphan_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn acknowledge_open_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn acknowledge_paused_free_text_queue_head(
            &self,
            _file: &Path,
            _expected_head: &str,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            Ok(None)
        }

        fn prune_noise_queue_heads(&self, _file: &Path) -> Result<usize> {
            Ok(0)
        }
    }

    #[test]
    fn sync_reconciles_captured_answers_without_requiring_a_backlog_mirror() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- unanswered manual prompt\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // Exercise the production generation-fenced detached write path. A
        // synthetic retained editor cut would not observe the controller-owned
        // disk successor on the next authoritative retry.
        let effects = FakeEffects::default();
        effects.captured_answered.set(2);

        sync_with_effects(&effects, &doc)
            .expect("declarative sync must not require a backlog mirror directive");

        let reconciled = std::fs::read_to_string(&doc).unwrap();
        assert!(
            reconciled.contains("- 📥 unanswered manual prompt"),
            "sync should paint the pending lifecycle projection: {reconciled}"
        );
    }

    #[test]
    fn sync_unstrikes_legacy_waiting_owner_without_a_new_response() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "<!-- agent:queue id=\"main\" -->\n",
            "- ### Waiting on you — answer by adding a child line under an item: <!-- agent-doc:waiting-on-you -->\n",
            "  - ~~[#approve-pr] Approve and merge PR #42. <!-- agent-doc:waiting-item -->~~\n",
            "    - ~~**Response:** Approve and merge PR #42. <!-- agent-doc:waiting-metadata -->~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#approve-pr] Approve and merge PR #42.\n",
            "<!-- /agent:todo -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let effects = FakeEffects::with_current(content);

        sync_with_effects(&effects, &doc)
            .expect("declarative sync should repair a struck live waiting owner");

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(
            repaired.contains(
                "  - ⟦you: 1 · agent: 0 · done: 0⟧ [#approve-pr] Approve and merge PR #42. <!-- agent-doc:waiting-item -->\n"
            ),
            "{repaired}"
        );
        assert!(
            !repaired.contains(
                "~~[#approve-pr] Approve and merge PR #42. <!-- agent-doc:waiting-item -->~~"
            ),
            "{repaired}"
        );
        assert!(
            repaired
                .contains("- **Response:** Approve and merge PR #42. ▫️")
                && !repaired.contains("~~**Response:** Approve and merge PR #42."),
            "agent-doc reply rows remain inert without being struck: {repaired}"
        );
        assert!(
            !repaired.contains("agent-doc:waiting-metadata"),
            "{repaired}"
        );

        sync_with_effects(&effects, &doc).expect("the waiting repair should be idempotent");
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), repaired);
    }

    #[test]
    fn sync_recovers_completion_cell_welded_under_queue_row() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "- reported recovery\n",
            "  ### Re: Subagent completed: `subagent:old` returned work for queue item “old task” — agent-doc\n",
            "  > **Subagent response:**\n",
            "  >\n",
            "  > Complete.\n",
            "  > **Queue prompt:** old task\n",
            "  <!-- agent-doc-subagent-completion key=legacy-key -->\n",
            "- next task\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let effects = FakeEffects::with_current(content);
        effects.reject_scaffold_during_capture.set(true);

        sync_with_effects(&effects, &doc)
            .expect("queue sync should recover the cell before lifecycle parsing");

        let reconciled = std::fs::read_to_string(&doc).unwrap();
        let components = agent_doc_element::element::parse(&reconciled).unwrap();
        let queue = components
            .iter()
            .find(|component| component.name == "queue")
            .unwrap()
            .content(&reconciled);
        let exchange = components
            .iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&reconciled);
        assert!(
            !queue.contains("### Re: Subagent completed"),
            "{reconciled}"
        );
        assert!(
            !queue.contains("> **Queue prompt:** old task"),
            "{reconciled}"
        );
        assert!(
            exchange.contains("### Re: Subagent completed"),
            "{reconciled}"
        );
        assert!(
            exchange.contains(agent_doc_document::subagent_completion::RESPONSE_DIGEST_PREFIX),
            "{reconciled}"
        );
        agent_doc_template::guard_no_conversation_content_inside_tracked_components(&reconciled)
            .unwrap();

        sync_with_effects(&effects, &doc).expect("the recovery must be idempotent");
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), reconciled);
    }

    /// `#queuesyncterminalresponse`: an explicit full queue sync is a derived-state
    /// repaint, not an instruction to reopen an answered row.  In particular, the
    /// canonical visible response child is context for the terminal parent; it
    /// must not cause the parent to be treated as a live group and projected back
    /// to `Pending`.
    #[test]
    fn sync_preserves_terminal_row_with_current_and_retained_response_children() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "<!-- agent:queue id=\"prelude\" -->\n",
            "<!-- /agent:queue -->\n",
            "## Run <!-- agent:run id=\"main\" subagents preset=\"#spec-test-build-install-commit-push: approve all worktree removals\" priority go -->\n",
            "- ~~The response id belongs in every related response header. [#q-f07f85cbaefd] ⟦local⟧~~ — auto-struck: answered this cycle (#ftstrike); response: [#r-0648593c3bc4] ⟦addressed-at:2026-11-01T12:08:38.000Z⟧\n",
            "  - **Response [#r-0648593c3bc4]:** Added the response id to each related header.\n",
            "  - 📥 **Response [#r-fedcba987654]:** Older retained response context.\n",
            "- another live row\n",
            "<!-- /agent:run -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let effects = FakeEffects::with_current(content);

        sync_with_effects(&effects, &doc).expect("terminal response evidence must be sync-safe");

        let expected = content
            .replace(
                "  - 📥 **Response [#r-fedcba987654]:**",
                "  - **Response [#r-fedcba987654]:**",
            )
            .replace("- another live row\n", "- 📥 another live row\n");
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            expected,
            "queue sync may paint live siblings but must not reopen or canonicalize a terminal row"
        );

        sync_with_effects(&effects, &doc).expect("response metadata repair must be idempotent");
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), expected);
    }

    #[test]
    fn sync_plan_is_read_only_and_uses_shared_document_generic_inventory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("generic.md");
        let content =
            "<!-- agent:run id=recovery subagents -->\n- ⚠️ orphan attempt\n<!-- /agent:run -->\n";
        std::fs::write(&doc, content).unwrap();
        let effects = FakeEffects::with_current(content);

        plan_with_effects(&effects, &doc).unwrap();

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
        let plan = build_queue_sync_reconciliation_plan(&doc, content, None).unwrap();
        assert_eq!(plan.payload.reconcile_unknown.len(), 1);
        assert_eq!(plan.recovery_responses.len(), 1);
        assert!(
            plan.recovery_responses[0]
                .render_markdown()
                .starts_with("**Response [#r-")
        );
        assert!(plan.recovery_responses[0].choices.is_empty());
    }

    #[test]
    fn sync_payload_keeps_other_ready_dispatches_visible_while_one_worker_runs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:run subagents go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "<!-- /agent:run -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let (_, claim) = crate::queue_claim::claim(&doc, "do [#a]", "subagent:a", 600).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            "do [#a]",
            "subagent:a",
            "/root/a",
            600,
            claim.generation,
        )
        .unwrap();
        let current = std::fs::read_to_string(&doc).unwrap();

        let payload = queue_sync_reconciliation_payload(&doc, &current, None).unwrap();

        assert_eq!(payload.running.len(), 1);
        assert_eq!(payload.pending.len(), 1);
        assert_eq!(
            payload.running[0].operation,
            agent_doc_queue::queue_sync_payload::QueueSyncOperation::Wait
        );
        assert_eq!(
            payload.pending[0].operation,
            agent_doc_queue::queue_sync_payload::QueueSyncOperation::Dispatch
        );
        assert_eq!(
            payload.pending[0].execution_mode,
            agent_doc_queue::queue_sync_payload::QueueExecutionMode::SubagentDispatch
        );
    }

    #[test]
    fn sync_routes_named_backlog_before_applying_legacy_mirror() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:run id=triage -->\n<!-- /agent:run -->\n",
            "<!-- agent:run id=build -->\n<!-- /agent:run -->\n",
            "<!-- agent:backlog queue=build -->\n",
            "- [ ] [#compile] compile release\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#classify] classify incident\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();

        sync_with_effects(&FakeEffects::default(), &doc)
            .expect("explicit routing and the legacy mirror should compose");

        let synced = std::fs::read_to_string(&doc).unwrap();
        let components = agent_doc_element::element::parse(&synced).unwrap();
        let queues = agent_doc_queue::queue_set::QueueSet::parse(&synced, &components).unwrap();
        let triage = queues.block("triage").unwrap().component.content(&synced);
        let build = queues.block("build").unwrap().component.content(&synced);
        assert!(triage.contains("do [#classify]"), "{synced}");
        assert!(!triage.contains("do [#compile]"), "{synced}");
        assert!(build.contains("do [#compile]"), "{synced}");
        assert!(!build.contains("do [#classify]"), "{synced}");
    }

    #[test]
    fn sync_accepts_enqueue_marker_without_queue_attr() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] :inbox_tray: add me\n",
            "- [ ] [#beta] leave me alone\n",
            "- [/] [#gated] /enqueue blocked\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects::default();
        sync_with_effects(&effects, &doc).expect("enqueue marker should append to queue");
        let result = std::fs::read_to_string(&doc).unwrap();

        assert!(
            result.contains("- ### 📥 do [#alpha]"),
            "marked item should be queued with pending lifecycle state:\n{result}"
        );
        assert!(
            !result.contains("- do [#beta]"),
            "unmarked item must not be queued:\n{result}"
        );
        assert!(
            !result.contains("- do [#gated]"),
            "gated marker must not be queued:\n{result}"
        );
    }

    #[test]
    fn consume_strikes_multiple_answered_free_text_heads() {
        // #multi-head-consume-one-per-finalize: a single turn answered two
        // free-text heads; `queue consume --count 2` drains both stragglers
        // deterministically while leaving the trailing id-backed head intact.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: head one\n\nDone.\n",
            "### Re: head two\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- head one free text\n",
            "- head two free text\n",
            "- do [#keepme]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects::default();
        consume_with_options(&effects, &doc, 2, ConsumeOptions { force_disk: true })
            .expect("consume two answered free-text heads");
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("~head one free text~"),
            "head one must be struck:\n{result}"
        );
        assert!(
            result.contains("~head two free text~"),
            "head two must be struck:\n{result}"
        );
        assert!(
            result.contains("- do [#keepme]"),
            "trailing id-backed head must be preserved:\n{result}"
        );
    }

    #[test]
    fn consume_stops_at_id_backed_head_after_striking_free_text() {
        // count overruns the free-text run: strike the one free-text head, then
        // stop cleanly at the id-backed head instead of desyncing it.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n### Re: only free\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- only free head\n",
            "- do [#keepme]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects::default();
        consume_with_options(&effects, &doc, 5, ConsumeOptions { force_disk: true })
            .expect("consume should stop at the id-backed head");
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("~only free head~"),
            "free head struck:\n{result}"
        );
        assert!(
            result.contains("- do [#keepme]"),
            "id-backed head preserved:\n{result}"
        );
    }

    #[test]
    fn consume_treats_bare_bracket_id_head_as_id_backed() {
        // Regression: a bare `[#id]` head (no `do` prefix) must be classified
        // id-backed via topic_resolves_to_exact_id, not struck as free text.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- [#admin-recover]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects::default();
        let err = consume(&effects, &doc, 1).unwrap_err();
        assert!(
            err.to_string().contains("id-backed"),
            "a bare [#id] head must be refused, not struck: {err}"
        );
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- [#admin-recover]") && !result.contains("~[#admin-recover]~"),
            "the id-backed head must be left intact:\n{result}"
        );
    }

    #[test]
    fn consume_treats_bare_hash_id_without_open_work_as_free_text() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- #unknown-preset\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#different-work] still open\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects::default();
        consume(&effects, &doc, 1).expect("an unowned bare #id is a consumable free-text head");
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- ~#unknown-preset~"),
            "the bare hash head must be struck instead of refused as id-backed:\n{result}"
        );
    }

    #[test]
    fn consume_bails_on_leading_id_backed_head() {
        // An id-backed head must be reaped via --done, never struck blind here.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#someid]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects::default();
        let err = consume(&effects, &doc, 1).unwrap_err();
        assert!(
            err.to_string().contains("id-backed"),
            "should refuse a leading id-backed head: {err}"
        );
    }

    fn write_claim_projection_doc(root: &Path, body: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("claim-runtime.md");
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             <!-- agent:queue go -->\n{body}<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    fn write_named_claim_projection_doc(root: &Path, body: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("claim-runtime.md");
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             <!-- agent:run id=\"main\" subagents go -->\n{body}<!-- /agent:run -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    /// The 2026-10-26 `tasks/agent-doc/agent-doc-bugs.md` shape: an immediate
    /// `main` run queue followed by an hourly `issues` loop that displays
    /// copies of the same work.
    fn two_queue_doc(main: &str, issues: &str) -> String {
        format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             ## Queue: Main\n<!-- agent:run id=\"main\" subagents priority go -->\n{main}<!-- /agent:run -->\n\n\
             ## Run: Issues\n<!-- agent:loop id=issues interval=1h priority -->\n{issues}<!-- /agent:loop -->\n"
        )
    }

    fn write_two_queue_doc(root: &Path, main: &str, issues: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("mirror-markers.md");
        let content = two_queue_doc(main, issues);
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    /// `#queuemirrormarkers` cases 1-3. A claim is keyed by work identity, so
    /// every queue that displays the identity shows the ledger's authoritative
    /// attempt; a copy with no claim, receipt, or completion cell is pending.
    #[test]
    fn claim_projection_reaches_every_queue_displaying_the_identity() {
        let dir = tempfile::tempdir().unwrap();
        let onboarding = "We need to work on making agent-doc easy to onboard.";
        let doc = write_two_queue_doc(
            dir.path(),
            "- 📥 do [#layoutunprovenliveverify]\n\
             - ~~release + publish ⟦effect:1⟧~~\n\
             - 📥 release + publish ⟦effect:2⟧\n",
            &format!(
                "- 🚧 [#subagent:layout_liveverify#1] do [#layoutunprovenliveverify]\n\
                 - ☑️ [#subagent:release_publish_patch#3] release + publish\n\
                 - 🚧 {onboarding}\n"
            ),
        );
        // Generation 1 expires; the coordinator re-dispatches generation 2 from
        // the main queue.
        {
            let _clock = crate::queue_claim::pin_claim_clock_for_current_thread(1_000);
            crate::queue_claim::claim(
                &doc,
                "do [#layoutunprovenliveverify]",
                "subagent:layout_liveverify",
                60,
            )
            .unwrap();
        }
        let _clock = crate::queue_claim::pin_claim_clock_for_current_thread(2_000);
        let (outcome, claim) = crate::queue_claim::claim(
            &doc,
            "do [#layoutunprovenliveverify]",
            "subagent:layout_liveverify",
            3_600,
        )
        .unwrap();
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        assert_eq!(claim.generation, 2);

        let current = std::fs::read_to_string(&doc).unwrap();
        let projection = derive_claim_runtime_projection(&doc, &current, None).unwrap();
        assert!(projection.changed);
        assert_eq!(
            projection.target_content,
            two_queue_doc(
                "- 🟡 [#subagent:layout_liveverify#2] do [#layoutunprovenliveverify]\n\
                 - ~~release + publish ⟦effect:1⟧~~\n\
                 - 📥 release + publish ⟦effect:2⟧\n",
                &format!(
                    // Case 1: the loop copy shows the live generation, not the
                    // expired #1. Case 2: the reported row is terminal document
                    // authority even after its attempt is gone. Case 3: a `🚧`
                    // with no worker binding and no in-session selection is not
                    // a lifecycle state.
                    "- 🟡 [#subagent:layout_liveverify#2] do [#layoutunprovenliveverify]\n\
                     - ☑️ [#subagent:release_publish_patch#3] release + publish\n\
                     - 📥 {onboarding}\n"
                ),
            ),
        );
        let settled =
            derive_claim_runtime_projection(&doc, &projection.target_content, None).unwrap();
        assert!(!settled.changed, "the projection is idempotent");
    }

    /// `#queuemirrormarkers`: releasing or ending an attempt clears it from
    /// every queue, and a sibling queue's struck row (an `agent:loop` keeps its
    /// pass-owned struck rows for restore) is never reopened by another
    /// queue's live claim.
    #[test]
    fn claim_release_clears_every_queue_and_never_reopens_a_sibling_struck_row() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_two_queue_doc(
            dir.path(),
            "- do [#jb263]\n",
            "- do [#jb263]\n- ~~do [#jb263]~~\n",
        );
        crate::queue_claim::claim(&doc, "#jb263", "subagent:jb263_feed_verify", 3_600).unwrap();
        let current = std::fs::read_to_string(&doc).unwrap();
        let claimed = derive_claim_runtime_projection(&doc, &current, None).unwrap();
        assert_eq!(
            claimed.target_content,
            two_queue_doc(
                "- 🟡 [#subagent:jb263_feed_verify#1] do [#jb263]\n",
                "- 🟡 [#subagent:jb263_feed_verify#1] do [#jb263]\n- ~~do [#jb263]~~\n",
            ),
        );

        crate::queue_claim::release(&doc, "#jb263").unwrap();
        let released =
            derive_claim_runtime_projection(&doc, &claimed.target_content, None).unwrap();
        assert_eq!(
            released.target_content,
            two_queue_doc(
                "- 📥 do [#jb263]\n",
                "- 📥 do [#jb263]\n- ~~do [#jb263]~~\n"
            ),
        );
    }

    /// Preflight projects its selected queue with pass-local evidence and asks
    /// for the siblings only; the selected queue's bytes stay its own.
    #[test]
    fn sibling_scope_leaves_the_projected_queue_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_two_queue_doc(dir.path(), "- 🚧 do [#a]\n", "- 🚧 do [#a]\n");
        let current = std::fs::read_to_string(&doc).unwrap();
        let selected = vec!["do [#a]".to_string()];
        let projection = derive_claim_runtime_projection_scoped(
            &doc,
            &current,
            None,
            ClaimProjectionScope::SiblingsOf {
                projected_queue_id: "main",
                selected_heads: &selected,
            },
        )
        .unwrap();
        assert!(
            !projection.changed,
            "a sibling copy of the pass's selected head keeps the same 🚧"
        );
        let projection = derive_claim_runtime_projection_scoped(
            &doc,
            &current,
            None,
            ClaimProjectionScope::SiblingsOf {
                projected_queue_id: "main",
                selected_heads: &[],
            },
        )
        .unwrap();
        assert_eq!(
            projection.target_content,
            two_queue_doc("- 🚧 do [#a]\n", "- 📥 do [#a]\n")
        );
    }

    #[test]
    fn claim_controller_repairs_torn_duplicate_owner_before_projection_guard() {
        let dir = tempfile::tempdir().unwrap();
        let torn_item = "Several of this queue's items were in a ❔ state. Now the incorrect 📥 state. They should be in the ☑️ state. Fix agent-doc causes.";
        let next_item = "sdk demo follow-up";
        let body = format!(
            "- 🟡 [#subagent:queue-state-fix#3] [#subagent:queue-state-fix#3📥 {torn_item}\n- 📥 {next_item}\n"
        );
        let doc = write_claim_projection_doc(dir.path(), &body);
        crate::queue_claim::claim(&doc, next_item, "subagent:sdk-demo", 600).unwrap();

        let current = std::fs::read_to_string(&doc).unwrap();
        let projection = derive_claim_runtime_projection(&doc, &current, None)
            .expect("controller projection must repair the proven torn duplicate owner");

        assert!(projection.changed);
        assert!(
            projection
                .target_content
                .contains(&format!("- 📥 {torn_item}\n")),
            "{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains(&format!("- 🟡 [#subagent:sdk-demo#1] {next_item}\n")),
            "{}",
            projection.target_content
        );
    }

    #[test]
    fn claim_start_report_and_release_project_the_complete_visible_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_claim_projection_doc(
            dir.path(),
            "- 🚧 inline coordinator work\n- dispatched worker work\n",
        );
        crate::queue_claim::claim(&doc, "dispatched worker work", "subagent:worker", 600).unwrap();

        let effects = FakeEffects::default();
        assert!(project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap());
        let claimed = std::fs::read_to_string(&doc).unwrap();
        assert!(
            claimed.contains("- 📥 inline coordinator work"),
            "{claimed}"
        );
        assert!(
            claimed.contains("- 🟡 [#subagent:worker#1] dispatched worker work"),
            "{claimed}"
        );
        let claimed_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            claimed_snapshot.contains("- 🟡 [#subagent:worker#1] dispatched worker work"),
            "{claimed_snapshot}"
        );

        crate::queue_claim::start_attempt(
            &doc,
            "dispatched worker work",
            "subagent:worker",
            "worker:runtime-projection",
            600,
            1,
        )
        .unwrap();
        assert!(project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap());
        let running = std::fs::read_to_string(&doc).unwrap();
        assert!(
            running.contains("- 🚧 [#subagent:worker#1] dispatched worker work"),
            "{running}"
        );

        let reported =
            crate::queue_claim::report_attempt(&doc, "dispatched worker work", Some(1)).unwrap();
        crate::queue_lineage::record_prose_only(&doc, &reported, "test prose result").unwrap();
        let drift = claim_reports_with_effects(&effects, &doc).unwrap();
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].visible_projection_state, "out_of_sync");
        assert_eq!(drift[0].expected_visible_marker, "☑️");
        assert_eq!(drift[0].observed_visible_markers, ["🚧"]);
        assert_eq!(
            drift[0].next_action_argv,
            Some(vec![
                "agent-doc".to_string(),
                "queue".to_string(),
                "repair-projection".to_string(),
                doc.to_string_lossy().into_owned(),
            ]),
        );
        assert!(project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap());
        let reported = std::fs::read_to_string(&doc).unwrap();
        assert!(
            reported.contains("- ☑️ [#subagent:worker#1] dispatched worker work"),
            "{reported}"
        );
        let synchronized = claim_reports_with_effects(&effects, &doc).unwrap();
        assert_eq!(synchronized[0].visible_projection_state, "synchronized");
        // `#reportedcloseout`: a synchronized reported attempt is closeout
        // debt, so its next action is the first closeout step, never empty.
        let closeout = synchronized[0]
            .reported_closeout
            .as_ref()
            .expect("reported attempt carries its closeout");
        assert_eq!(
            synchronized[0].next_action_argv.as_deref(),
            closeout.next_action_argv(),
        );
        assert_eq!(
            &synchronized[0].next_action_argv.as_ref().unwrap()[..3],
            ["agent-doc", "queue", "consume"],
            "{closeout:#?}"
        );

        crate::claim_release_receipt::capture_response_for_attempt(
            &doc,
            "dispatched worker work",
            1,
            "worker response",
        )
        .unwrap();
        let released = crate::claim_release_receipt::release_attempt(
            &doc,
            "dispatched worker work",
            crate::claim_release_receipt::ReleaseKind::Reported,
            Some(1),
        )
        .unwrap()
        .unwrap();
        project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap();
        let released_projection = std::fs::read_to_string(&doc).unwrap();
        assert!(
            released_projection.contains("- 📥 inline coordinator work"),
            "{released_projection}"
        );
        assert!(
            released_projection.contains("- ☑️ [#subagent:worker#1] dispatched worker work")
                && !released_projection.contains("- 🟡 dispatched worker work"),
            "{released_projection}"
        );
        let released_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            released_snapshot.contains("- 📥 inline coordinator work")
                && released_snapshot.contains("- ☑️ [#subagent:worker#1] dispatched worker work")
                && !released_snapshot.contains("- 🟡 dispatched worker work"),
            "{released_snapshot}"
        );
        assert_eq!(released.item_text, "dispatched worker work");
    }

    #[test]
    fn failed_claim_projection_rolls_back_admission_before_spawn_acknowledgement() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_claim_projection_doc(dir.path(), "- dispatch me\n");
        let effects = FakeEffects::default();
        effects.reject_convergence.set(true);

        let error = claim_with_runtime_projection(
            &effects,
            &doc,
            "dispatch me",
            "subagent:projection-refused",
            600,
        )
        .unwrap_err();

        assert!(
            format!("{error:#}").contains("no worker may be spawned"),
            "{error:#}"
        );
        assert!(
            crate::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .is_empty(),
            "a failed visible projection must not strand a durable claim"
        );
        assert!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .contains("- dispatch me\n"),
            "the row remains visibly unclaimed"
        );
    }

    #[test]
    fn claim_and_clean_requeue_ignore_unrelated_lifecycle_projection_drift() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_claim_projection_doc(
            dir.path(),
            "- dispatch me\n- 🚧 unrelated historical drift\n",
        );
        let effects = FakeEffects::default();
        *effects.required_preserved_line.borrow_mut() =
            Some("- 🚧 unrelated historical drift".to_string());

        let (_, claim) = claim_with_runtime_projection(
            &effects,
            &doc,
            "dispatch me",
            "subagent:target-scoped",
            600,
        )
        .expect("unrelated drift cannot veto target admission");
        let claimed = std::fs::read_to_string(&doc).unwrap();
        assert!(
            claimed.contains("- 🟡 [#subagent:target-scoped#1] dispatch me\n"),
            "{claimed}"
        );
        assert!(
            claimed.contains("- 🚧 unrelated historical drift\n"),
            "{claimed}"
        );

        crate::claim_release_receipt::release_attempt(
            &doc,
            &claim.item_text,
            crate::claim_release_receipt::ReleaseKind::Requeue,
            Some(claim.generation),
        )
        .unwrap();
        project_claim_item_runtime_state_with_effects(&effects, &doc, &claim.item_text, true)
            .expect("unrelated drift cannot veto clean requeue projection");
        let released = std::fs::read_to_string(&doc).unwrap();
        assert!(released.contains("- 📥 dispatch me\n"), "{released}");
        assert!(
            released.contains("- 🚧 unrelated historical drift\n"),
            "{released}"
        );
        assert!(
            crate::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .is_empty()
        );
    }

    #[test]
    fn claim_and_start_project_an_active_counterpart_with_a_dedicated_status() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("claim-active.md");
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Status <!-- agent:status -->\n<!-- /agent:status -->\n\n",
            "## Active <!-- agent:active priority -->\n<!-- /agent:active -->\n\n",
            "## Run <!-- agent:run id=\"main\" subagents go -->\n",
            "- dispatched worker work\n",
            "<!-- /agent:run -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        crate::queue_claim::claim(&doc, "dispatched worker work", "subagent:worker", 600).unwrap();
        let effects = FakeEffects::default();
        assert!(project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap());
        let claimed = std::fs::read_to_string(&doc).unwrap();
        assert!(claimed.contains("- **Queue** (1)"), "{claimed}");
        assert!(claimed.contains("- **Status:** claimed"), "{claimed}");
        assert!(!claimed.contains("**Sources:** queue"), "{claimed}");

        crate::queue_claim::start_attempt(
            &doc,
            "dispatched worker work",
            "subagent:worker",
            "worker:active-projection",
            600,
            1,
        )
        .unwrap();
        assert!(project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap());
        let running = std::fs::read_to_string(&doc).unwrap();
        assert!(running.contains("- **Status:** running"), "{running}");
        assert!(!running.contains("- **Status:** claimed"), "{running}");
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(snapshot.contains("- **Status:** running"), "{snapshot}");
    }

    /// `#managedlistnodrift`: through the real state.db loader, `agent:active`
    /// follows the durable claim lifecycle (claimed and running under Running,
    /// reported under Needs integration with an age), whatever marker the row
    /// shows, and a second render is a no-op.
    #[test]
    fn durable_active_follows_claim_start_report_regardless_of_row_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("durable-active.md");
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Active <!-- agent:active priority -->\n<!-- /agent:active -->\n\n",
            "## Run <!-- agent:run id=\"main\" subagents go -->\n",
            "- ☑️ dispatched worker work\n",
            "<!-- /agent:run -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let render = |content: &str| {
            let lifecycle = crate::active_lifecycle::load(&doc).unwrap();
            let rendered =
                agent_doc_work_projection::reconcile_managed_active_items_with(content, &lifecycle)
                    .unwrap()
                    .unwrap_or_else(|| content.to_string());
            assert!(
                agent_doc_work_projection::reconcile_managed_active_items_with(&rendered, &lifecycle)
                    .unwrap()
                    .is_none(),
                "a second render is a no-op"
            );
            rendered
        };
        // A stale `☑️` with no durable record is not active work.
        let unbacked = render(content);
        assert!(!unbacked.contains("dispatched worker work\n    - **Status"), "{unbacked}");

        crate::queue_claim::claim(&doc, "dispatched worker work", "subagent:worker", 600).unwrap();
        let claimed = render(content);
        assert!(claimed.contains("- **Running** (1)\n"), "{claimed}");
        assert!(claimed.contains("- **Status:** claimed"), "{claimed}");

        let running = crate::queue_claim::start_attempt(
            &doc,
            "dispatched worker work",
            "subagent:worker",
            "worker:durable-active",
            600,
            1,
        )
        .unwrap();
        assert!(render(content).contains("- **Status:** running"));

        crate::queue_claim::report_attempt(&doc, &running.item_text, Some(running.generation))
            .unwrap();
        let reported = render(content);
        assert!(reported.contains("- **Needs integration** (1)\n"), "{reported}");
        assert!(reported.contains("- **Status:** reported\n    - **Age:** since attached "), "{reported}");
        assert!(!reported.contains("- **Running**"), "{reported}");
    }

    /// A completed occurrence may be removed and later reintroduced with the
    /// same stable prompt id. The id names the work across attempts; the
    /// generation names the attempt. A focused claim/start projection must
    /// therefore replace the stale terminal `agent:active` status immediately
    /// while leaving the same leaf under another parent untouched.
    #[test]
    fn reintroduced_stable_id_projects_fresh_generation_into_queue_and_active() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("claim-active-reintroduced.md");
        let source = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Active <!-- agent:active priority -->\n<!-- /agent:active -->\n\n",
            "## Run <!-- agent:run id=\"main\" subagents go -->\n",
            "- https://example.test/pull/1\n",
            "  - #address\n",
            "- https://example.test/pull/2\n",
            "  - #address\n",
            "<!-- /agent:run -->\n",
        );
        let admitted = agent_doc_queue::prompt_identity::project(source, "session:claim-reopen");
        assert_eq!(admitted.assignments.len(), 2);
        let first_id = admitted.assignments[0].id.clone();
        let second_id = admitted.assignments[1].id.clone();
        assert_ne!(
            first_id, second_id,
            "different parent vectors need distinct ids"
        );
        std::fs::write(&doc, &admitted.content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &admitted.content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects::default();
        let first_handle = format!("#{first_id}");
        let first_claim = {
            let _clock = crate::queue_claim::pin_claim_clock_for_current_thread(1_000);
            let (_, claim) =
                claim_with_runtime_projection(&effects, &doc, &first_handle, "subagent:first", 60)
                    .unwrap();
            let running = crate::queue_claim::start_attempt(
                &doc,
                &claim.item_text,
                "subagent:first",
                "worker:first",
                60,
                claim.generation,
            )
            .unwrap();
            crate::queue_claim::report_attempt(&doc, &running.item_text, Some(running.generation))
                .unwrap()
        };
        project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap();
        let reported = std::fs::read_to_string(&doc).unwrap();
        assert!(reported.contains("- **Status:** reported"), "{reported}");

        crate::queue_claim::cancel_removed_attempt(
            &doc,
            &first_claim.item_text,
            Some(first_claim.generation),
        )
        .unwrap();

        // Recreate the exact first parent vector without copying its id. Stable
        // identity admission must recover its old id, while pull/2 keeps its
        // unrelated identity.
        let old_first = reported
            .lines()
            .find(|line| line.contains('☑') && line.contains(&format!("[#{}]", first_id)))
            .expect("reported first row");
        let reintroduced_raw = reported.replacen(old_first, "  - #address", 1);
        let reintroduced =
            agent_doc_queue::prompt_identity::project(&reintroduced_raw, "session:claim-reopen");
        assert_eq!(reintroduced.assignments.len(), 1);
        assert_eq!(reintroduced.assignments[0].id, first_id);
        assert!(
            reintroduced.content.contains(&format!("[#{}]", second_id)),
            "unrelated parent identity was lost: {}",
            reintroduced.content
        );
        std::fs::write(&doc, &reintroduced.content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &reintroduced.content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _clock = crate::queue_claim::pin_claim_clock_for_current_thread(2_000);
        let (_, second_claim) =
            claim_with_runtime_projection(&effects, &doc, &first_handle, "subagent:second", 600)
                .unwrap();
        assert_eq!(second_claim.generation, 2);
        let running = crate::queue_claim::start_attempt(
            &doc,
            &second_claim.item_text,
            "subagent:second",
            "worker:second",
            600,
            second_claim.generation,
        )
        .unwrap();
        project_claim_item_runtime_state_with_effects(&effects, &doc, &running.item_text, false)
            .unwrap();

        let projected = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            projected
                .matches(&format!("🚧 [#subagent:second#2] #address [#{}]", first_id))
                .count(),
            1,
            "one fresh generation must own the reintroduced occurrence: {projected}"
        );
        assert!(!projected.contains("[#subagent:first#1]"), "{projected}");
        assert!(projected.contains("- **Status:** running"), "{projected}");
        assert!(!projected.contains("- **Status:** reported"), "{projected}");
        assert_eq!(
            crate::queue_claim::load_ledger(&doc).unwrap().claims.len(),
            1,
            "the retry must replace, not duplicate, worker ownership"
        );
    }

    /// `#queueprojectiondrift`: text-identical review leaves under different
    /// pull-request parents retain their own worker bindings. Projection must
    /// never show pull 11 with pull 10's owner merely because their leaf text
    /// is shared.
    #[test]
    fn repeated_nested_leaf_projects_the_owner_of_its_exact_parent_vector() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_claim_projection_doc(
            dir.path(),
            concat!(
                "- https://github.com/haiven-dev/haiven-cli/pull/10\n",
                "  - #adversarial-pr\n",
                "- https://github.com/haiven-dev/haiven-cli/pull/11\n",
                "  - #adversarial-pr\n",
            ),
        );
        let pull10 = "https://github.com/haiven-dev/haiven-cli/pull/10: #adversarial-pr";
        let pull11 = "https://github.com/haiven-dev/haiven-cli/pull/11: #adversarial-pr";
        let owner10 = "subagent:adversarial-pr-cli10";
        let owner11 = "subagent:adversarial-pr-cli11";
        crate::queue_claim::claim(&doc, pull10, owner10, 600).unwrap();
        crate::queue_claim::claim(&doc, pull11, owner11, 600).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            pull10,
            owner10,
            "/root/adversarial-pr-cli10",
            600,
            1,
        )
        .unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            pull11,
            owner11,
            "/root/adversarial-pr-cli11",
            600,
            1,
        )
        .unwrap();

        let current = std::fs::read_to_string(&doc).unwrap();
        let projection = derive_claim_runtime_projection(&doc, &current, None).unwrap();
        assert!(projection.target_content.contains(&format!(
            "- https://github.com/haiven-dev/haiven-cli/pull/10\n  - 🚧 [#{owner10}#1] #adversarial-pr\n"
        )));
        assert!(projection.target_content.contains(&format!(
            "- https://github.com/haiven-dev/haiven-cli/pull/11\n  - 🚧 [#{owner11}#1] #adversarial-pr\n"
        )));
        assert!(
            !projection
                .target_content
                .contains(&format!("pull/11\n  - 🚧 [#{owner10}#1]"))
        );
    }

    /// `#adnestedidenticalchildprojection`: the exact `docs.md` PR 201-205
    /// shape keeps every identical executable child through claim/start
    /// projection. If an older binary already dropped PR 202-205's children,
    /// the durable lineage claims restore those rows and their existing worker
    /// bindings without allocating another attempt.
    #[test]
    fn docs_pr_201_205_nested_children_survive_and_repair_from_claim_lineage() {
        let dir = tempfile::tempdir().unwrap();
        let body = (201..=205)
            .map(|number| {
                format!(
                    "- ### https://github.com/haiven-dev/haiven-sdk/pull/{number}\n  - #adversarial-pr\n"
                )
            })
            .collect::<String>();
        let doc = write_named_claim_projection_doc(dir.path(), &body);

        for number in 202..=205 {
            let item =
                format!("https://github.com/haiven-dev/haiven-sdk/pull/{number}: #adversarial-pr");
            let owner = format!("subagent:pr{number}");
            let worker = format!("/root/pr{number}");
            crate::queue_claim::claim(&doc, &item, &owner, 600).unwrap();
            crate::queue_claim::start_attempt(&doc, &item, &owner, &worker, 600, 1).unwrap();
        }

        let current = std::fs::read_to_string(&doc).unwrap();
        let projection = derive_claim_runtime_projection(&doc, &current, None).unwrap();
        assert_eq!(
            projection.target_content.matches("#adversarial-pr").count(),
            5,
            "every PR keeps its distinct child: {}",
            projection.target_content
        );
        for number in 202..=205 {
            assert!(
                projection.target_content.contains(&format!(
                    "- ### https://github.com/haiven-dev/haiven-sdk/pull/{number}\n  - 🚧 [#subagent:pr{number}#1] #adversarial-pr\n"
                )),
                "{}",
                projection.target_content
            );
        }

        let damaged_body = concat!(
            "- ### https://github.com/haiven-dev/haiven-sdk/pull/201\n",
            "  - #adversarial-pr\n",
            "- ### https://github.com/haiven-dev/haiven-sdk/pull/202\n",
            "- ### https://github.com/haiven-dev/haiven-sdk/pull/203\n",
            "- ### https://github.com/haiven-dev/haiven-sdk/pull/204\n",
            "- ### https://github.com/haiven-dev/haiven-sdk/pull/205\n",
        );
        let damaged = format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             <!-- agent:run id=\"main\" subagents go -->\n{damaged_body}<!-- /agent:run -->\n"
        );
        std::fs::write(&doc, &damaged).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &damaged,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let before = crate::queue_claim::load_ledger(&doc).unwrap();

        assert!(
            project_claim_runtime_state_with_effects(&FakeEffects::default(), &doc, None).unwrap()
        );
        let repaired = std::fs::read_to_string(&doc).unwrap();
        for number in 202..=205 {
            assert!(
                repaired.contains(&format!(
                    "- ### https://github.com/haiven-dev/haiven-sdk/pull/{number}\n  - 🚧 [#subagent:pr{number}#1] #adversarial-pr\n"
                )),
                "{repaired}"
            );
        }
        let after = crate::queue_claim::load_ledger(&doc).unwrap();
        assert_eq!(
            after.claims, before.claims,
            "repair must not redispatch work"
        );
    }

    /// A parked parent has no executable child and no claim. A stale running
    /// glyph is presentation drift, not worker liveness, and projection clears
    /// it without disturbing the operator's parked comment.
    #[test]
    fn parked_parent_without_a_claim_never_projects_as_running() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_claim_projection_doc(
            dir.path(),
            "- 🚧 https://github.com/haiven-dev/haiven-cli/pull/7\n  - %% parked %%\n",
        );
        let current = std::fs::read_to_string(&doc).unwrap();
        let projection = derive_claim_runtime_projection(&doc, &current, None).unwrap();
        assert!(projection.changed);
        assert!(!projection.target_content.contains("🚧"));
        assert!(
            projection.target_content.contains(
                "- 📥 https://github.com/haiven-dev/haiven-cli/pull/7\n  - %% parked %%\n"
            ),
            "{}",
            projection.target_content
        );
    }

    /// `#claimprojectiontypingresidue` (api.md, 2026-10-29): repeated claim
    /// refreshes raced an operator typing `- Is this done?`. A refresh painted
    /// the unrelated intermediate row `- IS this ` as pending, and the later
    /// completed row landed beside that captured fragment. Claim transitions
    /// project their own evidence without taking ownership of a bare row that
    /// may still be under construction.
    #[test]
    fn claim_transition_does_not_mark_an_unrelated_partially_typed_row_pending() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_claim_projection_doc(dir.path(), "- dispatched worker work\n- IS this \n");
        crate::queue_claim::claim(&doc, "dispatched worker work", "subagent:worker", 600).unwrap();

        let effects = FakeEffects::default();
        assert!(project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap());
        let projected = std::fs::read_to_string(&doc).unwrap();

        assert!(
            projected.contains("- 🟡 [#subagent:worker#1] dispatched worker work\n"),
            "{projected}"
        );
        assert!(projected.contains("- IS this \n"), "{projected}");
        assert!(!projected.contains("- 📥 IS this \n"), "{projected}");
        assert_eq!(projected.matches("IS this").count(), 1, "{projected}");
    }

    #[test]
    fn cancelled_generation_never_projects_stale_report_or_completion_onto_readded_row() {
        let dir = tempfile::tempdir().unwrap();
        let item = "finish sdk demo";
        let doc = write_claim_projection_doc(dir.path(), &format!("- {item}\n"));
        let (_, claim) = crate::queue_claim::claim(&doc, item, "subagent:sdk", 600).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            item,
            "subagent:sdk",
            "worker:sdk",
            600,
            claim.generation,
        )
        .unwrap();
        crate::queue_claim::report_attempt(&doc, item, Some(claim.generation)).unwrap();
        crate::claim_release_receipt::capture_response_for_attempt(
            &doc,
            item,
            claim.generation,
            "sdk response",
        )
        .unwrap();
        crate::claim_release_receipt::release_attempt(
            &doc,
            item,
            crate::claim_release_receipt::ReleaseKind::Reported,
            Some(claim.generation),
        )
        .unwrap()
        .unwrap();
        let stale = crate::claim_release_receipt::load(&doc)
            .unwrap()
            .receipts
            .remove(0);
        crate::queue_claim::record_released_cancellation(&doc, &stale).unwrap();

        // Model the cancellation crash boundary: both the old outbox receipt
        // and its completion cell remain physically present. Their exact
        // generation/key tombstone makes them non-governing.
        let key = stale.completion_key.as_deref().unwrap();
        let cell = agent_doc_document::subagent_completion::cell(&stale.owner, item, key);
        let current = std::fs::read_to_string(&doc).unwrap().replace(
            "<!-- agent:queue go -->",
            &format!(
                "<!-- agent:exchange -->\n{cell}\n<!-- /agent:exchange -->\n\n<!-- agent:queue go -->"
            ),
        );
        let cancelled = derive_claim_runtime_projection(&doc, &current, None).unwrap();
        assert!(cancelled.target_content.contains(&format!("- 📥 {item}\n")));
        assert!(!cancelled.target_content.contains("☑️"));
        assert!(!cancelled.target_content.contains("📫"));
        assert!(!cancelled.target_content.contains("📤"));
        assert!(!cancelled.target_content.contains("🔄"));
        assert!(!cancelled.target_content.contains("[#subagent:sdk#"));

        let (_, replacement) =
            crate::queue_claim::claim(&doc, item, "subagent:sdk-retry", 600).unwrap();
        assert_eq!(replacement.generation, claim.generation + 1);
        let replacement_projection =
            derive_claim_runtime_projection(&doc, &cancelled.target_content, None).unwrap();
        assert!(replacement_projection.target_content.contains(&format!(
            "- 🟡 [#subagent:sdk-retry#{}] {item}\n",
            replacement.generation
        )));
        assert!(!replacement_projection.target_content.contains("☑️"));
        assert!(!replacement_projection.target_content.contains("📫"));
        assert!(!replacement_projection.target_content.contains("📤"));
        assert!(!replacement_projection.target_content.contains("🔄"));
    }

    #[test]
    fn concurrent_claim_start_projection_retries_editor_normalization_and_lists_both_claims() {
        let dir = tempfile::tempdir().unwrap();
        let doc =
            write_claim_projection_doc(dir.path(), "- free text worker\n- [#normalized-worker]\n");
        let disk = std::fs::read_to_string(&doc).unwrap();
        let editor_normalized =
            disk.replace("- [#normalized-worker]\n", "- do [#normalized-worker]\n");
        let effects = Arc::new(RacingClaimEffects::new(editor_normalized));
        let claimed = Arc::new(Barrier::new(2));
        let started = Arc::new(Barrier::new(2));

        let attempts = [
            ("free text worker", "subagent:free", "worker:free"),
            (
                "[#normalized-worker]",
                "subagent:normalized",
                "worker:normalized",
            ),
        ];
        let mut workers = Vec::new();
        for (item, owner, worker_id) in attempts {
            let doc = doc.clone();
            let effects = Arc::clone(&effects);
            let claimed = Arc::clone(&claimed);
            let started = Arc::clone(&started);
            workers.push(std::thread::spawn(move || {
                crate::queue_claim::claim_with_session(&doc, item, owner, 600, None).unwrap();
                claimed.wait();
                crate::queue_claim::start_attempt(&doc, item, owner, worker_id, 600, 1).unwrap();
                started.wait();
                project_claim_runtime_state_with_effects(effects.as_ref(), &doc, None).unwrap();
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }

        let live = effects.current();
        assert!(
            live.contains("- 🚧 [#subagent:free#1] free text worker\n"),
            "{live}"
        );
        assert!(
            live.contains("- 🚧 [#subagent:normalized#1] do [#normalized-worker]\n"),
            "{live}"
        );
        assert_eq!(live.matches("free text worker").count(), 1, "{live}");
        assert_eq!(live.matches("normalized-worker").count(), 1, "{live}");

        let reports = claim_reports_with_effects(effects.as_ref(), &doc).unwrap();
        assert_eq!(reports.len(), 2, "{reports:#?}");
        assert!(reports.iter().all(|report| {
            report.claim.state == agent_doc_queue::queue_claim::QueueClaimState::Running
        }));
    }

    /// `#claimrowmigrate` (agent-doc-bugs.md, 2026-10-26): a reported
    /// `release + publish` handback sat on its row when a text-identical row
    /// was restored beside it and preflight annotated the pair
    /// `⟦effect:1⟧`/`⟦effect:2⟧`. The claim jumped onto the restored row
    /// (`☑️ … ⟦effect:1⟧`) and its own row became `📥 … ⟦effect:2⟧`. The
    /// reported claim stays on the row it was projected onto.
    #[test]
    fn reported_claim_keeps_its_row_through_a_duplicate_effect_annotation() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_claim_projection_doc(
            dir.path(),
            "- #gh-fix https://x.test/issues/1\n- release + publish\n",
        );
        let owner = "subagent:release_publish_patch";
        let (_, claim) =
            crate::queue_claim::claim_with_session(&doc, "release + publish", owner, 600, None)
                .unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            "release + publish",
            owner,
            "worker:release",
            600,
            claim.generation,
        )
        .unwrap();
        crate::queue_claim::report_attempt_with_publication_state(
            &doc,
            "release + publish",
            Some(claim.generation),
            Some(agent_doc_element_queue::QueueReportState::Published),
        )
        .unwrap();
        let reported_row = format!(
            "- ☑️ [#{owner}#{}] release + publish ⟦published⟧",
            claim.generation
        );
        let projection =
            derive_claim_runtime_projection(&doc, &std::fs::read_to_string(&doc).unwrap(), None)
                .unwrap();
        assert!(
            projection
                .target_content
                .contains(&format!("{reported_row}\n")),
            "{}",
            projection.target_content
        );
        std::fs::write(&doc, &projection.target_content).unwrap();

        // The operator restores an identical row directly above the claimed
        // one, and preflight annotates both copies in the same queue cut. The
        // historical projection placed effect identity after publication
        // state. A reported row is now preserved byte-for-byte.
        let restored = projection.target_content.replace(
            &format!("{reported_row}\n"),
            &format!("- release + publish ⟦effect:1⟧\n{reported_row} ⟦effect:2⟧\n"),
        );
        std::fs::write(&doc, &restored).unwrap();

        let projection = derive_claim_runtime_projection(&doc, &restored, None).unwrap();
        assert!(
            projection.target_content.contains(&format!(
                "- ☑️ [#{owner}#{}] release + publish ⟦published⟧ ⟦effect:2⟧\n",
                claim.generation
            )),
            "{}",
            projection.target_content
        );
        assert_eq!(
            projection.target_content.matches("⟦published⟧").count(),
            1,
            "report state remains canonical instead of becoming state/effect/state: {}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains("- 📥 release + publish ⟦effect:1⟧\n"),
            "{}",
            projection.target_content
        );
        let reports = crate::queue_claim::claim_reports_for_content(&doc, &restored).unwrap();
        assert_eq!(reports.len(), 1, "{reports:#?}");
        assert_eq!(
            reports[0].claim.item_text,
            "release + publish ⟦published⟧ ⟦effect:2⟧"
        );
        assert_eq!(
            reports[0].claim.identity,
            agent_doc_queue::queue_claim::claim_identity(
                "release + publish ⟦effect:2⟧ ⟦published⟧"
            ),
            "legacy annotation order cannot mint a second claim identity"
        );
        assert_eq!(
            reports[0].claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
    }

    /// `#queueeffectidentity` (api.md, 2026-10-30): claiming a newly-added
    /// bare nested command must not project the attempt onto its older effect
    /// siblings.
    #[test]
    fn claiming_a_new_duplicate_nested_effect_does_not_start_prior_occurrences() {
        let dir = tempfile::tempdir().unwrap();
        let parent = "https://github.com/haiven-dev/haiven-contracts/pull/179";
        let doc = write_claim_projection_doc(
            dir.path(),
            &format!(
                "- {parent}\n  - #address ⟦effect:1⟧\n  - #address ⟦effect:3⟧\n  - #address\n"
            ),
        );
        let owner = "subagent:contracts_pr179_address_latest";
        let (_, claim) =
            crate::queue_claim::claim_with_session(&doc, "#address", owner, 600, None).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            &claim.item_text,
            owner,
            "worker:address-latest",
            600,
            claim.generation,
        )
        .unwrap();

        let current = std::fs::read_to_string(&doc).unwrap();
        let projection = derive_claim_runtime_projection(&doc, &current, None).unwrap();
        assert!(
            projection
                .target_content
                .contains("  - 📥 #address ⟦effect:1⟧\n"),
            "the first prior effect stays pending:\n{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains("  - 📥 #address ⟦effect:3⟧\n"),
            "the second prior effect stays pending:\n{}",
            projection.target_content
        );
        assert!(
            projection.target_content.contains(&format!(
                "  - 🚧 [#{owner}#{}] #address\n",
                claim.generation
            )),
            "only the newly-added bare occurrence is running:\n{}",
            projection.target_content
        );
        assert_eq!(
            projection.target_content.matches("🚧 [#").count(),
            1,
            "one claim attempt must project onto one nested effect"
        );
    }

    #[test]
    fn claim_projection_marks_only_the_exact_duplicate_effect() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_claim_projection_doc(
            dir.path(),
            "- release + publish ⟦effect:1⟧\n- release + publish ⟦effect:2⟧\n",
        );
        crate::queue_claim::claim(
            &doc,
            "release + publish ⟦effect:2⟧",
            "subagent:release-2",
            600,
        )
        .unwrap();

        let projection =
            derive_claim_runtime_projection(&doc, &std::fs::read_to_string(&doc).unwrap(), None)
                .unwrap();
        assert!(
            projection
                .target_content
                .contains("- 📥 release + publish ⟦effect:1⟧\n"),
            "{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains("- 🟡 [#subagent:release-2#1] release + publish ⟦effect:2⟧\n"),
            "{}",
            projection.target_content
        );
        assert!(
            !projection
                .target_content
                .contains("- 🟡 release + publish ⟦effect:1⟧\n"),
            "{}",
            projection.target_content
        );
    }

    #[test]
    fn running_binding_projects_only_its_matching_bare_queue_row() {
        let dir = tempfile::tempdir().unwrap();
        let item = "#gh-issue https://github.com/example/project/issues/299";
        let unrelated = "#gh-issue https://github.com/example/project/issues/300";
        let doc = write_claim_projection_doc(dir.path(), &format!("- {item}\n- {unrelated}\n"));
        crate::queue_claim::claim(&doc, item, "subagent:gh299", 600).unwrap();
        crate::queue_claim::start_attempt(&doc, item, "subagent:gh299", "/root/gh299", 600, 1)
            .unwrap();

        // Model a stale visible cut after the durable worker binding survived a
        // controller handoff: the exact running row is bare and its sibling is
        // unrelated. Reprojection must repair only the bound attempt's row.
        let stale = std::fs::read_to_string(&doc).unwrap();
        let projection = derive_claim_runtime_projection(&doc, &stale, None).unwrap();

        assert!(projection.changed);
        assert!(
            projection
                .target_content
                .contains(&format!("- 🚧 [#subagent:gh299#1] {item}\n")),
            "{}",
            projection.target_content
        );
        assert!(
            projection
                .target_content
                .contains(&format!("- 📥 {unrelated}\n")),
            "{}",
            projection.target_content
        );
        assert!(
            !projection
                .target_content
                .contains(&format!("- 🚧 {unrelated}\n")),
            "{}",
            projection.target_content
        );
    }

    #[test]
    fn running_id_claim_projects_onto_exact_tracked_item_alias() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk-demo.md");
        let id_head = "do [#rebaseexistingproject]";
        let question = "Why can't we just rebase the existing Project source code into the aggregate PR branch with the fixes?";
        let initial = format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             <!-- agent:backlog -->\n- [ ] [#rebaseexistingproject] {question}\n<!-- /agent:backlog -->\n\n\
             <!-- agent:queue subagents go -->\n- {id_head}\n- unrelated\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &initial).unwrap();
        crate::queue_claim::claim(&doc, id_head, "subagent:project_rebase_review", 600).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            id_head,
            "subagent:project_rebase_review",
            "/root/project_rebase_review",
            600,
            1,
        )
        .unwrap();

        // Model the authoritative cut observed in sdk-demo.md: the tracked
        // backlog row and id mirror are gone, but its exact prose remains live.
        let current = format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             <!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n\
             <!-- agent:queue subagents go -->\n- {question}\n- unrelated\n<!-- /agent:queue -->\n"
        );
        let projection = derive_claim_runtime_projection(&doc, &current, None).unwrap();
        assert!(
            projection.target_content.contains(&format!(
                "- 🚧 [#subagent:project_rebase_review#1] {question}\n"
            )),
            "{}",
            projection.target_content
        );
        assert!(projection.target_content.contains("- 📥 unrelated\n"));
        assert!(!projection.target_content.contains("- 🚧 unrelated\n"));

        let claims = crate::queue_claim::claim_states_for_content(&doc, &current).unwrap();
        assert_eq!(claims.len(), 1, "{claims:#?}");
        assert_eq!(claims[0].item_text, question);
        assert_eq!(
            claims[0].state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
    }

    #[test]
    fn manually_struck_row_with_live_claim_stays_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let item = "#gh-issue https://github.com/example/project/issues/299";
        let doc = write_claim_projection_doc(dir.path(), &format!("- {item}\n"));
        crate::queue_claim::claim(&doc, item, "subagent:gh299", 600).unwrap();
        crate::queue_claim::start_attempt(&doc, item, "subagent:gh299", "/root/gh299", 600, 1)
            .unwrap();

        let live = std::fs::read_to_string(&doc).unwrap();
        let live_projection = derive_claim_runtime_projection(&doc, &live, None).unwrap();
        assert_eq!(
            live_projection
                .target_content
                .matches(&format!("- 🚧 [#subagent:gh299#1] {item}\n"))
                .count(),
            1,
            "a live worked row must project exactly one typed lifecycle marker: {}",
            live_projection.target_content
        );

        // The operator's strike is terminal document authority even while the
        // old exact attempt remains in durable state.
        let terminal = live_projection.target_content.replace(
            &format!("- 🚧 [#subagent:gh299#1] {item}\n"),
            &format!("- ~~{item}~~\n"),
        );
        let terminal_projection = derive_claim_runtime_projection(&doc, &terminal, None).unwrap();
        assert!(
            terminal_projection
                .target_content
                .contains(&format!("- ~~{item}~~\n")),
            "{}",
            terminal_projection.target_content
        );
    }

    /// `#strikeisterminal`: finished-work evidence agrees with a strike. A
    /// struck row whose worker reported (receipt) or whose completion cell is
    /// still in the exchange stays struck; before, the claim projection
    /// reopened it as `❔` and the next pass painted `☑️` / `🔄` (live
    /// `release_promotion_concurrency` / `tmux_auto_sync`, 2026-10-26).
    #[test]
    fn struck_row_with_receipt_or_completion_cell_is_never_reopened() {
        let dir = tempfile::tempdir().unwrap();
        let receipted = "do [#release-promotion-concurrency]";
        let celled = "tmux pane auto sync is not working. Sync Tmux Panes works.";
        let doc =
            write_claim_projection_doc(dir.path(), &format!("- 🚧 {receipted}\n- 🚧 {celled}\n"));
        let mut cells = String::new();
        for (item, owner) in [
            (receipted, "subagent:release_promotion_concurrency"),
            (celled, "subagent:tmux_auto_sync"),
        ] {
            let (_, claimed) = crate::queue_claim::claim(&doc, item, owner, 600).unwrap();
            let running = crate::queue_claim::start_attempt(
                &doc,
                item,
                owner,
                &format!("/root/{owner}"),
                600,
                claimed.generation,
            )
            .unwrap();
            cells.push_str(
                &agent_doc_document::subagent_completion::cell_with_response(
                    &running.owner,
                    &running.item_text,
                    &crate::queue_claim::completion_feedback_key(&running).unwrap(),
                    "Done; tests pass.",
                )
                .unwrap(),
            );
            cells.push_str("\n\n");
            crate::queue_claim::report_attempt(&doc, item, Some(running.generation)).unwrap();
            crate::claim_release_receipt::capture_response_for_attempt(
                &doc,
                item,
                running.generation,
                "Done; tests pass.",
            )
            .unwrap();
            crate::claim_release_receipt::release(
                &doc,
                item,
                crate::claim_release_receipt::ReleaseKind::Reported,
            )
            .unwrap();
        }
        let disk = std::fs::read_to_string(&doc).unwrap();
        let struck = disk
            .replace(
                "<!-- agent:queue go -->\n",
                &format!(
                    "<!-- agent:exchange patch=append -->\n{cells}<!-- /agent:exchange -->\n\n<!-- agent:queue go -->\n"
                ),
            )
            .replace(&format!("- 🚧 {receipted}\n"), &format!("- ~~{receipted}~~\n"))
            .replace(&format!("- 🚧 {celled}\n"), &format!("- ~~{celled}~~\n"));
        assert!(
            agent_doc_document::subagent_completion::completion_evidence(&struck).len() == 2,
            "fixture must carry both completion cells: {struck}"
        );

        let projection = derive_claim_runtime_projection(&doc, &struck, None).unwrap();
        for head in [receipted, celled] {
            assert!(
                projection
                    .target_content
                    .contains(&format!("- ~~{head}~~\n")),
                "{head:?} must stay struck: {}",
                projection.target_content
            );
        }
        assert!(
            !projection.target_content.contains('❔')
                && !projection.target_content.contains("🔄")
                && !projection.target_content.contains("- ☑️"),
            "{}",
            projection.target_content
        );
    }

    /// `#strikeisterminal`, per queue: replay of `haiven-dev/tasks/sdk/game-sessions.md`
    /// (2026-10-28). At 06:01Z `--ack-id` struck four reported heads; at 06:02Z
    /// `backlog requeue` appended a fresh live row per id beside the struck
    /// one. A new claim must mark only the live row. It used to also reopen
    /// the struck row (`❔`, then `🟡`), which left two identical claimed rows
    /// in `agent:run`. The rows then stayed doubled through `☑️`.
    #[test]
    fn claim_on_requeued_row_never_reopens_its_struck_predecessor() {
        let dir = tempfile::tempdir().unwrap();
        let heads = [
            "do [#publish-session-contracts]",
            "do [#session-liveness-resume]",
            "do [#session-ws-events]",
            "do [#session-roster-list-update]",
        ];
        let mut body = String::from("- ~~do [#commitpush]~~\n");
        for head in heads {
            body.push_str(&format!("- ~~{head}~~\n"));
        }
        for head in heads {
            body.push_str(&format!("- 📥 {head}\n"));
        }
        let doc = write_claim_projection_doc(dir.path(), &body);
        let effects = FakeEffects::with_current(std::fs::read_to_string(&doc).unwrap());

        for (index, head) in heads.iter().enumerate() {
            let owner = format!("subagent:row-{index}");
            let (_, claimed) = crate::queue_claim::claim(&doc, head, &owner, 600).unwrap();
            project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap();
            let projected = std::fs::read_to_string(&doc).unwrap();
            let claimed_row = format!("- 🟡 [#{owner}#{}] {head}\n", claimed.generation);
            assert_eq!(
                projected.matches(&claimed_row).count(),
                1,
                "exactly one row may carry the claim: {projected}"
            );
            assert!(
                projected.contains(&format!("- ~~{head}~~\n")),
                "the earlier attempt's struck row must stay struck: {projected}"
            );
            assert!(!projected.contains('❔'), "{projected}");
        }

        // Running and Reported keep the single row through the whole cycle.
        for (index, head) in heads.iter().enumerate() {
            let owner = format!("subagent:row-{index}");
            let running = crate::queue_claim::start_attempt(
                &doc,
                head,
                &owner,
                &format!("/root/{owner}"),
                600,
                1,
            )
            .unwrap();
            crate::queue_claim::report_attempt(&doc, head, Some(running.generation)).unwrap();
        }
        project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap();
        let reported = std::fs::read_to_string(&doc).unwrap();
        for (index, head) in heads.iter().enumerate() {
            assert_eq!(
                reported
                    .matches(&format!("- ☑️ [#subagent:row-{index}#1] {head}\n"))
                    .count(),
                1,
                "{reported}"
            );
            assert_eq!(reported.matches(&format!("- ~~{head}~~\n")).count(), 1);
        }
        // The projection is a fixpoint: a fresh pass changes nothing.
        let settled = derive_claim_runtime_projection(&doc, &reported, None).unwrap();
        assert!(!settled.changed, "{}", settled.target_content);
    }

    #[test]
    fn claim_projection_rebases_on_editor_authority_cut() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_claim_projection_doc(dir.path(), "- dispatched worker work\n");
        crate::queue_claim::claim(&doc, "dispatched worker work", "subagent:worker", 600).unwrap();
        let disk = std::fs::read_to_string(&doc).unwrap();
        let editor = disk.replace(
            "- dispatched worker work\n",
            "- operator unsaved sibling\n- dispatched worker work\n",
        );
        let effects = FakeEffects::with_current(editor);

        assert!(project_claim_runtime_state_with_effects(&effects, &doc, None).unwrap());
        let projected = std::fs::read_to_string(&doc).unwrap();
        assert!(
            projected.contains("- operator unsaved sibling"),
            "{projected}"
        );
        assert!(
            !projected.contains("📥 operator unsaved sibling"),
            "{projected}"
        );
        assert!(
            projected.contains("- 🟡 [#subagent:worker#1] dispatched worker work"),
            "{projected}"
        );
    }

    /// `#sdkdocumentselfheal`: a terminal operator-request row is durable
    /// proof that its same-id Todo receipt can close. Full background repair
    /// must fold that proof even when no later foreground preflight occurs.
    #[test]
    fn full_runtime_projection_closes_only_terminal_operator_request_todo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let request_id = "subagent-operator-request-sdk-self-heal";
        let document = format!(
            "<!-- agent:queue -->\n- ~~[#{request_id}]: integrate~~\n<!-- /agent:queue -->\n\
             <!-- agent:todo -->\n\
             - [ ] [#{request_id}] Integrate the completed worker result\n\
             - [ ] [#ordinary-sdk-work] Keep real SDK work open\n\
             <!-- /agent:todo -->\n"
        );
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, &document).unwrap();
        let effects = FakeEffects::with_current(document);

        assert_eq!(
            project_all_queue_runtime_state_once_with_effects(&effects, &doc).unwrap(),
            QueueRuntimeProjectionOnceOutcome::Settled { changed: true }
        );
        let projected = std::fs::read_to_string(&doc).unwrap();
        assert!(
            projected.contains(&format!("- [x] [#{request_id}]")),
            "{projected}"
        );
        assert!(
            projected.contains("- [ ] [#ordinary-sdk-work] Keep real SDK work open"),
            "{projected}"
        );
        assert!(
            project_all_queue_runtime_state_once_with_effects(&effects, &doc).unwrap()
                == QueueRuntimeProjectionOnceOutcome::Settled { changed: false },
            "the repaired projection must be a fixpoint"
        );
    }

    /// A successful delivery call is not proof that its successor won the
    /// editor CRDT race. Background self-heal must observe that supersession
    /// and stop after one attempt instead of competing with the operator.
    #[test]
    fn background_runtime_projection_never_retries_a_superseded_editor_cut() {
        struct SupersededEffects {
            current: RefCell<String>,
            convergence_attempts: Cell<usize>,
        }

        impl QueueRuntimeProjectionEffects for SupersededEffects {
            fn current_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
                Ok(self.current.borrow().clone())
            }

            fn converge_document_or_disk(
                &self,
                _file: &Path,
                _target_content: &str,
                _source_content: &str,
                _reason: &str,
            ) -> Result<()> {
                self.convergence_attempts
                    .set(self.convergence_attempts.get() + 1);
                // Model an operator edit superseding the repair cut while its
                // delivery is in flight.
                self.current.borrow_mut().push_str("\noperator edit wins\n");
                Ok(())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let request_id = "subagent-operator-request-superseded-self-heal";
        let current = format!(
            "<!-- agent:queue -->\n- ~~[#{request_id}]~~\n<!-- /agent:queue -->\n\
             <!-- agent:todo -->\n- [ ] [#{request_id}] Close only after terminal proof\n<!-- /agent:todo -->\n"
        );
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, &current).unwrap();
        let effects = SupersededEffects {
            current: RefCell::new(current),
            convergence_attempts: Cell::new(0),
        };
        let request = ensure_queue_projection_repair_request(
            &doc,
            queue_repair_generation(&doc).unwrap(),
            "ordinary_projection_reconcile",
        )
        .unwrap();

        assert_eq!(
            project_all_queue_runtime_state_once_for_generation_with_effects(
                &effects,
                &doc,
                request.generation,
            )
            .unwrap(),
            QueueRuntimeProjectionOnceOutcome::Superseded,
            "a superseded background CAS must defer"
        );
        assert_eq!(effects.convergence_attempts.get(), 1);
        assert_eq!(
            load_queue_projection_repair_request(&doc).unwrap(),
            Some(request),
            "operator CAS supersession must retain the exact durable request"
        );
    }

    fn upsert_test_state(doc: &Path, state_kind: &str, payload_json: String) {
        let root = agent_doc_fs::find_project_root(doc).unwrap();
        let document_hash = agent_doc_hash::path_hash(doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash,
                state_kind: state_kind.to_string(),
                canonical_path: doc.canonicalize().unwrap().display().to_string(),
                payload_json,
                updated_at_secs: crate::queue_claim::claim_clock_now_secs(),
            },
        )
        .unwrap();
    }

    #[test]
    fn clean_slate_clears_only_attempt_projection_state_and_reprojects_pending() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = "<!-- agent:queue -->\n- repair projection drift\n<!-- /agent:queue -->\n";
        std::fs::write(&doc, content).unwrap();

        let (_, mut claim) =
            crate::queue_claim::claim(&doc, "repair projection drift", "subagent:finished", 60)
                .unwrap();
        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
        claim.expires_at_secs = 0;
        let claims = agent_doc_queue::queue_claim::QueueClaimLedger {
            claims: vec![claim.clone()],
            cancellations: Vec::new(),
        };
        upsert_test_state(
            &doc,
            agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND,
            serde_json::to_string(&claims).unwrap(),
        );
        upsert_test_state(
            &doc,
            crate::queue_lineage::QUEUE_LINEAGE_STATE_KIND,
            serde_json::to_string(&crate::queue_lineage::QueueLineageLedger::default()).unwrap(),
        );
        upsert_test_state(&doc, "operator_delete_tombstones", "{}".to_string());

        let effects = FakeEffects::with_current(content);
        let outcome = clean_slate_with_effects(&effects, &doc).unwrap();
        assert_eq!(outcome.cleared_claims, 1);
        assert_eq!(outcome.cleared_release_receipts, 0);
        assert_eq!(
            outcome.projection,
            QueueRuntimeProjectionOnceOutcome::Settled { changed: true }
        );
        assert!(
            crate::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .is_empty()
        );
        assert!(
            crate::claim_release_receipt::load(&doc)
                .unwrap()
                .receipts
                .is_empty()
        );

        let root = agent_doc_fs::find_project_root(&doc).unwrap();
        let document_hash = agent_doc_hash::path_hash(&doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        for preserved in [
            crate::queue_lineage::QUEUE_LINEAGE_STATE_KIND,
            "operator_delete_tombstones",
        ] {
            assert!(
                agent_doc_sqlite::state_store::load_queue_document_state_from_db(
                    &conn,
                    &document_hash,
                    preserved,
                )
                .unwrap()
                .is_some(),
                "clean-slate removed preserved state family {preserved}"
            );
        }
        let projected = std::fs::read_to_string(&doc).unwrap();
        assert!(
            projected.contains("- 📥 repair projection drift"),
            "{projected}"
        );
    }

    #[test]
    fn clean_slate_supersession_durably_requests_and_wakes_background_repair() {
        use std::cell::{Cell, RefCell};

        struct SupersededCleanSlateEffects {
            current: RefCell<String>,
            convergence_attempts: Cell<usize>,
        }

        impl QueueRuntimeProjectionEffects for SupersededCleanSlateEffects {
            fn current_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
                Ok(self.current.borrow().clone())
            }

            fn converge_document_or_disk(
                &self,
                _file: &Path,
                _target_content: &str,
                _source_content: &str,
                _reason: &str,
            ) -> Result<()> {
                self.convergence_attempts
                    .set(self.convergence_attempts.get() + 1);
                self.current.borrow_mut().push_str("\nnewer operator cut\n");
                Ok(())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = "<!-- agent:queue -->\n- repair after reset\n<!-- /agent:queue -->\n";
        std::fs::write(&doc, content).unwrap();
        let (_, mut claim) =
            crate::queue_claim::claim(&doc, "repair after reset", "subagent:lost", 60).unwrap();
        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
        claim.expires_at_secs = 0;
        upsert_test_state(
            &doc,
            agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND,
            serde_json::to_string(&agent_doc_queue::queue_claim::QueueClaimLedger {
                claims: vec![claim],
                cancellations: Vec::new(),
            })
            .unwrap(),
        );
        let effects = SupersededCleanSlateEffects {
            current: RefCell::new(content.to_string()),
            convergence_attempts: Cell::new(0),
        };

        let outcome = clean_slate_with_effects(&effects, &doc).unwrap();
        assert_eq!(
            outcome.projection,
            QueueRuntimeProjectionOnceOutcome::Superseded
        );
        assert_eq!(effects.convergence_attempts.get(), 1);
        let request = load_queue_projection_repair_request(&doc)
            .unwrap()
            .expect("superseded projection must enqueue durable repair");
        assert_eq!(request.generation, 1);
        assert_eq!(request.reason, "clean_slate_projection_superseded");
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("queue_clean_slate_reset_committed"), "{ops}");
    }

    #[test]
    fn clean_slate_projection_error_keeps_committed_reset_auditable_and_enqueued() {
        struct ProjectionErrorEffects;

        impl QueueRuntimeProjectionEffects for ProjectionErrorEffects {
            fn current_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
                bail!("injected projection read failure")
            }

            fn converge_document_or_disk(
                &self,
                _file: &Path,
                _target_content: &str,
                _source_content: &str,
                _reason: &str,
            ) -> Result<()> {
                unreachable!()
            }
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = "<!-- agent:queue -->\n- repair after error\n<!-- /agent:queue -->\n";
        std::fs::write(&doc, content).unwrap();
        let (_, mut claim) =
            crate::queue_claim::claim(&doc, "repair after error", "subagent:lost", 60).unwrap();
        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
        claim.expires_at_secs = 0;
        upsert_test_state(
            &doc,
            agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND,
            serde_json::to_string(&agent_doc_queue::queue_claim::QueueClaimLedger {
                claims: vec![claim],
                cancellations: Vec::new(),
            })
            .unwrap(),
        );

        let error = clean_slate_with_effects(&ProjectionErrorEffects, &doc).unwrap_err();
        assert!(error.to_string().contains("durably deferred"), "{error:#}");
        assert!(
            crate::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .is_empty()
        );
        let request = load_queue_projection_repair_request(&doc)
            .unwrap()
            .expect("projection error must enqueue durable repair");
        assert_eq!(request.generation, 1);
        assert_eq!(request.reason, "clean_slate_projection_error");
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("queue_clean_slate_reset_committed"), "{ops}");
    }

    #[test]
    fn pre_reset_self_heal_generation_cannot_write_after_reset() {
        use std::cell::Cell;

        struct ResetDuringAuthorityRead {
            current: String,
            reset: Cell<bool>,
            writes: Cell<usize>,
        }

        impl QueueRuntimeProjectionEffects for ResetDuringAuthorityRead {
            fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
                if !self.reset.replace(true) {
                    let reset = agent_doc_controller_io::project_controller::queue_clean_slate_reset_for_file(file)?;
                    assert_eq!(reset.generation, 1);
                }
                Ok(self.current.clone())
            }

            fn converge_document_or_disk(
                &self,
                _file: &Path,
                _target_content: &str,
                _source_content: &str,
                _reason: &str,
            ) -> Result<()> {
                self.writes.set(self.writes.get() + 1);
                Ok(())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = "<!-- agent:queue -->\n- stale marker candidate\n<!-- /agent:queue -->\n";
        std::fs::write(&doc, content).unwrap();
        let effects = ResetDuringAuthorityRead {
            current: content.to_string(),
            reset: Cell::new(false),
            writes: Cell::new(0),
        };

        assert_eq!(
            project_all_queue_runtime_state_once_with_effects(&effects, &doc).unwrap(),
            QueueRuntimeProjectionOnceOutcome::Superseded
        );
        assert_eq!(effects.writes.get(), 0, "old generation must not write");
        assert_eq!(queue_repair_generation(&doc).unwrap(), 1);
    }

    #[test]
    fn old_self_heal_clear_cannot_retire_a_newer_repair_request() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(
            &doc,
            "<!-- agent:queue -->\n- generation fence\n<!-- /agent:queue -->\n",
        )
        .unwrap();

        let first =
            agent_doc_controller_io::project_controller::queue_clean_slate_reset_for_file(&doc)
                .unwrap();
        assert_eq!(first.generation, 1);
        let first_request = ensure_queue_projection_repair_request(&doc, 1, "first").unwrap();
        let second =
            agent_doc_controller_io::project_controller::queue_clean_slate_reset_for_file(&doc)
                .unwrap();
        assert_eq!(second.generation, 2);
        let second_request = ensure_queue_projection_repair_request(&doc, 2, "second").unwrap();
        assert!(second_request.request_generation > first_request.request_generation);

        let source = std::fs::read_to_string(&doc).unwrap();
        let stale_target = source.replace("generation fence", "stale generation marker");
        assert_eq!(
            agent_doc_controller_io::project_controller::converge_queue_projection_for_generation(
                &doc,
                &stale_target,
                &source,
                "stale_generation_test",
                1,
                agent_doc_controller_io::project_controller::QueueProjectionWriter::LifecycleOwner,
            )
            .unwrap(),
            agent_doc_controller_io::project_controller::QueueProjectionGenerationWriteOutcome::Superseded,
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), source);

        assert!(!clear_queue_projection_repair_request(&doc, &first_request).unwrap());
        let request = load_queue_projection_repair_request(&doc)
            .unwrap()
            .expect("new request must survive old worker settlement");
        assert_eq!(request.generation, 2);
        assert_eq!(request.reason, "second");
        assert!(clear_queue_projection_repair_request(&doc, &second_request).unwrap());
        assert!(
            load_queue_projection_repair_request(&doc)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn ordinary_projection_request_is_durable_idempotent_and_exactly_consumed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(
            &doc,
            "<!-- agent:queue -->\n- ordinary drift\n<!-- /agent:queue -->\n",
        )
        .unwrap();

        let generation = queue_repair_generation(&doc).unwrap();
        let first = ensure_queue_projection_repair_request(
            &doc,
            generation,
            "ordinary_projection_reconcile",
        )
        .unwrap();
        // A retry or restarted supervisor reuses the durable token rather than
        // manufacturing a retry storm.
        let restarted =
            ensure_queue_projection_repair_request(&doc, generation, "ordinary_projection_retry")
                .unwrap();
        assert_eq!(restarted, first);
        assert_eq!(
            load_queue_projection_repair_request(&doc).unwrap(),
            Some(first.clone())
        );

        let mut stale = first.clone();
        stale.request_generation = stale.request_generation.saturating_sub(1);
        assert!(!clear_queue_projection_repair_request(&doc, &stale).unwrap());
        assert_eq!(
            load_queue_projection_repair_request(&doc).unwrap(),
            Some(first.clone())
        );
        assert!(clear_queue_projection_repair_request(&doc, &first).unwrap());
        assert!(
            load_queue_projection_repair_request(&doc)
                .unwrap()
                .is_none()
        );

        let next = ensure_queue_projection_repair_request(
            &doc,
            generation,
            "ordinary_projection_reconcile",
        )
        .unwrap();
        assert!(next.request_generation > first.request_generation);
    }

    #[test]
    fn ordinary_projection_error_leaves_its_durable_request_for_restart() {
        struct ReadErrorEffects;

        impl QueueRuntimeProjectionEffects for ReadErrorEffects {
            fn current_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
                bail!("injected ordinary projection read error")
            }

            fn converge_document_or_disk(
                &self,
                _file: &Path,
                _target_content: &str,
                _source_content: &str,
                _reason: &str,
            ) -> Result<()> {
                unreachable!()
            }
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(
            &doc,
            "<!-- agent:queue -->\n- error recovery\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let request = ensure_queue_projection_repair_request(
            &doc,
            queue_repair_generation(&doc).unwrap(),
            "ordinary_projection_reconcile",
        )
        .unwrap();

        let error = project_all_queue_runtime_state_once_for_generation_with_effects(
            &ReadErrorEffects,
            &doc,
            request.generation,
        )
        .unwrap_err();
        assert!(error.to_string().contains("injected ordinary"));
        assert_eq!(
            load_queue_projection_repair_request(&doc).unwrap(),
            Some(request),
            "a replacement supervisor must be able to resume the exact request"
        );
    }

    #[test]
    fn production_repair_wake_runs_one_unchanged_worker_then_quiesces() {
        use std::cell::Cell;

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let initial = "<!-- agent:queue -->\n- already settled\n<!-- /agent:queue -->\n";
        std::fs::write(&doc, initial).unwrap();
        // Normalize the fixture before the measured request/wake/worker cycle.
        let setup_effects = FakeEffects::with_current(initial);
        project_all_queue_runtime_state_once_with_effects(&setup_effects, &doc).unwrap();
        let content = std::fs::read_to_string(&doc).unwrap();
        let wake_count = Cell::new(0usize);
        let generation = queue_repair_generation(&doc).unwrap();

        // External recovery records one durable request and publishes exactly
        // one maintenance wake through the same production helper.
        let requested = ensure_queue_projection_repair_request_with_wake(
            &doc,
            generation,
            "external_recovery",
            true,
            |_| {
                wake_count.set(wake_count.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(wake_count.get(), 1);

        // The idle watcher consumes that edge, reuses the token without
        // publishing a self-wake, and runs one unchanged generation-fenced
        // worker.
        let worker_request = ensure_queue_projection_repair_request_with_wake(
            &doc,
            generation,
            "ordinary_projection_reconcile",
            false,
            |_| panic!("idle consumption must not publish its own delivery wake"),
        )
        .unwrap();
        assert_eq!(worker_request, requested);
        assert_eq!(wake_count.get(), 1);
        let effects = FakeEffects::with_current(content);
        assert_eq!(
            project_all_queue_runtime_state_once_for_generation_with_effects(
                &effects,
                &doc,
                worker_request.generation,
            )
            .unwrap(),
            QueueRuntimeProjectionOnceOutcome::Settled { changed: false }
        );
        assert!(clear_queue_projection_repair_request(&doc, &worker_request).unwrap());
        assert!(
            load_queue_projection_repair_request(&doc)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            wake_count.get(),
            1,
            "settlement must publish no successor wake"
        );

        let root = agent_doc_fs::find_project_root(&doc).unwrap();
        let document_hash = agent_doc_hash::path_hash(&doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        let sequence = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
            &conn,
            &document_hash,
            agent_doc_queue::queue_repair_generation::QUEUE_PROJECTION_REPAIR_SEQUENCE_STATE_KIND,
        )
        .unwrap()
        .expect("repair sequence")
        .payload_json;
        let sequence: agent_doc_queue::queue_repair_generation::QueueProjectionRepairSequence =
            serde_json::from_str(&sequence).unwrap();
        assert_eq!(sequence.generation, requested.request_generation);
    }

    #[test]
    fn clean_slate_refuses_a_live_worker_or_completion_handoff_without_clearing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = "<!-- agent:queue -->\n- live worker\n<!-- /agent:queue -->\n";
        std::fs::write(&doc, content).unwrap();
        crate::queue_claim::claim(&doc, "live worker", "subagent:live", 3600).unwrap();
        let effects = FakeEffects::with_current(content);
        let error = clean_slate_with_effects(&effects, &doc).unwrap_err();
        assert!(error.to_string().contains("claimed/running/reported"));
        assert_eq!(
            crate::queue_claim::load_ledger(&doc).unwrap().claims.len(),
            1
        );

        let completion = agent_doc_document::subagent_completion::cell(
            "subagent:finished",
            "live worker",
            "completion-key",
        );
        let with_completion =
            format!("<!-- agent:exchange -->\n{completion}\n<!-- /agent:exchange -->\n{content}");
        std::fs::write(&doc, &with_completion).unwrap();
        let effects = FakeEffects::with_current(with_completion);
        let error = clean_slate_with_effects(&effects, &doc).unwrap_err();
        assert!(error.to_string().contains("worker completion handoff"));
        assert_eq!(
            crate::queue_claim::load_ledger(&doc).unwrap().claims.len(),
            1
        );
    }

    #[test]
    fn clean_slate_refuses_reported_recovered_and_release_receipt_authority() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content =
            "<!-- agent:queue -->\n- preserve response authority\n<!-- /agent:queue -->\n";
        std::fs::write(&doc, content).unwrap();

        let (_, mut claim) =
            crate::queue_claim::claim(&doc, "preserve response authority", "subagent:reported", 60)
                .unwrap();
        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Reported;
        upsert_test_state(
            &doc,
            agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND,
            serde_json::to_string(&agent_doc_queue::queue_claim::QueueClaimLedger {
                claims: vec![claim.clone()],
                cancellations: Vec::new(),
            })
            .unwrap(),
        );
        let effects = FakeEffects::with_current(content);
        let error = clean_slate_with_effects(&effects, &doc).unwrap_err();
        assert!(
            error.to_string().contains("reported or recovered"),
            "{error:#}"
        );
        assert_eq!(
            crate::queue_claim::load_ledger(&doc).unwrap().claims,
            vec![claim.clone()]
        );

        claim.state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
        claim.lost_from = Some(agent_doc_queue::queue_claim::QueueClaimState::Running);
        claim.completion_recovered_after_loss = true;
        upsert_test_state(
            &doc,
            agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND,
            serde_json::to_string(&agent_doc_queue::queue_claim::QueueClaimLedger {
                claims: vec![claim.clone()],
                cancellations: Vec::new(),
            })
            .unwrap(),
        );
        let error = clean_slate_with_effects(&effects, &doc).unwrap_err();
        assert!(
            error.to_string().contains("reported or recovered"),
            "{error:#}"
        );
        assert_eq!(
            crate::queue_claim::load_ledger(&doc).unwrap().claims,
            vec![claim.clone()]
        );

        upsert_test_state(
            &doc,
            agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND,
            serde_json::to_string(&agent_doc_queue::queue_claim::QueueClaimLedger::default())
                .unwrap(),
        );
        let response = agent_doc_queue::claim_release_receipt::CapturedQueueResponse::new(
            "response-id".to_string(),
            "authoritative worker response",
        )
        .unwrap();
        let receipt = agent_doc_queue::claim_release_receipt::ClaimReleaseReceipt {
            identity: claim.identity.clone(),
            item_text: claim.item_text.clone(),
            owner: claim.owner.clone(),
            generation: claim.generation,
            completion_recovered_after_loss: true,
            owner_session: None,
            publication_state: None,
            claimed_at_secs: claim.claimed_at_secs,
            worker_id: Some("worker-1".to_string()),
            completion_key: Some("completion-key".to_string()),
            response: Some(response.clone()),
            released_at_secs: 1,
            expires_at_secs: 2,
            attempt_closed_at_secs: None,
        };
        upsert_test_state(
            &doc,
            agent_doc_queue::claim_release_receipt::CLAIM_RELEASE_RECEIPTS_STATE_KIND,
            serde_json::to_string(
                &agent_doc_queue::claim_release_receipt::ClaimReleaseReceiptLedger {
                    receipts: vec![receipt.clone()],
                },
            )
            .unwrap(),
        );
        let error = clean_slate_with_effects(&effects, &doc).unwrap_err();
        assert!(
            error.to_string().contains("await integration/strike"),
            "{error:#}"
        );
        let retained = crate::claim_release_receipt::load(&doc).unwrap();
        assert_eq!(retained.receipts, vec![receipt]);
        assert_eq!(retained.receipts[0].response.as_ref(), Some(&response));
    }
}

/// `#waitingitemclaimmarker`: an operator-answered waiting item is agent-owned
/// dispatch work (`#waitingitemnext`), so its claim must project `🟡` onto the
/// visible head. The runtime projector used to skip every waiting-item head,
/// settle with nothing written, and return spawn permission while `queue
/// claims` reported the row bare; the coordinator then released and requeued
/// the claim every cycle (sdk-dev, 2026-11-04: 24+ identical responses).
#[cfg(test)]
mod waiting_item_claim_projection_tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    const ITEM_MARKER: &str = agent_doc_queue::queue_consume::WAITING_ON_YOU_ITEM_MARKER;
    const REPLY_MARKER: &str = agent_doc_queue::queue_consume::LEGACY_AGENT_DOC_REPLY_MARKER;

    /// The live shape: an answered item nested inside another waiting item's
    /// tree whose own attempt already reported, under a `subagents` queue.
    fn waiting_doc() -> String {
        format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             <!-- agent:queue subagents go -->\n\
             - ### ☑️ [#subagent:outer#1] ⟦☑️ you: 0 · agent: 1 · done: 0⟧ [#outer] Check the outer result {ITEM_MARKER}\n  \
             - **Next:** agent — integrate the reported result, then close the item or ask what remains (evidence: ☑️ `subagent:outer#1`) ▫️ {REPLY_MARKER}\n  \
             - ⟦📥 you: 0 · agent: 1 · done: 0⟧ [#answered] Decide how downloads are served {ITEM_MARKER}\n    \
             - **Next:** agent — pick up your reply “make it public” and attempt `#answered` (evidence: your reply) ▫️ {REPLY_MARKER}\n    \
             - make it public [#q-0123456789ab]\n\
             <!-- /agent:queue -->\n\n\
             <!-- agent:todo -->\n\
             - [ ] [#outer] Check the outer result\n\
             - [ ] [#answered] Decide how downloads are served\n\
             <!-- /agent:todo -->\n"
        )
    }

    fn write_doc(root: &Path, content: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("waiting.md");
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    fn answered_row(content: &str) -> String {
        content
            .lines()
            .find(|line| line.contains("[#answered]") && line.contains(ITEM_MARKER))
            .unwrap_or_else(|| panic!("no answered head in\n{content}"))
            .to_string()
    }

    /// Disk-backed effects. With `hide_marker`, the admission verifier's read
    /// models a replica that never shows the projected marker while the
    /// projector's own reads see it.
    #[derive(Default)]
    struct DiskEffects {
        hide_marker: RefCell<Option<String>>,
        verify_reads: Cell<usize>,
    }

    impl QueueRuntimeProjectionEffects for DiskEffects {
        fn current_document_content(&self, file: &Path, source: &str) -> Result<String> {
            let content = std::fs::read_to_string(file)?;
            if source == "queue_claim_admission_verify" {
                self.verify_reads.set(self.verify_reads.get() + 1);
                if let Some(marker) = self.hide_marker.borrow().as_deref() {
                    return Ok(content.replace(marker, ""));
                }
            }
            Ok(content)
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            anyhow::ensure!(
                std::fs::read_to_string(file)? == source_content,
                "projection must converge from the authoritative current cut"
            );
            std::fs::write(file, target_content)?;
            Ok(())
        }
    }

    #[test]
    fn claim_admission_projects_claimed_marker_onto_an_answered_waiting_item() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &waiting_doc());
        let effects = DiskEffects::default();

        let (_, claim) =
            claim_with_runtime_projection(&effects, &doc, "#answered", "subagent:worker", 600)
                .expect("an answered waiting item is admitted with a visible 🟡");
        assert_eq!(effects.verify_reads.get(), 1, "admission reads the queue-claims observer");

        let claimed = std::fs::read_to_string(&doc).unwrap();
        assert!(
            answered_row(&claimed).starts_with("  - 🟡 [#subagent:worker#1] ⟦"),
            "{}",
            answered_row(&claimed)
        );
        let reports = crate::queue_claim::claim_reports_for_content(&doc, &claimed).unwrap();
        let report = reports
            .iter()
            .find(|report| report.claim.generation == claim.generation)
            .unwrap();
        assert_eq!(report.visible_projection_state, "synchronized", "{report:#?}");
        assert_eq!(report.observed_visible_markers, ["🟡"]);
        // The reported outer item keeps its own receipt projection.
        assert!(
            claimed.contains("- ### ☑️ [#subagent:outer#1] ⟦☑️"),
            "{claimed}"
        );

        // Closeout's waiting-status pass and the runtime projector agree on
        // one fixed point: neither strips the other's projection.
        let rerendered =
            agent_doc_queue::queue_consume::project_waiting_on_you_tree(&claimed, "")
                .unwrap()
                .unwrap_or_else(|| claimed.clone());
        assert!(
            answered_row(&rerendered).starts_with("  - 🟡 [#subagent:worker#1] ⟦🟡 "),
            "{}",
            answered_row(&rerendered)
        );
        std::fs::write(&doc, &rerendered).unwrap();
        assert!(
            !project_claim_item_runtime_state_with_effects(&effects, &doc, "#answered", false)
                .unwrap(),
            "the runtime projection is already settled after the waiting pass"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), rerendered);

        crate::queue_claim::start_attempt(
            &doc,
            "#answered",
            "subagent:worker",
            "worker:waiting",
            600,
            claim.generation,
        )
        .unwrap();
        assert!(
            project_claim_item_runtime_state_with_effects(&effects, &doc, "#answered", false)
                .unwrap()
        );
        let running = std::fs::read_to_string(&doc).unwrap();
        assert!(
            answered_row(&running).starts_with("  - 🚧 [#subagent:worker#1] ⟦"),
            "{}",
            answered_row(&running)
        );
    }

    #[test]
    fn requeued_waiting_item_claim_clears_its_marker() {
        use crate::claim_release_receipt::{ReleaseKind, release_attempt};

        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &waiting_doc());
        let effects = DiskEffects::default();
        let (_, claim) =
            claim_with_runtime_projection(&effects, &doc, "#answered", "subagent:worker", 600)
                .unwrap();
        release_attempt(&doc, "#answered", ReleaseKind::Requeue, Some(claim.generation)).unwrap();
        project_claim_item_runtime_state_with_effects(&effects, &doc, &claim.item_text, true)
            .unwrap();
        let released = std::fs::read_to_string(&doc).unwrap();
        let row = answered_row(&released);
        assert!(row.starts_with("  - ⟦"), "{row}");
        assert!(!row.contains("[#subagent:worker#1]"), "{row}");
    }

    #[test]
    fn unrepresentable_claim_projection_refuses_admission_once_and_holds_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &waiting_doc());
        let content = std::fs::read_to_string(&doc).unwrap();
        assert!(
            crate::subagent_dispatch::pending_subagent_dispatch_for_content(&doc, &content)
                .unwrap()
                .iter()
                .any(|head| head.contains("[#answered]")),
            "the answered waiting item is offered for subagent dispatch"
        );

        let effects = DiskEffects::default();
        *effects.hide_marker.borrow_mut() = Some("🟡 [#subagent:worker#1] ".to_string());
        let error =
            claim_with_runtime_projection(&effects, &doc, "#answered", "subagent:worker", 600)
                .expect_err("a settled projection the queue-claims observer cannot see is refused");
        let refused = error
            .downcast_ref::<ClaimAdmissionRefused>()
            .expect("the refusal is typed");
        assert_eq!(refused.blocker, CLAIM_PROJECTION_UNREPRESENTABLE);
        assert!(format!("{error:#}").contains("no worker may be spawned"), "{error:#}");

        // Compensated: no live claim, no visible marker, a terminal receipt.
        assert!(crate::queue_claim::load_ledger(&doc).unwrap().claims.is_empty());
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(answered_row(&after).starts_with("  - ⟦"), "{}", answered_row(&after));
        let lineage = crate::queue_lineage::load(&doc).unwrap();
        assert_eq!(
            crate::queue_lineage::admission_refused_identities(&lineage),
            vec![(
                agent_doc_queue::queue_claim::claim_identity("#answered"),
                CLAIM_PROJECTION_UNREPRESENTABLE.to_string()
            )]
        );

        // One failure, one named blocker: the row is not re-offered, so the
        // coordinator cannot claim/refuse/re-claim it every cycle, and it is
        // out of the inline loop like any other held subagent head.
        assert!(
            !crate::subagent_dispatch::pending_subagent_dispatch_for_content(&doc, &after)
                .unwrap()
                .iter()
                .any(|head| head.contains("[#answered]")),
            "a refused admission is held from dispatch"
        );
        assert!(
            crate::queue_claim::claimed_items_for_content(&doc, &after)
                .claims(answered_row(&after).trim_start().trim_start_matches("- ")),
            "a refused admission is out of the in-session loop"
        );
        assert!(
            crate::subagent_dispatch::out_of_loop_items_for_content(&doc, &after)
                .claims(answered_row(&after).trim_start().trim_start_matches("- "))
        );

        // An explicit newer claim lifts the hold once the projection works.
        let effects = DiskEffects::default();
        let (_, reclaimed) =
            claim_with_runtime_projection(&effects, &doc, "#answered", "subagent:worker", 600)
                .expect("an explicit claim is admitted once the marker is visible");
        assert_eq!(reclaimed.generation, 2);
        let lineage = crate::queue_lineage::load(&doc).unwrap();
        assert!(crate::queue_lineage::admission_refused_identities(&lineage).is_empty());
        assert!(
            answered_row(&std::fs::read_to_string(&doc).unwrap())
                .starts_with("  - 🟡 [#subagent:worker#2] ⟦")
        );
    }

    #[test]
    fn transient_projection_failure_rolls_back_without_holding_dispatch() {
        struct RefusingEffects;
        impl QueueRuntimeProjectionEffects for RefusingEffects {
            fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
                Ok(std::fs::read_to_string(file)?)
            }
            fn converge_document_or_disk(
                &self,
                _file: &Path,
                _target_content: &str,
                _source_content: &str,
                _reason: &str,
            ) -> Result<()> {
                bail!("synthetic editor delivery refusal")
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &waiting_doc());
        let error =
            claim_with_runtime_projection(&RefusingEffects, &doc, "#answered", "subagent:w", 600)
                .unwrap_err();
        assert!(error.downcast_ref::<ClaimAdmissionRefused>().is_none(), "{error:#}");
        let lineage = crate::queue_lineage::load(&doc).unwrap();
        assert!(crate::queue_lineage::admission_refused_identities(&lineage).is_empty());
    }

    /// A CRDT relay modeled as a compare-and-swap register: a projection write
    /// lands only when its `source_content` still equals the relay current,
    /// otherwise it is refused exactly as `crdt_cp_write` refuses it
    /// (`recovery=retry_crdt_merge`). For the first `race_rounds` writes a
    /// concurrent writer thread lands an unrelated CRDT edit between the
    /// projection's authority read and its CAS, so the refusal is a real
    /// consequence of the interleaving, not a scripted error.
    struct CasRelayEffects {
        current: std::sync::Mutex<String>,
        race_rounds: usize,
        converges: std::sync::atomic::AtomicUsize,
        refusals: std::sync::atomic::AtomicUsize,
        race_open: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
        race_landed: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }

    /// The concurrent CRDT writer's half of the race channels.
    struct ConcurrentWriter {
        race_open: std::sync::mpsc::Receiver<()>,
        race_landed: std::sync::mpsc::Sender<()>,
    }

    const CONCURRENT_EDIT: &str = "concurrent operator note\n";

    impl CasRelayEffects {
        fn new(current: String, race_rounds: usize) -> (Self, ConcurrentWriter) {
            let (open_tx, open_rx) = std::sync::mpsc::channel();
            let (landed_tx, landed_rx) = std::sync::mpsc::channel();
            (
                Self {
                    current: std::sync::Mutex::new(current),
                    race_rounds,
                    converges: Default::default(),
                    refusals: Default::default(),
                    race_open: std::sync::Mutex::new(Some(open_tx)),
                    race_landed: std::sync::Mutex::new(landed_rx),
                },
                ConcurrentWriter {
                    race_open: open_rx,
                    race_landed: landed_tx,
                },
            )
        }

        /// The concurrent CRDT writer: lands one edit each time a raced
        /// projection write is between its authority read and its CAS. It
        /// exits when the claim returns (`close_race`), so a regression fails
        /// the assertions instead of deadlocking the scope.
        fn run_concurrent_writer(&self, writer: ConcurrentWriter, file: &Path) {
            while writer.race_open.recv().is_ok() {
                let mut current = self.current.lock().unwrap();
                current.push_str(CONCURRENT_EDIT);
                std::fs::write(file, current.as_str()).unwrap();
                drop(current);
                writer.race_landed.send(()).unwrap();
            }
        }

        fn close_race(&self) {
            self.race_open.lock().unwrap().take();
        }

        fn claim_racing_writer(
            &self,
            writer: ConcurrentWriter,
            file: &Path,
        ) -> Result<(
            agent_doc_queue::queue_claim::ClaimOutcome,
            agent_doc_queue::queue_claim::QueueClaim,
        )> {
            std::thread::scope(|scope| {
                scope.spawn(|| self.run_concurrent_writer(writer, file));
                let claimed =
                    claim_with_runtime_projection(self, file, "#answered", "subagent:worker", 600);
                self.close_race();
                claimed
            })
        }
    }

    impl QueueRuntimeProjectionEffects for CasRelayEffects {
        fn current_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
            Ok(self.current.lock().unwrap().clone())
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            use std::sync::atomic::Ordering;
            if self.converges.fetch_add(1, Ordering::SeqCst) < self.race_rounds
                && let Some(open) = self.race_open.lock().unwrap().as_ref()
            {
                open.send(()).unwrap();
                self.race_landed.lock().unwrap().recv().unwrap();
            }
            let mut current = self.current.lock().unwrap();
            if *current != source_content {
                self.refusals.fetch_add(1, Ordering::SeqCst);
                bail!(
                    "project controller command `crdt_cp_write` failed: CP relay write refused for {}: expected_hash={} current_hash={} recovery=retry_crdt_merge",
                    file.display(),
                    agent_doc_hash::content_hash(source_content),
                    agent_doc_hash::content_hash(&current),
                );
            }
            *current = target_content.to_string();
            std::fs::write(file, target_content)?;
            Ok(())
        }
    }

    #[test]
    fn claim_projection_retries_a_concurrent_crdt_write_without_burning_its_generation() {
        use std::sync::atomic::Ordering;
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &waiting_doc());
        let (effects, writer) = CasRelayEffects::new(waiting_doc(), 1);

        let (_, claim) = effects
            .claim_racing_writer(writer, &doc)
            .expect("a CAS refusal from a concurrent CRDT write is retried inside the claim");

        // The race really happened: one write was refused, the retry landed.
        assert_eq!(effects.refusals.load(Ordering::SeqCst), 1);
        assert_eq!(effects.converges.load(Ordering::SeqCst), 2);

        // One admission, one generation: the projection retried, the ledger
        // transition was neither rolled back nor re-allocated.
        assert_eq!(claim.generation, 1);
        let ledger = crate::queue_claim::load_ledger(&doc).unwrap();
        assert_eq!(ledger.claims.len(), 1, "{ledger:#?}");
        assert_eq!(ledger.claims[0].generation, 1);
        let lineage = crate::queue_lineage::load(&doc).unwrap();
        assert!(crate::queue_lineage::admission_refused_identities(&lineage).is_empty());

        // The retry rebased onto the concurrent write instead of clobbering it.
        let landed = std::fs::read_to_string(&doc).unwrap();
        assert!(landed.ends_with(CONCURRENT_EDIT), "{landed}");
        assert!(
            answered_row(&landed).starts_with("  - 🟡 [#subagent:worker#1] ⟦"),
            "{}",
            answered_row(&landed)
        );
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("queue_claim_runtime_projection_cas_retry attempt=1 max=8")
                && ops.contains("recovery=retry_crdt_merge (#claimprojectionretry)"),
            "{ops}"
        );
    }

    #[test]
    fn claim_projection_cas_retry_is_bounded_and_rolls_back_once_exhausted() {
        use std::sync::atomic::Ordering;
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &waiting_doc());
        // Every projection write races a concurrent CRDT write.
        let (effects, writer) =
            CasRelayEffects::new(waiting_doc(), CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS);

        let error = effects
            .claim_racing_writer(writer, &doc)
            .expect_err("a document that never stops moving exhausts the bounded retry");

        assert_eq!(
            effects.refusals.load(Ordering::SeqCst),
            CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS
        );
        let rendered = format!("{error:#}");
        assert!(rendered.contains("refused on all 8 attempts"), "{rendered}");
        assert!(rendered.contains("no worker may be spawned"), "{rendered}");
        // Still transient: rolled back, not terminalized as a held blocker.
        assert!(error.downcast_ref::<ClaimAdmissionRefused>().is_none(), "{rendered}");
        assert!(crate::queue_claim::load_ledger(&doc).unwrap().claims.is_empty());
        let lineage = crate::queue_lineage::load(&doc).unwrap();
        assert!(crate::queue_lineage::admission_refused_identities(&lineage).is_empty());
    }

    #[test]
    fn non_cas_projection_failure_is_not_retried() {
        struct CountingRefusal(std::cell::Cell<usize>);
        impl QueueRuntimeProjectionEffects for CountingRefusal {
            fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
                Ok(std::fs::read_to_string(file)?)
            }
            fn converge_document_or_disk(
                &self,
                _file: &Path,
                target_content: &str,
                _source_content: &str,
                _reason: &str,
            ) -> Result<()> {
                // The rollback repair writes the cleared projection; only the
                // admission write (which carries the marker) is counted.
                if target_content.contains("[#subagent:w#1]") {
                    self.0.set(self.0.get() + 1);
                }
                bail!("synthetic editor delivery refusal")
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &waiting_doc());
        let effects = CountingRefusal(std::cell::Cell::new(0));
        claim_with_runtime_projection(&effects, &doc, "#answered", "subagent:w", 600).unwrap_err();
        assert_eq!(effects.0.get(), 1, "only `retry_crdt_merge` refusals earn a retry");
    }
}

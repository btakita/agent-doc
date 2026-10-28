//! # Module: queue_cmd
//!
//! CLI subcommands for managing the `agent:queue` component.
//!
//! - `agent-doc queue sync <FILE>` — one-shot sync from backlog items with
//!   `queue` attribute or per-item enqueue markers into `agent:queue`.
//! - `agent-doc queue consume <FILE> [--count N]` — explicitly strike the
//!   leading N free-text queue head(s) the agent has already answered.
//! - `agent-doc queue consume <FILE> --id <id>` — escape hatch (#orphanqhead)
//!   that strikes an orphaned id-backed head whose backing backlog item was
//!   already reaped (or is gone), so it stops re-firing the auto-loop.
//! - `agent-doc queue consume <FILE> --ack-id <id>` — explicit acknowledgement
//!   (#freshqueueauth) for an id-backed correction head while leaving the open
//!   backlog item unresolved.
//! - `agent-doc queue consume <FILE> --ack-text <text>` — exact acknowledgement
//!   for a free-text head completed through direct harness steering while the
//!   queue remained paused.

use anyhow::{Result, bail};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use crate::one_shot_sync::OneShotQueueSyncResult;
use agent_doc_element_backlog::backlog;
use agent_doc_queue::document_queue::QueueEntry;
use agent_doc_queue::queue_heads::active_queue_head_text;
use agent_doc_queue::queue_heads::{ActiveQueueHeadKind, classify_active_queue_head};

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

pub trait QueueCommandEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<()>;

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
    fn prune_noise_queue_heads(&self, file: &Path) -> Result<usize>;
    fn converge_captured_answered_free_text_projection(&self, _file: &Path) -> Result<usize> {
        Ok(0)
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
    let components = agent_doc_element::element::parse(current)?;
    if !components.iter().any(|component| component.name == "queue") {
        return Ok(ClaimRuntimeProjection {
            target_content: current.to_string(),
            persisted_heads: Vec::new(),
            changed: false,
        });
    }
    let claim_ledger = crate::queue_claim::load_ledger_following(file, current);
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
            if claim.state != agent_doc_queue::queue_claim::QueueClaimState::Reported
                && claim.is_expired(now)
            {
                claim.state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
            }
            (claim.identity.clone(), claim)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let receipts = receipt_ledger
        .unwrap_or_default()
        .receipts
        .into_iter()
        .filter(|receipt| {
            !cancellations
                .iter()
                .any(|cancelled| cancelled.cancels_attempt(&receipt.identity, receipt.generation))
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
        .map(|evidence| agent_doc_queue::queue_claim::claim_identity(&evidence.item))
        .collect::<std::collections::HashSet<_>>();
    let cleared_identity = cleared_item.map(agent_doc_queue::queue_claim::claim_identity);
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
    let targets_for = |block: agent_doc_queue::queue_mirror_projection::QueueProjectionBlock<'_>| {
        let targets = block
            .entries
            .iter()
            .filter_map(|entry| match entry {
                QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
                    let struck = matches!(entry, QueueEntry::Completed(_));
                    // `#queuemirrormarkers`: a sibling queue's struck row is its
                    // own terminal (or `agent:loop` pass-owned) evidence.
                    // Another queue's live claim never reopens it.
                    if struck && !block.scheduler_selected {
                        return None;
                    }
                    let identity = agent_doc_queue::queue_claim::claim_identity(&prompt.text);
                    let observed =
                        agent_doc_document::queue_projection::lifecycle_marker(&prompt.text);
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
                    // `#strikeisterminal`: only a live attempt reopens a struck
                    // row; receipts and completion cells agree with the strike.
                    let durable_on_struck = struck
                        && agent_doc_queue::queue_claim::struck_row_reopened_by_claim(
                            claim_states.get(&identity),
                        );
                    if struck && !durable_on_struck {
                        return None;
                    }
                    let claim = (cleared_identity.as_ref() != Some(&identity))
                        .then(|| claim_states.get(&identity))
                        .flatten();
                    let receipt = (cleared_identity.as_ref() != Some(&identity) && claim.is_none())
                        .then(|| receipts.get(&identity))
                        .flatten();
                    let marker = if state_degraded || durable_on_struck {
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Unknown
                    } else if let Some(claim) = claim {
                        claim.state.projection_marker()
                    } else if receipt.is_some() {
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Reported
                    } else if completion_evidence.contains(&identity) {
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Reconciling
                    } else if selected.claims(&prompt.text) {
                        agent_doc_document::queue_projection::QueueLifecycleMarker::Running
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
                        text: agent_doc_document::queue_projection::strip_in_progress_marker(
                            &prompt.text,
                        ),
                        marker,
                        owner,
                        reopen_completed: durable_on_struck,
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
        changed: target_content != current,
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
    if projection.changed {
        // Keep the recovery baseline aligned with this binary-owned cosmetic
        // mutation. Otherwise a later exchange update can make the unrelated
        // queue diff block the document-only auto-commit boundary.
        if let Ok(Some(snapshot)) = agent_doc_snapshot_io::load_document_baseline(file) {
            let synced = agent_doc_document::queue_projection::sync_in_progress_marker_regions(
                &snapshot,
                &projection.target_content,
            );
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
    agent_doc_cycle_state_io::set_projected_in_progress_queue_heads(
        file,
        &projection.persisted_heads,
    )?;
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
    effects: &impl QueueCommandEffects,
    file: &Path,
    cleared_item: Option<&str>,
) -> Result<bool> {
    let mut wrote_successor = false;
    for attempt in 1..=CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS {
        // Start every attempt from the live authority. This first absorbs any
        // operator/editor normalization that raced the ledger transition; the
        // marker overlay below is then rederived by stable work identity.
        let current = effects.current_document_content(file, "queue_claim_runtime_projection")?;
        // A completion cell is durable attempt-scoped evidence. Reconcile it
        // into the ledger before deriving markers, including after restart.
        crate::queue_claim::reconcile_completion_evidence(file, &current)?;
        let projection = derive_claim_runtime_projection(file, &current, cleared_item)?;
        if projection.changed {
            effects.converge_document_or_disk(
                file,
                &projection.target_content,
                &current,
                "queue_claim_runtime_projection",
            )?;
            wrote_successor = true;
        }

        // Convergence may legitimately preserve a concurrent editor cell over
        // this cosmetic successor. Success is therefore a property of a fresh
        // authoritative cut, not of the write call returning `Ok(())`.
        let authoritative =
            effects.current_document_content(file, "queue_claim_runtime_projection_verify")?;
        let settled = derive_claim_runtime_projection(file, &authoritative, cleared_item)?;
        if !settled.changed {
            let accepted = ClaimRuntimeProjection {
                target_content: authoritative,
                persisted_heads: settled.persisted_heads,
                changed: wrote_successor,
            };
            record_claim_runtime_projection(file, &accepted, cleared_item)?;
            return Ok(wrote_successor);
        }

        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_runtime_projection_retry attempt={attempt} max={} (#queueclaimprojectionrace)",
                CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS
            ),
        );
    }

    bail!(
        "queue claim runtime projection did not converge after {} authoritative retries; the claim ledger transition is durable but its live queue marker is not yet verified",
        CLAIM_RUNTIME_PROJECTION_MAX_ATTEMPTS
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
) -> Result<()> {
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    match effects.acknowledge_paused_free_text_queue_head(file, expected_head)? {
        Some(outcome) => println!(
            "{}: acknowledged exact paused free-text queue head (remaining: {}); \
             queue stays paused (#paused-free-text-ack).",
            file.display(),
            outcome.remaining
        ),
        None => println!(
            "{}: no paused free-text queue head to acknowledge.",
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

pub fn sync_with_effects(effects: &impl QueueCommandEffects, file: &Path) -> Result<()> {
    let content = effects.current_document_content(file, "queue_sync")?;
    match crate::one_shot_sync::sync_one_shot_backlog_queue_with_snapshot(
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
    )? {
        OneShotQueueSyncResult::AlreadyInSync {
            requested_count,
            mode,
        } => {
            println!(
                "{}: queue already in sync ({} active backlog id(s), {:?} mode). No changes.",
                file.display(),
                requested_count,
                mode
            );
        }
        OneShotQueueSyncResult::Synced(applied) => {
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
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

    #[derive(Default)]
    struct FakeEffects {
        current: RefCell<Option<String>>,
    }

    impl FakeEffects {
        fn with_current(content: impl Into<String>) -> Self {
            Self {
                current: RefCell::new(Some(content.into())),
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
            result.contains("- do [#alpha]"),
            "marked item should be queued:\n{result}"
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
        assert_eq!(outcome, agent_doc_queue::queue_claim::ClaimOutcome::Reassigned);
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
                    // expired #1. Case 2: the released report's annotation
                    // goes; no attempt governs the bare loop copy. Case 3: a
                    // `🚧` with no worker binding and no in-session selection
                    // is not a lifecycle state.
                    "- 🟡 [#subagent:layout_liveverify#2] do [#layoutunprovenliveverify]\n\
                     - 📥 release + publish\n\
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
            two_queue_doc("- 📥 do [#jb263]\n", "- 📥 do [#jb263]\n- ~~do [#jb263]~~\n"),
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
        assert!(claimed.contains("- 📥 inline coordinator work"), "{claimed}");
        assert!(
            claimed.contains(
                "- 🟡 [#subagent:worker#1] dispatched worker work"
            ),
            "{claimed}"
        );
        let claimed_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            claimed_snapshot.contains(
                "- 🟡 [#subagent:worker#1] dispatched worker work"
            ),
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
            running.contains(
                "- 🚧 [#subagent:worker#1] dispatched worker work"
            ),
            "{running}"
        );

        crate::queue_claim::report_attempt(&doc, "dispatched worker work", Some(1)).unwrap();
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
            reported.contains(
                "- ☑️ [#subagent:worker#1] dispatched worker work"
            ),
            "{reported}"
        );
        let synchronized = claim_reports_with_effects(&effects, &doc).unwrap();
        assert_eq!(synchronized[0].visible_projection_state, "synchronized");
        assert!(synchronized[0].next_action_argv.is_none());

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
            released_projection.contains(
                "- ☑️ [#subagent:worker#1] dispatched worker work"
            )
                && !released_projection.contains("- 🟡 dispatched worker work"),
            "{released_projection}"
        );
        let released_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            released_snapshot.contains("- 📥 inline coordinator work")
                && released_snapshot.contains(
                    "- ☑️ [#subagent:worker#1] dispatched worker work"
                )
                && !released_snapshot.contains("- 🟡 dispatched worker work"),
            "{released_snapshot}"
        );
        assert_eq!(released.item_text, "dispatched worker work");
    }

    #[test]
    fn cancelled_generation_never_projects_stale_report_or_completion_onto_readded_row() {
        let dir = tempfile::tempdir().unwrap();
        let item = "finish sdk demo";
        let doc = write_claim_projection_doc(dir.path(), &format!("- {item}\n"));
        let (_, claim) =
            crate::queue_claim::claim(&doc, item, "subagent:sdk", 600).unwrap();
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
        assert!(live.contains("- 🚧 [#subagent:free#1] free text worker\n"), "{live}");
        assert!(live.contains("- 🚧 [#subagent:normalized#1] do [#normalized-worker]\n"), "{live}");
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
        crate::queue_claim::report_attempt(&doc, "release + publish", Some(claim.generation))
            .unwrap();
        let reported_row = format!("- ☑️ [#{owner}#{}] release + publish", claim.generation);
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
        // one, and preflight annotates both copies in the same queue cut.
        let restored = projection.target_content.replace(
            &format!("{reported_row}\n"),
            &format!("- release + publish ⟦effect:1⟧\n{reported_row} ⟦effect:2⟧\n"),
        );
        std::fs::write(&doc, &restored).unwrap();

        let projection = derive_claim_runtime_projection(&doc, &restored, None).unwrap();
        assert!(
            projection
                .target_content
                .contains(&format!("{reported_row} ⟦effect:2⟧\n")),
            "{}",
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
        assert_eq!(reports[0].claim.item_text, "release + publish ⟦effect:2⟧");
        assert_eq!(
            reports[0].claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
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
        let doc = write_claim_projection_doc(
            dir.path(),
            &format!("- {item}\n- {unrelated}\n"),
        );
        crate::queue_claim::claim(&doc, item, "subagent:gh299", 600).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            item,
            "subagent:gh299",
            "/root/gh299",
            600,
            1,
        )
        .unwrap();

        // Model a stale visible cut after the durable worker binding survived a
        // controller handoff: the exact running row is bare and its sibling is
        // unrelated. Reprojection must repair only the bound attempt's row.
        let stale = std::fs::read_to_string(&doc).unwrap();
        let projection = derive_claim_runtime_projection(&doc, &stale, None).unwrap();

        assert!(projection.changed);
        assert!(
            projection.target_content.contains(&format!(
                "- 🚧 [#subagent:gh299#1] {item}\n"
            )),
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
            projection
                .target_content
                .contains(&format!(
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
    fn manually_struck_row_with_live_claim_reopens_as_explicit_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let item = "#gh-issue https://github.com/example/project/issues/299";
        let doc = write_claim_projection_doc(dir.path(), &format!("- {item}\n"));
        crate::queue_claim::claim(&doc, item, "subagent:gh299", 600).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            item,
            "subagent:gh299",
            "/root/gh299",
            600,
            1,
        )
        .unwrap();

        let live = std::fs::read_to_string(&doc).unwrap();
        let live_projection = derive_claim_runtime_projection(&doc, &live, None).unwrap();
        assert_eq!(
            live_projection
                .target_content
                .matches(&format!(
                    "- 🚧 [#subagent:gh299#1] {item}\n"
                ))
                .count(),
            1,
            "a live worked row must project exactly one typed lifecycle marker: {}",
            live_projection.target_content
        );

        // Markdown strike styling cannot retire an exact live attempt. The
        // projection reopens the row as degraded, preserving owner/generation.
        let terminal = live_projection.target_content.replace(
            &format!("- 🚧 [#subagent:gh299#1] {item}\n"),
            &format!("- ~~{item}~~\n"),
        );
        let terminal_projection = derive_claim_runtime_projection(&doc, &terminal, None).unwrap();
        assert!(
            terminal_projection
                .target_content
                .contains(&format!(
                    "- ❔ [#subagent:gh299#1] {item}\n"
                )),
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
        let doc = write_claim_projection_doc(
            dir.path(),
            &format!("- 🚧 {receipted}\n- 🚧 {celled}\n"),
        );
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
            projected.contains("- 📥 operator unsaved sibling"),
            "{projected}"
        );
        assert!(
            projected.contains(
                "- 🟡 [#subagent:worker#1] dispatched worker work"
            ),
            "{projected}"
        );
    }
}

//! Goal-loop redispatch (`#goalpresetterminal`).
//!
//! A queue row whose resolved `/goal` preset declares final states
//! (`REVIEW-CLEAN ...`, `BLOCKED ...`) owns a whole review → fix → fresh-review
//! loop. When its worker hands back without a completing final state, the
//! hand-back is an interrupted loop, not a completion. An already-answered
//! human-review `BLOCKED` does not complete (`#operatororderclosed`). `queue
//! report-complete` then:
//!
//! 1. records the hand-back (the worker's delivery worktree and remaining
//!    findings, verbatim) in the per-document `queue_goal_loop_handoffs` state
//!    row,
//! 2. requeues the exact attempt (`queue release --requeue` semantics), so the
//!    SAME row stays open and becomes dispatchable again, with no duplicate
//!    row, and
//! 3. leaves the row for the coordinator to claim and dispatch again; the next
//!    `queue brief` / dispatch authorization for the row carries the recorded
//!    hand-back.
//!
//! A hand-back that opens on a final state takes the ordinary report path and
//! retires the row's recorded hand-off. Closeout of the row is still gated by
//! the coordinator response carrying a final-state line
//! ([`agent_doc_queue::goal_preset_terminal`]).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_queue::goal_preset_terminal::{
    GOAL_LOOP_HANDOFFS_STATE_KIND, GoalLoopHandoff, GoalLoopHandoffLedger,
};
use agent_doc_queue::queue_claim::QueueClaim;
use anyhow::{Context, Result};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn state_identity(file: &Path) -> Result<Option<(PathBuf, String, String)>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok(Some((root, hash, canonical.to_string_lossy().into_owned())))
}

fn load_from_conn(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
) -> Result<GoalLoopHandoffLedger> {
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        conn,
        document_hash,
        GOAL_LOOP_HANDOFFS_STATE_KIND,
    )?
    else {
        return Ok(GoalLoopHandoffLedger::default());
    };
    serde_json::from_str(&record.payload_json).context("parse queue goal-loop hand-offs")
}

/// The document's recorded goal-loop hand-offs.
pub fn load(file: &Path) -> Result<GoalLoopHandoffLedger> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(GoalLoopHandoffLedger::default());
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(GoalLoopHandoffLedger::default());
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    load_from_conn(&conn, &document_hash)
}

fn mutate<T: Default>(
    file: &Path,
    mutate: impl FnOnce(&mut GoalLoopHandoffLedger) -> T,
) -> Result<T> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(T::default());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin queue goal-loop hand-off transaction")?;
    let result = (|| {
        let mut ledger = load_from_conn(&conn, &document_hash)?;
        let value = mutate(&mut ledger);
        if ledger.handoffs.is_empty() {
            agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
                &conn,
                &document_hash,
                GOAL_LOOP_HANDOFFS_STATE_KIND,
            )?;
        } else {
            agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
                &conn,
                &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                    document_hash: document_hash.clone(),
                    state_kind: GOAL_LOOP_HANDOFFS_STATE_KIND.to_string(),
                    canonical_path,
                    payload_json: serde_json::to_string(&ledger)
                        .context("serialize queue goal-loop hand-offs")?,
                    updated_at_secs: now_secs(),
                },
            )?;
        }
        Ok(value)
    })();
    match result {
        Ok(value) => {
            conn.execute_batch("COMMIT")
                .context("commit queue goal-loop hand-off transaction")?;
            Ok(value)
        }
        Err(err) => {
            if let Err(rollback) = conn.execute_batch("ROLLBACK") {
                eprintln!(
                    "[queue-claim] WARNING: goal-loop hand-off rollback failed after {err:#}: {rollback}"
                );
            }
            Err(err)
        }
    }
}

/// A hand-back `queue report-complete` requeued instead of reporting.
#[derive(Debug, Clone)]
pub struct GoalLoopRequeue {
    /// The requeued attempt.
    pub claim: QueueClaim,
    /// The final states the row's preset declares.
    pub final_states: Vec<String>,
    /// `#operatororderclosed`: skip the immediate pool refill. True only when
    /// the hand-back is an already-answered human-review `BLOCKED` and the
    /// pull-request response that says why is already written for this head.
    pub suppress_immediate_refill: bool,
    /// `#operatororderclosed`: the hand-back is `BLOCKED` on a human review the
    /// preset already refuses to approve or dismiss.
    pub answered_blocked_review: bool,
    /// `#addressinline`: an `#address` or `#adversarial-pr` hand-back whose
    /// root inline discussion threads are not shown on the current head.
    pub inline_threads_pending: bool,
}

/// `#goalpresetterminal`: decide `queue report-complete` for a goal-loop row.
///
/// Returns `Ok(None)` when the ordinary report path applies: `item` has no
/// live subagent claim, its preset declares no final states, or `response`
/// opens on a declared final state that completes the order (which also
/// retires the row's recorded hand-off). An already-answered human-review
/// `BLOCKED` does not complete (`#operatororderclosed`). Otherwise the
/// hand-back is an interrupted loop: it is recorded,
/// the exact attempt is requeued so the same row is dispatchable again, and
/// the requeued claim is returned. The caller reprojects the claim marker.
pub fn requeue_nonterminal_hand_back(
    file: &Path,
    content: &str,
    item: &str,
    generation: Option<u64>,
    response: &str,
) -> Result<Option<GoalLoopRequeue>> {
    let ledger = crate::queue_claim::load_ledger_following(file, content)?;
    // `#reportcompleteselector`: the same selector resolution as the report.
    let Some(claim) = ledger.resolve_selector(item)?.cloned() else {
        return Ok(None);
    };
    if !claim.owner.starts_with("subagent:") {
        return Ok(None);
    }
    let presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(content, file);
    let preset_pairs = agent_doc_queue::goal_preset_terminal::resolved_presets(&presets);
    let final_states = agent_doc_queue::goal_preset_terminal::head_final_states_in(
        &claim.item_text,
        &preset_pairs,
    );
    // `#addressinline`: `#address` declares no final states, so the empty-state
    // return must not skip a row whose inline discussion threads are unsettled.
    let inline_goal = agent_doc_queue::inline_discussion::head_requires_inline_discussions(
        &claim.item_text,
        &preset_pairs,
    );
    let inline_pending =
        inline_goal && !agent_doc_queue::inline_discussion::inline_threads_settled(response);
    if final_states.is_empty() && !inline_goal {
        return Ok(None);
    }
    let bodies = agent_doc_queue::goal_preset_terminal::head_goal_preset_bodies_in(
        &claim.item_text,
        &preset_pairs,
    );
    let live = agent_doc_queue::queue_continuation::live_queue_head_identities(content);
    let completes = !final_states.is_empty()
        && agent_doc_queue::goal_preset_terminal::hand_back_completes_goal_loop(
            response,
            &final_states,
            &bodies,
        );
    // A settled `#address` row, and a settled `REVIEW-CLEAN` or `BLOCKED` line,
    // take the ordinary report path. An already-answered human-review `BLOCKED`
    // does not complete (`#operatororderclosed`).
    if !inline_pending && (final_states.is_empty() || completes) {
        let identity = claim.identity.clone();
        mutate(file, |ledger| {
            ledger.forget(&identity);
            if let Some(live) = live.as_ref() {
                ledger.retain_live(live);
            }
        })?;
        return Ok(None);
    }
    let generation = generation.unwrap_or(claim.generation);
    if generation != claim.generation {
        anyhow::bail!(
            "stale queue claim generation {generation}; current generation is {}",
            claim.generation
        );
    }
    let handoff = GoalLoopHandoff {
        identity: claim.identity.clone(),
        item_text: claim.item_text.clone(),
        owner: claim.owner.clone(),
        worker_id: claim.worker.as_ref().map(|worker| worker.worker_id.clone()),
        generation: claim.generation,
        final_states: final_states.clone(),
        response: response.trim().to_string(),
        recorded_at_secs: now_secs(),
        quota_block: None,
        prompt_pressure: None,
        inline_threads_pending: inline_pending,
    };
    mutate(file, |ledger| {
        if let Some(live) = live.as_ref() {
            ledger.retain_live(live);
        }
        ledger.record(handoff);
    })?;
    let requeued = crate::claim_release_receipt::release_attempt(
        file,
        &claim.item_text,
        crate::claim_release_receipt::ReleaseKind::Requeue,
        Some(claim.generation),
    )?
    .unwrap_or(claim);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_goal_loop_hand_back_requeued owner={} generation={} final_states={} response_bytes={} item_bytes={} ({})",
            requeued.owner,
            requeued.generation,
            final_states.join(","),
            response.trim().len(),
            requeued.item_text.trim().len(),
            if inline_pending {
                "#addressinline"
            } else {
                "#goalpresetterminal"
            },
        ),
    );
    let answered_blocked_review =
        agent_doc_queue::goal_preset_terminal::blocked_review_order_already_answered(
            response, &bodies,
        );
    let suppress_immediate_refill = answered_blocked_review
        && agent_doc_queue::goal_preset_terminal::response_published_pull_request_explanation(
            response,
        );
    Ok(Some(GoalLoopRequeue {
        claim: requeued,
        final_states,
        suppress_immediate_refill,
        answered_blocked_review,
        inline_threads_pending: inline_pending,
    }))
}

/// `#promptpressure`: record an unfinished prompt hand-back and requeue the
/// exact attempt. `stored_response` is what the next brief quotes: the prose
/// hand-back, or the one-line review pointer. No completion cell is written.
pub fn requeue_with_prompt_pressure(
    file: &Path,
    content: &str,
    claim: &QueueClaim,
    generation: Option<u64>,
    stored_response: &str,
    kind: agent_doc_queue::goal_preset_terminal::PromptPressureKind,
) -> Result<QueueClaim> {
    let generation = generation.unwrap_or(claim.generation);
    if generation != claim.generation {
        anyhow::bail!(
            "stale queue claim generation {generation}; current generation is {}",
            claim.generation
        );
    }
    let live = agent_doc_queue::queue_continuation::live_queue_head_identities(content);
    let handoff = GoalLoopHandoff {
        identity: claim.identity.clone(),
        item_text: claim.item_text.clone(),
        owner: claim.owner.clone(),
        worker_id: claim.worker.as_ref().map(|worker| worker.worker_id.clone()),
        generation: claim.generation,
        final_states: Vec::new(),
        response: stored_response.trim().to_string(),
        recorded_at_secs: now_secs(),
        quota_block: None,
        prompt_pressure: Some(kind.clone()),
        inline_threads_pending: false,
    };
    mutate(file, |ledger| {
        if let Some(live) = live.as_ref() {
            ledger.retain_live(live);
        }
        ledger.record(handoff);
    })?;
    let requeued = crate::claim_release_receipt::release_attempt(
        file,
        &claim.item_text,
        crate::claim_release_receipt::ReleaseKind::Requeue,
        Some(claim.generation),
    )?
    .unwrap_or_else(|| claim.clone());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_prompt_pressure_requeued owner={} generation={} kind={kind:?} response_bytes={} item_bytes={} (#promptpressure)",
            requeued.owner,
            requeued.generation,
            stored_response.trim().len(),
            requeued.item_text.trim().len(),
        ),
    );
    Ok(requeued)
}

/// Record an explicitly requeued quota-blocked attempt, so the next worker's
/// brief names its partial branch and worktree (`#harnessquotaexhausted`).
pub fn record_quota_requeue(file: &Path, claim: &QueueClaim) -> Result<()> {
    let Some(block) = claim.quota_block.clone() else {
        return Ok(());
    };
    let handoff = GoalLoopHandoff {
        identity: claim.identity.clone(),
        item_text: claim.item_text.clone(),
        owner: claim.owner.clone(),
        worker_id: claim.worker.as_ref().map(|worker| worker.worker_id.clone()),
        generation: claim.generation,
        final_states: Vec::new(),
        response: block.evidence.clone(),
        recorded_at_secs: now_secs(),
        quota_block: Some(block),
        prompt_pressure: None,
        inline_threads_pending: false,
    };
    mutate(file, |ledger| ledger.record(handoff))
}

/// Attach the row's recorded interrupted hand-back, if any, to a dispatch
/// authorization for `item`, so the next worker continues the same loop. An
/// unreadable ledger is reported and treated as "no hand-off".
pub fn attach_prior_attempt(
    file: &Path,
    item: &str,
    authorization: &mut agent_doc_queue::subagent_brief::SubagentAuthorization,
) {
    let ledger = match load(file) {
        Ok(ledger) => ledger,
        Err(err) => {
            eprintln!(
                "[queue-claim] WARNING: could not read goal-loop hand-offs for {}: {err:#}",
                file.display()
            );
            return;
        }
    };
    let identity = agent_doc_queue::queue_claim::claim_identity(item);
    let normalized = agent_doc_document::queue_projection::strip_priority_markers(item)
        .trim()
        .to_string();
    let handoff = ledger.latest_for(&identity).or_else(|| {
        ledger.handoffs.iter().rev().find(|handoff| {
            agent_doc_document::queue_projection::strip_priority_markers(&handoff.item_text).trim()
                == normalized
        })
    });
    if let Some(handoff) = handoff {
        authorization.attach_prior_goal_attempt(handoff.clone());
    }
}

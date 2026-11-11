//! Durable reported-worker release receipts (`#reportedrelease`).
//!
//! Storage for [`agent_doc_queue::claim_release_receipt`]: one
//! `queue_document_state` row per document
//! (`state_kind = "queue_claim_release_receipts"`) in the project `state.db`,
//! beside (not inside) the claim ledger, so a claim mutation never rewrites a
//! receipt and the other way round.
//!
//! `queue release` records a receipt for the claim it ends; `queue release
//! --requeue` retains a started attempt as `Lost`, removes a never-started
//! claim cleanly, and forgets any receipt. Readers (mid-turn steering, the
//! subagent dispatch plan, the Codex Stop hook) are actorless
//! one-shot boundaries, like the claim readers: an unreadable receipt row is
//! reported on stderr and treated as "no receipts", the pre-receipt behaviour.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_queue::claim_release_receipt::{
    CLAIM_RELEASE_RECEIPTS_STATE_KIND, ClaimReleaseReceipt, ClaimReleaseReceiptLedger,
    DEFAULT_RELEASE_RECEIPT_TTL_SECS,
};
use agent_doc_queue::queue_claim::{ClaimedQueueItems, QueueClaim};
use agent_doc_queue::queue_continuation::live_queue_head_identities;
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
) -> Result<ClaimReleaseReceiptLedger> {
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        conn,
        document_hash,
        CLAIM_RELEASE_RECEIPTS_STATE_KIND,
    )?
    else {
        return Ok(ClaimReleaseReceiptLedger::default());
    };
    serde_json::from_str(&record.payload_json).context("parse queue claim release receipts")
}

/// The document's stored receipts, including rows awaiting closure pruning.
pub fn load(file: &Path) -> Result<ClaimReleaseReceiptLedger> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(ClaimReleaseReceiptLedger::default());
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(ClaimReleaseReceiptLedger::default());
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    load_from_conn(&conn, &document_hash)
}

/// One open, exactly attested reported-release receipt whose queue row's
/// directive id names a tracked item (`#nestedreceiptack`,
/// `#dupreceiptconsume`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentIdReceiptAttempt {
    pub owner: String,
    pub generation: u64,
    /// The released claim's queue row text.
    pub item_text: String,
}

/// `#dupreceiptconsume`: every distinct exact worker attempt whose open
/// reported-release receipt was recorded for a queue row whose directive id is
/// `target_id`, oldest claim first. Closed attempts and legacy owner-only
/// receipts never qualify. More than one is normal: an open Todo answered
/// twice, or dispatched as a waiting row and later answered by a queue row,
/// leaves one receipt per attempt, and only the caller can tell which of them
/// still has a reported row in the document.
pub fn reported_receipt_attempts_for_parent_id(
    file: &Path,
    target_id: &str,
) -> Result<Vec<ParentIdReceiptAttempt>> {
    let mut receipts = load(file)?.receipts;
    receipts.sort_by_key(|receipt| receipt.claimed_at_secs);
    let mut attempts: Vec<ParentIdReceiptAttempt> = Vec::new();
    for receipt in receipts {
        // `#todoeditrevivesrow`: a closed attempt is consumed work, never the
        // receipt a re-projected parent row of the same id acknowledges.
        if receipt.attempt_closed() || !receipt.attests_exact_attempt() {
            continue;
        }
        let Some(id) =
            agent_doc_queue::queue_response::queue_prompt_done_id(receipt.item_text.trim())
        else {
            continue;
        };
        if !id.eq_ignore_ascii_case(target_id)
            || attempts.iter().any(|attempt| {
                attempt.owner == receipt.owner && attempt.generation == receipt.generation
            })
        {
            continue;
        }
        attempts.push(ParentIdReceiptAttempt {
            owner: receipt.owner,
            generation: receipt.generation,
            item_text: receipt.item_text,
        });
    }
    Ok(attempts)
}

/// `#nestedreceiptack`: the one exact worker attempt (`owner`, `generation`)
/// whose reported-release receipt was recorded for a queue row whose directive
/// id is `target_id`. Only receipts that attest an exact attempt qualify;
/// several distinct attempts are ambiguous and refused here. `queue consume
/// --ack-id` resolves them against the document instead
/// ([`reported_receipt_attempts_for_parent_id`], `#dupreceiptconsume`).
pub fn reported_receipt_attempt_for_parent_id(
    file: &Path,
    target_id: &str,
) -> Result<Option<(String, u64)>> {
    let attempts = reported_receipt_attempts_for_parent_id(file, target_id)?;
    if attempts.len() > 1 {
        anyhow::bail!(
            "{}: [#{target_id}] has {} reported-release receipts from distinct attempts ({}); preserve them for explicit repair",
            file.display(),
            attempts.len(),
            attempts
                .iter()
                .map(|attempt| format!("{}#{}", attempt.owner, attempt.generation))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(attempts
        .into_iter()
        .next()
        .map(|attempt| (attempt.owner, attempt.generation)))
}

/// End the reported-release receipt of exactly `owner`/`generation`
/// (`#nestedreceiptack`, `#strikeretires`). Returns whether one was removed.
pub fn retire_exact_reported_receipt(
    file: &Path,
    owner: &str,
    generation: u64,
    source: &str,
) -> Result<bool> {
    let removed = mutate(file, |ledger| {
        let before = ledger.receipts.len();
        ledger
            .receipts
            .retain(|receipt| !(receipt.owner == owner && receipt.generation == generation));
        before - ledger.receipts.len()
    })?;
    if removed > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_release_receipt_exact_retired owner={owner} generation={generation} source={source} (#nestedreceiptack #strikeretires)"
            ),
        );
    }
    Ok(removed > 0)
}

/// Close the reported-release receipt of exactly `owner`/`generation`
/// (`#todoeditrevivesrow`), keeping its response capture. The strike that
/// ended that exact attempt calls this, so the consumed attempt stops
/// governing its identity even when a row with the same `[#id]` is live again
/// in the successor (a todo edit re-projects the waiting row under new text).
/// Returns whether one was closed.
pub fn close_exact_reported_receipt(
    file: &Path,
    owner: &str,
    generation: u64,
    source: &str,
) -> Result<bool> {
    let now = now_secs();
    let closed = mutate(file, |ledger| {
        ledger.close_exact_attempt(owner, generation, now)
    })?;
    if closed > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_release_receipt_exact_closed owner={owner} generation={generation} source={source} response=retained (#todoeditrevivesrow)"
            ),
        );
    }
    Ok(closed > 0)
}

/// Remove and return the exact reported-release receipt cancelled by operator
/// row deletion. A stale generation never consumes a replacement receipt.
pub fn cancel_reported_attempt(
    file: &Path,
    item: &str,
    generation: Option<u64>,
) -> Result<Option<ClaimReleaseReceipt>> {
    let identity = agent_doc_queue::queue_claim::claim_identity(item);
    let cancelled = mutate(file, |ledger| {
        let index = ledger.receipts.iter().position(|receipt| {
            receipt.identity == identity
                && (receipt.generation == 0 || generation == Some(receipt.generation))
        })?;
        Some(ledger.receipts.remove(index))
    })?;
    if let Some(receipt) = &cancelled {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_release_receipt_cancelled owner={} generation={} item_bytes={} (#queue-removal-cancels)",
                receipt.owner,
                receipt.generation,
                receipt.item_text.trim().len(),
            ),
        );
    }
    Ok(cancelled)
}

/// Apply `mutate` to the receipts in one immediate transaction. A document
/// outside any agent-doc project has no state.db and nothing to record.
fn mutate<T: Default>(
    file: &Path,
    mutate: impl FnOnce(&mut ClaimReleaseReceiptLedger) -> T,
) -> Result<T> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(T::default());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin queue release receipt transaction")?;
    let result = (|| {
        let mut ledger = load_from_conn(&conn, &document_hash)?;
        let value = mutate(&mut ledger);
        if ledger.receipts.is_empty() {
            agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
                &conn,
                &document_hash,
                CLAIM_RELEASE_RECEIPTS_STATE_KIND,
            )?;
        } else {
            agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
                &conn,
                &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                    document_hash: document_hash.clone(),
                    state_kind: CLAIM_RELEASE_RECEIPTS_STATE_KIND.to_string(),
                    canonical_path,
                    payload_json: serde_json::to_string(&ledger)
                        .context("serialize queue claim release receipts")?,
                    updated_at_secs: now_secs(),
                },
            )?;
        }
        Ok(value)
    })();
    match result {
        Ok(value) => {
            conn.execute_batch("COMMIT")
                .context("commit queue release receipt transaction")?;
            Ok(value)
        }
        Err(err) => {
            if let Err(rollback) = conn.execute_batch("ROLLBACK") {
                eprintln!(
                    "[queue-claim] WARNING: release receipt rollback failed after {err:#}: {rollback}"
                );
            }
            Err(err)
        }
    }
}

/// How a `queue release` ends its claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseKind {
    /// The worker reported back: record a receipt so the line is persisted by
    /// the coordinator and never offered to a new worker.
    Reported,
    /// Abandoned, failed, or requeued work: no receipt. A started attempt is
    /// retained as `Lost`; a claim with no worker is removed cleanly.
    Requeue,
}

/// Durably capture the full worker response for one exact attempt. This must
/// succeed before the claim may advance to `Reported`.
pub fn capture_response(
    file: &Path,
    claim: &QueueClaim,
    response: &str,
) -> Result<ClaimReleaseReceipt> {
    let completion_key = crate::queue_claim::completion_feedback_key(claim)?;
    let now = now_secs();
    let refused = mutate(file, |ledger| {
        ledger
            .record_response_capture(
                claim,
                completion_key.clone(),
                response,
                now,
                DEFAULT_RELEASE_RECEIPT_TTL_SECS,
            )
            .err()
    })?;
    if let Some(refused) = refused {
        return Err(refused.into());
    }
    let receipt = load(file)?
        .receipts
        .into_iter()
        .find(|receipt| {
            receipt.identity == claim.identity
                && receipt.generation == claim.generation
                && receipt.worker_id == claim.worker.as_ref().map(|worker| worker.worker_id.clone())
        })
        .context("captured queue response receipt was not persisted")?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_response_receipt_captured owner={} worker={} generation={} response_id={} body_bytes={}",
            claim.owner,
            receipt.worker_id.as_deref().unwrap_or("<unbound>"),
            claim.generation,
            receipt.completion_key.as_deref().unwrap_or("<missing>"),
            response.trim().len(),
        ),
    );
    Ok(receipt)
}

/// Resolve and capture the response for the current exact attempt. This is a
/// convenient boundary for recovery/tests that have attempt coordinates but
/// not an already-loaded claim; it retains the same generation fence as the
/// report transition.
pub fn capture_response_for_attempt(
    file: &Path,
    item: &str,
    generation: u64,
    response: &str,
) -> Result<ClaimReleaseReceipt> {
    let claim = claim_before_release(file, item, Some(generation), true)?
        .context("queue response capture requires a live exact attempt")?;
    capture_response(file, &claim, response)
}

fn claim_before_release(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    enforce_generation: bool,
) -> Result<Option<QueueClaim>> {
    let mut ledger = crate::queue_claim::load_ledger(file)?;
    let claim = ledger.release(item);
    let Some(claim) = claim else {
        return Ok(None);
    };
    if enforce_generation && claim.generation != generation.unwrap_or_default() {
        anyhow::bail!(
            "stale queue claim generation {}; current generation is {}",
            generation.map_or_else(|| "<missing>".to_string(), |value| value.to_string()),
            claim.generation,
        );
    }
    Ok(Some(claim))
}

fn ensure_captured_response(file: &Path, claim: &QueueClaim) -> Result<()> {
    let receipt = load(file)?.receipts.into_iter().find(|receipt| {
        receipt.identity == claim.identity
            && receipt.generation == claim.generation
            && receipt.worker_id == claim.worker.as_ref().map(|worker| worker.worker_id.clone())
    });
    anyhow::ensure!(
        receipt.is_some_and(|receipt| {
            receipt.response.as_ref().is_some_and(
                agent_doc_queue::claim_release_receipt::CapturedQueueResponse::is_valid,
            )
        }),
        "refusing reported release for generation {}: no durable response receipt; use `agent-doc queue report-complete` first",
        claim.generation,
    );
    Ok(())
}

/// `agent-doc queue release`: end the claim on `item` and record (or, for
/// [`ReleaseKind::Requeue`], forget) its release receipt. A reported release is
/// deliberately three monotonic phases: mark the claim `Reported`, record the
/// receipt, then remove the claim. Every crash cut therefore leaves either
/// `A(q)` or `R(q)` true; the live row can never transiently regress to bare.
pub fn release(file: &Path, item: &str, kind: ReleaseKind) -> Result<Option<QueueClaim>> {
    match kind {
        ReleaseKind::Reported => {
            if let Some(claim) = claim_before_release(file, item, None, false)? {
                ensure_captured_response(file, &claim)?;
            }
            let prepared = crate::queue_claim::prepare_reported_release(file, item)?;
            apply_release_receipt(file, item, kind, prepared.clone())?;
            if prepared.is_some() {
                crate::queue_claim::release(file, item)?;
            }
            Ok(prepared)
        }
        ReleaseKind::Requeue => {
            let released = crate::queue_claim::abandon(file, item)?;
            apply_release_receipt(file, item, kind, released)
        }
    }
}

pub fn release_attempt(
    file: &Path,
    item: &str,
    kind: ReleaseKind,
    generation: Option<u64>,
) -> Result<Option<QueueClaim>> {
    match kind {
        ReleaseKind::Reported => {
            if let Some(claim) = claim_before_release(file, item, generation, true)? {
                ensure_captured_response(file, &claim)?;
            }
            let prepared =
                crate::queue_claim::prepare_reported_release_attempt(file, item, generation)?;
            apply_release_receipt(file, item, kind, prepared.clone())?;
            if prepared.is_some() {
                crate::queue_claim::release_attempt(file, item, generation)?;
            }
            Ok(prepared)
        }
        ReleaseKind::Requeue => {
            let released = crate::queue_claim::abandon_attempt(file, item, generation)?;
            if let Some(claim) = released.as_ref() {
                crate::goal_loop_handoff::record_quota_requeue(file, claim)?;
            }
            apply_release_receipt(file, item, kind, released)
        }
    }
}

fn apply_release_receipt(
    file: &Path,
    item: &str,
    kind: ReleaseKind,
    released: Option<QueueClaim>,
) -> Result<Option<QueueClaim>> {
    match kind {
        ReleaseKind::Reported => {
            if let Some(claim) = &released {
                let live = std::fs::read_to_string(file)
                    .ok()
                    .as_deref()
                    .and_then(live_queue_head_identities);
                let now = now_secs();
                let completion_key = crate::queue_claim::completion_feedback_key(claim)?;
                let refused = mutate(file, |ledger| {
                    ledger.prune(now, live.as_ref());
                    ledger
                        .record(claim, completion_key, now, DEFAULT_RELEASE_RECEIPT_TTL_SECS)
                        .err()
                })?;
                if let Some(refused) = refused {
                    return Err(refused.into());
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_claim_release_receipt_recorded owner={} item_bytes={} (#reportedrelease)",
                        claim.owner,
                        claim.item_text.trim().len()
                    ),
                );
            }
        }
        ReleaseKind::Requeue => {
            let forgot = mutate(file, |ledger| {
                let by_item = ledger.forget(item);
                let by_claim = released
                    .as_ref()
                    .is_some_and(|claim| ledger.forget(&claim.item_text));
                by_item || by_claim
            })?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_claim_release_requeued receipt_forgotten={forgot} item_bytes={} (#reportedrelease)",
                    item.trim().len()
                ),
            );
        }
    }
    Ok(released)
}

fn ledger_or_warn(file: &Path) -> ClaimReleaseReceiptLedger {
    match load(file) {
        Ok(mut ledger) => {
            // Cancellation is a terminal generation fence. If a crash lands
            // the cancellation audit before the old reported receipt is
            // physically retired, the receipt is already non-governing and
            // cannot attach to a same-text re-add.
            match crate::queue_claim::load_ledger(file) {
                Ok(claims) => ledger.receipts.retain(|receipt| {
                    !claims.cancellations.iter().any(|cancelled| {
                        cancelled.claim.identity == receipt.identity
                            && cancelled.claim.generation == receipt.generation
                    })
                }),
                Err(err) => eprintln!(
                    "[queue-claim] WARNING: could not read cancellation receipts for {}; reported receipts remain fail-closed work evidence: {err:#}",
                    file.display()
                ),
            }
            ledger
        }
        Err(err) => {
            eprintln!(
                "[queue-claim] WARNING: could not read release receipts for {}; treating every released line as unreported: {err:#}",
                file.display()
            );
            ClaimReleaseReceiptLedger::default()
        }
    }
}

/// The receipt governing the live queue line `text` of `content`, if its
/// worker reported and no live claim superseded it.
pub fn governing_receipt(file: &Path, content: &str, text: &str) -> Option<ClaimReleaseReceipt> {
    let ledger = ledger_or_warn(file);
    if ledger.receipts.is_empty() {
        return None;
    }
    let live = live_queue_head_identities(content);
    let claimed = crate::queue_claim::ledger_claimed_items_for_content(file, content);
    ledger
        .governing(now_secs(), live.as_ref(), &claimed, text)
        .cloned()
}

/// Lines of `content` whose worker reported: never offered to a new worker.
pub fn reported_items_for_content(file: &Path, content: &str) -> ClaimedQueueItems {
    let ledger = ledger_or_warn(file);
    if ledger.receipts.is_empty() {
        return ClaimedQueueItems::none();
    }
    let live = live_queue_head_identities(content);
    let claimed = crate::queue_claim::ledger_claimed_items_for_content(file, content);
    ledger
        .reported_items(now_secs(), live.as_ref(), &claimed)
        .with_user_prompt_backlog_same_work(content)
}

/// Lines whose durable release receipt proves the exact started claim
/// generation and worker. These rows may be closed by an exact response echo
/// even when they entered the queue after the turn baseline.
pub fn attested_reported_items_for_content(file: &Path, content: &str) -> ClaimedQueueItems {
    let ledger = ledger_or_warn(file);
    if ledger.receipts.is_empty() {
        return ClaimedQueueItems::none();
    }
    let live = live_queue_head_identities(content);
    let claimed = crate::queue_claim::ledger_claimed_items_for_content(file, content);
    ledger.attested_reported_items(now_secs(), live.as_ref(), &claimed)
}

/// Exact live queue-head texts backed by an exact-attempt release receipt.
pub fn attested_reported_head_texts_for_content(file: &Path, content: &str) -> Vec<String> {
    let attested = attested_reported_items_for_content(file, content);
    if attested.is_empty() {
        return Vec::new();
    }
    agent_doc_queue::queue_continuation::live_queue_head_texts(content)
        .unwrap_or_default()
        .into_iter()
        .filter(|head| attested.claims(head))
        .collect()
}

/// Exact live queue-head texts governed by durable report receipts.
pub fn reported_head_texts_for_content(file: &Path, content: &str) -> Vec<String> {
    let reported = reported_items_for_content(file, content);
    if reported.is_empty() {
        return Vec::new();
    }
    agent_doc_queue::queue_continuation::live_queue_head_texts(content)
        .unwrap_or_default()
        .into_iter()
        .filter(|head| reported.claims(head))
        .collect()
}

/// Released owner and exact attempt generation of every governing receipt on
/// a live line of `content`, by line identity (mid-turn steering's
/// `persist_reported` routing and the Stop hook's writeback reminder). The
/// generation fences the free-text strike those emitters prescribe
/// (`#closeoutguidanceagree`); zero marks a legacy unfenced receipt.
pub fn reported_owners_for_content(
    file: &Path,
    content: &str,
) -> std::collections::HashMap<agent_doc_element_queue::QueueItemIdentity, (String, u64)> {
    let ledger = ledger_or_warn(file);
    if ledger.receipts.is_empty() {
        return Default::default();
    }
    let live = live_queue_head_identities(content);
    let claimed = crate::queue_claim::ledger_claimed_items_for_content(file, content);
    let now = now_secs();
    ledger
        .receipts
        .iter()
        .filter(|receipt| {
            ledger
                .governing(now, live.as_ref(), &claimed, &receipt.item_text)
                .is_some_and(|governing| governing == *receipt)
        })
        .map(|receipt| {
            (
                receipt.identity.clone(),
                (receipt.owner.clone(), receipt.generation),
            )
        })
        .collect()
}

/// Durably drop receipts whose line closed (closeout reconciliation, beside
/// the claim prune). Time never erases work evidence from a live row.
pub fn prune_closed(file: &Path, content: &str) -> Result<usize> {
    if load(file)?.receipts.is_empty() {
        return Ok(0);
    }
    if terminal_projection_pending(file)? {
        agent_doc_ops_log_io::log_op(
            file,
            "queue_claim_release_receipt_prune_deferred reason=retained_projection_pending",
        );
        return Ok(0);
    }
    let live_rows = live_queue_head_identities(content).unwrap_or_default();
    let mut live = live_rows.clone();
    // A closed row is not enough to retire the last full response authority.
    // The queue receipt does not yet carry the repository commit + archive
    // index/path checksum proof needed to authorize GC, so full captures stay
    // fail-closed even after editor projection converges.
    let mut retained = std::collections::HashSet::new();
    for receipt in load(file)?.receipts {
        if receipt.response.is_some() {
            retained.insert(receipt.identity.clone());
            live.insert(receipt.identity);
        }
    }
    let now = now_secs();
    let (pruned, closed) = mutate(file, |ledger| {
        let pruned = ledger.prune(now, Some(&live));
        // `#todoeditrevivesrow`: a kept full capture whose row closed is
        // response evidence, not ownership. Left open (a pre-close strike, a
        // deferred or hand strike), it projected `☑️` onto the next row with
        // its identity, e.g. a todo edit that re-projected the waiting row.
        let closed = ledger.close_struck(&retained, Some(&live_rows), now);
        (pruned, closed)
    })?;
    if pruned > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!("queue_claim_release_receipt_prune pruned={pruned} (#reportedrelease)"),
        );
    }
    if closed > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_release_receipt_closed_on_prune closed={closed} response=retained (#todoeditrevivesrow)"
            ),
        );
    }
    Ok(pruned)
}

/// Durably drop the receipts of the rows a strike closed (`#strikeretires`),
/// at the strike's own mutation boundary rather than at the next closeout.
/// `after` is the struck successor: a struck identity that still has a live
/// row there keeps its receipt for that row. Call this only after the strike
/// successor was accepted, so a failed write never loses work evidence from a
/// still-live row.
pub fn retire_struck(
    file: &Path,
    struck_heads: &[String],
    after: &str,
    _projection_evidence: &str,
    source: &str,
) -> Result<usize> {
    if struck_heads.is_empty() || load(file)?.receipts.is_empty() {
        return Ok(0);
    }
    if terminal_projection_pending(file)? {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_release_receipt_retire_deferred source={source} reason=retained_projection_pending"
            ),
        );
        return Ok(0);
    }
    let mut struck = struck_heads
        .iter()
        .map(|head| agent_doc_queue::queue_claim::claim_identity(head))
        .collect::<std::collections::HashSet<_>>();
    // `#consumedclaimmismatch`: a full response capture is kept as recovery
    // authority, but the strike still closes its attempt. Without the close
    // mark the retained receipt read as live ownership forever, and deleting
    // the struck row reported `degraded_claim_mismatch` for integrated,
    // consumed work.
    let mut retained = std::collections::HashSet::new();
    for receipt in load(file)?.receipts {
        if receipt.response.is_some() && struck.remove(&receipt.identity) {
            retained.insert(receipt.identity);
        }
    }
    let live = agent_doc_queue::queue_consume::live_work_identities(after);
    let now = now_secs();
    let (retired, closed) = mutate(file, |ledger| {
        let retired = ledger.retire_struck(&struck, Some(&live));
        let closed = ledger.close_struck(&retained, Some(&live), now);
        (retired, closed)
    })?;
    if retired > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_release_receipt_retired_on_strike retired={retired} source={source} (#strikeretires)"
            ),
        );
    }
    if closed > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_release_receipt_closed_on_strike closed={closed} source={source} response=retained (#consumedclaimmismatch)"
            ),
        );
    }
    Ok(retired)
}

fn terminal_projection_pending(file: &Path) -> Result<bool> {
    Ok(
        agent_doc_cycle_state_io::load_document_projection(file)?.is_some_and(|projection| {
            projection.document.pending_write.is_some()
                || !projection.document.pending_write_journal.is_empty()
        }),
    )
}

/// End the struck rows' durable finished-work evidence (`#strikeretires`):
/// their `Reported` / terminal claim attempts
/// ([`crate::queue_claim::retire_struck_claims`]) and their reported-release
/// receipts ([`retire_struck`]). Every binary strike path calls this one step
/// after its successor is accepted, so neither can project `☑️` onto a later
/// row with the same text. A caller must not fail on it: a claim or receipt
/// the strike could not end is pruned by the next closeout, so the failure is
/// reported, never fatal.
pub fn retire_struck_or_warn(file: &Path, struck_heads: &[String], after: &str, source: &str) {
    if let Err(err) = crate::queue_claim::retire_struck_claims(file, struck_heads, after, source) {
        eprintln!(
            "[queue] WARNING: could not end the claims of struck rows in {} ({source}): {err:#}",
            file.display()
        );
    }
    if let Err(err) = retire_struck(file, struck_heads, after, after, source) {
        eprintln!(
            "[queue] WARNING: could not retire the release receipts of struck rows in {} ({source}): {err:#}",
            file.display()
        );
    }
}

/// Retire struck receipts after an accepted successor while proving that the
/// exact response was visible in the authority cut from which that successor
/// was derived. This covers atomic strikes that intentionally remove their
/// completion cell in the same mutation.
pub fn retire_struck_with_projection_or_warn(
    file: &Path,
    struck_heads: &[String],
    after: &str,
    projection_evidence: &str,
    source: &str,
) {
    if let Err(err) = crate::queue_claim::retire_struck_claims(file, struck_heads, after, source) {
        eprintln!(
            "[queue] WARNING: could not end the claims of struck rows in {} ({source}): {err:#}",
            file.display()
        );
    }
    if let Err(err) = retire_struck(file, struck_heads, after, projection_evidence, source) {
        eprintln!(
            "[queue] WARNING: could not retire the release receipts of struck rows in {} ({source}): {err:#}",
            file.display()
        );
    }
}

/// [`retire_struck_or_warn`] for a strike successor known only as the
/// accepted `before -> after` pair (the controller's answered-head
/// projection): the struck rows are [`heads_struck_by_successor`].
///
/// [`heads_struck_by_successor`]: agent_doc_queue::queue_consume::heads_struck_by_successor
pub fn retire_struck_by_successor_or_warn(file: &Path, before: &str, after: &str, source: &str) {
    let struck_heads = agent_doc_queue::queue_consume::heads_struck_by_successor(before, after);
    retire_struck_with_projection_or_warn(file, &struck_heads, after, before, source);
}

/// Durably drop every receipt whose work has no live row and no active claim
/// (`#stalecompletionrepair`): the one-time repair of receipts left behind by
/// strikes made before `#strikeretires` or by hand. Uses the strike path's own
/// ledger retirement. Call only after the document successor (if any) was
/// accepted. Returns the dropped receipts.
pub fn retire_without_live_work(
    file: &Path,
    kept: &agent_doc_queue::stale_completion_evidence::KeptWork,
    _converged_content: &str,
    source: &str,
) -> Result<Vec<ClaimReleaseReceipt>> {
    if load(file)?.receipts.is_empty() {
        return Ok(Vec::new());
    }
    let retired = mutate(file, |ledger| {
        let mut protected = kept.clone();
        for receipt in &ledger.receipts {
            if receipt.response.is_some() {
                protected.identities.insert(receipt.identity.clone());
            }
        }
        agent_doc_queue::stale_completion_evidence::retire_stale_receipts(ledger, &protected)
    })?;
    if !retired.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_release_receipt_retired_stale retired={} source={source} (#stalecompletionrepair)",
                retired.len()
            ),
        );
    }
    Ok(retired)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_doc(root: &Path, prompts: &[&str]) -> PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        let queue: String = prompts.iter().map(|p| format!("- {p}\n")).collect();
        std::fs::write(
            &doc,
            format!(
                "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Queue\n\n<!-- agent:queue go -->\n{queue}<!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        doc
    }

    /// `#todoeditrevivesrow` (agent-doc-bugs.md, 2026-11-05,
    /// `lazilysqlitestoreopen`): a reported, released attempt's row was struck
    /// at 02:15Z by a binary that kept the response-bearing receipt open, and
    /// `queue claims` was empty when the row reappeared as a live
    /// `☑️ [#subagent:lazilystore#1] do [#lazilysqlitestoreopen]` at 05:47Z:
    /// the open receipt still governed the `[#id]` identity. Closeout
    /// reconciliation now closes a kept capture whose row has closed, so a
    /// later row with the same id is fresh work, not the consumed attempt.
    #[test]
    fn todoeditrevivesrow_prune_closes_a_struck_attempts_retained_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let item = "do [#lazilysqlitestoreopen]";
        let owner = "subagent:lazilystore";
        let doc = write_doc(dir.path(), &[item, "other"]);
        let live = std::fs::read_to_string(&doc).unwrap();
        let (_, claim) = crate::queue_claim::claim(&doc, item, owner, 3600).unwrap();
        crate::queue_claim::start_attempt(&doc, item, owner, "worker:lazilystore", 3600, 1)
            .unwrap();
        capture_response_for_attempt(&doc, item, claim.generation, "SqliteStore::open landed")
            .unwrap();
        release(&doc, item, ReleaseKind::Reported).unwrap();
        assert!(governing_receipt(&doc, &live, item).is_some());

        // A strike that left the receipt open (pre-close binary, deferred or
        // hand strike): the reappearing row is still governed by it.
        let struck = live.replace(&format!("- {item}\n"), &format!("- ~~{item}~~\n"));
        let reappeared = live.replace(&format!("- {item}\n"), &format!("- ~~{item}~~\n- {item}\n"));
        assert!(
            governing_receipt(&doc, &reappeared, item).is_some(),
            "the legacy open receipt revives the re-added row"
        );
        let reported_rows = |current: &str| {
            crate::sync_payload::build_project_queue_sync_payload(&doc, current, None)
                .unwrap()
                .reported_checkmark
                .len()
        };
        assert_eq!(
            reported_rows(&reappeared),
            1,
            "preflight serves it as reported"
        );

        assert_eq!(
            prune_closed(&doc, &struck).unwrap(),
            0,
            "the capture is kept"
        );
        let receipts = load(&doc).unwrap().receipts;
        assert_eq!(receipts.len(), 1, "{receipts:?}");
        assert!(receipts[0].attempt_closed(), "{receipts:?}");
        assert!(receipts[0].response.is_some(), "{receipts:?}");
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("queue_claim_release_receipt_closed_on_prune closed=1"),
            "{ops_log}"
        );

        for current in [live.as_str(), reappeared.as_str()] {
            assert!(
                governing_receipt(&doc, current, item).is_none(),
                "{current}"
            );
            assert!(!reported_items_for_content(&doc, current).claims(item));
            assert!(!attested_reported_items_for_content(&doc, current).claims(item));
            assert!(reported_head_texts_for_content(&doc, current).is_empty());
            assert!(reported_owners_for_content(&doc, current).is_empty());
            assert_eq!(reported_rows(current), 0, "{current}");
            let projected =
                crate::queue_cmd::derive_claim_runtime_projection(&doc, current, None).unwrap();
            assert!(
                !projected.target_content.contains("☑️"),
                "the consumed attempt never re-projects:\n{}",
                projected.target_content
            );
        }
        assert_eq!(
            reported_receipt_attempt_for_parent_id(&doc, "lazilysqlitestoreopen").unwrap(),
            None
        );

        // A live row keeps an open receipt governing it: closure needs the
        // row to be gone, exactly like the plain-receipt prune.
        let (_, next) = crate::queue_claim::claim(&doc, item, owner, 3600).unwrap();
        assert!(
            next.generation > claim.generation,
            "lineage keeps the fence"
        );
        crate::queue_claim::start_attempt(
            &doc,
            item,
            owner,
            "worker:lazilystore-2",
            3600,
            next.generation,
        )
        .unwrap();
        capture_response_for_attempt(&doc, item, next.generation, "second attempt").unwrap();
        release(&doc, item, ReleaseKind::Reported).unwrap();
        assert_eq!(prune_closed(&doc, &live).unwrap(), 0);
        let receipt = governing_receipt(&doc, &live, item).expect("the new attempt governs");
        assert_eq!(receipt.generation, next.generation);
        assert!(!receipt.attempt_closed());
    }

    /// `#reportedrelease`: a reported release keeps a receipt through the
    /// real `state.db`; a re-claim supersedes it, a requeue forgets it, and a
    /// struck line prunes it.
    #[test]
    fn reported_release_records_a_receipt_that_requeue_and_closure_end() {
        let dir = tempfile::tempdir().unwrap();
        let question = "Does the page status enum have an archived value?";
        let doc = write_doc(dir.path(), &[question, "other"]);
        let content = std::fs::read_to_string(&doc).unwrap();

        // No claim: nothing released, nothing recorded.
        assert!(
            release(&doc, question, ReleaseKind::Reported)
                .unwrap()
                .is_none()
        );
        assert!(governing_receipt(&doc, &content, question).is_none());

        crate::queue_claim::claim(&doc, question, "subagent:page-status-enum", 3600).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            question,
            "subagent:page-status-enum",
            "worker:page-status-enum",
            3600,
            1,
        )
        .unwrap();

        // Every durable cut of the three-phase handoff carries work evidence:
        // A(Reported), then A(Reported)+R, then R. A crash at either boundary
        // therefore cannot make this live row look never-started.
        let prepared = crate::queue_claim::prepare_reported_release(&doc, question)
            .unwrap()
            .expect("claim prepared");
        let attempts = crate::queue_claim::claim_states_for_content(&doc, &content).unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
        assert!(governing_receipt(&doc, &content, question).is_none());

        capture_response(&doc, &prepared, "exact worker response").unwrap();

        let _ = apply_release_receipt(
            &doc,
            question,
            ReleaseKind::Reported,
            Some(prepared.clone()),
        )
        .unwrap();
        assert_eq!(
            crate::queue_claim::claim_states_for_content(&doc, &content)
                .unwrap()
                .len(),
            1
        );
        assert!(
            governing_receipt(&doc, &content, question).is_none(),
            "the live claim has precedence"
        );
        let released = crate::queue_claim::release(&doc, question)
            .unwrap()
            .expect("claim released after receipt");
        assert_eq!(released.owner, "subagent:page-status-enum");
        let receipt = governing_receipt(&doc, &content, question).expect("receipt governs");
        assert_eq!(receipt.owner, "subagent:page-status-enum");
        assert_eq!(receipt.generation, 1);
        assert_eq!(
            receipt.worker_id.as_deref(),
            Some("worker:page-status-enum")
        );
        assert_eq!(
            receipt.completion_key.as_deref(),
            Some(
                crate::queue_claim::completion_feedback_key(&prepared)
                    .unwrap()
                    .as_str()
            )
        );
        assert!(receipt.attests_exact_attempt());
        assert!(reported_items_for_content(&doc, &content).claims(question));
        assert!(attested_reported_items_for_content(&doc, &content).claims(question));
        assert!(!reported_items_for_content(&doc, &content).claims("other"));

        crate::queue_claim::claim(&doc, question, "subagent:again", 3600).unwrap();
        assert!(
            governing_receipt(&doc, &content, question).is_none(),
            "a live claim supersedes the receipt"
        );
        release(&doc, question, ReleaseKind::Requeue).unwrap();
        assert!(governing_receipt(&doc, &content, question).is_none());
        assert!(load(&doc).unwrap().receipts.is_empty(), "requeue forgot it");
        let attempts = crate::queue_claim::claim_states_for_content(&doc, &content).unwrap();
        assert!(
            attempts.is_empty(),
            "requeue removes a claim that never bound a replacement worker"
        );

        let (_, third) = crate::queue_claim::claim(&doc, question, "subagent:third", 3600).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            question,
            "subagent:third",
            "worker:third",
            3600,
            third.generation,
        )
        .unwrap();
        let capture =
            capture_response_for_attempt(&doc, question, third.generation, "third response")
                .unwrap();
        release(&doc, question, ReleaseKind::Reported).unwrap();
        let struck = content.replace(&format!("- {question}\n"), "");
        assert_eq!(prune_closed(&doc, &content).unwrap(), 0);
        assert_eq!(
            prune_closed(&doc, &struck).unwrap(),
            0,
            "row closure alone must retain the recovery tombstone"
        );
        let projected = format!(
            "{struck}\n{}\n",
            agent_doc_document::subagent_completion::cell_with_response(
                "subagent:third",
                question,
                capture.completion_key.as_deref().unwrap(),
                "third response",
            )
            .unwrap()
        );
        assert_eq!(
            prune_closed(&doc, &projected).unwrap(),
            0,
            "editor projection is necessary but not sufficient for response GC"
        );
        assert_eq!(
            load(&doc).unwrap().receipts.len(),
            1,
            "retain the full response until a future commit/archive receipt proves its durable location"
        );
    }

    #[test]
    fn cancellation_tombstone_fences_a_receipt_left_by_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let item = "finish sdk demo";
        let doc = write_doc(dir.path(), &[item]);
        let content = std::fs::read_to_string(&doc).unwrap();
        let (_, claim) = crate::queue_claim::claim(&doc, item, "subagent:sdk", 3600).unwrap();
        crate::queue_claim::start_attempt(
            &doc,
            item,
            "subagent:sdk",
            "worker:sdk",
            3600,
            claim.generation,
        )
        .unwrap();
        capture_response_for_attempt(&doc, item, claim.generation, "sdk response").unwrap();
        release_attempt(&doc, item, ReleaseKind::Reported, Some(claim.generation))
            .unwrap()
            .expect("reported claim");
        let stale = load(&doc).unwrap().receipts.remove(0);

        // Model the crash boundary after the cancellation audit commits but
        // before the stale receipt is physically removed.
        crate::queue_claim::record_released_cancellation(&doc, &stale).unwrap();
        assert_eq!(load(&doc).unwrap().receipts, vec![stale]);
        assert!(
            governing_receipt(&doc, &content, item).is_none(),
            "an exact cancellation tombstone makes the leftover receipt non-governing"
        );
        assert!(!reported_items_for_content(&doc, &content).claims(item));

        let (_, replacement) =
            crate::queue_claim::claim(&doc, item, "subagent:sdk-retry", 3600).unwrap();
        assert_eq!(replacement.generation, claim.generation + 1);
        assert!(governing_receipt(&doc, &content, item).is_none());
    }
}

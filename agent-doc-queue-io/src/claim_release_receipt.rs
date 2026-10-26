//! Durable reported-worker release receipts (`#reportedrelease`).
//!
//! Storage for [`agent_doc_queue::claim_release_receipt`]: one
//! `queue_document_state` row per document
//! (`state_kind = "queue_claim_release_receipts"`) in the project `state.db`,
//! beside (not inside) the claim ledger, so a claim mutation never rewrites a
//! receipt and the other way round.
//!
//! `queue release` records a receipt for the claim it ends; `queue release
//! --requeue` retains the attempt as `Lost` and forgets any receipt. Readers (mid-turn
//! steering, the subagent dispatch plan, the Codex Stop hook) are actorless
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
    /// Abandoned, failed, or requeued work: no receipt; the retained `Lost`
    /// attempt makes the line visibly reclaimable.
    Requeue,
}

/// `agent-doc queue release`: end the claim on `item` and record (or, for
/// [`ReleaseKind::Requeue`], forget) its release receipt. A reported release is
/// deliberately three monotonic phases: mark the claim `Reported`, record the
/// receipt, then remove the claim. Every crash cut therefore leaves either
/// `A(q)` or `R(q)` true; the live row can never transiently regress to bare.
pub fn release(file: &Path, item: &str, kind: ReleaseKind) -> Result<Option<QueueClaim>> {
    match kind {
        ReleaseKind::Reported => {
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
    ledger.reported_items(now_secs(), live.as_ref(), &claimed)
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

/// Released owner of every governing receipt on a live line of `content`, by
/// line identity (mid-turn steering's `persist_reported` routing).
pub fn reported_owners_for_content(
    file: &Path,
    content: &str,
) -> std::collections::HashMap<agent_doc_element_queue::QueueItemIdentity, String> {
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
        .map(|receipt| (receipt.identity.clone(), receipt.owner.clone()))
        .collect()
}

/// Durably drop receipts whose line closed (closeout reconciliation, beside
/// the claim prune). Time never erases work evidence from a live row.
pub fn prune_closed(file: &Path, content: &str) -> Result<usize> {
    if load(file)?.receipts.is_empty() {
        return Ok(0);
    }
    let live = live_queue_head_identities(content);
    let now = now_secs();
    let pruned = mutate(file, |ledger| ledger.prune(now, live.as_ref()))?;
    if pruned > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!("queue_claim_release_receipt_prune pruned={pruned} (#reportedrelease)"),
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
    source: &str,
) -> Result<usize> {
    if struck_heads.is_empty() || load(file)?.receipts.is_empty() {
        return Ok(0);
    }
    let struck = struck_heads
        .iter()
        .map(|head| agent_doc_queue::queue_claim::claim_identity(head))
        .collect::<std::collections::HashSet<_>>();
    let live = agent_doc_queue::queue_consume::live_work_identities(after);
    let retired = mutate(file, |ledger| ledger.retire_struck(&struck, Some(&live)))?;
    if retired > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_release_receipt_retired_on_strike retired={retired} source={source} (#strikeretires)"
            ),
        );
    }
    Ok(retired)
}

/// [`retire_struck`] for a caller that must not fail on it: a receipt the
/// strike could not retire is pruned by the next closeout, so the failure is
/// reported, never fatal.
pub fn retire_struck_or_warn(file: &Path, struck_heads: &[String], after: &str, source: &str) {
    if let Err(err) = retire_struck(file, struck_heads, after, source) {
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
    retire_struck_or_warn(file, &struck_heads, after, source);
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
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            attempts[0].state,
            agent_doc_queue::queue_claim::QueueClaimState::Lost,
            "requeue retains non-blocking work evidence"
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
        release(&doc, question, ReleaseKind::Reported).unwrap();
        let struck = content.replace(&format!("- {question}\n"), "");
        assert_eq!(prune_closed(&doc, &content).unwrap(), 0);
        assert_eq!(prune_closed(&doc, &struck).unwrap(), 1);
        assert!(load(&doc).unwrap().receipts.is_empty());
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

//! Pure policy for reported-worker release receipts (`#reportedrelease`).
//!
//! A queue head claimed for a background worker (`#queueclaim`) is released
//! with `agent-doc queue release` when that worker reports back. Release ends
//! the claim, but the item is still a live, unanswered queue line: its answer
//! exists only in the coordinator's context until a closeout persists it.
//!
//! Observed 2026-10-15 on haiven-dev `tasks/docs.md`. The coordinator released
//! `subagent:page-status-enum` when it reported, then ended the turn with the
//! answer only in chat. Every consumer then read the unclaimed line as fresh
//! work: the Codex Stop hook bounced the agent to "answer" it, the agent
//! persisted a `### Re:` answer without the `> **Queue prompt:**` echo (so the
//! line stayed queued), and mid-turn steering offered the same line as
//! `dispatch=subagent`. The coordinator dispatched a second worker for a
//! question it had already answered.
//!
//! A **release receipt** keeps the typed fact that release used to discard:
//! *this line's worker reported, and the coordinator owns persisting its
//! result.* Consumers read it to name the right next action (persist the
//! reported result now, with the echo that strikes the line) and never to
//! offer the line to a new worker. `queue release --requeue` (abandoned or
//! failed work) records no receipt, so the line is dispatchable again.
//!
//! A receipt is inert once its line leaves the queue, a live claim on the
//! line takes precedence over it, and explicit `--requeue` removes it. It does
//! not age a worked-on live row back to bare: `expires_at_secs` remains only as
//! rolling-upgrade metadata. This module is pure.

use std::collections::HashSet;

use agent_doc_element_queue::{QueueItemIdentity, QueueReportState};
use serde::{Deserialize, Serialize};

use crate::queue_claim::{ClaimedQueueItems, QueueClaim, claim_identity};

/// `queue_document_state.state_kind` of the receipt ledger.
pub const CLAIM_RELEASE_RECEIPTS_STATE_KIND: &str = "queue_claim_release_receipts";

/// Legacy expiry metadata retained for payload compatibility. A live receipt
/// is now monotonic work evidence and requires closure or explicit requeue.
pub const DEFAULT_RELEASE_RECEIPT_TTL_SECS: u64 = crate::queue_claim::DEFAULT_QUEUE_CLAIM_TTL_SECS;

/// One released claim whose worker reported back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimReleaseReceipt {
    pub identity: QueueItemIdentity,
    /// The released claim's item text, for diagnostics.
    pub item_text: String,
    /// The owner whose claim was released, e.g. `subagent:page-status-enum`.
    pub owner: String,
    /// Exact claim generation that produced the report. Zero means a legacy
    /// receipt that predates generation-fenced worker attestation.
    #[serde(default)]
    pub generation: u64,
    /// The completed attempt was first observed as Lost, then recovered from
    /// its exact worker completion. Keep both facts after claim release.
    #[serde(default)]
    pub completion_recovered_after_loss: bool,
    /// Coordinator harness session that spawned the exact worker. Retained so
    /// recovery can address harness-native subagent history after claim release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_session: Option<crate::queue_claim::ClaimOwnerSession>,
    /// Worker-reported local/published state retained through coordinator closeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_state: Option<QueueReportState>,
    /// Creation time of the exact claim attempt; part of its attestation key.
    #[serde(default)]
    pub claimed_at_secs: u64,
    /// Concrete worker bound by `queue start`. Missing only on a legacy
    /// receipt; such a receipt remains work evidence but is not exact strike
    /// authority.
    #[serde(default)]
    pub worker_id: Option<String>,
    /// Binary attestation for the exact identity/owner/generation/worker
    /// attempt. Missing only on a legacy receipt.
    #[serde(default)]
    pub completion_key: Option<String>,
    /// Full binary-attested worker response captured before the attempt may
    /// advance to `Reported`. This is the recovery authority when the editor
    /// projection is interrupted or overwritten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<CapturedQueueResponse>,
    pub released_at_secs: u64,
    pub expires_at_secs: u64,
    /// `#consumedclaimmismatch`: when a binary strike closed this attempt's
    /// row (`queue consume --ack-id` / `--done`, a closeout strike) while the
    /// receipt was retained only because it carries the full response
    /// capture. A closed receipt is response-recovery evidence, never
    /// ownership: deleting its struck row is plain cleanup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_closed_at_secs: Option<u64>,
}

/// Durable response receipt for one exact `(generation, worker)` attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedQueueResponse {
    /// Attempt-scoped response/backlink id (the completion attestation key).
    pub response_id: String,
    pub response_sha256: String,
    pub body: String,
}

impl CapturedQueueResponse {
    pub fn new(response_id: String, body: &str) -> Result<Self, ReceiptRecordRefused> {
        let body = body.trim();
        if body.is_empty() {
            return Err(ReceiptRecordRefused::EmptyResponseBody);
        }
        Ok(Self {
            response_id,
            response_sha256: agent_doc_hash::content_hash(body),
            body: body.to_string(),
        })
    }

    pub fn is_valid(&self) -> bool {
        !self.response_id.is_empty()
            && !self.body.trim().is_empty()
            && agent_doc_hash::content_hash(self.body.trim()) == self.response_sha256
    }
}

impl ClaimReleaseReceipt {
    pub fn is_expired(&self, now_secs: u64) -> bool {
        now_secs >= self.expires_at_secs
    }

    /// Whether a binary strike already closed this attempt's row
    /// (`#consumedclaimmismatch`). The receipt then survives only as the
    /// retained response capture and owns no queue work.
    pub fn attempt_closed(&self) -> bool {
        self.attempt_closed_at_secs.is_some()
    }

    /// Whether this receipt proves one exact started worker attempt. Legacy
    /// owner-only receipts deliberately fail closed here: they still prevent a
    /// worked row from becoming bare, but cannot authorize post-baseline
    /// closeout of a possibly unrelated generation.
    pub fn attests_exact_attempt(&self) -> bool {
        let (Some(worker_id), Some(completion_key)) =
            (self.worker_id.as_deref(), self.completion_key.as_deref())
        else {
            return false;
        };
        self.generation > 0
            && !worker_id.is_empty()
            && crate::queue_claim::completion_attestation_key(
                &self.identity,
                &self.owner,
                self.generation,
                self.claimed_at_secs,
                worker_id,
            )
            .is_ok_and(|expected| expected == completion_key)
    }

    /// Reconstitute the durable reported attempt represented by this outbox
    /// receipt. Queue sync uses this when the live claim has already been
    /// released but coordinator closeout is still pending.
    pub fn as_reported_claim(&self) -> QueueClaim {
        QueueClaim {
            identity: self.identity.clone(),
            item_text: self.item_text.clone(),
            owner: self.owner.clone(),
            generation: self.generation,
            state: crate::queue_claim::QueueClaimState::Reported,
            lost_from: None,
            completion_recovered_after_loss: self.completion_recovered_after_loss,
            publication_state: self.publication_state,
            worker: self.worker_id.as_ref().map(|worker_id| {
                crate::queue_claim::QueueWorkerBinding {
                    worker_id: worker_id.clone(),
                    started_at_secs: self.claimed_at_secs,
                }
            }),
            dispatch_policy: None,
            tracked_item_aliases: Vec::new(),
            claimed_at_secs: self.claimed_at_secs,
            expires_at_secs: self.released_at_secs,
            owner_session: self.owner_session.clone(),
            anchor: None,
            followed_from: Vec::new(),
            followed_generation: 0,
            claim_time_row: None,
            reported_at_secs: Some(self.released_at_secs),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptRecordRefused {
    NotReported,
    WorkerUnbound,
    EmptyCompletionKey,
    InvalidCompletionKey,
    EmptyResponseBody,
    MissingResponseCapture,
    StaleAttempt {
        captured_generation: u64,
        incoming_generation: u64,
    },
}

impl std::fmt::Display for ReceiptRecordRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotReported => write!(f, "claim is not in reported state"),
            Self::WorkerUnbound => write!(f, "reported claim has no bound worker"),
            Self::EmptyCompletionKey => write!(f, "completion attestation key is empty"),
            Self::InvalidCompletionKey => {
                write!(
                    f,
                    "completion attestation key does not match the claim attempt"
                )
            }
            Self::EmptyResponseBody => write!(f, "captured queue response body is empty"),
            Self::MissingResponseCapture => {
                write!(f, "reported release has no durable response capture")
            }
            Self::StaleAttempt {
                captured_generation,
                incoming_generation,
            } => write!(
                f,
                "stale queue response generation {incoming_generation}; durable generation is {captured_generation}"
            ),
        }
    }
}

impl std::error::Error for ReceiptRecordRefused {}

/// The document's receipts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimReleaseReceiptLedger {
    #[serde(default)]
    pub receipts: Vec<ClaimReleaseReceipt>,
}

impl ClaimReleaseReceiptLedger {
    fn validate_attempt(
        claim: &QueueClaim,
        completion_key: &str,
    ) -> Result<(), ReceiptRecordRefused> {
        if claim.state == crate::queue_claim::QueueClaimState::Claimed {
            return Err(ReceiptRecordRefused::NotReported);
        }
        let Some(worker) = claim.worker.as_ref() else {
            return Err(ReceiptRecordRefused::WorkerUnbound);
        };
        if completion_key.is_empty() {
            return Err(ReceiptRecordRefused::EmptyCompletionKey);
        }
        let expected = crate::queue_claim::completion_attestation_key(
            &claim.identity,
            &claim.owner,
            claim.generation,
            claim.claimed_at_secs,
            &worker.worker_id,
        )
        .map_err(|_| ReceiptRecordRefused::InvalidCompletionKey)?;
        if completion_key != expected {
            return Err(ReceiptRecordRefused::InvalidCompletionKey);
        }
        Ok(())
    }

    /// Capture the full worker response before lifecycle state may advance.
    /// A stale capture never replaces a newer generation's receipt.
    pub fn record_response_capture(
        &mut self,
        claim: &QueueClaim,
        completion_key: String,
        response_body: &str,
        now_secs: u64,
        ttl_secs: u64,
    ) -> Result<(), ReceiptRecordRefused> {
        Self::validate_attempt(claim, &completion_key)?;
        if let Some(existing) = self
            .receipts
            .iter()
            .find(|receipt| receipt.identity == claim.identity)
            && existing.generation > claim.generation
        {
            return Err(ReceiptRecordRefused::StaleAttempt {
                captured_generation: existing.generation,
                incoming_generation: claim.generation,
            });
        }
        let response = CapturedQueueResponse::new(completion_key.clone(), response_body)?;
        self.receipts
            .retain(|receipt| receipt.identity != claim.identity);
        self.receipts.push(ClaimReleaseReceipt {
            identity: claim.identity.clone(),
            item_text: claim.item_text.clone(),
            owner: claim.owner.clone(),
            generation: claim.generation,
            completion_recovered_after_loss: claim.completion_recovered_after_loss,
            owner_session: claim.owner_session.clone(),
            publication_state: claim.publication_state,
            claimed_at_secs: claim.claimed_at_secs,
            worker_id: claim.worker.as_ref().map(|worker| worker.worker_id.clone()),
            completion_key: Some(completion_key),
            response: Some(response),
            released_at_secs: 0,
            expires_at_secs: now_secs.saturating_add(ttl_secs),
            attempt_closed_at_secs: None,
        });
        Ok(())
    }

    /// Record that `claim`'s worker reported and its claim was released. A
    /// newer receipt for the same line replaces an older one.
    pub fn record(
        &mut self,
        claim: &QueueClaim,
        completion_key: String,
        now_secs: u64,
        ttl_secs: u64,
    ) -> Result<(), ReceiptRecordRefused> {
        if claim.state != crate::queue_claim::QueueClaimState::Reported {
            return Err(ReceiptRecordRefused::NotReported);
        }
        Self::validate_attempt(claim, &completion_key)?;
        let Some(existing) = self.receipts.iter().find(|receipt| {
            receipt.identity == claim.identity
                && receipt.generation == claim.generation
                && receipt.worker_id == claim.worker.as_ref().map(|worker| worker.worker_id.clone())
        }) else {
            return Err(ReceiptRecordRefused::MissingResponseCapture);
        };
        let Some(response) = existing.response.clone().filter(CapturedQueueResponse::is_valid) else {
            return Err(ReceiptRecordRefused::MissingResponseCapture);
        };
        let worker = claim.worker.as_ref().expect("validated worker");
        self.receipts
            .retain(|receipt| receipt.identity != claim.identity);
        self.receipts.push(ClaimReleaseReceipt {
            identity: claim.identity.clone(),
            item_text: claim.item_text.clone(),
            owner: claim.owner.clone(),
            generation: claim.generation,
            completion_recovered_after_loss: claim.completion_recovered_after_loss,
            owner_session: claim.owner_session.clone(),
            publication_state: claim.publication_state,
            claimed_at_secs: claim.claimed_at_secs,
            worker_id: Some(worker.worker_id.clone()),
            completion_key: Some(completion_key),
            response: Some(response),
            released_at_secs: now_secs,
            expires_at_secs: now_secs.saturating_add(ttl_secs),
            attempt_closed_at_secs: None,
        });
        Ok(())
    }

    /// Drop the receipt for `item` (`queue release --requeue`). Returns
    /// whether one existed.
    pub fn forget(&mut self, item: &str) -> bool {
        let identity = claim_identity(item);
        let before = self.receipts.len();
        self.receipts.retain(|receipt| receipt.identity != identity);
        before != self.receipts.len()
    }

    /// Drop receipts whose line is no longer a live queue head. `live` of
    /// `None` (no queue component) closes every line. `now_secs` is accepted
    /// for source compatibility but never erases work evidence from a live row.
    pub fn prune(&mut self, _now_secs: u64, live: Option<&HashSet<QueueItemIdentity>>) -> usize {
        let before = self.receipts.len();
        self.receipts
            .retain(|receipt| live.is_some_and(|live| live.contains(&receipt.identity)));
        before - self.receipts.len()
    }

    /// Drop the receipts of rows a strike closed (`#strikeretires`): every
    /// receipt whose identity is in `struck` and has no live row in `live`.
    /// A receipt for a still-live identity is work evidence for that row and
    /// stays. Returns how many were dropped.
    pub fn retire_struck(
        &mut self,
        struck: &HashSet<QueueItemIdentity>,
        live: Option<&HashSet<QueueItemIdentity>>,
    ) -> usize {
        let before = self.receipts.len();
        self.receipts.retain(|receipt| {
            !struck.contains(&receipt.identity)
                || live.is_some_and(|live| live.contains(&receipt.identity))
        });
        before - self.receipts.len()
    }

    /// Mark the retained receipts of rows a strike closed
    /// (`#consumedclaimmismatch`): every receipt [`Self::retire_struck`] keeps
    /// only because it carries the full response capture, whose identity is in
    /// `struck` and has no live row in `live`. The response stays; the
    /// attempt stops reading as owned work. Returns how many were marked.
    pub fn close_struck(
        &mut self,
        struck: &HashSet<QueueItemIdentity>,
        live: Option<&HashSet<QueueItemIdentity>>,
        now_secs: u64,
    ) -> usize {
        let mut closed = 0;
        for receipt in &mut self.receipts {
            if receipt.attempt_closed()
                || !struck.contains(&receipt.identity)
                || live.is_some_and(|live| live.contains(&receipt.identity))
            {
                continue;
            }
            receipt.attempt_closed_at_secs = Some(now_secs.max(1));
            closed += 1;
        }
        closed
    }

    /// Mark the receipt of exactly `owner`/`generation` closed
    /// (`#todoeditrevivesrow`): the strike that ended that attempt closes its
    /// receipt by attempt, not by whether some row with the same identity is
    /// still (or again) live. The response capture stays as recovery evidence.
    /// Returns how many were marked.
    pub fn close_exact_attempt(&mut self, owner: &str, generation: u64, now_secs: u64) -> usize {
        let mut closed = 0;
        for receipt in &mut self.receipts {
            if receipt.attempt_closed()
                || receipt.owner != owner
                || receipt.generation != generation
            {
                continue;
            }
            receipt.attempt_closed_at_secs = Some(now_secs.max(1));
            closed += 1;
        }
        closed
    }

    /// The receipt that governs the live line `text`: still queued and not
    /// superseded by a live claim on the same line.
    ///
    /// `#todoeditrevivesrow`: a receipt whose attempt a strike closed governs
    /// nothing. Identity is text-insensitive for an id-backed row, so a todo
    /// edit (or any re-projection) that puts a fresh row with the same
    /// `[#id]` back in the queue must not inherit the consumed attempt's `☑️`.
    pub fn governing(
        &self,
        _now_secs: u64,
        live: Option<&HashSet<QueueItemIdentity>>,
        claimed: &ClaimedQueueItems,
        text: &str,
    ) -> Option<&ClaimReleaseReceipt> {
        if self.receipts.is_empty() || claimed.claims(text) {
            return None;
        }
        let live = live?;
        let identity = claim_identity(text);
        if !live.contains(&identity) {
            return None;
        }
        self.receipts
            .iter()
            .find(|receipt| receipt.identity == identity && !receipt.attempt_closed())
    }

    /// Every governing receipt's line identity: the lines a new worker must
    /// not be offered.
    pub fn reported_items(
        &self,
        _now_secs: u64,
        live: Option<&HashSet<QueueItemIdentity>>,
        claimed: &ClaimedQueueItems,
    ) -> ClaimedQueueItems {
        let Some(live) = live else {
            return ClaimedQueueItems::none();
        };
        ClaimedQueueItems::from_identities(
            self.receipts
                .iter()
                .filter(|receipt| {
                    !receipt.attempt_closed()
                        && live.contains(&receipt.identity)
                        && !claimed.claims(&receipt.item_text)
                })
                .map(|receipt| receipt.identity.clone()),
        )
    }

    /// Every governing receipt that proves one exact started worker attempt.
    /// This subset may authorize an exact-echo closeout for a row introduced
    /// after the turn baseline; owner-only legacy receipts may not.
    pub fn attested_reported_items(
        &self,
        now_secs: u64,
        live: Option<&HashSet<QueueItemIdentity>>,
        claimed: &ClaimedQueueItems,
    ) -> ClaimedQueueItems {
        let Some(live) = live else {
            return ClaimedQueueItems::none();
        };
        ClaimedQueueItems::from_identities(
            self.receipts
                .iter()
                .filter(|receipt| {
                    receipt.attests_exact_attempt()
                        && live.contains(&receipt.identity)
                        && !claimed.claims(&receipt.item_text)
                        && self
                            .governing(now_secs, Some(live), claimed, &receipt.item_text)
                            .is_some_and(|governing| governing == *receipt)
                })
                .map(|receipt| receipt.identity.clone()),
        )
    }
}

/// How a persisted answer closes queue line `item_text` (`#reportedrelease`):
/// a free-text line strikes only on the verbatim `> **Queue prompt:**` echo.
/// A row that leads with a directive `[#id]` closes with `queue consume
/// --ack-id <id>` when the worker handoff is integrated but the tracked item
/// stays open, or with `--done <id>` when its tracked work is complete. A
/// heading-only answer leaves the line queued, so it is served (and
/// dispatched) again.
///
/// `#reportedwritebackprescription`: a waiting-tree parent (`[#id] ... <!--
/// agent-doc:waiting-item -->`) has a free-text claim identity, but its
/// reported `☑️` attempt is a nested child: echoing the parent line never
/// strikes that child, while `--ack-id <id>` does (`#nestedreceiptack`). The
/// prescription therefore keys on the row's directive id, not its identity
/// class, and no longer offers `--pending-gate` (a backlog-only legacy alias
/// that cannot strike a Todo-backed reported row).
pub fn queue_line_closeout(document: &str, item_text: &str) -> String {
    let item = item_text.trim();
    let id = match claim_identity(item) {
        QueueItemIdentity::Id(id) => Some(id),
        QueueItemIdentity::FreeText(_) => crate::queue_response::queue_prompt_done_id(item),
    };
    match id {
        Some(id) => format!(
            "strike the reported row with `agent-doc queue consume {document} --ack-id {id}` (the tracked backlog/Todo item stays open and the struck row names its `(stays open: ...)` reason; this also closes a `☑️` row nested under a waiting parent); when the answer resolves the tracked item, pass `--done {id}` to `agent-doc finalize {document}` instead and close any parent item the same answer resolves (#ackstrikeopenbacklog)"
        ),
        None => format!(
            "open the response with the verbatim echo `> **Queue prompt:** {item}` and persist it with `agent-doc finalize {document}`; a `### Re:` heading alone leaves the line queued"
        ),
    }
}

/// The persist-now instruction for a line whose worker reported
/// (`#reportedrelease`): the closeout that strikes the line is the only
/// completion.
pub fn persist_reported_instruction(document: &str, item_text: &str, owner: &str) -> String {
    format!(
        "{owner} already reported and its claim was released. Its result is durable in agent-doc's completion evidence (preflight surfaces it as `queue_sync.reported_checkmark` `recovered_response`), so a harness switch or `/clear` does not lose it, but the document has not recorded it and the queue line is still open. Persist that worker's result in THIS turn: {}. That closeout strikes the line. Do NOT dispatch a new subagent for it, do NOT re-claim it, and do NOT end the turn with the answer only in chat.",
        queue_line_closeout(document, item_text)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(text: &str, owner: &str) -> QueueClaim {
        QueueClaim {
            identity: claim_identity(text),
            item_text: text.to_string(),
            owner: owner.to_string(),
            generation: 1,
            state: crate::queue_claim::QueueClaimState::Reported,
            lost_from: None,
            completion_recovered_after_loss: false,
            publication_state: None,
            worker: Some(crate::queue_claim::QueueWorkerBinding {
                worker_id: format!("worker:{owner}"),
                started_at_secs: 1,
            }),
            dispatch_policy: None,
            tracked_item_aliases: Vec::new(),
            claimed_at_secs: 1,
            expires_at_secs: 100,
            owner_session: None,
            anchor: None,
            followed_from: Vec::new(),
            followed_generation: 0,
            claim_time_row: None,
            reported_at_secs: None,
        }
    }

    fn live(texts: &[&str]) -> HashSet<QueueItemIdentity> {
        texts.iter().map(|text| claim_identity(text)).collect()
    }

    fn record(
        ledger: &mut ClaimReleaseReceiptLedger,
        claim: &QueueClaim,
        now_secs: u64,
        ttl_secs: u64,
    ) {
        let key = crate::queue_claim::completion_attestation_key(
            &claim.identity,
            &claim.owner,
            claim.generation,
            claim.claimed_at_secs,
            claim.worker.as_ref().unwrap().worker_id.as_str(),
        )
        .unwrap();
        ledger
            .record_response_capture(claim, key.clone(), "worker response", now_secs, ttl_secs)
            .unwrap();
        ledger.record(claim, key, now_secs, ttl_secs).unwrap();
    }

    #[test]
    fn a_receipt_governs_its_live_unclaimed_line_until_it_closes() {
        let question = "Does the page status enum have an archived value? What are the values?";
        let mut ledger = ClaimReleaseReceiptLedger::default();
        record(
            &mut ledger,
            &claim(question, "subagent:page-status-enum"),
            10,
            50,
        );
        let queued = live(&[question, "other line"]);
        let none = ClaimedQueueItems::none();

        let receipt = ledger
            .governing(20, Some(&queued), &none, question)
            .expect("released, still-queued line is governed by its receipt");
        assert_eq!(receipt.owner, "subagent:page-status-enum");
        assert!(
            ledger
                .governing(20, Some(&queued), &none, "other line")
                .is_none()
        );
        assert!(
            ledger
                .reported_items(20, Some(&queued), &none)
                .claims(question),
            "a reported line is never offered to a new worker"
        );

        // A live claim supersedes the receipt (a deliberate re-dispatch).
        let reclaimed = ClaimedQueueItems::from_identities([claim_identity(question)]);
        assert!(
            ledger
                .governing(20, Some(&queued), &reclaimed, question)
                .is_none()
        );
        assert!(
            ledger
                .reported_items(20, Some(&queued), &reclaimed)
                .is_empty()
        );

        // Closed (struck/removed) lines are not governed. Time alone cannot
        // regress durable work evidence to bare pending state.
        let closed = live(&["other line"]);
        assert!(
            ledger
                .governing(20, Some(&closed), &none, question)
                .is_none()
        );
        assert!(
            ledger
                .governing(60, Some(&queued), &none, question)
                .is_some()
        );
        assert!(ledger.governing(20, None, &none, question).is_none());
    }

    /// `#todoeditrevivesrow`: a receipt whose attempt a strike closed governs
    /// no live row, even one with the same id-backed identity under edited
    /// text; closing is exact to the ended attempt.
    #[test]
    fn a_closed_attempt_governs_no_row_with_its_identity() {
        let consumed = "[#hookcreatedsubmodules] Keep or remove the settings files";
        let edited = "[#hookcreatedsubmodules] Approve removing the settings files";
        let mut ledger = ClaimReleaseReceiptLedger::default();
        record(&mut ledger, &claim(consumed, "subagent:hookcreated"), 10, 50);
        record(&mut ledger, &claim("other line", "subagent:other"), 10, 50);
        let queued = live(&[edited, "other line"]);
        let none = ClaimedQueueItems::none();
        assert_eq!(claim_identity(consumed), claim_identity(edited));
        assert!(ledger.governing(20, Some(&queued), &none, edited).is_some());

        assert_eq!(ledger.close_exact_attempt("subagent:hookcreated", 2, 30), 0);
        assert_eq!(ledger.close_exact_attempt("subagent:hookcreated", 1, 30), 1);
        assert_eq!(
            ledger.close_exact_attempt("subagent:hookcreated", 1, 31),
            0,
            "closing is idempotent"
        );
        assert!(ledger.governing(20, Some(&queued), &none, edited).is_none());
        assert!(!ledger.reported_items(20, Some(&queued), &none).claims(edited));
        assert!(
            !ledger
                .attested_reported_items(20, Some(&queued), &none)
                .claims(edited)
        );
        assert!(
            ledger
                .governing(20, Some(&queued), &none, "other line")
                .is_some(),
            "another attempt's receipt is untouched"
        );
        assert_eq!(ledger.receipts.len(), 2, "the response capture is kept");
    }

    #[test]
    fn receipt_preserves_recovered_loss_provenance_after_claim_release() {
        let mut recovered = claim("recovered work", "subagent:recovered");
        recovered.completion_recovered_after_loss = true;
        recovered.owner_session = Some(crate::queue_claim::ClaimOwnerSession {
            harness: Some("claude".to_string()),
            session_id: Some("session-1".to_string()),
            pane: None,
            pid: None,
        });
        let mut ledger = ClaimReleaseReceiptLedger::default();
        record(&mut ledger, &recovered, 10, 50);

        let receipt = &ledger.receipts[0];
        assert!(receipt.completion_recovered_after_loss);
        let projected = receipt.as_reported_claim();
        assert!(projected.completion_recovered_after_loss);
        assert_eq!(projected.owner_session, recovered.owner_session);
        assert_eq!(
            projected.projection_marker(),
            agent_doc_document::queue_projection::QueueLifecycleMarker::LostReported
        );
    }

    #[test]
    fn requeue_forgets_and_prune_drops_only_closed_receipts() {
        let mut ledger = ClaimReleaseReceiptLedger::default();
        record(&mut ledger, &claim("a line", "subagent:a"), 10, 50);
        record(&mut ledger, &claim("b line", "subagent:b"), 10, 5);
        record(&mut ledger, &claim("a line", "subagent:a2"), 11, 50);
        assert_eq!(
            ledger.receipts.len(),
            2,
            "one receipt per line, newest wins"
        );
        assert_eq!(ledger.receipts[1].owner, "subagent:a2");

        assert_eq!(ledger.prune(20, Some(&live(&["a line", "b line"]))), 0);
        assert_eq!(ledger.receipts.len(), 2, "live work evidence never expires");
        assert_eq!(
            ledger.prune(20, Some(&live(&["b line"]))),
            1,
            "only struck a dropped"
        );

        record(&mut ledger, &claim("c line", "subagent:c"), 10, 50);
        assert!(ledger.forget("c line"));
        assert!(!ledger.forget("c line"));
    }

    /// `#strikeretires`: a strike drops the struck identity's receipt unless
    /// a live row still carries that identity; other receipts are untouched.
    #[test]
    fn retire_struck_drops_only_struck_identities_without_a_live_row() {
        let mut ledger = ClaimReleaseReceiptLedger::default();
        record(&mut ledger, &claim("a line", "subagent:a"), 10, 50);
        record(&mut ledger, &claim("b line", "subagent:b"), 10, 50);
        record(&mut ledger, &claim("c line", "subagent:c"), 10, 50);

        let struck = live(&["a line", "b line"]);
        assert_eq!(ledger.retire_struck(&struck, Some(&live(&["b line"]))), 1);
        let kept = ledger
            .receipts
            .iter()
            .map(|receipt| receipt.item_text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(kept, ["b line", "c line"]);
    }

    #[test]
    fn persist_instruction_names_the_echo_for_free_text_and_done_for_ids() {
        let free =
            persist_reported_instruction("tasks/docs.md", "What are the values?", "subagent:x");
        assert!(
            free.contains("> **Queue prompt:** What are the values?"),
            "{free}"
        );
        assert!(free.contains("agent-doc finalize tasks/docs.md"), "{free}");
        assert!(free.contains("Do NOT dispatch a new subagent"), "{free}");
        let id = persist_reported_instruction("tasks/docs.md", "do [#gh275]", "subagent:y");
        assert!(id.contains("--done gh275"), "{id}");
        assert!(
            id.contains("agent-doc queue consume tasks/docs.md --ack-id gh275"),
            "{id}"
        );
        assert!(id.contains("stays open"), "{id}");
        assert!(!id.contains("Queue prompt"), "{id}");
        assert!(!id.contains("--pending-gate"), "{id}");
        assert!(!free.contains("only in your context"), "{free}");
        assert!(free.contains("recovered_response"), "{free}");
    }

    /// `#reportedwritebackprescription`: a waiting-tree parent has a free-text
    /// claim identity, yet the only closeout that strikes its nested `☑️`
    /// attempt is `--ack-id <parent id>`; the echo never does.
    #[test]
    fn waiting_parent_closeout_prescribes_ack_id_not_the_echo() {
        // The exact 2026-11-03 row; its claim identity is free text.
        let parent = "[#vscodecodersmoke] Run scripts/vscode-remote-smoke.py on a real Coder workspace (procedure in runbooks/coder-workspace.md): install, register, remote extension host, tmux attach, remoteName, layout sync; then update the VS Code on Coder status cell in editors/SPEC.md section 13. <!-- agent-doc:waiting-item -->";
        assert!(matches!(
            claim_identity(parent),
            QueueItemIdentity::FreeText(_)
        ));
        let closeout = queue_line_closeout("tasks/bugs.md", parent);
        assert!(
            closeout.contains("agent-doc queue consume tasks/bugs.md --ack-id vscodecodersmoke"),
            "{closeout}"
        );
        assert!(!closeout.contains("Queue prompt"), "{closeout}");
        assert!(!closeout.contains("--pending-gate"), "{closeout}");
    }

    /// Formal receipt-admission truth table. Only a Reported claim with one
    /// concrete worker binding and a non-empty exact-attempt key may become
    /// closeout authority. Every other row fails closed without mutating the
    /// receipt ledger.
    #[test]
    fn receipt_record_truth_table_requires_exact_started_report() {
        let cases = [
            (
                crate::queue_claim::QueueClaimState::Claimed,
                true,
                "key",
                Some(ReceiptRecordRefused::NotReported),
            ),
            (
                crate::queue_claim::QueueClaimState::Running,
                true,
                "key",
                Some(ReceiptRecordRefused::NotReported),
            ),
            (
                crate::queue_claim::QueueClaimState::Reported,
                false,
                "key",
                Some(ReceiptRecordRefused::WorkerUnbound),
            ),
            (
                crate::queue_claim::QueueClaimState::Reported,
                true,
                "",
                Some(ReceiptRecordRefused::EmptyCompletionKey),
            ),
            (
                crate::queue_claim::QueueClaimState::Reported,
                true,
                "invalid-key",
                Some(ReceiptRecordRefused::InvalidCompletionKey),
            ),
            (
                crate::queue_claim::QueueClaimState::Reported,
                true,
                "<exact>",
                None,
            ),
        ];

        for (index, (state, bound, key, expected)) in cases.into_iter().enumerate() {
            let mut candidate = claim(&format!("row {index}"), "subagent:worker");
            candidate.state = state;
            candidate.publication_state = Some(QueueReportState::PublishedLocal);
            if !bound {
                candidate.worker = None;
            }
            let mut ledger = ClaimReleaseReceiptLedger::default();
            let key = if key == "<exact>" {
                crate::queue_claim::completion_attestation_key(
                    &candidate.identity,
                    &candidate.owner,
                    candidate.generation,
                    candidate.claimed_at_secs,
                    &candidate.worker.as_ref().unwrap().worker_id,
                )
                .unwrap()
            } else {
                key.to_string()
            };
            let actual = ledger.record(&candidate, key, 10, 50).err();
            let actual = if expected.is_none() {
                let exact = crate::queue_claim::completion_attestation_key(
                    &candidate.identity,
                    &candidate.owner,
                    candidate.generation,
                    candidate.claimed_at_secs,
                    &candidate.worker.as_ref().unwrap().worker_id,
                )
                .unwrap();
                ledger
                    .record_response_capture(&candidate, exact.clone(), "response", 9, 50)
                    .unwrap();
                ledger.record(&candidate, exact, 10, 50).err()
            } else {
                actual
            };
            assert_eq!(actual, expected, "case {index}");
            assert_eq!(ledger.receipts.len(), usize::from(expected.is_none()));
            if expected.is_none() {
                assert!(ledger.receipts[0].attests_exact_attempt());
                assert_eq!(
                    ledger.receipts[0].publication_state,
                    Some(QueueReportState::PublishedLocal)
                );
            }
        }

        let legacy = ClaimReleaseReceipt {
            identity: claim_identity("legacy row"),
            item_text: "legacy row".to_string(),
            owner: "subagent:legacy".to_string(),
            generation: 0,
            completion_recovered_after_loss: false,
            owner_session: None,
            publication_state: None,
            claimed_at_secs: 1,
            worker_id: None,
            completion_key: None,
            response: None,
            released_at_secs: 1,
            expires_at_secs: 2,
            attempt_closed_at_secs: None,
        };
        assert!(
            !legacy.attests_exact_attempt(),
            "owner-only legacy evidence must never authorize a post-baseline strike"
        );
    }

    #[test]
    fn release_is_refused_until_full_response_capture_is_durable() {
        let candidate = claim("interrupt boundary", "subagent:worker");
        let key = crate::queue_claim::completion_attestation_key(
            &candidate.identity,
            &candidate.owner,
            candidate.generation,
            candidate.claimed_at_secs,
            &candidate.worker.as_ref().unwrap().worker_id,
        )
        .unwrap();
        let mut ledger = ClaimReleaseReceiptLedger::default();

        assert_eq!(
            ledger.record(&candidate, key.clone(), 10, 50),
            Err(ReceiptRecordRefused::MissingResponseCapture)
        );
        ledger
            .record_response_capture(&candidate, key.clone(), "complete body", 10, 50)
            .unwrap();
        let captured = &ledger.receipts[0];
        assert_eq!(captured.released_at_secs, 0, "capture is not release proof");
        assert_eq!(
            captured.response.as_ref().unwrap().body,
            "complete body"
        );
        assert!(ledger.record(&candidate, key, 11, 50).is_ok());
    }

    #[test]
    fn older_generation_cannot_overwrite_newer_durable_capture() {
        let mut newer = claim("same row", "subagent:new");
        newer.generation = 2;
        let newer_key = crate::queue_claim::completion_attestation_key(
            &newer.identity,
            &newer.owner,
            newer.generation,
            newer.claimed_at_secs,
            &newer.worker.as_ref().unwrap().worker_id,
        )
        .unwrap();
        let mut ledger = ClaimReleaseReceiptLedger::default();
        ledger
            .record_response_capture(&newer, newer_key, "new response", 20, 50)
            .unwrap();

        let older = claim("same row", "subagent:old");
        let older_key = crate::queue_claim::completion_attestation_key(
            &older.identity,
            &older.owner,
            older.generation,
            older.claimed_at_secs,
            &older.worker.as_ref().unwrap().worker_id,
        )
        .unwrap();
        assert_eq!(
            ledger.record_response_capture(&older, older_key, "stale response", 21, 50),
            Err(ReceiptRecordRefused::StaleAttempt {
                captured_generation: 2,
                incoming_generation: 1,
            })
        );
        assert_eq!(
            ledger.receipts[0].response.as_ref().unwrap().body,
            "new response"
        );
    }
}

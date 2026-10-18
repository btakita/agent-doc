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

use agent_doc_element_queue::QueueItemIdentity;
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
    pub released_at_secs: u64,
    pub expires_at_secs: u64,
}

impl ClaimReleaseReceipt {
    pub fn is_expired(&self, now_secs: u64) -> bool {
        now_secs >= self.expires_at_secs
    }
}

/// The document's receipts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimReleaseReceiptLedger {
    #[serde(default)]
    pub receipts: Vec<ClaimReleaseReceipt>,
}

impl ClaimReleaseReceiptLedger {
    /// Record that `claim`'s worker reported and its claim was released. A
    /// newer receipt for the same line replaces an older one.
    pub fn record(&mut self, claim: &QueueClaim, now_secs: u64, ttl_secs: u64) {
        self.receipts
            .retain(|receipt| receipt.identity != claim.identity);
        self.receipts.push(ClaimReleaseReceipt {
            identity: claim.identity.clone(),
            item_text: claim.item_text.clone(),
            owner: claim.owner.clone(),
            released_at_secs: now_secs,
            expires_at_secs: now_secs.saturating_add(ttl_secs),
        });
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

    /// The receipt that governs the live line `text`: still queued and not
    /// superseded by a live claim on the same line.
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
            .find(|receipt| receipt.identity == identity)
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
                    live.contains(&receipt.identity) && !claimed.claims(&receipt.item_text)
                })
                .map(|receipt| receipt.identity.clone()),
        )
    }
}

/// How a persisted answer closes queue line `item_text` (`#reportedrelease`):
/// a free-text line strikes only on the verbatim `> **Queue prompt:**` echo,
/// an id head only on `--done <id>`. A heading-only answer leaves the line
/// queued, so it is served (and dispatched) again.
pub fn queue_line_closeout(document: &str, item_text: &str) -> String {
    let item = item_text.trim();
    match claim_identity(item) {
        QueueItemIdentity::Id(id) => format!(
            "pass `--done {id}` to `agent-doc finalize {document}` (or `--pending-gate {id}` when it is gated)"
        ),
        QueueItemIdentity::FreeText(_) => format!(
            "open the response with the verbatim echo `> **Queue prompt:** {item}` and persist it with `agent-doc finalize {document}`; a `### Re:` heading alone leaves the line queued"
        ),
    }
}

/// The persist-now instruction for a line whose worker reported
/// (`#reportedrelease`): the closeout that strikes the line is the only
/// completion.
pub fn persist_reported_instruction(document: &str, item_text: &str, owner: &str) -> String {
    format!(
        "{owner} already reported and its claim was released, so this queue line is answered only in your context, not in the document. Persist that worker's result in THIS turn: {}. That closeout strikes the line. Do NOT dispatch a new subagent for it, do NOT re-claim it, and do NOT end the turn with the answer only in chat.",
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
            claimed_at_secs: 1,
            expires_at_secs: 100,
            owner_session: None,
            anchor: None,
            followed_from: Vec::new(),
        }
    }

    fn live(texts: &[&str]) -> HashSet<QueueItemIdentity> {
        texts.iter().map(|text| claim_identity(text)).collect()
    }

    #[test]
    fn a_receipt_governs_its_live_unclaimed_line_until_it_closes() {
        let question = "Does the page status enum have an archived value? What are the values?";
        let mut ledger = ClaimReleaseReceiptLedger::default();
        ledger.record(&claim(question, "subagent:page-status-enum"), 10, 50);
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

    #[test]
    fn requeue_forgets_and_prune_drops_only_closed_receipts() {
        let mut ledger = ClaimReleaseReceiptLedger::default();
        ledger.record(&claim("a line", "subagent:a"), 10, 50);
        ledger.record(&claim("b line", "subagent:b"), 10, 5);
        ledger.record(&claim("a line", "subagent:a2"), 11, 50);
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

        ledger.record(&claim("c line", "subagent:c"), 10, 50);
        assert!(ledger.forget("c line"));
        assert!(!ledger.forget("c line"));
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
        assert!(!id.contains("Queue prompt"), "{id}");
    }
}

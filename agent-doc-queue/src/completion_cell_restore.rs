//! Restore binary completion cells that a closeout deleted while their
//! attempts were still open (`#completioncellrestore`).
//!
//! On 2026-11-05 `queue report-complete` accepted about 20 subagent reports and
//! each cell was projected and settled; the coordinator's next closeouts then
//! deleted every cell not yet in HEAD (`#completioncellslost`, fixed in the
//! response-supersession path). The rows kept their reported markers, because
//! those are re-derived from the claim ledger and the reported-release
//! receipts, but the cells are not re-derived from anything, so the worker
//! responses vanished from `agent:exchange`.
//!
//! Every such response is still durable: `report-complete` captures the full
//! worker response in the attempt's receipt
//! ([`ClaimReleaseReceipt::response`]) BEFORE it writes the cell. The cell is a
//! pure function of `(owner, item, key, response)`
//! ([`cell_with_response`]), so it can be rebuilt byte for byte.
//!
//! This module is the pure decision over one document cut, its claim ledger,
//! and its receipts. An attempt whose receipt carries a valid response and
//! exactly attests one started worker attempt, and whose key no cell in the
//! document carries (attested or edited), gets its cell back when no newer
//! attempt of the same work owns the claim ledger and either:
//! - its attempt is open and its work is still kept ([`kept_work`]): a live
//!   row in some queue or an active claim; or
//! - its attempt is closed and the caller names its key. Receipts cannot tell
//!   the cells this repair exists for from correctly retired ones: a strike
//!   closes the attempt either way, and an attempt whose cell a closeout had
//!   already deleted was struck later like any other. Only history can, so
//!   closed attempts are listed and restored only when named, the rule
//!   `#stalecompletionrepair` applies to its own undecidable cells. A closed
//!   attempt with a live row of the same work stays out even when named: its
//!   cell would project `🔄` onto that row.
//!
//! A cell restored for a closed attempt has no live row, so
//! `queue repair-stale-completion` retires it again. A cell restored by
//! default never conflicts with that repair.

use std::collections::HashSet;

use agent_doc_document::subagent_completion::{cell_with_response, completion_evidence, marker};

use crate::claim_release_receipt::{ClaimReleaseReceipt, ClaimReleaseReceiptLedger};
use crate::queue_claim::{QueueClaim, QueueClaimLedger, claim_identity};
use crate::stale_completion_evidence::{StaleEvidenceRefusal, kept_work};

/// One completion cell the restore adds back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredCompletionCell {
    pub owner: String,
    /// The queue item text the cell's heading records.
    pub item: String,
    pub generation: u64,
    /// The attempt's completion attestation key (the cell's marker key).
    pub key: String,
    /// The complete cell text, as `report-complete` wrote it (no trailing
    /// newline).
    pub cell: String,
}

/// Why an attempt with a saved response gets no restored cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreSkipReason {
    /// A binary strike closed the attempt, and its key was not named. Its cell
    /// may have been retired by that strike or deleted before it.
    AttemptClosed { closed_at_secs: u64 },
    /// A named closed attempt whose work has a live row: its cell would
    /// project `🔄` onto that row.
    ClosedAttemptLiveRow,
    /// The receipt has no captured response, or its digest does not match.
    NoValidResponse,
    /// A legacy receipt without an exact worker attempt attestation.
    NotExactAttempt,
    /// A newer attempt of the same work owns the claim ledger.
    Superseded { current_generation: u64 },
    /// The work has no live row in any queue and no active claim: a restored
    /// cell would be stale evidence.
    NoLiveWork,
    /// The document carries the key's marker, but not as an attested cell
    /// (the cell was edited); the operator's text is left alone.
    EditedCellPresent,
}

impl std::fmt::Display for RestoreSkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AttemptClosed { closed_at_secs } => write!(
                f,
                "a strike closed this attempt (closed_at_secs={closed_at_secs}); restore it only with --key"
            ),
            Self::ClosedAttemptLiveRow => write!(
                f,
                "this closed attempt's work has a live row, which its cell would mark as reconciling"
            ),
            Self::NoValidResponse => write!(f, "no valid captured worker response"),
            Self::NotExactAttempt => {
                write!(f, "legacy receipt without an exact worker attestation")
            }
            Self::Superseded { current_generation } => write!(
                f,
                "superseded by the claim ledger's generation {current_generation}"
            ),
            Self::NoLiveWork => write!(
                f,
                "its work has no live queue row and no active claim (the cell would be stale)"
            ),
            Self::EditedCellPresent => write!(
                f,
                "the document carries this key's marker in an edited (unattested) cell"
            ),
        }
    }
}

/// An attempt with a saved response that the restore leaves alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedRestore {
    pub owner: String,
    pub item: String,
    pub generation: u64,
    pub key: Option<String>,
    pub reason: RestoreSkipReason,
}

/// What a restore would add to one document cut.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionCellRestorePlan {
    /// Cells to add, oldest attempt first.
    pub restored: Vec<RestoredCompletionCell>,
    /// Attempts whose attested cell is already in the document.
    pub present: usize,
    /// Attempts with a saved response that get no cell, and why.
    pub skipped: Vec<SkippedRestore>,
    /// Named keys no receipt carries (a typo, or a pruned receipt).
    pub missing_keys: Vec<String>,
}

impl CompletionCellRestorePlan {
    pub fn is_empty(&self) -> bool {
        self.restored.is_empty()
    }
}

/// The claim-ledger attempt that is exactly `receipt`'s attempt.
fn exact_claim<'a>(
    claims: &'a QueueClaimLedger,
    receipt: &ClaimReleaseReceipt,
) -> Option<&'a QueueClaim> {
    claims.claims.iter().find(|claim| {
        claim.identity == receipt.identity
            && claim.generation == receipt.generation
            && claim
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str())
                == receipt.worker_id.as_deref()
    })
}

/// A newer claim attempt of `receipt`'s work, if the ledger holds one.
fn newer_claim<'a>(
    claims: &'a QueueClaimLedger,
    receipt: &ClaimReleaseReceipt,
) -> Option<&'a QueueClaim> {
    claims
        .claims
        .iter()
        .find(|claim| claim.identity == receipt.identity && claim.generation > receipt.generation)
}

/// Plan the completion cells to restore into `content`, including the closed
/// attempts named in `named_keys`. Refuses when the document's queues cannot be
/// read, because a missed live row would misjudge an attempt's work.
pub fn plan_completion_cell_restore(
    content: &str,
    claims: &QueueClaimLedger,
    receipts: &ClaimReleaseReceiptLedger,
    named_keys: &HashSet<String>,
) -> Result<CompletionCellRestorePlan, StaleEvidenceRefusal> {
    let kept = kept_work(content, claims)?;
    let present_keys = completion_evidence(content)
        .into_iter()
        .map(|evidence| evidence.key)
        .collect::<HashSet<_>>();
    let mut plan = CompletionCellRestorePlan::default();
    let mut ordered = receipts.receipts.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| {
        (a.claimed_at_secs, a.generation, &a.item_text).cmp(&(
            b.claimed_at_secs,
            b.generation,
            &b.item_text,
        ))
    });
    for receipt in ordered {
        let Some(response) = receipt.response.as_ref() else {
            // No saved response: nothing to rebuild a cell from.
            continue;
        };
        let live_claim = exact_claim(claims, receipt);
        let owner = live_claim.map_or(receipt.owner.as_str(), |claim| claim.owner.as_str());
        let item = live_claim.map_or(receipt.item_text.as_str(), |claim| claim.item_text.as_str());
        let skip = |reason| SkippedRestore {
            owner: owner.to_string(),
            item: item.to_string(),
            generation: receipt.generation,
            key: receipt.completion_key.clone(),
            reason,
        };
        if !response.is_valid() {
            plan.skipped.push(skip(RestoreSkipReason::NoValidResponse));
            continue;
        }
        let Some(key) = receipt
            .completion_key
            .as_deref()
            .filter(|_| receipt.attests_exact_attempt())
        else {
            plan.skipped.push(skip(RestoreSkipReason::NotExactAttempt));
            continue;
        };
        if present_keys.contains(key) {
            plan.present += 1;
            continue;
        }
        if content.contains(&marker(key)) {
            plan.skipped
                .push(skip(RestoreSkipReason::EditedCellPresent));
            continue;
        }
        if let Some(newer) = newer_claim(claims, receipt) {
            plan.skipped.push(skip(RestoreSkipReason::Superseded {
                current_generation: newer.generation,
            }));
            continue;
        }
        let Ok(cell) = cell_with_response(owner, item, key, &response.body) else {
            plan.skipped.push(skip(RestoreSkipReason::NoValidResponse));
            continue;
        };
        let Some(evidence) = completion_evidence(&cell).into_iter().next() else {
            plan.skipped.push(skip(RestoreSkipReason::NoValidResponse));
            continue;
        };
        // The identity `repair-stale-completion` would judge this cell by.
        let identity = claim_identity(&evidence.item);
        if let Some(closed_at_secs) = receipt.attempt_closed_at_secs {
            if !named_keys.contains(key) {
                plan.skipped
                    .push(skip(RestoreSkipReason::AttemptClosed { closed_at_secs }));
                continue;
            }
            if kept.has_live_row(&identity) {
                plan.skipped
                    .push(skip(RestoreSkipReason::ClosedAttemptLiveRow));
                continue;
            }
        } else if !kept.keeps(&identity) {
            plan.skipped.push(skip(RestoreSkipReason::NoLiveWork));
            continue;
        }
        plan.restored.push(RestoredCompletionCell {
            owner: owner.to_string(),
            item: item.to_string(),
            generation: receipt.generation,
            key: key.to_string(),
            cell,
        });
    }
    let known = receipts
        .receipts
        .iter()
        .filter_map(|receipt| receipt.completion_key.as_deref())
        .collect::<HashSet<_>>();
    plan.missing_keys = named_keys
        .iter()
        .filter(|key| !known.contains(key.as_str()))
        .cloned()
        .collect();
    plan.missing_keys.sort();
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim_release_receipt::CapturedQueueResponse;
    use crate::queue_claim::{QueueClaimState, completion_attestation_key};
    use crate::stale_completion_evidence::plan_stale_completion_cells;

    fn doc(exchange: &str, queue: &str) -> String {
        format!(
            concat!(
                "---\nagent_doc_session: restore\n---\n\n",
                "<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n{queue}<!-- /agent:queue -->\n",
            ),
            exchange = exchange,
            queue = queue,
        )
    }

    fn attempt(item: &str, owner: &str, generation: u64, claimed_at: u64) -> (QueueClaim, String) {
        let identity = claim_identity(item);
        let worker = format!("worker:{owner}");
        let key =
            completion_attestation_key(&identity, owner, generation, claimed_at, &worker).unwrap();
        let base = ClaimReleaseReceipt {
            identity,
            item_text: item.to_string(),
            owner: owner.to_string(),
            generation,
            completion_recovered_after_loss: false,
            owner_session: None,
            publication_state: None,
            claimed_at_secs: claimed_at,
            worker_id: Some(worker),
            completion_key: None,
            response: None,
            released_at_secs: claimed_at,
            expires_at_secs: u64::MAX,
            attempt_closed_at_secs: None,
        };
        (base.as_reported_claim(), key)
    }

    fn receipt(claim: &QueueClaim, key: &str, body: &str) -> ClaimReleaseReceipt {
        ClaimReleaseReceipt {
            identity: claim.identity.clone(),
            item_text: claim.item_text.clone(),
            owner: claim.owner.clone(),
            generation: claim.generation,
            completion_recovered_after_loss: false,
            owner_session: None,
            publication_state: None,
            claimed_at_secs: claim.claimed_at_secs,
            worker_id: claim.worker.as_ref().map(|w| w.worker_id.clone()),
            completion_key: Some(key.to_string()),
            response: Some(CapturedQueueResponse::new(key.to_string(), body).unwrap()),
            released_at_secs: 0,
            expires_at_secs: u64::MAX,
            attempt_closed_at_secs: None,
        }
    }

    /// The 2026-11-05 shape: reported attempts (one still claimed, one
    /// released) whose cells a closeout deleted. Both come back byte for
    /// byte, in attempt order, and the stale-completion repair keeps them.
    #[test]
    fn restores_deleted_cells_of_open_reported_attempts_byte_for_byte() {
        let (claimed, claimed_key) = attempt("do [#alpha]", "subagent:alpha", 1, 100);
        let (released, released_key) = attempt("do [#beta]", "subagent:beta", 2, 50);
        let (kept_attempt, kept_key) = attempt("do [#gamma]", "subagent:gamma", 1, 10);
        let kept_cell =
            cell_with_response("subagent:gamma", "do [#gamma]", &kept_key, "Gamma done.").unwrap();
        let content = doc(
            &format!("### Re: prior — gpt-5\n\nDone.\n\n{kept_cell}\n"),
            "- ☑️ do [#alpha]\n- ☑️ do [#beta]\n- ☑️ do [#gamma]\n",
        );
        let claims = QueueClaimLedger {
            claims: vec![claimed.clone(), kept_attempt.clone()],
            ..QueueClaimLedger::default()
        };
        let receipts = ClaimReleaseReceiptLedger {
            receipts: vec![
                receipt(&claimed, &claimed_key, "Alpha done.\n\n- tests pass"),
                receipt(&released, &released_key, "Beta done."),
                receipt(&kept_attempt, &kept_key, "Gamma done."),
            ],
        };

        let plan =
            plan_completion_cell_restore(&content, &claims, &receipts, &HashSet::new()).unwrap();
        assert_eq!(plan.present, 1, "{plan:#?}");
        assert!(plan.skipped.is_empty(), "{plan:#?}");
        let keys = plan
            .restored
            .iter()
            .map(|cell| cell.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(keys, [released_key.as_str(), claimed_key.as_str()]);
        assert_eq!(
            plan.restored[1].cell,
            cell_with_response(
                "subagent:alpha",
                "do [#alpha]",
                &claimed_key,
                "Alpha done.\n\n- tests pass"
            )
            .unwrap()
        );

        // The restored cells are kept, tracked live-row evidence for the
        // stale-completion repair: the two repairs never undo each other.
        let mut restored = content.clone();
        for cell in &plan.restored {
            restored = restored.replace(
                "<!-- /agent:exchange -->",
                &format!("\n{}\n<!-- /agent:exchange -->", cell.cell),
            );
        }
        let tracked = [claimed_key.clone(), released_key.clone(), kept_key.clone()]
            .into_iter()
            .collect::<HashSet<_>>();
        let kept = kept_work(&restored, &claims).unwrap();
        let stale =
            plan_stale_completion_cells(&restored, &kept, &tracked, &HashSet::new()).unwrap();
        assert!(stale.is_empty(), "{stale:#?}");
        assert_eq!(stale.kept_live_cells + stale.kept_claimed_cells, 3);

        // Idempotent: planning over the restored cut finds nothing to add.
        let again =
            plan_completion_cell_restore(&restored, &claims, &receipts, &HashSet::new()).unwrap();
        assert!(again.is_empty(), "{again:#?}");
        assert_eq!(again.present, 3);
    }

    #[test]
    fn never_restores_closed_superseded_struck_edited_or_legacy_attempts() {
        let (closed, closed_key) = attempt("do [#closed]", "subagent:closed", 1, 1);
        let (old, old_key) = attempt("do [#redo]", "subagent:redo", 1, 2);
        let (newer, _) = attempt("do [#redo]", "subagent:redo", 2, 3);
        let (struck, struck_key) = attempt("do [#struck]", "subagent:struck", 1, 4);
        let (edited, edited_key) = attempt("do [#edited]", "subagent:edited", 1, 5);
        let (legacy, legacy_key) = attempt("do [#legacy]", "subagent:legacy", 1, 6);
        let edited_cell =
            cell_with_response("subagent:edited", "do [#edited]", &edited_key, "Edited.")
                .unwrap()
                .replace("> Edited.", "> Operator changed this.");
        let content = doc(
            &format!("{edited_cell}\n"),
            "- ☑️ do [#closed]\n- 🚧 do [#redo]\n- ~~do [#struck]~~\n- ☑️ do [#edited]\n- ☑️ do [#legacy]\n",
        );
        let claims = QueueClaimLedger {
            claims: vec![QueueClaim {
                state: QueueClaimState::Running,
                ..newer
            }],
            ..QueueClaimLedger::default()
        };
        let mut closed_receipt = receipt(&closed, &closed_key, "Closed.");
        closed_receipt.attempt_closed_at_secs = Some(9);
        let mut legacy_receipt = receipt(&legacy, &legacy_key, "Legacy.");
        legacy_receipt.worker_id = None;
        let mut tampered = receipt(&struck, &struck_key, "Struck.");
        let receipts = ClaimReleaseReceiptLedger {
            receipts: vec![
                closed_receipt,
                receipt(&old, &old_key, "Old attempt."),
                tampered.clone(),
                receipt(&edited, &edited_key, "Edited."),
                legacy_receipt,
            ],
        };
        let plan =
            plan_completion_cell_restore(&content, &claims, &receipts, &HashSet::new()).unwrap();
        assert!(plan.restored.is_empty(), "{plan:#?}");
        let reasons = plan
            .skipped
            .iter()
            .map(|skip| (skip.item.as_str(), skip.reason.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            reasons,
            [
                (
                    "do [#closed]",
                    RestoreSkipReason::AttemptClosed { closed_at_secs: 9 }
                ),
                (
                    "do [#redo]",
                    RestoreSkipReason::Superseded {
                        current_generation: 2
                    }
                ),
                ("do [#struck]", RestoreSkipReason::NoLiveWork),
                ("do [#edited]", RestoreSkipReason::EditedCellPresent),
                ("do [#legacy]", RestoreSkipReason::NotExactAttempt),
            ]
        );

        // A response whose digest no longer matches is not restore authority.
        tampered.response.as_mut().unwrap().body = "Forged.".to_string();
        let live = content.replace("- ~~do [#struck]~~\n", "- ☑️ do [#struck]\n");
        let plan = plan_completion_cell_restore(
            &live,
            &QueueClaimLedger::default(),
            &ClaimReleaseReceiptLedger {
                receipts: vec![tampered],
            },
            &HashSet::new(),
        )
        .unwrap();
        assert!(plan.restored.is_empty());
        assert_eq!(plan.skipped[0].reason, RestoreSkipReason::NoValidResponse);
    }

    /// The cells this repair exists for: a closeout deleted the cell, then a
    /// strike closed the attempt, so the receipt alone cannot tell it from a
    /// correctly retired cell. It is listed, restored only when named, and a
    /// named attempt whose work has a live row stays out.
    #[test]
    fn a_closed_attempt_is_restored_only_when_its_key_is_named() {
        let (lost, lost_key) = attempt("do [#lost]", "subagent:lost", 1, 1);
        let (retired, retired_key) = attempt("do [#retired]", "subagent:retired", 1, 2);
        let (requeued, requeued_key) = attempt("do [#requeued]", "subagent:requeued", 1, 3);
        let (carried, carried_key) = attempt("do [#carried]", "subagent:carried", 1, 4);
        let carried_cell =
            cell_with_response("subagent:carried", "do [#carried]", &carried_key, "Kept.").unwrap();
        let content = doc(
            &format!("{carried_cell}\n"),
            "- ~~do [#lost]~~\n- ~~do [#retired]~~\n- 📥 do [#requeued]\n- ~~do [#carried]~~\n",
        );
        let closed = |claim: &QueueClaim, key: &str, body: &str| ClaimReleaseReceipt {
            attempt_closed_at_secs: Some(9),
            ..receipt(claim, key, body)
        };
        let receipts = ClaimReleaseReceiptLedger {
            receipts: vec![
                closed(&lost, &lost_key, "Lost."),
                closed(&retired, &retired_key, "Retired."),
                closed(&requeued, &requeued_key, "Requeued."),
                closed(&carried, &carried_key, "Kept."),
            ],
        };
        let claims = QueueClaimLedger::default();

        let plan =
            plan_completion_cell_restore(&content, &claims, &receipts, &HashSet::new()).unwrap();
        assert!(plan.restored.is_empty(), "{plan:#?}");
        assert_eq!(
            plan.present, 1,
            "a carried closed cell is present: {plan:#?}"
        );
        assert_eq!(plan.skipped.len(), 3, "{plan:#?}");

        let named = [
            lost_key.clone(),
            requeued_key.clone(),
            carried_key.clone(),
            "typo".to_string(),
        ]
        .into_iter()
        .collect::<HashSet<_>>();
        let plan = plan_completion_cell_restore(&content, &claims, &receipts, &named).unwrap();
        assert_eq!(plan.restored.len(), 1, "{plan:#?}");
        assert_eq!(plan.restored[0].key, lost_key);
        assert_eq!(plan.present, 1);
        assert_eq!(plan.missing_keys, ["typo"]);
        let reasons = plan
            .skipped
            .iter()
            .map(|skip| (skip.item.as_str(), skip.reason.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            reasons,
            [
                (
                    "do [#retired]",
                    RestoreSkipReason::AttemptClosed { closed_at_secs: 9 }
                ),
                ("do [#requeued]", RestoreSkipReason::ClosedAttemptLiveRow),
            ]
        );
    }

    #[test]
    fn refuses_a_cut_without_a_queue() {
        let content = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        assert_eq!(
            plan_completion_cell_restore(
                content,
                &QueueClaimLedger::default(),
                &ClaimReleaseReceiptLedger::default(),
                &HashSet::new(),
            ),
            Err(StaleEvidenceRefusal::NoQueueComponent)
        );
    }
}

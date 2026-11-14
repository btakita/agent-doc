//! Durable-state evidence for the shared closeout gate (`#transientcloseout`,
//! `#stopquiescentrows`).
//!
//! [`agent_doc_queue::transient_closeout`] classifies live queue rows from
//! the document text. Whether a `🟡`/`🚧` row's claim is heartbeat-fresh and
//! whether a closed `agent:loop` window holds a `📥` row back live in
//! `state.db`, so every turn-end gate (the Claude/Grok Stop hook and
//! session-check) reads them here, the same way, before it classifies.

use std::path::Path;

use anyhow::Result;

use agent_doc_queue::transient_closeout::{
    CloseoutEvidence, TransientCloseoutAssessment, assess_transient_closeout, fresh_claim_phase,
};

/// The evidence for `content` at `now_secs`. Reads the claim ledger and the
/// schedule state without writing either. An unreadable ledger proves no
/// claim fresh, so its rows keep refusing.
pub fn closeout_evidence_at(file: &Path, content: &str, now_secs: u64) -> CloseoutEvidence {
    let (fresh_claims, reportable_lost) = match crate::queue_claim::load_ledger_following_read_only(
        file, content,
    ) {
        Ok(ledger) => {
            let frozen_secs = crate::subagent_pool::frozen_secs();
            let fresh = ledger
                .claims
                .iter()
                .filter_map(|claim| {
                    fresh_claim_phase(
                        claim,
                        now_secs,
                        frozen_secs,
                        agent_doc_queue::queue_claim::DISPATCH_EVIDENCE_GRACE_SECS,
                    )
                    .map(|phase| (claim.identity.clone(), phase))
                })
                .collect();
            // `#lostrowreportreopen`: an ended worker's recorded,
            // unreported hand-back owes a report, not continue or retry.
            let reportable = ledger
                .claims
                .iter()
                .filter(|claim| claim.lost_end_hand_back().is_some())
                .map(|claim| claim.identity.clone())
                .collect();
            (fresh, reportable)
        }
        Err(err) => {
            eprintln!(
                "[agent-doc] could not read queue claims for {} to prove worker freshness; claimed rows keep refusing closeout: {err:#}",
                file.display()
            );
            Default::default()
        }
    };
    let schedule_held_heads = crate::queue_schedule::schedule_held_heads(file, content)
        .into_iter()
        .collect();
    CloseoutEvidence {
        fresh_claims,
        schedule_held_heads,
        reportable_lost,
    }
}

/// [`closeout_evidence_at`] on the claim clock.
pub fn closeout_evidence(file: &Path, content: &str) -> CloseoutEvidence {
    closeout_evidence_at(file, content, crate::queue_claim::claim_clock_now_secs())
}

/// Classify `content`'s live transient rows with the durable evidence.
pub fn assess(file: &Path, content: &str) -> Result<TransientCloseoutAssessment> {
    assess_transient_closeout(content, &closeout_evidence(file, content))
}

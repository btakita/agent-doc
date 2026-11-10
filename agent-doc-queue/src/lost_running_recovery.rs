//! Automatic continue-then-retry for a lost running claim (`#lostrunningcontinue`).
//!
//! A `⚠️🚧` attempt is a worker that had started and was then lost. Every
//! observation continues that same generation and worker, including while a
//! turn is already in progress. `turn_in_progress` is recorded on the attempt;
//! it never suppresses the action. Continue fails closed when there is no
//! worker to resume or the heartbeat is already past the frozen bound. That
//! failure is what authorizes a new generation. An explicit requeue abandon
//! of a started attempt stays fail-closed, as do a harness freeze that still
//! holds its claim, a frozen handback that was not report-completed, quota
//! exhaustion, and lost-reported closeout.

use crate::queue_claim::{LostContinueRecord, QueueClaim, QueueClaimState};
use crate::subagent_pool::{DEFAULT_WORKER_FROZEN_SECS, last_heartbeat_secs};

/// What to do with one lost-running attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LostRunningAction {
    /// Resume the same generation and worker.
    Continue,
    /// Continue already failed; offer a new generation.
    Retry,
}

impl LostRunningAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Retry => "retry",
        }
    }
}

/// One claim this recovery pass touched, or recognized as an idempotent retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryApplied {
    pub item_text: String,
    pub generation: u64,
    pub action: LostRunningAction,
    pub changed: bool,
    pub during_turn: bool,
}

/// Whether `claim` is the `⚠️🚧` case this recovery owns.
pub fn is_lost_running_candidate(claim: &QueueClaim) -> bool {
    if claim.state == QueueClaimState::QuotaBlocked
        || claim.quota_block.is_some()
        || claim.lost_from == Some(QueueClaimState::QuotaBlocked)
    {
        return false;
    }
    // A harness freeze whose worker is still bound is revived by the exact
    // heartbeat, not replaced from here. A frozen handback that was never
    // report-completed is not this loss either. An explicit requeue abandon
    // of a started attempt stays fail-closed (`#losslessqueuerecovery`).
    if claim.retains_frozen_worker_claim()
        || frozen_unreported_handback(claim)
        || claim
            .frozen
            .as_ref()
            .is_some_and(|freeze| freeze.is_abandon())
    {
        return false;
    }
    claim.projection_marker()
        == agent_doc_document::queue_projection::QueueLifecycleMarker::LostRunning
}

fn frozen_unreported_handback(claim: &QueueClaim) -> bool {
    claim.frozen.is_some()
        && claim.publication_state.is_some()
        && claim.state != QueueClaimState::Reported
        && !claim.completion_recovered_after_loss
}

/// Continue, or retry when a previous continue failed. `None` for every other
/// claim. Callers project TTL expiry first; a still-`Running` row is not lost.
pub fn recovery_action(claim: &QueueClaim) -> Option<LostRunningAction> {
    if !is_lost_running_candidate(claim) {
        return None;
    }
    if claim
        .lost_continue
        .as_ref()
        .is_some_and(|record| record.failed)
    {
        Some(LostRunningAction::Retry)
    } else {
        Some(LostRunningAction::Continue)
    }
}

/// Whether dispatch may offer a new generation for this lost running attempt.
pub fn retry_authorized(claim: &QueueClaim) -> bool {
    recovery_action(claim) == Some(LostRunningAction::Retry)
}

/// Whether applying recovery at `now_secs` would change `claim`.
pub fn recovery_would_change(claim: &QueueClaim, now_secs: u64) -> bool {
    let mut projected = claim.clone();
    projected.project_expiry(now_secs);
    match recovery_action(&projected) {
        Some(LostRunningAction::Continue) => true,
        Some(LostRunningAction::Retry) => projected != *claim,
        None => false,
    }
}

/// Resume the same worker when the binding is still plausible.
///
/// Success does not write a heartbeat. The worker's own refresh is the proof
/// of life; stamping one here would continue a dead worker forever.
pub fn default_continue_attempt(claim: &QueueClaim, now_secs: u64) -> Result<(), String> {
    let Some(worker) = claim.worker.as_ref() else {
        return Err("lost running claim has no worker to continue".to_string());
    };
    if worker.worker_id.trim().is_empty() {
        return Err("lost running claim has an empty worker id".to_string());
    }
    if claim
        .frozen
        .as_ref()
        .is_some_and(|freeze| freeze.is_abandon())
    {
        return Err("lost running claim was abandoned for requeue".to_string());
    }
    let last = last_heartbeat_secs(claim).unwrap_or(0);
    let stale_for = now_secs.saturating_sub(last);
    if stale_for >= DEFAULT_WORKER_FROZEN_SECS {
        return Err(format!(
            "worker heartbeat is stale for {stale_for}s; continue refused"
        ));
    }
    Ok(())
}

/// Record a continue attempt on `claim`. `Ok` restores the same generation
/// and worker. `Err` leaves the attempt lost and authorizes retry.
pub fn apply_continue_result(
    claim: &mut QueueClaim,
    now_secs: u64,
    ttl_secs: u64,
    turn_in_progress: bool,
    continue_result: Result<(), String>,
) {
    match continue_result {
        Ok(()) => {
            claim.state = QueueClaimState::Running;
            claim.lost_from = None;
            claim.expires_at_secs = now_secs.saturating_add(ttl_secs.max(1));
            claim.lost_continue = Some(LostContinueRecord {
                attempted_at_secs: now_secs,
                failed: false,
                reason: String::new(),
                during_turn: turn_in_progress,
            });
        }
        Err(reason) => {
            claim.lost_continue = Some(LostContinueRecord {
                attempted_at_secs: now_secs,
                failed: true,
                reason,
                during_turn: turn_in_progress,
            });
        }
    }
}

/// Continue each lost-running claim in `claims`, then retry when that fails.
/// A second pass after a failed continue does not call `continue_one` again.
pub fn recover_claims(
    claims: &mut [QueueClaim],
    now_secs: u64,
    ttl_secs: u64,
    turn_in_progress: bool,
    continue_one: &mut impl FnMut(&QueueClaim) -> Result<(), String>,
) -> Vec<RecoveryApplied> {
    let mut applied = Vec::new();
    for claim in claims {
        let snapshot = claim.clone();
        claim.project_expiry(now_secs);
        let Some(action) = recovery_action(claim) else {
            *claim = snapshot;
            continue;
        };
        let action = match action {
            LostRunningAction::Retry => LostRunningAction::Retry,
            LostRunningAction::Continue => {
                let result = continue_one(claim);
                apply_continue_result(claim, now_secs, ttl_secs, turn_in_progress, result);
                if claim
                    .lost_continue
                    .as_ref()
                    .is_some_and(|record| record.failed)
                {
                    LostRunningAction::Retry
                } else {
                    LostRunningAction::Continue
                }
            }
        };
        applied.push(RecoveryApplied {
            item_text: claim.item_text.clone(),
            generation: claim.generation,
            action,
            changed: *claim != snapshot,
            during_turn: turn_in_progress,
        });
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_claim::{QueueClaimLedger, QueueWorkerBinding, QueueWorkerFreeze};

    fn lost_running(item: &str, started_at_secs: u64) -> QueueClaim {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(item, "subagent:boss", started_at_secs, 3_600);
        let stored = &mut ledger.claims[0];
        stored.state = QueueClaimState::Running;
        stored.worker = Some(QueueWorkerBinding {
            worker_id: "boss7adv".to_string(),
            started_at_secs,
        });
        stored.heartbeat_at_secs = Some(started_at_secs);
        stored.mark_lost();
        stored.clone()
    }

    #[test]
    fn lost_running_claim_is_continued_during_a_turn() {
        let mut claims = vec![lost_running("adversarial review", 1_000)];
        let generation = claims[0].generation;
        let worker = claims[0].worker.clone();
        let mut continued = false;
        let applied = recover_claims(&mut claims, 5_000, 3_600, true, &mut |_| {
            continued = true;
            Ok(())
        });
        assert!(continued, "continue runs even while a turn is in progress");
        assert_eq!(applied[0].action, LostRunningAction::Continue);
        assert!(applied[0].changed);
        assert!(applied[0].during_turn);
        assert_eq!(claims[0].state, QueueClaimState::Running);
        assert_eq!(claims[0].generation, generation);
        assert_eq!(claims[0].worker, worker);
        assert!(claims[0].lost_from.is_none());
        assert_eq!(claims[0].heartbeat_at_secs, Some(1_000));
        assert_eq!(claims[0].expires_at_secs, 5_000 + 3_600);
        let record = claims[0].lost_continue.as_ref().unwrap();
        assert!(!record.failed);
        assert!(record.during_turn);
        assert!(recovery_action(&claims[0]).is_none());
    }

    #[test]
    fn failed_continue_retries_without_a_second_continue_call() {
        let mut claims = vec![lost_running("adversarial review", 1_000)];
        let mut calls = 0;
        let applied = recover_claims(&mut claims, 5_000, 3_600, true, &mut |_| {
            calls += 1;
            Err("worker is gone".to_string())
        });
        assert_eq!(calls, 1);
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert!(applied[0].during_turn);
        assert_eq!(claims[0].state, QueueClaimState::Lost);
        assert!(retry_authorized(&claims[0]));
        let record = claims[0].lost_continue.as_ref().unwrap();
        assert!(record.failed);
        assert_eq!(record.reason, "worker is gone");
        assert!(record.during_turn);

        let applied = recover_claims(&mut claims, 5_100, 3_600, true, &mut |_| {
            calls += 1;
            Ok(())
        });
        assert_eq!(calls, 1, "a failed continue is not attempted again");
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert!(!applied[0].changed);
        assert!(retry_authorized(&claims[0]));
    }

    #[test]
    fn an_idle_observation_also_continues() {
        let mut claims = vec![lost_running("adversarial review", 1_000)];
        let applied = recover_claims(&mut claims, 5_000, 3_600, false, &mut |_| Ok(()));
        assert_eq!(applied[0].action, LostRunningAction::Continue);
        assert!(!applied[0].during_turn);
        assert_eq!(claims[0].state, QueueClaimState::Running);
    }

    #[test]
    fn frozen_unreported_handback_is_not_this_recovery() {
        let mut claim = lost_running("handed back", 1_000);
        claim.frozen = Some(QueueWorkerFreeze {
            frozen_at_secs: 1_100,
            reason: "harness reported the worker ended".to_string(),
            abandoned: false,
        });
        claim.publication_state = Some(crate::QueueReportState::Local);
        // A non-abandon freeze keeps the claim; project_expiry must not be
        // required for the exclusion.
        assert!(recovery_action(&claim).is_none());
        assert!(!retry_authorized(&claim));
    }

    #[test]
    fn quota_block_and_lost_reported_are_not_retried_here() {
        let mut quota = lost_running("quota", 1_000);
        quota.lost_from = Some(QueueClaimState::QuotaBlocked);
        assert!(recovery_action(&quota).is_none());

        let mut reported = lost_running("reported", 1_000);
        reported.state = QueueClaimState::Reported;
        reported.completion_recovered_after_loss = true;
        reported.lost_from = Some(QueueClaimState::Lost);
        assert!(recovery_action(&reported).is_none());
    }

    #[test]
    fn default_continue_fails_when_the_heartbeat_is_stale_or_the_worker_is_gone() {
        let fresh = lost_running("fresh", 1_000);
        assert!(default_continue_attempt(&fresh, 1_000 + DEFAULT_WORKER_FROZEN_SECS - 1).is_ok());
        assert!(default_continue_attempt(&fresh, 1_000 + DEFAULT_WORKER_FROZEN_SECS).is_err());

        let mut no_worker = fresh.clone();
        no_worker.worker = None;
        assert!(default_continue_attempt(&no_worker, 1_500).is_err());

        let mut abandoned = fresh.clone();
        abandoned.frozen = Some(QueueWorkerFreeze {
            frozen_at_secs: 1_100,
            reason: crate::queue_claim::ABANDONED_REQUEUE_REASON.to_string(),
            abandoned: true,
        });
        assert!(default_continue_attempt(&abandoned, 1_500).is_err());
        assert!(recovery_action(&abandoned).is_none());
        assert!(!retry_authorized(&abandoned));
    }
}

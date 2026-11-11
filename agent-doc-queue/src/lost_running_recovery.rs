//! Automatic continue-then-retry for a lost running claim (`#lostrunningcontinue`).
//!
//! A `⚠️🚧` attempt is a worker that had started and was then lost. Every
//! observation continues that same generation and worker, including while a
//! turn is already in progress. `turn_in_progress` is recorded on the attempt;
//! it never suppresses the action. Continue fails closed when there is no
//! worker to resume, the heartbeat is already past the frozen bound, or the
//! attempt was abandoned for requeue. That failure authorizes a new generation
//! in the same pass (`#transientcloseout`). An explicit requeue must not resume
//! the abandoned worker, and it must not leave the row stuck on `⚠️🚧`. A
//! harness freeze whose worker has already stopped, with no handback, stays
//! `Running` and is not marked Lost. Continue fails because that worker is
//! gone, and the failure authorizes a new generation in the same pass
//! (`#transientcloseout`). A worker that is still running is not this case.
//! A Lost attempt that already recorded `lost_continue.failed` is not held by
//! a non-abandon freeze: the owed action is a new generation. A Lost or
//! Running freeze whose continue has not failed stays undispatchable, and
//! `queue claim --refresh` revives that same generation. A frozen handback
//! that was not report-completed, quota exhaustion, and lost-reported
//! closeout stay outside this recovery.

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
    // A non-abandon freeze keeps a Lost or Running attempt undispatchable
    // until continue has failed. `queue claim --refresh` still revives that
    // generation. Once `lost_continue.failed` is set, the freeze must not
    // hide the owed new generation. A Running freeze is
    // [`is_stopped_harness_freeze`] and this path does not mark it Lost. A
    // frozen handback that was never report-completed is not this loss
    // either. An explicit requeue abandon still fails continue below; it is
    // a candidate so that failure can authorize a new generation while the
    // row shows `⚠️🚧` (`#transientcloseout`).
    let freeze_holds_generation =
        claim.retains_frozen_worker_claim() && !continue_already_failed(claim);
    if freeze_holds_generation || frozen_unreported_handback(claim) {
        return false;
    }
    claim.projection_marker()
        == agent_doc_document::queue_projection::QueueLifecycleMarker::LostRunning
}

/// A harness freeze whose worker has stopped and that has no handback.
///
/// The attempt stays [`QueueClaimState::Running`]. It is not Lost. Continue
/// cannot resume the stopped worker, so the failure authorizes a new
/// generation. A freeze that already carries a handback is not this case.
pub fn is_stopped_harness_freeze(claim: &QueueClaim) -> bool {
    if claim.state == QueueClaimState::QuotaBlocked
        || claim.quota_block.is_some()
        || claim.lost_from == Some(QueueClaimState::QuotaBlocked)
        || frozen_unreported_handback(claim)
    {
        return false;
    }
    claim.state == QueueClaimState::Running
        && claim
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.worker_id.trim().is_empty())
        && claim
            .frozen
            .as_ref()
            .is_some_and(|freeze| !freeze.is_abandon())
        && claim.publication_state.is_none()
}

fn is_continue_retry_candidate(claim: &QueueClaim) -> bool {
    is_lost_running_candidate(claim) || is_stopped_harness_freeze(claim)
}

fn continue_already_failed(claim: &QueueClaim) -> bool {
    claim
        .lost_continue
        .as_ref()
        .is_some_and(|record| record.failed)
}

fn frozen_unreported_handback(claim: &QueueClaim) -> bool {
    claim.frozen.is_some()
        && claim.publication_state.is_some()
        && claim.state != QueueClaimState::Reported
        && !claim.completion_recovered_after_loss
}

/// Continue, or retry when a previous continue failed. `None` for every other
/// claim. Callers project TTL expiry first. A stopped harness freeze stays
/// `Running`; a worker that is still running is not a candidate.
pub fn recovery_action(claim: &QueueClaim) -> Option<LostRunningAction> {
    if !is_continue_retry_candidate(claim) {
        return None;
    }
    if continue_already_failed(claim) {
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
    // The freeze is the harness report that this worker already stopped.
    // A fresh heartbeat must not resume it or authorize a second generation
    // for a worker that is actually still running (`#transientcloseout`).
    if is_stopped_harness_freeze(claim) {
        return Err("harness freeze worker is gone; continue refused".to_string());
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
    use crate::queue_claim::{
        ClaimOutcome, LostContinueRecord, QueueClaimLedger, QueueWorkerBinding, QueueWorkerFreeze,
    };

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
        assert_eq!(
            recovery_action(&abandoned),
            Some(LostRunningAction::Continue),
            "an abandoned ⚠️🚧 row is continued, not skipped"
        );
        assert!(
            !retry_authorized(&abandoned),
            "retry waits until continue has failed"
        );

        let mut claims = vec![abandoned];
        let applied = recover_claims(&mut claims, 1_500, 3_600, true, &mut |claim| {
            default_continue_attempt(claim, 1_500)
        });
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert!(applied[0].during_turn);
        assert_eq!(claims[0].state, QueueClaimState::Lost);
        assert!(retry_authorized(&claims[0]));
        assert!(
            claims[0].lost_continue.as_ref().is_some_and(
                |record| record.failed && record.reason.contains("abandoned for requeue")
            )
        );
    }

    #[test]
    fn a_stopped_harness_freeze_authorizes_retry_without_becoming_lost() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("annocorrupt", "subagent:annocorrupt", 1_000, 60);
        ledger
            .start(
                "annocorrupt",
                "subagent:annocorrupt",
                "worker-annocorrupt",
                1_000,
                60,
                Some(1),
            )
            .unwrap();
        ledger
            .freeze_attempt(
                "annocorrupt",
                Some(1),
                1_100,
                "harness reported the worker ended",
            )
            .unwrap();
        let claim = &mut ledger.claims[0];
        claim.project_expiry(9_000);
        assert_eq!(claim.state, QueueClaimState::Running);
        assert!(claim.lost_from.is_none());
        assert!(claim.publication_state.is_none());
        assert_eq!(
            claim.projection_marker(),
            agent_doc_document::queue_projection::QueueLifecycleMarker::Running
        );
        assert_eq!(
            recovery_action(claim),
            Some(LostRunningAction::Continue),
            "continue is attempted before the stopped worker authorizes retry"
        );
        assert!(!retry_authorized(claim));

        let mut claims = vec![claim.clone()];
        let applied = recover_claims(&mut claims, 9_000, 60, true, &mut |claim| {
            default_continue_attempt(claim, 9_000)
        });
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert!(applied[0].during_turn);
        assert_eq!(claims[0].state, QueueClaimState::Running);
        assert!(claims[0].lost_from.is_none());
        assert_eq!(claims[0].generation, 1);
        assert!(retry_authorized(&claims[0]));
        assert!(
            !claims[0].blocks_dispatch(9_000),
            "the stopped freeze must be claimable"
        );
        assert!(
            claims[0].lost_continue.as_ref().is_some_and(|record| {
                record.failed && record.reason.contains("worker is gone")
            })
        );

        ledger.claims[0] = claims[0].clone();
        assert_eq!(
            ledger.claim("annocorrupt", "subagent:annocorrupt", 9_000, 60),
            ClaimOutcome::Reassigned
        );
        assert_eq!(ledger.claims[0].generation, 2);
        assert_eq!(ledger.claims[0].state, QueueClaimState::Claimed);
        assert!(ledger.claims[0].frozen.is_none());
        assert!(ledger.claims[0].lost_from.is_none());
    }

    #[test]
    fn a_heartbeat_after_a_stopped_freeze_continues_the_same_generation_later() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("annocorrupt", "subagent:annocorrupt", 1_000, 60);
        ledger
            .start(
                "annocorrupt",
                "subagent:annocorrupt",
                "worker-annocorrupt",
                1_000,
                60,
                Some(1),
            )
            .unwrap();
        ledger
            .freeze_attempt(
                "annocorrupt",
                Some(1),
                1_100,
                "harness reported the worker ended",
            )
            .unwrap();
        let mut claims = vec![ledger.claims[0].clone()];
        let applied = recover_claims(&mut claims, 9_000, 60, true, &mut |claim| {
            default_continue_attempt(claim, 9_000)
        });
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        ledger.claims[0] = claims[0].clone();
        let revived = ledger
            .refresh_or_revive_attempt_in_session(
                "annocorrupt",
                "subagent:annocorrupt",
                9_000,
                3_600,
                None,
                Some(1),
            )
            .unwrap();
        assert_eq!(revived.generation, 1);
        assert_eq!(revived.state, QueueClaimState::Running);
        assert!(revived.frozen.is_none());
        assert!(revived.lost_continue.is_none());
        assert!(!retry_authorized(&revived));
        assert!(revived.blocks_dispatch(9_000));

        let expired_at = revived.expires_at_secs;
        let mut later = vec![revived];
        let applied = recover_claims(&mut later, expired_at, 3_600, true, &mut |claim| {
            default_continue_attempt(claim, expired_at)
        });
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].action, LostRunningAction::Continue);
        assert_eq!(later[0].generation, 1);
        assert_eq!(later[0].state, QueueClaimState::Running);
        assert!(later[0].lost_from.is_none());
        assert!(!retry_authorized(&later[0]));
        assert_eq!(
            later[0]
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str()),
            Some("worker-annocorrupt")
        );
    }

    #[test]
    fn a_live_running_worker_does_not_authorize_a_second_generation() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("keeprows", "subagent:keeprows", 1_000, 3_600);
        ledger
            .start(
                "keeprows",
                "subagent:keeprows",
                "worker-keeprows",
                1_000,
                3_600,
                Some(1),
            )
            .unwrap();
        let claim = &ledger.claims[0];
        assert!(claim.frozen.is_none());
        assert_eq!(claim.heartbeat_at_secs, Some(1_000));
        let mut projected = claim.clone();
        projected.project_expiry(1_500);
        assert_eq!(projected.state, QueueClaimState::Running);
        assert!(recovery_action(&projected).is_none());
        assert!(!retry_authorized(&projected));
        assert!(default_continue_attempt(&projected, 1_500).is_ok());

        let mut claims = vec![projected];
        let applied = recover_claims(&mut claims, 1_500, 3_600, true, &mut |claim| {
            default_continue_attempt(claim, 1_500)
        });
        assert!(applied.is_empty());
        assert_eq!(claims[0].state, QueueClaimState::Running);
        assert_eq!(claims[0].generation, 1);
        assert!(claims[0].lost_continue.is_none());
        assert!(claims[0].blocks_dispatch(1_500));
        assert_eq!(
            ledger.claim("keeprows", "subagent:keeprows", 1_500, 3_600),
            ClaimOutcome::Refreshed
        );
        assert_eq!(ledger.claims[0].generation, 1);
        assert_eq!(ledger.claims[0].state, QueueClaimState::Running);
    }

    fn frozen_running_ledger() -> QueueClaimLedger {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("lostmarker", "subagent:lostmarker", 1_000, 60);
        ledger
            .start(
                "lostmarker",
                "subagent:lostmarker",
                "worker-lostmarker",
                1_000,
                60,
                Some(1),
            )
            .unwrap();
        ledger
            .freeze_attempt(
                "lostmarker",
                Some(1),
                1_100,
                "subagent ended before coordinator integration",
            )
            .unwrap();
        ledger
    }

    #[test]
    fn a_lost_non_abandon_freeze_stays_undispatchable_until_continue_has_failed() {
        let mut running = frozen_running_ledger();
        let running_claim = &running.claims[0];
        assert_eq!(running_claim.state, QueueClaimState::Running);
        assert!(running_claim.lost_from.is_none());
        assert!(running_claim.retains_frozen_worker_claim());
        assert!(!retry_authorized(running_claim));
        assert!(running_claim.blocks_dispatch(9_000));
        running.claims[0].project_expiry(9_000);
        assert_eq!(
            running.claims[0].state,
            QueueClaimState::Running,
            "a stopped Running freeze is not marked Lost"
        );

        let mut ledger = frozen_running_ledger();
        ledger.claims[0].mark_lost();
        let claim = &ledger.claims[0];
        assert_eq!(claim.state, QueueClaimState::Lost);
        assert_eq!(claim.lost_from, Some(QueueClaimState::Running));
        assert!(claim.retains_frozen_worker_claim());
        assert!(!is_lost_running_candidate(claim));
        assert!(recovery_action(claim).is_none());
        assert!(!retry_authorized(claim));
        assert!(claim.blocks_dispatch(9_000));

        let revived = ledger
            .refresh_or_revive_attempt_in_session(
                "lostmarker",
                "subagent:lostmarker",
                9_000,
                3_600,
                None,
                Some(1),
            )
            .unwrap();
        assert_eq!(revived.generation, 1);
        assert_eq!(revived.state, QueueClaimState::Running);
        assert!(revived.frozen.is_none());
        assert!(revived.lost_continue.is_none());
        assert_eq!(ledger.claims.len(), 1);
        assert_eq!(ledger.claims[0].generation, 1);

        let mut failed = frozen_running_ledger();
        failed.claims[0].mark_lost();
        failed.claims[0].lost_continue = Some(LostContinueRecord {
            attempted_at_secs: 2_000,
            failed: true,
            reason: "harness freeze worker is gone; continue refused".to_string(),
            during_turn: true,
        });
        let claim = &failed.claims[0];
        assert_eq!(claim.state, QueueClaimState::Lost);
        assert!(is_lost_running_candidate(claim));
        assert_eq!(recovery_action(claim), Some(LostRunningAction::Retry));
        assert!(retry_authorized(claim));
        assert!(
            !claim.blocks_dispatch(9_000),
            "a failed continue owes a new generation"
        );

        let mut claims = vec![claim.clone()];
        let mut calls = 0;
        let applied = recover_claims(&mut claims, 9_000, 60, true, &mut |_| {
            calls += 1;
            Ok(())
        });
        assert_eq!(calls, 0, "a failed continue is not attempted again");
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert_eq!(claims[0].state, QueueClaimState::Lost);
        assert_eq!(claims[0].lost_from, Some(QueueClaimState::Running));
        assert_eq!(claims[0].generation, 1);
        assert!(claims[0].frozen.is_some());

        failed.claims[0] = claims[0].clone();
        assert_eq!(
            failed.claim("lostmarker", "subagent:lostmarker", 9_000, 60),
            ClaimOutcome::Reassigned
        );
        assert_eq!(failed.claims[0].generation, 2);
        assert_eq!(failed.claims[0].state, QueueClaimState::Claimed);
        assert!(failed.claims[0].frozen.is_none());
        assert!(failed.claims[0].lost_continue.is_none());
    }
}

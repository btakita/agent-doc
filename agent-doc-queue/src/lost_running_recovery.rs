//! Automatic continue-then-retry for a lost running claim (`#lostrunningcontinue`).
//!
//! A `⚠️🚧` attempt is a worker that had started and was then lost. Every
//! observation continues that same generation and worker, including while a
//! turn is already in progress. `turn_in_progress` is recorded on the attempt;
//! it never suppresses the action. Continue fails closed when there is no
//! worker to resume, the heartbeat is already past the frozen bound, or the
//! attempt was abandoned for requeue. That failure authorizes a new generation
//! in the same pass (`#transientcloseout`). An explicit requeue of a running
//! claim must not resume the abandoned worker and must not paint `⚠️🚧`.
//! The attempt stays `Running` with the abandon freeze, so a failed continue
//! authorizes one replacement. Low-level freeze recording temporarily keeps a
//! stopped worker with no handback `Running` as recovery evidence. The public
//! `queue freeze` command
//! synchronously attempts continue, records its failure because the worker is
//! gone, and authorizes a new generation in the same pass
//! (`#transientcloseout`). A worker that is still running is not this case.
//! A Lost non-abandon freeze is still `⚠️🚧`. The same pass continues it.
//! Continue fails because that worker is gone, and the failure authorizes a
//! new generation. Before that recovery pass, exact proof can revive the
//! ledger entry at a low-level seam; that is not coordinator guidance and the
//! public freeze path does not expose an old-generation rebind.
//! A Running freeze is not marked Lost. A Lost attempt that already recorded
//! `lost_continue.failed` is not held by the freeze. A frozen handback that
//! was not report-completed, quota exhaustion, and lost-reported closeout
//! stay outside this recovery.
//!
//! A frozen handback includes the subagent-end hook's freeze of a worker that
//! FINISHED: its recorded reason carries the worker's terminal hand-back
//! (`#finishedheadrefill`, 2026-11-15: three finished workers on
//! `agent-doc-bugs.md` had continue fail on them, so the work pool offered each
//! finished row for a new claim before the coordinator ran `queue
//! report-complete`). Such an attempt is [`report_owed`], never continued or
//! retried. Only an end with no captured hand-back, a wrapper exit while a
//! child task still runs, or a coordinator `queue freeze` with its own reason
//! is a stopped worker this recovery retries.

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
    // A non-abandon freeze on a still-Running attempt is
    // [`is_stopped_harness_freeze`]: this path does not mark it Lost, and
    // until continue has failed it stays undispatchable. A Lost attempt is
    // already `⚠️🚧`. The freeze must not skip the continue attempt. Continue
    // fails because that worker is gone, and the same pass authorizes a new
    // generation (`#transientcloseout`). Once `lost_continue.failed` is set,
    // the freeze must not hide that retry. At the low-level seam only, exact
    // proof may revive the ledger entry before continue runs; the public
    // freeze command performs this recovery synchronously. A frozen handback
    // that was never report-completed is not this loss. An explicit requeue of
    // a still-`Running` attempt is [`authorized_running_requeue`], not this
    // `⚠️🚧` candidate: continue fails on the abandon, and that failure
    // authorizes one replacement without painting the row lost.
    let running_freeze_holds_generation = claim.state == QueueClaimState::Running
        && claim.retains_frozen_worker_claim()
        && !continue_already_failed(claim);
    if running_freeze_holds_generation || frozen_unreported_handback(claim) {
        return false;
    }
    claim.projection_marker()
        == agent_doc_document::queue_projection::QueueLifecycleMarker::LostRunning
}

/// A transient low-level freeze whose worker has stopped and has no handback.
///
/// Until recovery runs, the ledger attempt stays [`QueueClaimState::Running`].
/// It is not Lost. Continue cannot resume the stopped worker, so the failure
/// authorizes a new generation. The public freeze command runs that recovery
/// synchronously. A freeze that already carries a handback is not this case.
pub fn is_stopped_harness_freeze(claim: &QueueClaim) -> bool {
    claim.state == QueueClaimState::Running && harness_freeze_worker_is_gone(claim)
}

/// A transient non-abandon ledger freeze with no handback. The worker has
/// stopped. Continue cannot resume it, including when already `Lost`.
fn harness_freeze_worker_is_gone(claim: &QueueClaim) -> bool {
    if claim.state == QueueClaimState::QuotaBlocked
        || claim.quota_block.is_some()
        || claim.lost_from == Some(QueueClaimState::QuotaBlocked)
        || frozen_unreported_handback(claim)
    {
        return false;
    }
    matches!(
        claim.state,
        QueueClaimState::Running | QueueClaimState::Lost
    ) && claim
        .worker
        .as_ref()
        .is_some_and(|worker| !worker.worker_id.trim().is_empty())
        && claim
            .frozen
            .as_ref()
            .is_some_and(|freeze| !freeze.is_abandon())
        && claim.publication_state.is_none()
}

/// An explicit `release --requeue` of a worker that had started.
///
/// The attempt stays [`QueueClaimState::Running`] so its projection is `🚧`,
/// not `⚠️🚧`. Continue cannot resume the abandoned worker. The recorded
/// failure authorizes one replacement generation. A claim that never started,
/// and a loss that is not this requeue, stay on the lost path.
pub fn authorized_running_requeue(claim: &QueueClaim) -> bool {
    claim.state == QueueClaimState::Running
        && claim.lost_from.is_none()
        && claim
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.worker_id.trim().is_empty())
        && claim
            .frozen
            .as_ref()
            .is_some_and(|freeze| freeze.is_abandon())
        && !report_owed(claim)
}

fn is_continue_retry_candidate(claim: &QueueClaim) -> bool {
    is_lost_running_candidate(claim)
        || is_stopped_harness_freeze(claim)
        || authorized_running_requeue(claim)
}

fn continue_already_failed(claim: &QueueClaim) -> bool {
    claim
        .lost_continue
        .as_ref()
        .is_some_and(|record| record.failed)
}

fn frozen_unreported_handback(claim: &QueueClaim) -> bool {
    report_owed(claim)
}

/// Whether `claim` is a finished worker's unreported result
/// (`#finishedheadrefill`): a freeze on an attempt that already carries a
/// delivered publication state, or a harness end-freeze that recorded the
/// worker's terminal hand-back ([`QueueClaim::ended_hand_back_owed`]).
///
/// The worker is done, not lost. The coordinator owes `queue report-complete`
/// for this exact generation (or a continue of the same worker, or an
/// explicit `queue release --requeue`). Continue never runs and never fails
/// for it, so no new generation is authorized, and the work pool keeps the
/// head out of its refill. A ledger an older binary wrote with
/// `lost_continue.failed` already set is not retried either: this check
/// comes before that record is read.
pub fn report_owed(claim: &QueueClaim) -> bool {
    let published = claim.frozen.is_some()
        && claim.publication_state.is_some()
        && claim.state != QueueClaimState::Reported
        && !claim.completion_recovered_after_loss;
    published || claim.ended_hand_back_owed().is_some()
}

/// Continue, or retry when a previous continue failed. `None` for every other
/// claim. Callers project TTL expiry first. A stopped harness freeze remains
/// `Running` only as transient ledger evidence before this pass; a worker that
/// is still running is not a candidate. A Lost non-abandon freeze is
/// continued, and that continue fails in the same pass.
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
    // The freeze is the harness report that this worker already stopped,
    // whether the attempt is still `Running` or already `Lost`. A fresh
    // heartbeat must not resume it or authorize a second generation for a
    // worker that is actually still running (`#transientcloseout`).
    if harness_freeze_worker_is_gone(claim) {
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
/// and worker. `Err` records the failure and authorizes retry. A still-
/// `Running` requeue stays `Running`; a claim that was already `Lost` stays
/// `Lost`.
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
        ClaimOutcome, QueueClaimLedger, QueueWorkerBinding, QueueWorkerFreeze,
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
        // An unreported handback retains its ledger entry for reconciliation;
        // project_expiry must not be required for the exclusion. This is not
        // the no-handback CLI freeze that synchronously authorizes a retry.
        assert!(recovery_action(&claim).is_none());
        assert!(!retry_authorized(&claim));
    }

    /// `#finishedheadrefill`: the subagent-end hook froze a worker that
    /// FINISHED (its freeze recorded the terminal hand-back) before the
    /// coordinator ran `queue report-complete`. Continue must not run or fail
    /// on it, so no new generation is authorized and the row stays held, on a
    /// `Running` attempt, on a `Lost` one, and on a ledger an older binary
    /// already marked `lost_continue.failed`. An end with no captured
    /// hand-back is still a stopped worker this recovery retries.
    #[test]
    fn a_finished_workers_recorded_hand_back_is_report_owed_not_retried() {
        let ended = |reason: String| {
            let mut claim = lost_running("do [#finished]", 1_000);
            claim.state = QueueClaimState::Running;
            claim.lost_from = None;
            claim.frozen = Some(QueueWorkerFreeze {
                frozen_at_secs: 1_100,
                reason,
                abandoned: false,
            });
            claim
        };
        let finished = ended(crate::queue_claim::subagent_end_freeze_reason(Some(
            "**Summary** fixed it. **What changed** branch fix/x, commit abc123.",
        )));
        assert!(finished.ended_hand_back_owed().is_some());
        assert!(report_owed(&finished));
        assert!(!is_stopped_harness_freeze(&finished));
        assert!(recovery_action(&finished).is_none());
        assert!(!retry_authorized(&finished));
        assert!(finished.blocks_dispatch(9_000));
        let mut claims = vec![finished.clone()];
        let applied = recover_claims(&mut claims, 1_200, 3_600, false, &mut |_| {
            panic!("continue must not run on a finished worker's attempt")
        });
        assert!(applied.is_empty(), "{applied:?}");
        assert_eq!(claims[0], finished, "recovery leaves the attempt untouched");

        let mut lost = finished.clone();
        lost.mark_lost();
        assert!(lost.lost_end_hand_back().is_some());
        assert!(recovery_action(&lost).is_none());
        assert!(lost.blocks_dispatch(9_000));

        let mut legacy = finished.clone();
        legacy.lost_continue = Some(LostContinueRecord {
            attempted_at_secs: 1_150,
            failed: true,
            reason: "harness freeze worker is gone; continue refused".to_string(),
            during_turn: false,
        });
        assert!(
            !retry_authorized(&legacy),
            "an older binary's failed continue does not authorize a retry of finished work"
        );
        assert!(legacy.blocks_dispatch(9_000));

        for reason in [
            crate::queue_claim::subagent_end_freeze_reason(None),
            crate::queue_claim::subagent_end_freeze_reason(Some(
                "Task abc is still running. Use /codex:status.",
            )),
            "ended without a result".to_string(),
        ] {
            let stopped = ended(reason);
            assert!(!report_owed(&stopped), "{stopped:?}");
            assert!(is_stopped_harness_freeze(&stopped), "{stopped:?}");
            assert_eq!(recovery_action(&stopped), Some(LostRunningAction::Continue));
        }
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
    fn a_ledger_only_refresh_before_replacement_can_cancel_retry() {
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
        // This exercises an internal ordering seam: exact proof arrives after
        // recovery mutation but before the replacement is claimed. It is not
        // a coordinator contract; public `queue freeze` emits the fresh retry.
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
        assert!(is_lost_running_candidate(claim));
        assert_eq!(
            recovery_action(claim),
            Some(LostRunningAction::Continue),
            "a lost freeze is continued; retry waits until that continue fails"
        );
        assert!(!retry_authorized(claim));
        assert!(
            claim.blocks_dispatch(9_000),
            "a freeze stays undispatchable until continue has failed"
        );
        assert!(default_continue_attempt(claim, 9_000).is_err());

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
        let mut claims = vec![failed.claims[0].clone()];
        let applied = recover_claims(&mut claims, 9_000, 60, true, &mut |claim| {
            default_continue_attempt(claim, 9_000)
        });
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert!(applied[0].during_turn);
        assert_eq!(claims[0].state, QueueClaimState::Lost);
        assert_eq!(claims[0].lost_from, Some(QueueClaimState::Running));
        assert_eq!(claims[0].generation, 1);
        assert!(claims[0].frozen.is_some());
        assert!(retry_authorized(&claims[0]));
        assert!(
            !claims[0].blocks_dispatch(9_000),
            "a failed continue owes a new generation"
        );
        assert!(claims[0].lost_continue.as_ref().is_some_and(|record| {
            record.failed && record.during_turn && record.reason.contains("worker is gone")
        }));

        let mut calls = 0;
        let applied = recover_claims(&mut claims, 9_100, 60, true, &mut |_| {
            calls += 1;
            Ok(())
        });
        assert_eq!(calls, 0, "a failed continue is not attempted again");
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert!(!applied[0].changed);
        assert_eq!(claims[0].state, QueueClaimState::Lost);

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

    /// An explicit requeue of a worker that had started stays `Running` (`🚧`).
    /// Continue fails closed and authorizes one replacement. After that
    /// replacement is started, another claim does not open a second generation.
    #[test]
    fn authorized_running_requeue_stays_running_and_a_second_claim_does_not_bind() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("requeue row", "subagent:first", 1_000, 3_600);
        ledger
            .start(
                "requeue row",
                "subagent:first",
                "worker-first",
                1_000,
                3_600,
                Some(1),
            )
            .unwrap();
        ledger.claims[0].frozen = Some(QueueWorkerFreeze {
            frozen_at_secs: 1_100,
            reason: crate::queue_claim::ABANDONED_REQUEUE_REASON.to_string(),
            abandoned: true,
        });
        let claim = &ledger.claims[0];
        assert!(authorized_running_requeue(claim));
        assert_eq!(claim.state, QueueClaimState::Running);
        assert!(claim.lost_from.is_none());
        assert_ne!(
            claim.projection_marker(),
            agent_doc_document::queue_projection::QueueLifecycleMarker::LostRunning
        );
        assert!(
            !claim.blocks_dispatch(1_500),
            "an authorized requeue is claimable without painting lost"
        );

        let mut claims = vec![claim.clone()];
        let applied = recover_claims(&mut claims, 1_500, 3_600, false, &mut |claim| {
            default_continue_attempt(claim, 1_500)
        });
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert_eq!(claims[0].state, QueueClaimState::Running);
        assert!(claims[0].lost_from.is_none());
        assert_ne!(
            claims[0].projection_marker(),
            agent_doc_document::queue_projection::QueueLifecycleMarker::LostRunning
        );
        assert!(claims[0].lost_continue.as_ref().is_some_and(|record| {
            record.failed && record.reason.contains("abandoned for requeue")
        }));
        assert_eq!(
            claims[0]
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str()),
            Some("worker-first")
        );

        ledger.claims[0] = claims[0].clone();
        assert_eq!(
            ledger.claim("requeue row", "subagent:second", 1_600, 3_600),
            ClaimOutcome::Reassigned
        );
        assert_eq!(ledger.claims[0].generation, 2);
        assert_eq!(ledger.claims[0].state, QueueClaimState::Claimed);
        ledger
            .start(
                "requeue row",
                "subagent:second",
                "worker-second",
                1_600,
                3_600,
                Some(2),
            )
            .unwrap();
        assert_eq!(
            ledger.claim("requeue row", "subagent:third", 1_700, 3_600),
            ClaimOutcome::HeldByOther
        );
        assert_eq!(ledger.claims[0].generation, 2);
        assert_eq!(ledger.claims[0].state, QueueClaimState::Running);
        assert_eq!(ledger.claims.len(), 1);
    }

    /// A running worker past its TTL, still inside the frozen heartbeat bound,
    /// keeps its generation. `claim` does not run recovery, so it must not
    /// reassign that generation to another owner.
    #[test]
    fn expired_running_worker_is_not_reassigned_before_recovery() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("live row", "subagent:live", 1_000, 600);
        ledger
            .start(
                "live row",
                "subagent:live",
                "worker-live",
                1_000,
                600,
                Some(1),
            )
            .unwrap();
        let now = 1_000 + 601;
        assert!(ledger.claims[0].is_expired(now));
        assert!(now - 1_000 < DEFAULT_WORKER_FROZEN_SECS);
        assert!(
            ledger.claims[0].blocks_dispatch(now),
            "a live worker past TTL still holds its generation until recovery"
        );
        assert_eq!(
            ledger.claim("live row", "subagent:other", now, 600),
            ClaimOutcome::HeldByOther
        );
        assert_eq!(ledger.claims[0].generation, 1);
        assert_eq!(ledger.claims[0].state, QueueClaimState::Running);
        assert!(ledger.claims[0].lost_from.is_none());
    }
}

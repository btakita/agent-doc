//! Persist lost-running continue-then-retry (`#lostrunningcontinue`).
//!
//! The decision lives in [`agent_doc_queue::lost_running_recovery`]. This
//! module applies it to the claim ledger. `turn_in_progress` is stored on the
//! attempt and does not skip the write.

use std::path::Path;

use agent_doc_queue::lost_running_recovery::{self, RecoveryApplied};
use anyhow::Result;

/// Continue lost-running claims, using `continue_one` as the resume attempt.
pub fn recover_lost_running(
    file: &Path,
    turn_in_progress: bool,
    mut continue_one: impl FnMut(&agent_doc_queue::queue_claim::QueueClaim) -> Result<(), String>,
) -> Result<Vec<RecoveryApplied>> {
    let now = crate::queue_claim::claim_clock_now_secs();
    let ledger = crate::queue_claim::load_ledger(file)?;
    if !ledger
        .claims
        .iter()
        .any(|claim| lost_running_recovery::recovery_would_change(claim, now))
    {
        return Ok(Vec::new());
    }
    let applied = crate::queue_claim::mutate_ledger(file, |ledger| {
        Ok(lost_running_recovery::recover_claims(
            &mut ledger.claims,
            now,
            agent_doc_queue::queue_claim::DEFAULT_QUEUE_CLAIM_TTL_SECS,
            turn_in_progress,
            &mut continue_one,
        ))
    })?;
    for item in applied.iter().filter(|item| item.changed) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "lost_running_recovery action={} generation={} during_turn={} item_bytes={} (#lostrunningcontinue)",
                item.action.as_str(),
                item.generation,
                turn_in_progress,
                item.item_text.len(),
            ),
        );
    }
    Ok(applied)
}

/// [`recover_lost_running`] with the production continue attempt.
pub fn recover_lost_running_automatically(
    file: &Path,
    turn_in_progress: bool,
) -> Result<Vec<RecoveryApplied>> {
    let now = crate::queue_claim::claim_clock_now_secs();
    recover_lost_running(file, turn_in_progress, |claim| {
        lost_running_recovery::default_continue_attempt(claim, now)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::lost_running_recovery::LostRunningAction;
    use agent_doc_queue::queue_claim::QueueClaimState;
    use agent_doc_queue::subagent_pool::DEFAULT_WORKER_FROZEN_SECS;

    fn project(item: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue go -->\n- {item}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &content).unwrap();
        (dir, file, content)
    }

    #[test]
    fn a_lost_running_claim_is_continued_during_a_turn_and_a_failed_continue_retries() {
        let item = "#subagents continue the lost running worker";
        let (_dir, file, content) = project(item);
        let pinned = 1_000_000;
        let _clock = crate::queue_claim::pin_claim_clock_for_current_thread(pinned);
        let (_, claimed) = crate::queue_claim::claim(&file, item, "subagent:boss7adv", 60).unwrap();
        crate::queue_claim::start_attempt(
            &file,
            item,
            "subagent:boss7adv",
            "boss7adv",
            60,
            claimed.generation,
        )
        .unwrap();

        let _expired = crate::queue_claim::pin_claim_clock_for_current_thread(pinned + 60);
        let applied = recover_lost_running(&file, true, |_| Ok(())).unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].action, LostRunningAction::Continue);
        assert!(applied[0].during_turn);
        let continued = crate::queue_claim::load_ledger(&file).unwrap();
        assert_eq!(continued.claims[0].state, QueueClaimState::Running);
        assert_eq!(continued.claims[0].generation, claimed.generation);
        assert_eq!(
            continued.claims[0]
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str()),
            Some("boss7adv")
        );
        assert!(
            continued.claims[0]
                .lost_continue
                .as_ref()
                .is_some_and(|record| record.during_turn && !record.failed)
        );
        assert!(
            crate::subagent_dispatch::pending_subagent_dispatch_for_content(&file, &content)
                .unwrap()
                .is_empty(),
            "a continued worker stays on this generation"
        );

        let _stale = crate::queue_claim::pin_claim_clock_for_current_thread(
            pinned + DEFAULT_WORKER_FROZEN_SECS,
        );
        let failed =
            recover_lost_running(&file, true, |_| Err("continue refused".to_string())).unwrap();
        assert_eq!(failed[0].action, LostRunningAction::Retry);
        assert!(failed[0].during_turn);
        let retried = crate::queue_claim::load_ledger(&file).unwrap();
        assert_eq!(retried.claims[0].state, QueueClaimState::Lost);
        assert!(
            retried.claims[0]
                .lost_continue
                .as_ref()
                .is_some_and(|record| record.failed && record.during_turn)
        );
        let offered =
            crate::subagent_dispatch::pending_subagent_dispatch_for_content(&file, &content)
                .unwrap();
        assert!(
            offered.iter().any(|head| head.contains(item)),
            "failed continue offers the row again, got {offered:?}"
        );
    }

    #[test]
    fn automatic_continue_retries_a_stale_worker_during_a_turn() {
        let item = "#subagents retry when continue cannot resume";
        let (_dir, file, content) = project(item);
        let pinned = 2_000_000;
        let _clock = crate::queue_claim::pin_claim_clock_for_current_thread(pinned);
        let (_, claimed) = crate::queue_claim::claim(&file, item, "subagent:boss8adv", 60).unwrap();
        crate::queue_claim::start_attempt(
            &file,
            item,
            "subagent:boss8adv",
            "boss8adv",
            60,
            claimed.generation,
        )
        .unwrap();
        let _stale = crate::queue_claim::pin_claim_clock_for_current_thread(
            pinned + DEFAULT_WORKER_FROZEN_SECS,
        );
        let applied = recover_lost_running_automatically(&file, true).unwrap();
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert!(applied[0].during_turn);
        let offered =
            crate::subagent_dispatch::pending_subagent_dispatch_for_content(&file, &content)
                .unwrap();
        assert!(
            offered.iter().any(|head| head.contains(item)),
            "stale lost-running worker is retried during a turn, got {offered:?}"
        );
    }

    #[test]
    fn a_stopped_harness_freeze_is_offered_without_becoming_lost() {
        let item = "#subagents annocorrupt stopped without a handback";
        let (_dir, file, content) = project(item);
        let pinned = 3_000_000;
        let _clock = crate::queue_claim::pin_claim_clock_for_current_thread(pinned);
        let (_, claimed) =
            crate::queue_claim::claim(&file, item, "subagent:annocorrupt", 60).unwrap();
        crate::queue_claim::start_attempt(
            &file,
            item,
            "subagent:annocorrupt",
            "worker-annocorrupt",
            60,
            claimed.generation,
        )
        .unwrap();
        crate::queue_claim::freeze_attempt(
            &file,
            item,
            Some(claimed.generation),
            "harness reported the worker ended",
        )
        .unwrap();
        let _later = crate::queue_claim::pin_claim_clock_for_current_thread(pinned + 10_000);
        let applied = recover_lost_running_automatically(&file, true).unwrap();
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        let stored = crate::queue_claim::load_ledger(&file).unwrap();
        assert_eq!(stored.claims[0].state, QueueClaimState::Running);
        assert!(stored.claims[0].lost_from.is_none());
        assert_eq!(stored.claims[0].generation, claimed.generation);
        assert!(agent_doc_queue::lost_running_recovery::retry_authorized(
            &stored.claims[0]
        ));
        let offered =
            crate::subagent_dispatch::pending_subagent_dispatch_for_content(&file, &content)
                .unwrap();
        assert!(
            offered.iter().any(|head| head.contains(item)),
            "stopped harness freeze is offered for a new generation, got {offered:?}"
        );
    }

    #[test]
    fn a_lost_non_abandon_freeze_is_retried_during_a_turn() {
        let item = "#subagents lostretry stayed lost after the harness freeze";
        let (_dir, file, content) = project(item);
        let pinned = 4_000_000;
        let _clock = crate::queue_claim::pin_claim_clock_for_current_thread(pinned);
        let (_, claimed) =
            crate::queue_claim::claim(&file, item, "subagent:lostretry", 60).unwrap();
        crate::queue_claim::start_attempt(
            &file,
            item,
            "subagent:lostretry",
            "lostretry",
            60,
            claimed.generation,
        )
        .unwrap();
        crate::queue_claim::freeze_attempt(
            &file,
            item,
            Some(claimed.generation),
            "harness reported the worker ended",
        )
        .unwrap();
        let abandoned = crate::queue_claim::abandon_attempt(&file, item, Some(claimed.generation))
            .unwrap()
            .expect("frozen attempt is retained as lost");
        assert_eq!(abandoned.state, QueueClaimState::Lost);
        assert!(
            abandoned
                .frozen
                .as_ref()
                .is_some_and(|freeze| !freeze.is_abandon()),
            "an existing harness freeze is not rewritten into an abandon"
        );
        assert!(!agent_doc_queue::lost_running_recovery::retry_authorized(
            &abandoned
        ));

        let applied = recover_lost_running_automatically(&file, true).unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].action, LostRunningAction::Retry);
        assert!(applied[0].during_turn);
        let stored = crate::queue_claim::load_ledger(&file).unwrap();
        assert_eq!(stored.claims[0].state, QueueClaimState::Lost);
        assert_eq!(stored.claims[0].lost_from, Some(QueueClaimState::Running));
        assert_eq!(stored.claims[0].generation, claimed.generation);
        assert!(stored.claims[0].frozen.is_some());
        assert!(agent_doc_queue::lost_running_recovery::retry_authorized(
            &stored.claims[0]
        ));
        assert!(
            stored.claims[0]
                .lost_continue
                .as_ref()
                .is_some_and(|record| record.failed
                    && record.during_turn
                    && record.reason.contains("worker is gone"))
        );
        let offered =
            crate::subagent_dispatch::pending_subagent_dispatch_for_content(&file, &content)
                .unwrap();
        assert!(
            offered.iter().any(|head| head.contains(item)),
            "lost harness freeze is retried during a turn, got {offered:?}"
        );
    }
}

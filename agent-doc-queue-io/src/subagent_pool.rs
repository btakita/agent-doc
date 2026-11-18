//! Machine probe and refill projection for the subagent work pool
//! (`#subagentworkpool`). Policy lives in `agent_doc_queue::subagent_pool`.
//!
//! Every refill trigger (`queue report-complete`, `queue release`, a lost
//! attempt, `queue freeze`, and the planning boundaries: preflight, `plan`,
//! mid-turn steering, `queue pool`) renders the same [`PoolRefill`], so the
//! coordinator gets the next head's exact claim / brief / start commands at
//! the moment a slot frees, in the same turn.

use std::num::NonZeroUsize;
use std::path::Path;

use anyhow::Result;

use agent_doc_queue::subagent_pool::{
    FrozenWorker, PoolRefill, PoolRefillTrigger, PoolSize, PoolSizeInputs, RefillHead, RefreshDue,
    ReportOwedWorker, UnstartedDispatch,
};

/// Operator override of the default pool size for a bare `subagents` queue.
pub const POOL_SIZE_ENV: &str = "AGENT_DOC_SUBAGENT_POOL_SIZE";
/// Concurrent heavy-verification slots. This is the seam the build-slot gate
/// (`#buildconcurrencygate`) plugs into: when the gate knows its slot count
/// it exports (or this function reads) it, and the pool default follows.
pub const BUILD_SLOTS_ENV: &str = "AGENT_DOC_BUILD_SLOTS";
/// Legacy stale-heartbeat setting retained for compatibility. Heartbeat age
/// no longer freezes a running worker.
pub const FROZEN_SECS_ENV: &str = "AGENT_DOC_SUBAGENT_FROZEN_SECS";

fn env_nonzero(name: &str) -> Option<NonZeroUsize> {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<NonZeroUsize>().ok())
}

/// Build-slot count from the build gate, when one is configured.
pub fn build_slots_hint() -> Option<NonZeroUsize> {
    env_nonzero(BUILD_SLOTS_ENV)
}

/// The machine inputs the default pool size is derived from.
pub fn pool_size_inputs() -> PoolSizeInputs {
    PoolSizeInputs {
        override_size: env_nonzero(POOL_SIZE_ENV),
        build_slots: build_slots_hint(),
        cores: std::thread::available_parallelism().ok(),
    }
}

/// The default pool size for a bare `subagents` queue on this machine.
pub fn default_pool_size() -> PoolSize {
    agent_doc_queue::subagent_pool::default_pool_size(pool_size_inputs())
}

/// Read the legacy stale-heartbeat setting for API compatibility. Pool
/// classification ignores it; only authoritative stop evidence freezes work.
pub fn frozen_secs() -> u64 {
    std::env::var(FROZEN_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(agent_doc_queue::subagent_pool::DEFAULT_WORKER_FROZEN_SECS)
}

/// The pool refill for `content` at `trigger`, using the claim clock.
pub fn pool_refill_for_content(
    file: &Path,
    content: &str,
    trigger: PoolRefillTrigger,
) -> Result<PoolRefill> {
    pool_refill_for_content_at(
        file,
        content,
        trigger,
        crate::queue_claim::claim_clock_now_secs(),
    )
}

/// [`pool_refill_for_content`] at an explicit clock, for deterministic tests.
pub fn pool_refill_for_content_at(
    file: &Path,
    content: &str,
    trigger: PoolRefillTrigger,
    now_secs: u64,
) -> Result<PoolRefill> {
    let harness = pool_refill_session_harness();
    pool_refill_for_content_at_for_harness(file, content, trigger, now_secs, harness.as_deref())
}

/// Refill to paste into a turn that is already running on `harness`.
///
/// `None` when nothing is unstarted and no free slot has a head. `harness` is
/// the owner pane's harness, so the quota gate does not guess from this process.
pub fn pool_running_turn_refill_for_content(
    file: &Path,
    content: &str,
    harness: Option<&str>,
) -> Result<Option<agent_doc_queue::subagent_pool::PoolRunningTurnRefill>> {
    let refill = pool_refill_for_content_at_for_harness(
        file,
        content,
        PoolRefillTrigger::RunningTurn,
        crate::queue_claim::claim_clock_now_secs(),
        harness,
    )?;
    Ok(refill.running_turn_refill())
}

/// Harness a new worker would run on: this process's coordinator session.
/// `None` outside a known harness, which the refill gate refuses.
fn pool_refill_session_harness() -> Option<String> {
    crate::queue_claim::current_owner_session().and_then(|session| session.harness)
}

/// [`pool_refill_for_content_at`] for an explicit target harness.
fn pool_refill_for_content_at_for_harness(
    file: &Path,
    content: &str,
    trigger: PoolRefillTrigger,
    now_secs: u64,
    harness: Option<&str>,
) -> Result<PoolRefill> {
    // `#lostrunningcontinue`: continue before this refill classifies the slot.
    // A failed continue is what frees the row for retry. The flag records a
    // running-turn refill and does not suppress the attempt.
    let turn_in_progress = trigger == PoolRefillTrigger::RunningTurn;
    if let Err(error) =
        crate::lost_running::recover_lost_running_automatically(file, turn_in_progress)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!("lost_running_recovery_skipped error={error:#} (#lostrunningcontinue)"),
        );
    }
    let document = file.display().to_string();
    let (plan, pools) =
        crate::subagent_dispatch::queue_attr_subagent_plan_and_pools(file, content)?;
    let frozen_bound = frozen_secs();
    let review_blockers = agent_doc_queue::prompt_pressure::review_blocked_prompts(content);
    let unstarted = crate::queue_claim::active_claims_for_content(file, content)?
        .into_iter()
        .filter(|claim| !review_blockers.holds_external(&claim.item_text))
        .filter(|claim| {
            agent_doc_queue::queue_claim::dispatch_evidence_missing(
                claim,
                now_secs,
                agent_doc_queue::queue_claim::DISPATCH_EVIDENCE_GRACE_SECS,
            )
        })
        .map(|claim| {
            let obligation = crate::queue_claim::dispatch_acknowledgement_obligation(file, &claim);
            UnstartedDispatch {
                item: claim.item_text.clone(),
                owner: claim.owner.clone(),
                generation: claim.generation,
                claimed_for_secs: now_secs.saturating_sub(claim.claimed_at_secs),
                brief_command: crate::subagent_dispatch::brief_command(&document, &claim.item_text),
                start_command: obligation.start_command,
                release_command: obligation.release_command,
            }
        })
        .collect::<Vec<_>>();
    let claim_states = crate::queue_claim::claim_states_for_content(file, content)?;
    let refresh_due = claim_states
        .iter()
        .filter(|claim| !review_blockers.holds_external(&claim.item_text))
        .filter(|claim| {
            agent_doc_queue::subagent_pool::heartbeat_due(claim, now_secs, frozen_bound)
        })
        .filter_map(|claim| {
            let last = agent_doc_queue::subagent_pool::last_heartbeat_secs(claim)?;
            let document = crate::queue_claim::shell_single_quote(&document);
            let item = crate::queue_claim::shell_single_quote(
                &agent_doc_queue::queue_claim::claim_item_handle(&claim.item_text),
            );
            Some(RefreshDue {
                item: claim.item_text.clone(),
                owner: claim.owner.clone(),
                generation: claim.generation,
                worker_id: claim
                    .worker
                    .as_ref()
                    .map(|worker| worker.worker_id.clone())
                    .unwrap_or_default(),
                last_heartbeat_secs: last,
                heartbeat_age_secs: now_secs.saturating_sub(last),
                overdue_secs: agent_doc_queue::subagent_pool::heartbeat_overdue_secs(
                    claim, now_secs,
                ),
                expired: matches!(
                    agent_doc_queue::subagent_pool::classify_attempt(
                        claim,
                        now_secs,
                        frozen_bound
                    ),
                    agent_doc_queue::subagent_pool::AttemptSlot::Unconfirmed
                ),
                refresh_command: format!(
                    "agent-doc queue claim {document} --item {item} --owner {} --refresh --generation {}",
                    crate::queue_claim::shell_single_quote(&claim.owner),
                    claim.generation,
                ),
                freeze_command: format!(
                    "agent-doc queue freeze {document} --item {item} --generation {} --reason '<what the harness reported>'",
                    claim.generation,
                ),
            })
        })
        .collect::<Vec<_>>();
    // `#poolquotarefillgate`: `queue claim` refuses a new worker on this
    // harness. Withhold the heads that refusal would reject and keep the reason.
    let quota_refill_block =
        agent_doc_queue::worker_quota::pool_refill_quota_block(&claim_states, harness).map(
            |claim| agent_doc_queue::subagent_pool::QuotaRefillBlock {
                harness: harness.map(agent_doc_queue::worker_quota::canonical_harness_name),
                reason: agent_doc_queue::worker_quota::quota_blocked_next_step(&document, claim),
            },
        );
    // `#finishedheadrefill`: a finished worker's unreported result owes a
    // report for its exact generation, never a new claim.
    let report_owed = claim_states
        .iter()
        .filter(|claim| {
            matches!(
                agent_doc_queue::subagent_pool::classify_attempt(claim, now_secs, frozen_bound),
                agent_doc_queue::subagent_pool::AttemptSlot::ReportOwed
            )
        })
        .map(|claim| {
            let quoted_document = crate::queue_claim::shell_single_quote(&document);
            let item = crate::queue_claim::shell_single_quote(
                &agent_doc_queue::queue_claim::claim_item_handle(&claim.item_text),
            );
            ReportOwedWorker {
                item: claim.item_text.clone(),
                owner: claim.owner.clone(),
                generation: claim.generation,
                worker_id: claim
                    .worker
                    .as_ref()
                    .map(|worker| worker.worker_id.clone())
                    .unwrap_or_default(),
                hand_back_excerpt: claim
                    .ended_hand_back_owed()
                    .unwrap_or_default()
                    .to_string(),
                report_command: format!(
                    "agent-doc queue report-complete {quoted_document} --item {item} --generation {} --response '<complete worker hand-back>'",
                    claim.generation,
                ),
                release_command: format!(
                    "agent-doc queue release {quoted_document} --item {item} --generation {} --requeue",
                    claim.generation,
                ),
            }
        })
        .collect::<Vec<_>>();
    let frozen = claim_states
        .into_iter()
        .filter(|claim| !review_blockers.holds_external(&claim.item_text))
        .filter_map(|claim| {
            let frozen =
                agent_doc_queue::subagent_pool::worker_frozen(&claim, now_secs, frozen_bound)?;
            Some(FrozenWorker {
                worker_id: claim
                    .worker
                    .as_ref()
                    .map(|worker| worker.worker_id.clone())
                    .unwrap_or_default(),
                item: claim.item_text,
                owner: claim.owner,
                generation: claim.generation,
                frozen,
            })
        })
        .collect::<Vec<_>>();
    let next_heads = if quota_refill_block.is_some() {
        Vec::new()
    } else {
        crate::subagent_dispatch::pending_subagent_dispatch_for_content(file, content)?
            .into_iter()
            .map(|item| RefillHead {
                claim_command: crate::subagent_dispatch::claim_command(&document, &item),
                brief_command: crate::subagent_dispatch::brief_command(&document, &item),
                start_command: crate::subagent_dispatch::start_command(&document, &item),
                item,
            })
            .collect::<Vec<_>>()
    };
    let waiting_for_slot = plan.map(|plan| plan.held.len()).unwrap_or_default();
    Ok(PoolRefill {
        trigger,
        pools,
        unstarted,
        refresh_due,
        frozen,
        report_owed,
        next_heads,
        quota_refill_block,
        waiting_for_slot,
    })
}

/// The heartbeat wake `content` calls for (`#poolidleheartbeat`), by the same
/// projection preflight hands the woken coordinator, so a wake never fires for
/// a due set the coordinator would not see. `None` for a document whose queue
/// dispatches no subagents, which is preflight's own gate on that field.
pub fn pool_heartbeat_wake_for_content(
    file: &Path,
    content: &str,
) -> Result<Option<agent_doc_queue::subagent_pool::PoolHeartbeatWake>> {
    if !crate::subagent_dispatch::queue_subagent_dispatch_enabled_for_content(file, content)? {
        return Ok(None);
    }
    let refill = pool_refill_for_content(file, content, PoolRefillTrigger::Plan)?;
    Ok(agent_doc_queue::subagent_pool::pool_heartbeat_wake(
        &refill.refresh_due,
    ))
}

/// The rendered refill for a command's output; a failure is reported as a
/// warning and never fails the command that triggered it.
pub fn render_pool_refill_or_warn(
    file: &Path,
    content: &str,
    trigger: PoolRefillTrigger,
) -> Option<String> {
    match pool_refill_for_content(file, content, trigger) {
        Ok(refill) => {
            if refill.is_actionable() {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "subagent_pool_refill trigger={} unstarted={} refresh_due={} next_heads={} frozen={} report_owed={} waiting={} quota_withheld={} (#subagentworkpool)",
                        trigger.as_str(),
                        refill.unstarted.len(),
                        refill.refresh_due.len(),
                        refill.next_heads.len(),
                        refill.frozen.len(),
                        refill.report_owed.len(),
                        refill.waiting_for_slot,
                        u8::from(refill.quota_refill_block.is_some()),
                    ),
                );
            }
            refill.render()
        }
        Err(err) => Some(format!(
            "[queue] WARNING: could not compute the subagent work-pool refill for {}: {err:#}. Run `agent-doc queue pool {}` before dispatching more work.",
            file.display(),
            crate::queue_claim::shell_single_quote(&file.display().to_string()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool_doc(attr: &str, prompts: &[&str]) -> String {
        let queue: String = prompts.iter().map(|p| format!("- {p}\n")).collect();
        format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue {attr} go -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    fn start(file: &Path, item: &str, owner: &str) {
        let (_, claim) = crate::queue_claim::claim(file, item, owner, 7200).unwrap();
        crate::queue_claim::start_attempt(
            file,
            item,
            owner,
            &format!("worker:{owner}"),
            7200,
            claim.generation,
        )
        .unwrap();
    }

    /// One worker is running and the operator adds two heads. Free slots must
    /// name both as claim/start now (`#subagentworkpool`, infra.md 2026-11-10).
    #[test]
    fn heads_added_while_a_worker_is_running_fill_the_free_slots() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc(
            "subagents=4",
            &["do [#running]", "do [#added-1]", "do [#added-2]"],
        );
        std::fs::write(&file, &content).unwrap();
        let clock = crate::queue_claim::claim_clock_now_secs();
        let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock);
        start(&file, "do [#running]", "subagent:running");

        let refill = pool_running_turn_refill_for_content(&file, &content, Some("grok"))
            .unwrap()
            .expect("free slots take the two new heads");
        assert_eq!(refill.next_heads, 2, "{refill:?}");
        assert_eq!(refill.unstarted, 0);
        assert!(refill.text.contains("do [#added-1]"), "{}", refill.text);
        assert!(refill.text.contains("do [#added-2]"), "{}", refill.text);
        assert!(refill.text.contains("queue claim"), "{}", refill.text);
        assert!(
            refill.text.contains("trigger=running_turn"),
            "{}",
            refill.text
        );
        assert!(
            !refill.text.contains("do [#running]"),
            "the running worker is not offered again: {}",
            refill.text
        );
    }

    /// A frozen worker frees its slot: failed-continue recovery offers that
    /// same head first, and its fresh replacement attempt consumes the slot
    /// (`#subagentworkpool`).
    #[test]
    fn freezing_a_worker_requeues_its_head_and_frees_the_slot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("subagents=2", &["do [#a]", "do [#b]", "do [#c]", "do [#d]"]);
        std::fs::write(&file, &content).unwrap();
        let clock = crate::queue_claim::claim_clock_now_secs();
        let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock);
        start(&file, "do [#a]", "subagent:a");
        start(&file, "do [#b]", "subagent:b");

        let full = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
        assert!(full.next_heads.is_empty(), "{full:#?}");
        assert_eq!(full.waiting_for_slot, 2, "{full:#?}");
        assert_eq!(full.pools[0].occupancy.free, 0);

        let frozen =
            crate::queue_claim::freeze_attempt(&file, "do [#a]", Some(1), "ended without a result")
                .unwrap();
        assert_eq!(
            frozen.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        let refill = pool_refill_for_content(&file, &content, PoolRefillTrigger::Frozen).unwrap();
        assert_eq!(
            refill
                .next_heads
                .iter()
                .map(|head| head.item.as_str())
                .collect::<Vec<_>>(),
            ["do [#a]"],
            "{refill:#?}"
        );
        assert!(
            refill.frozen.is_empty(),
            "failed-continue recovery exposes the same head as retry work: {refill:#?}"
        );
        assert!(
            !crate::queue_claim::claimed_items_for_content(&file, &content).claims("do [#a]"),
            "failed-continue recovery releases the frozen claim before retry"
        );
        let text = refill.render().unwrap();
        assert!(text.contains("trigger=frozen"), "{text}");
        assert!(text.contains("queue claim"), "{text}");
        assert!(text.contains("queue brief"), "{text}");
        assert!(text.contains("queue start"), "{text}");

        start(&file, "do [#a]", "subagent:a-retry");
        let resumed = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
        assert!(resumed.frozen.is_empty(), "{resumed:#?}");
        assert!(
            resumed.next_heads.is_empty(),
            "the heartbeat re-occupies the slot"
        );
    }

    /// `#finishedheadrefill`, the live sequence (agent-doc-bugs.md,
    /// 2026-11-15, three times): a worker finishes, the harness's
    /// `SubagentStop` freezes its generation with the hand-back before the
    /// coordinator runs `queue report-complete`, and the next refill (the
    /// PostToolUse steering one) offered `Refill a free pool slot with queue
    /// item "do [#a]"` for that finished row. The head is report-owed: it
    /// keeps its slot, it is never a refill head, and its guidance is
    /// `queue report-complete` for generation 1. That report still succeeds,
    /// and only then is the slot handed to the next head.
    #[test]
    fn a_finished_worker_awaiting_its_report_is_not_offered_again() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("subagents=2", &["do [#a]", "do [#b]", "do [#c]"]);
        std::fs::write(&file, &content).unwrap();
        let clock = crate::queue_claim::claim_clock_now_secs();
        let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock);
        start(&file, "do [#a]", "subagent:a");
        start(&file, "do [#b]", "subagent:b");

        let reason = agent_doc_queue::queue_claim::subagent_end_freeze_reason(Some(
            "**Summary** fixed the refill. **What changed** branch fix/a, commit 1234abc.",
        ));
        match crate::worker_quota_autodetect::apply_subagent_freeze_autodetect(
            std::slice::from_ref(&file),
            Some(&file),
            "worker:subagent:a",
            &reason,
        )
        .unwrap()
        {
            crate::worker_quota_autodetect::SubagentFreezeAutodetect::Applied(applied) => {
                assert_eq!(applied.claim.generation, 1);
            }
            other => panic!("the subagent-end hook freezes the finished worker: {other:?}"),
        }

        for trigger in [
            PoolRefillTrigger::Plan,
            PoolRefillTrigger::Frozen,
            PoolRefillTrigger::RunningTurn,
        ] {
            let refill = pool_refill_for_content(&file, &content, trigger).unwrap();
            assert!(
                refill.next_heads.iter().all(|head| head.item != "do [#a]"),
                "a finished row is never a refill head: {refill:#?}"
            );
            assert!(refill.next_heads.is_empty(), "{refill:#?}");
            assert_eq!(refill.pools[0].occupancy.report_owed, 1, "{refill:#?}");
            assert_eq!(refill.pools[0].occupancy.running, 1, "{refill:#?}");
            assert_eq!(refill.pools[0].occupancy.free, 0, "{refill:#?}");
            assert!(refill.frozen.is_empty(), "{refill:#?}");
            assert_eq!(refill.waiting_for_slot, 1, "{refill:#?}");
            let owed = refill
                .report_owed
                .iter()
                .map(|entry| (entry.item.as_str(), entry.generation))
                .collect::<Vec<_>>();
            assert_eq!(owed, [("do [#a]", 1)], "{refill:#?}");
            let text = refill.render().unwrap();
            assert!(!text.contains("Refill a free pool slot"), "{text}");
            assert!(text.contains("queue report-complete"), "{text}");
            assert!(text.contains("--generation 1"), "{text}");
            assert!(text.contains("report-owed 1"), "{text}");
        }
        assert!(
            crate::queue_claim::claimed_items_for_content(&file, &content).claims("do [#a]"),
            "the finished attempt keeps its claim for the report"
        );
        let held = crate::queue_claim::claim_states_for_content(&file, &content)
            .unwrap()
            .into_iter()
            .find(|claim| claim.item_text == "do [#a]")
            .unwrap();
        assert_eq!(held.generation, 1);
        assert!(held.lost_continue.is_none(), "{held:#?}");

        let reported = crate::queue_claim::report_attempt(&file, "do [#a]", Some(1)).unwrap();
        assert_eq!(
            reported.state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
        assert_eq!(reported.generation, 1);
        let after =
            pool_refill_for_content(&file, &content, PoolRefillTrigger::ReportComplete).unwrap();
        assert!(after.report_owed.is_empty(), "{after:#?}");
        assert_eq!(
            after
                .next_heads
                .iter()
                .map(|head| head.item.as_str())
                .collect::<Vec<_>>(),
            ["do [#c]"],
            "the report frees the slot for the next head, not the finished one: {after:#?}"
        );
    }

    /// A bound worker keeps its slot indefinitely. Heartbeat age alone cannot
    /// authorize a duplicate dispatch or lose the original tracking.
    #[test]
    fn a_long_running_worker_keeps_its_slot_without_a_timeout() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("subagents=1", &["do [#a]", "do [#b]"]);
        std::fs::write(&file, &content).unwrap();
        let clock = crate::queue_claim::claim_clock_now_secs();
        {
            let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock);
            start(&file, "do [#a]", "subagent:a");
            let fresh = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
            assert!(fresh.next_heads.is_empty(), "{fresh:#?}");
        }
        let stale = clock + agent_doc_queue::subagent_pool::DEFAULT_WORKER_FROZEN_SECS * 10;
        let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(stale);
        let refill = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
        assert!(
            refill.frozen.is_empty(),
            "elapsed time is not stop evidence: {refill:#?}"
        );
        assert_eq!(
            refill
                .next_heads
                .iter()
                .map(|head| head.item.as_str())
                .collect::<Vec<_>>(),
            Vec::<&str>::new(),
            "a long turn must not offer either the running row or its sibling: {refill:#?}"
        );
        let claim = crate::queue_claim::claim_states_for_content(&file, &content)
            .unwrap()
            .into_iter()
            .find(|claim| claim.item_text == "do [#a]")
            .expect("retried attempt");
        assert_eq!(
            claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert!(claim.lost_continue.is_none(), "{claim:#?}");
        assert_eq!(refill.pools[0].occupancy.running, 1, "{refill:#?}");
        assert_eq!(refill.pools[0].occupancy.free, 0, "{refill:#?}");
    }

    /// `#subagentworkpool`, the Claude Code miss (2026-11-05): a working
    /// worker's reservation window elapses because nothing refreshed it. A
    /// heartbeat is due well before then. The bound attempt remains Running,
    /// holds its slot, is never re-offered, and a later refresh keeps that
    /// generation.
    #[test]
    fn an_expired_bound_attempt_is_refreshed_not_redispatched() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("subagents=1", &["do [#a]", "do [#b]"]);
        std::fs::write(&file, &content).unwrap();
        let clock = crate::queue_claim::claim_clock_now_secs();
        {
            let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock);
            start(&file, "do [#a]", "subagent:a");
        }
        {
            let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock + 3_600);
            let due = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
            assert_eq!(due.refresh_due.len(), 1, "{due:#?}");
            assert!(
                !due.refresh_due[0].expired,
                "due well before expiry: {due:#?}"
            );
            assert!(due.is_actionable(), "a due heartbeat reaches steering");
        }
        let expired = clock + 7_300;
        let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(expired);
        let refill = pool_refill_for_content(&file, &content, PoolRefillTrigger::Lost).unwrap();
        assert!(
            refill.next_heads.is_empty(),
            "a long-running worker is tracked, not offered: {refill:#?}"
        );
        assert!(
            refill.frozen.is_empty(),
            "expiry alone is not frozen: {refill:#?}"
        );
        assert_eq!(refill.pools[0].occupancy.running, 1, "{refill:#?}");
        assert_eq!(refill.pools[0].occupancy.unconfirmed, 0, "{refill:#?}");
        assert_eq!(refill.pools[0].occupancy.free, 0, "{refill:#?}");
        let continued = crate::queue_claim::claim_states_for_content(&file, &content)
            .unwrap()
            .into_iter()
            .find(|claim| claim.item_text == "do [#a]")
            .expect("continued attempt");
        assert_eq!(
            continued.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert_eq!(continued.generation, 1);
        assert!(continued.lost_continue.is_none(), "{continued:#?}");
        assert!(
            refill
                .refresh_due
                .iter()
                .any(|due| due.item == "do [#a]" && !due.expired),
            "an elapsed reminder stays visible without changing lifecycle: {refill:#?}"
        );
        assert!(
            crate::subagent_dispatch::pending_subagent_dispatch_for_content(&file, &content)
                .unwrap()
                .is_empty()
        );

        let revived =
            crate::queue_claim::refresh_attempt(&file, "do [#a]", "subagent:a", 7200, Some(1))
                .unwrap();
        assert_eq!(
            revived.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert_eq!(
            revived.generation, 1,
            "the same attempt, not a new generation"
        );
        assert_eq!(
            revived
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str()),
            Some("worker:subagent:a")
        );
        let live = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
        assert_eq!(live.pools[0].occupancy.running, 1, "{live:#?}");
        assert!(!live.is_actionable(), "{live:#?}");
    }

    /// A stopped harness freeze with no handback initially stays `Running`.
    /// The refill immediately records failed continue, frees the slot, and
    /// offers the row for a fresh generation (`#lostrunningcontinue`).
    #[test]
    fn a_stopped_harness_freeze_is_offered_as_a_fresh_generation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("subagents=1", &["do [#a]", "do [#b]"]);
        std::fs::write(&file, &content).unwrap();
        let clock = crate::queue_claim::claim_clock_now_secs();
        {
            let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock);
            start(&file, "do [#a]", "subagent:a");
            crate::queue_claim::freeze_attempt(&file, "do [#a]", Some(1), "session restarted")
                .unwrap();
        }
        let expired = clock + 7_300;
        let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(expired);
        let states = crate::queue_claim::claim_states_for_content(&file, &content).unwrap();
        let frozen_claim = states
            .iter()
            .find(|claim| claim.item_text == "do [#a]")
            .expect("frozen attempt");
        assert_eq!(
            frozen_claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running,
            "expiry must not turn a harness freeze into lost"
        );
        assert!(frozen_claim.frozen.is_some());
        assert!(frozen_claim.blocks_dispatch(expired));
        let refill = pool_refill_for_content(&file, &content, PoolRefillTrigger::Frozen).unwrap();
        assert_eq!(
            refill
                .next_heads
                .iter()
                .map(|head| head.item.as_str())
                .collect::<Vec<_>>(),
            ["do [#a]"],
            "a stopped freeze is offered for a new generation: {refill:#?}"
        );
        assert!(
            refill.frozen.is_empty(),
            "retry frees the slot: {refill:#?}"
        );
        assert_eq!(refill.pools[0].occupancy.free, 1, "{refill:#?}");
        let offered = crate::queue_claim::claim_states_for_content(&file, &content)
            .unwrap()
            .into_iter()
            .find(|claim| claim.item_text == "do [#a]")
            .expect("stopped freeze");
        assert_eq!(
            offered.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running,
            "continue failure must not mark the freeze Lost"
        );
        assert!(offered.lost_from.is_none());
        assert_eq!(offered.generation, 1);
        assert!(agent_doc_queue::lost_running_recovery::retry_authorized(
            &offered
        ));
        assert!(!offered.blocks_dispatch(expired));
        let text = refill.render().unwrap();
        assert!(text.contains("queue claim"), "{text}");
        assert!(text.contains("queue brief"), "{text}");
        assert!(text.contains("queue start"), "{text}");

        let (_, replacement) =
            crate::queue_claim::claim(&file, "do [#a]", "subagent:a-retry", 7200).unwrap();
        assert_eq!(
            replacement.generation, 2,
            "fresh refill claim: {replacement:#?}"
        );
        assert_eq!(replacement.owner, "subagent:a-retry");
        crate::queue_claim::start_attempt(
            &file,
            "do [#a]",
            "subagent:a-retry",
            "worker:subagent:a-retry",
            7200,
            replacement.generation,
        )
        .unwrap();
        let held = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
        assert!(
            held.next_heads.iter().all(|head| head.item != "do [#a]"),
            "the fresh retry occupies the slot: {held:#?}"
        );
        assert_eq!(held.pools[0].occupancy.running, 1, "{held:#?}");
    }

    /// `#waitingfreezelive` (2026-11-13): the hook froze a worker that was
    /// only waiting on its own background `make tmux-ci`, and the pool offered
    /// its row for a duplicate dispatch (since `#finishedheadrefill` the
    /// recorded hand-back makes that head report-owed instead). Proof of life, whether the
    /// coordinator's generation-fenced `queue claim --refresh` or the hook's
    /// next waiting stop, clears that freeze and its failed continue, puts the
    /// worker back in its slot, and withdraws the offer.
    #[test]
    fn proof_of_life_after_a_waiting_freeze_withdraws_the_duplicate_offer() {
        for via_coordinator_refresh in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
            let file = dir.path().join("task.md");
            let content = pool_doc("subagents=1", &["do [#a]", "do [#b]"]);
            std::fs::write(&file, &content).unwrap();
            start(&file, "do [#a]", "subagent:a");
            crate::queue_claim::freeze_attempt(
                &file,
                "do [#a]",
                Some(1),
                "subagent ended before coordinator integration: The commit is in. Waiting on `make tmux-ci` before I push.",
            )
            .unwrap();
            // `#finishedheadrefill`: the stop recorded a hand-back, so the
            // head is report-owed and no longer offered for a duplicate
            // dispatch even before proof of life arrives.
            let owed = pool_refill_for_content(&file, &content, PoolRefillTrigger::Frozen).unwrap();
            assert!(
                owed.next_heads.iter().all(|head| head.item != "do [#a]"),
                "{owed:#?}"
            );
            assert_eq!(owed.report_owed.len(), 1, "{owed:#?}");

            let claim = if via_coordinator_refresh {
                crate::queue_claim::refresh_attempt(&file, "do [#a]", "subagent:a", 7200, Some(1))
                    .unwrap()
            } else {
                let (claim, cleared) = crate::queue_claim::record_bound_worker_proof_of_life(
                    &file,
                    "do [#a]",
                    Some(1),
                    "worker:subagent:a",
                )
                .unwrap();
                assert!(cleared);
                claim
            };
            assert!(claim.frozen.is_none(), "{claim:#?}");
            assert!(claim.lost_continue.is_none(), "{claim:#?}");
            assert_eq!(claim.generation, 1);
            assert_eq!(
                claim
                    .worker
                    .as_ref()
                    .map(|worker| worker.worker_id.as_str()),
                Some("worker:subagent:a")
            );

            let held = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
            assert!(
                held.next_heads.iter().all(|head| head.item != "do [#a]"),
                "via_coordinator_refresh={via_coordinator_refresh}: {held:#?}"
            );
            assert!(held.frozen.is_empty(), "{held:#?}");
            assert_eq!(held.pools[0].occupancy.running, 1, "{held:#?}");
        }
    }

    /// `#poolidleheartbeat`, through the real ledger: nothing before the batch
    /// has had its coalesce window, one wake identity while the due set stands,
    /// and nothing once the coordinator refreshed. Elapsed time leaves the
    /// same due set and never changes worker lifecycle.
    #[test]
    fn a_heartbeat_wake_follows_the_real_due_set() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("subagents=2", &["do [#a]", "do [#b]"]);
        std::fs::write(&file, &content).unwrap();
        let clock = crate::queue_claim::claim_clock_now_secs();
        let wake_at = |at: u64| {
            let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(at);
            pool_heartbeat_wake_for_content(&file, &content).unwrap()
        };
        {
            let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock);
            start(&file, "do [#a]", "subagent:a");
            start(&file, "do [#b]", "subagent:b");
        }
        let coalesce = agent_doc_queue::subagent_pool::POOL_HEARTBEAT_WAKE_COALESCE_SECS;
        assert_eq!(wake_at(clock), None, "a fresh worker");
        assert_eq!(wake_at(clock + 3_600), None, "due, holding for siblings");

        let due = wake_at(clock + 3_600 + coalesce).expect("the batch wakes");
        assert_eq!((due.due, due.expired), (2, 0));
        assert_eq!(
            wake_at(clock + 3_600 + coalesce + 60).map(|wake| wake.fingerprint),
            Some(due.fingerprint.clone()),
            "a later tick over the same set is the same wake"
        );

        assert_eq!(
            wake_at(clock + 7_300).map(|wake| wake.fingerprint),
            Some(due.fingerprint),
            "elapsed time keeps the same due set"
        );
        let continued = crate::queue_claim::claim_states_for_content(&file, &content).unwrap();
        assert_eq!(continued.len(), 2, "{continued:#?}");
        assert!(continued.iter().all(|claim| {
            claim.state == agent_doc_queue::queue_claim::QueueClaimState::Running
                && claim.generation == 1
                && claim.lost_continue.is_none()
        }));

        let refreshed = clock + 7_300;
        {
            let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(refreshed);
            for (item, owner) in [("do [#a]", "subagent:a"), ("do [#b]", "subagent:b")] {
                crate::queue_claim::refresh_attempt(&file, item, owner, 7200, Some(1)).unwrap();
            }
        }
        assert_eq!(wake_at(refreshed + 60), None, "a refresh answers the wake");
    }

    /// A queue that dispatches no subagents has no preflight pool field, so
    /// waking its coordinator could hand it nothing.
    #[test]
    fn a_queue_without_subagents_never_wakes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("auto", &["do [#a]"]);
        std::fs::write(&file, &content).unwrap();
        let clock = crate::queue_claim::claim_clock_now_secs();
        {
            let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock);
            start(&file, "do [#a]", "subagent:a");
        }
        let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(clock + 7_300);
        assert_eq!(
            pool_heartbeat_wake_for_content(&file, &content).unwrap(),
            None
        );
    }

    /// An attempt abandoned with `release --requeue` is evidence the worker no
    /// longer owns it: the slot frees and a late heartbeat cannot revive it.
    #[test]
    fn an_abandoned_attempt_frees_its_slot_and_stays_lost() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("subagents=1", &["do [#a]", "do [#b]"]);
        std::fs::write(&file, &content).unwrap();
        start(&file, "do [#a]", "subagent:a");
        crate::queue_claim::abandon_attempt(&file, "do [#a]", Some(1)).unwrap();

        let refill = pool_refill_for_content(&file, &content, PoolRefillTrigger::Release).unwrap();
        assert_eq!(refill.pools[0].occupancy.free, 1, "{refill:#?}");
        assert!(refill.frozen.is_empty(), "{refill:#?}");
        assert!(
            crate::queue_claim::refresh_attempt(&file, "do [#a]", "subagent:a", 7200, Some(1))
                .is_err(),
            "an abandoned attempt is never revived"
        );
    }

    /// `release --requeue` of a started worker stays `🚧`. The head can be
    /// claimed once. After that replacement is running, a refill and another
    /// claim do not bind a second generation. `subagents=2` and a sibling head
    /// keep capacity from hiding the duplicate.
    #[test]
    fn authorized_running_requeue_is_not_painted_lost() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("subagents=2", &["do [#a]", "do [#b]"]);
        std::fs::write(&file, &content).unwrap();
        start(&file, "do [#a]", "subagent:a");
        crate::queue_claim::abandon_attempt(&file, "do [#a]", Some(1)).unwrap();

        let abandoned = crate::queue_claim::load_ledger(&file)
            .unwrap()
            .claims
            .into_iter()
            .find(|claim| claim.item_text.contains("#a"))
            .expect("the started attempt stays on the ledger");
        assert_eq!(
            abandoned.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert!(abandoned.lost_from.is_none());
        assert_ne!(
            abandoned.projection_marker(),
            agent_doc_document::queue_projection::QueueLifecycleMarker::LostRunning
        );
        assert!(
            abandoned
                .frozen
                .as_ref()
                .is_some_and(|freeze| freeze.is_abandon())
        );
        assert_eq!(
            abandoned
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str()),
            Some("worker:subagent:a")
        );

        let offered =
            crate::subagent_dispatch::pending_subagent_dispatch_for_content(&file, &content)
                .unwrap();
        assert!(
            offered.iter().any(|head| head.contains("#a")),
            "the requeued head is recoverable, got {offered:?}"
        );
        let continued = crate::queue_claim::load_ledger(&file)
            .unwrap()
            .claims
            .into_iter()
            .find(|claim| claim.item_text.contains("#a"))
            .expect("continue records the abandon on the same attempt");
        assert_eq!(
            continued.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert!(continued.lost_continue.as_ref().is_some_and(|record| {
            record.failed && record.reason.contains("abandoned for requeue")
        }));

        let (outcome, next) =
            crate::queue_claim::claim(&file, "do [#a]", "subagent:a2", 7200).unwrap();
        assert_eq!(
            outcome,
            agent_doc_queue::queue_claim::ClaimOutcome::Reassigned
        );
        assert_eq!(next.generation, 2);
        assert_eq!(
            next.state,
            agent_doc_queue::queue_claim::QueueClaimState::Claimed
        );
        assert!(next.worker.is_none());
        crate::queue_claim::start_attempt(
            &file,
            "do [#a]",
            "subagent:a2",
            "worker:a2",
            7200,
            next.generation,
        )
        .unwrap();

        let offered =
            crate::subagent_dispatch::pending_subagent_dispatch_for_content(&file, &content)
                .unwrap();
        assert!(
            !offered.iter().any(|head| head.contains("#a")),
            "a live replacement is not offered again, got {offered:?}"
        );
        let again = crate::queue_claim::claim(&file, "do [#a]", "subagent:a3", 7200);
        let err = again.expect_err("a live replacement is not claimed again");
        assert!(
            err.to_string()
                .contains("already held by subagent:a2 generation 2"),
            "{err:#}"
        );
        let held = crate::queue_claim::load_ledger(&file)
            .unwrap()
            .claims
            .into_iter()
            .find(|claim| claim.item_text.contains("#a"))
            .expect("the replacement generation is the only live attempt");
        assert_eq!(held.generation, 2);
        assert_eq!(
            held.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
    }

    fn block_on_quota(file: &Path, harness: &str) {
        crate::queue_claim::block_attempt_on_quota(
            file,
            "do [#a]",
            Some(1),
            agent_doc_queue::worker_quota::WorkerQuotaBlock {
                harness: Some(harness.to_string()),
                limit: "session limit".to_string(),
                evidence: "You've hit your session limit · resets 9pm".to_string(),
                reset_hint: Some("resets 9pm".to_string()),
                worktree: Some("/work/tree".to_string()),
                branch: Some("fix/partial".to_string()),
                blocked_at_secs: 10,
            },
        )
        .unwrap();
    }

    fn refill_for(file: &Path, content: &str, harness: Option<&str>) -> PoolRefill {
        pool_refill_for_content_at_for_harness(
            file,
            content,
            PoolRefillTrigger::ReportComplete,
            crate::queue_claim::claim_clock_now_secs(),
            harness,
        )
        .unwrap()
    }

    /// `#poolquotarefillgate`: a freed slot is not refilled onto the harness
    /// that just ran out. Claim refuses that harness; the pool must not list
    /// the head, and it must say why.
    #[test]
    fn a_quota_block_withholds_next_heads_and_reports_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = pool_doc("subagents=2", &["do [#a]", "do [#b]", "do [#c]"]);
        std::fs::write(&file, &content).unwrap();
        start(&file, "do [#a]", "subagent:a");
        block_on_quota(&file, "claude-code");

        let refused = refill_for(&file, &content, Some("claude"));
        assert!(refused.next_heads.is_empty(), "{refused:#?}");
        let block = refused.quota_refill_block.as_ref().expect("reason");
        assert_eq!(block.harness.as_deref(), Some("claude-code"));
        assert!(
            block.reason.contains("session limit of claude-code"),
            "{block:?}"
        );
        assert!(
            block
                .reason
                .contains("do not start another worker on claude-code"),
            "{block:?}"
        );
        let text = refused.render().unwrap();
        assert!(
            text.contains("next heads withheld (worker_quota_exhausted on claude-code)"),
            "{text}"
        );
        assert!(text.contains("no refill head is offered"), "{text}");
        assert!(!text.contains("do [#b]"), "{text}");
        assert!(refused.is_actionable(), "the reason has to reach steering");

        let other = refill_for(&file, &content, Some("codex"));
        assert_eq!(
            other
                .next_heads
                .iter()
                .map(|head| head.item.as_str())
                .collect::<Vec<_>>(),
            ["do [#b]", "do [#c]"],
            "{other:#?}"
        );
        assert!(other.quota_refill_block.is_none(), "{other:#?}");

        let unknown = refill_for(&file, &content, None);
        assert!(
            unknown.next_heads.is_empty(),
            "unknown harness fails closed: {unknown:#?}"
        );
        assert_eq!(
            unknown
                .quota_refill_block
                .as_ref()
                .map(|block| block.harness.as_deref()),
            Some(None),
        );

        let claude_claim = crate::queue_claim::claim_with_session(
            &file,
            "do [#b]",
            "subagent:b",
            7200,
            Some(agent_doc_queue::queue_claim::ClaimOwnerSession {
                harness: Some("claude".to_string()),
                session_id: Some("coordinator".to_string()),
                pane: None,
                pid: None,
            }),
        );
        let err = claude_claim.unwrap_err().to_string();
        assert!(err.contains("worker_quota_exhausted"), "{err}");
        assert!(
            err.contains("refusing to start a new worker on claude"),
            "{err}"
        );

        crate::queue_claim::start_attempt(
            &file,
            "do [#a]",
            "subagent:a",
            "worker:subagent:a",
            7200,
            1,
        )
        .unwrap();
        let resumed = refill_for(&file, &content, Some("claude"));
        assert!(resumed.quota_refill_block.is_none(), "{resumed:#?}");
        assert_eq!(
            resumed
                .next_heads
                .iter()
                .map(|head| head.item.as_str())
                .collect::<Vec<_>>(),
            ["do [#b]"],
            "the resumed worker holds one slot: {resumed:#?}"
        );
    }
}

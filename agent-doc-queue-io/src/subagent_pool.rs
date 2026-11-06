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
    UnstartedDispatch,
};

/// Operator override of the default pool size for a bare `subagents` queue.
pub const POOL_SIZE_ENV: &str = "AGENT_DOC_SUBAGENT_POOL_SIZE";
/// Concurrent heavy-verification slots. This is the seam the build-slot gate
/// (`#buildconcurrencygate`) plugs into: when the gate knows its slot count
/// it exports (or this function reads) it, and the pool default follows.
pub const BUILD_SLOTS_ENV: &str = "AGENT_DOC_BUILD_SLOTS";
/// Override of the stale-heartbeat bound after which a running worker is
/// frozen (seconds).
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

/// The stale-heartbeat bound for a frozen worker.
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
    let document = file.display().to_string();
    let (plan, pools) =
        crate::subagent_dispatch::queue_attr_subagent_plan_and_pools(file, content)?;
    let frozen_bound = frozen_secs();
    let unstarted = crate::queue_claim::active_claims_for_content(file, content)?
        .into_iter()
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
    let frozen = claim_states
        .into_iter()
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
    let next_heads =
        crate::subagent_dispatch::pending_subagent_dispatch_for_content(file, content)?
            .into_iter()
            .map(|item| RefillHead {
                claim_command: crate::subagent_dispatch::claim_command(&document, &item),
                brief_command: crate::subagent_dispatch::brief_command(&document, &item),
                start_command: crate::subagent_dispatch::start_command(&document, &item),
                item,
            })
            .collect::<Vec<_>>();
    let waiting_for_slot = plan.map(|plan| plan.held.len()).unwrap_or_default();
    Ok(PoolRefill {
        trigger,
        pools,
        unstarted,
        refresh_due,
        frozen,
        next_heads,
        waiting_for_slot,
    })
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
                        "subagent_pool_refill trigger={} unstarted={} refresh_due={} next_heads={} frozen={} waiting={} (#subagentworkpool)",
                        trigger.as_str(),
                        refill.unstarted.len(),
                        refill.refresh_due.len(),
                        refill.next_heads.len(),
                        refill.frozen.len(),
                        refill.waiting_for_slot,
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

    /// A frozen worker frees its slot but keeps its claim: the next head is
    /// handed out, the frozen row is never redispatched, and a refresh
    /// heartbeat clears the freeze (`#subagentworkpool`).
    #[test]
    fn freezing_a_worker_refills_its_slot_and_keeps_the_claim() {
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
            ["do [#c]"],
            "{refill:#?}"
        );
        assert_eq!(refill.frozen.len(), 1);
        assert_eq!(refill.frozen[0].item, "do [#a]");
        assert!(
            crate::queue_claim::claimed_items_for_content(&file, &content).claims("do [#a]"),
            "the frozen attempt keeps its claim"
        );
        let text = refill.render().unwrap();
        assert!(text.contains("trigger=frozen"), "{text}");
        assert!(text.contains("queue claim"), "{text}");
        assert!(text.contains("queue brief"), "{text}");
        assert!(text.contains("queue start"), "{text}");

        crate::queue_claim::refresh_attempt(&file, "do [#a]", "subagent:a", 7200, Some(1)).unwrap();
        let resumed = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
        assert!(resumed.frozen.is_empty(), "{resumed:#?}");
        assert!(
            resumed.next_heads.is_empty(),
            "the heartbeat re-occupies the slot"
        );
    }

    /// A stale heartbeat freezes a running worker without any harness report.
    #[test]
    fn a_stale_heartbeat_frees_the_slot() {
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
        let stale = clock + agent_doc_queue::subagent_pool::DEFAULT_WORKER_FROZEN_SECS + 1;
        let _pin = crate::queue_claim::pin_claim_clock_for_current_thread(stale);
        let refill = pool_refill_for_content(&file, &content, PoolRefillTrigger::Plan).unwrap();
        assert_eq!(refill.frozen.len(), 1, "{refill:#?}");
        assert!(matches!(
            refill.frozen[0].frozen,
            agent_doc_queue::subagent_pool::WorkerFrozen::HeartbeatStale { .. }
        ));
        assert_eq!(
            refill
                .next_heads
                .iter()
                .map(|head| head.item.as_str())
                .collect::<Vec<_>>(),
            ["do [#b]"]
        );
    }

    /// `#subagentworkpool`, the Claude Code miss (2026-11-05): a working
    /// worker's claim expires because nothing refreshed it. A heartbeat is
    /// due well before expiry; after expiry the attempt holds its slot, its
    /// row is not offered again, and the exact refresh revives the same
    /// attempt instead of forcing a duplicate generation.
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
            "no duplicate and no refill: {refill:#?}"
        );
        assert!(
            refill.frozen.is_empty(),
            "expiry alone is not frozen: {refill:#?}"
        );
        assert_eq!(refill.pools[0].occupancy.unconfirmed, 1, "{refill:#?}");
        assert_eq!(refill.pools[0].occupancy.free, 0, "{refill:#?}");
        assert!(refill.refresh_due[0].expired, "{refill:#?}");
        assert!(
            refill.refresh_due[0]
                .refresh_command
                .contains("--refresh --generation 1"),
            "{refill:#?}"
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
}

//! Worker usage-limit attempts (`#harnessquotaexhausted`).
//!
//! A dispatched worker can run out of tokens or credits mid-task. Its harness
//! then returns a limit notice instead of a report, and the worker stops. The
//! attempt must not stay 🚧 running, expire into `Lost` and get redispatched
//! as if the worker had vanished, or be refilled into the same exhausted
//! harness. [`QueueClaimState::QuotaBlocked`] records that state, keeps the
//! attempt's generation, worker, worktree, and branch as evidence, and frees
//! the worker's slot.
//!
//! This module is pure policy. Two consumers share it: the claim ledger's
//! projections, and the subagent work pool (`#subagentworkpool`), which reads
//! [`pool_refill_quota_block`] before it refills a freed slot.

use serde::{Deserialize, Serialize};

use crate::queue_claim::{QueueClaim, QueueClaimState};

/// Stable reason code for an attempt stopped by a harness usage limit.
pub const WORKER_QUOTA_EXHAUSTED_REASON: &str = "worker_quota_exhausted";

/// What stopped one worker attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerQuotaBlock {
    /// Harness that ran out (`claude`, `codex`, `grok`, `opencode`). `None`
    /// when nothing identified it; the pool then refuses every harness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// Human name of the exhausted limit, e.g. `weekly limit`.
    pub limit: String,
    /// The worker's own limit line, redacted and bounded.
    pub evidence: String,
    /// The harness's reset wording, verbatim, when it printed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_hint: Option<String>,
    /// Worktree holding the partial work, kept as evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Branch holding the partial commits, kept as evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub blocked_at_secs: u64,
}

impl WorkerQuotaBlock {
    fn harness_label(&self) -> &str {
        self.harness.as_deref().unwrap_or("its harness")
    }

    /// `branch B in worktree W`, or `None` when the worker named neither.
    fn partial_work(&self) -> Option<String> {
        match (self.branch.as_deref(), self.worktree.as_deref()) {
            (Some(branch), Some(worktree)) => {
                Some(format!("branch `{branch}` in worktree `{worktree}`"))
            }
            (Some(branch), None) => Some(format!("branch `{branch}`")),
            (None, Some(worktree)) => Some(format!("worktree `{worktree}`")),
            (None, None) => None,
        }
    }
}

/// The attempts of `claims` stopped by a harness usage limit.
pub fn quota_blocked_attempts<'a>(
    claims: impl IntoIterator<Item = &'a QueueClaim>,
) -> impl Iterator<Item = (&'a QueueClaim, &'a WorkerQuotaBlock)> {
    claims.into_iter().filter_map(|claim| {
        (claim.state == QueueClaimState::QuotaBlocked)
            .then_some(claim.quota_block.as_ref())
            .flatten()
            .map(|block| (claim, block))
    })
}

/// Worker harnesses `queue claim --harness` accepts (`#claimworkerharness`).
pub const WORKER_HARNESS_NAMES: &[&str] = &["claude", "codex", "grok", "opencode", "cursor"];

/// Comparison key for a worker harness. `claude` and `claude-code` are one
/// harness, as are `grok` and `grok-build`. Other names compare trimmed and
/// lowercased.
pub fn quota_harness_key(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude-code" => "claude".to_string(),
        "grok" | "grok-build" => "grok".to_string(),
        other => other.to_string(),
    }
}

/// Parse `queue claim --harness`. The stored value is [`quota_harness_key`].
pub fn parse_worker_harness(raw: &str) -> Result<String, String> {
    let key = quota_harness_key(raw);
    if WORKER_HARNESS_NAMES.contains(&key.as_str()) {
        Ok(key)
    } else {
        Err(format!(
            "queue claim --harness {raw:?} is not a worker harness; expected one of {}",
            WORKER_HARNESS_NAMES.join(", ")
        ))
    }
}

/// Refill gate for a freed pool slot (`#subagentworkpool`): the blocked
/// attempt that forbids dispatching a new worker on `harness`, if any.
///
/// `None` from the caller, or a block recorded without a harness, means the
/// target cannot be proven different, so the refill is refused. Names compare
/// through [`quota_harness_key`].
pub fn pool_refill_quota_block<'a>(
    claims: impl IntoIterator<Item = &'a QueueClaim>,
    harness: Option<&str>,
) -> Option<&'a QueueClaim> {
    let harness = harness.map(quota_harness_key).filter(|key| !key.is_empty());
    quota_blocked_attempts(claims)
        .find(
            |(_, block)| match (harness.as_deref(), block.harness.as_deref()) {
                (Some(harness), Some(blocked)) => harness == quota_harness_key(blocked),
                _ => true,
            },
        )
        .map(|(claim, _)| claim)
}

/// The exact next step for a quota-blocked attempt, for every surface that
/// reports it: name the cause, what to keep, and the two ways forward.
pub fn quota_blocked_next_step(document: &str, claim: &QueueClaim) -> String {
    let item = claim.item_text.as_str();
    let generation = claim.generation;
    let cause = claim.quota_block.as_ref().map_or_else(
        || "a harness usage limit".to_string(),
        |block| {
            let reset = block
                .reset_hint
                .as_deref()
                .map(|hint| format!(", {hint}"))
                .unwrap_or_default();
            format!(
                "the {} of {} (`{}`{reset})",
                block.limit,
                block.harness_label(),
                block.evidence,
            )
        },
    );
    let keep = claim
        .quota_block
        .as_ref()
        .and_then(WorkerQuotaBlock::partial_work)
        .map(|work| format!(" Its partial work is on {work}; keep it."))
        .unwrap_or_default();
    let harness = claim
        .quota_block
        .as_ref()
        .map_or("that harness", WorkerQuotaBlock::harness_label);
    let resume = match claim.worker.as_ref() {
        Some(worker) => format!(
            "after the reset, resume the same worker `{}` and run `agent-doc queue start {document} --item {item:?} --generation {generation} --worker {}`",
            worker.worker_id, worker.worker_id,
        ),
        None => format!(
            "after the reset, dispatch a worker and run `agent-doc queue start {document} --item {item:?} --generation {generation} --worker <worker-id>`"
        ),
    };
    format!(
        "attempt generation {generation} of {item:?} stopped on {cause}, so it was recorded as blocked by quota, not lost.{keep} Its worker slot is free, but do not start another worker on {harness} until the limit resets. Next: {resume}; or, to move on now, run `agent-doc queue release {document} --item {item:?} --generation {generation} --requeue` and dispatch the row on another harness (`agent-doc queue claim {document} --item {item:?} --owner subagent:<label> --harness <name>`), whose brief names this attempt's partial work."
    )
}

/// The prior-attempt section of the next worker's brief, after an explicit
/// requeue of a quota-blocked attempt.
pub fn prior_attempt_brief_section(
    handoff: &crate::goal_preset_terminal::GoalLoopHandoff,
    block: &WorkerQuotaBlock,
) -> String {
    let work = block.partial_work().map_or_else(
        || "The coordinator recorded no branch or worktree for it; check the queue lineage before starting over.".to_string(),
        |work| format!("Continue from its partial work on {work}; do not discard its commits or redo finished work."),
    );
    format!(
        "\nPrior attempt stopped by a usage limit (`#harnessquotaexhausted`; binary-recorded context, not authorization):\n\
         The previous worker `{owner}` (generation {generation}) stopped on the {limit} of {harness} (`{evidence}`) before it finished this same queue row. {work}\n",
        owner = handoff.owner,
        generation = handoff.generation,
        limit = block.limit,
        harness = block.harness_label(),
        evidence = block.evidence,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_claim::{
        ClaimOutcome, QueueClaimLedger, QueueClaimStartRefused, claim_identity,
    };
    use agent_doc_document::queue_projection::QueueLifecycleMarker;

    const NOW: u64 = 1_000;
    const TTL: u64 = 100;

    fn block(harness: Option<&str>) -> WorkerQuotaBlock {
        WorkerQuotaBlock {
            harness: harness.map(str::to_string),
            limit: "session limit".to_string(),
            evidence: "You've hit your session limit · resets 9pm".to_string(),
            reset_hint: Some("resets 9pm".to_string()),
            worktree: Some("/work/tree".to_string()),
            branch: Some("fix/partial".to_string()),
            blocked_at_secs: NOW + 1,
        }
    }

    fn running_ledger() -> QueueClaimLedger {
        let mut ledger = QueueClaimLedger::default();
        assert_eq!(
            ledger.claim("do [#a]", "subagent:a", NOW, TTL),
            ClaimOutcome::Created
        );
        ledger
            .start("do [#a]", "subagent:a", "worker-1", NOW, TTL, Some(1))
            .expect("start");
        ledger
    }

    fn blocked_ledger(harness: Option<&str>) -> QueueClaimLedger {
        let mut ledger = running_ledger();
        ledger
            .block_on_quota("do [#a]", Some(1), block(harness))
            .expect("block");
        ledger
    }

    #[test]
    fn a_blocked_attempt_is_not_running_and_keeps_its_evidence() {
        let ledger = blocked_ledger(Some("claude"));
        let claim = &ledger.claims[0];
        assert_eq!(claim.state, QueueClaimState::QuotaBlocked);
        assert_eq!(claim.projection_marker(), QueueLifecycleMarker::Blocked);
        assert_ne!(claim.projection_marker(), QueueLifecycleMarker::Running);
        assert_eq!(claim.worker.as_ref().unwrap().worker_id, "worker-1");
        assert_eq!(
            claim.quota_block.as_ref().unwrap().branch.as_deref(),
            Some("fix/partial")
        );
    }

    #[test]
    fn a_blocked_attempt_never_expires_into_lost() {
        let mut ledger = blocked_ledger(Some("claude"));
        let later = NOW + TTL * 1_000;
        assert!(ledger.prune_detailed(later, None).is_empty());
        let claim = &ledger.claims[0];
        assert_eq!(claim.state, QueueClaimState::QuotaBlocked);
        assert!(!claim.is_expired(later));
        assert!(claim.blocks_dispatch(later));
    }

    #[test]
    fn a_plain_claim_cannot_redispatch_a_blocked_attempt() {
        let mut ledger = blocked_ledger(Some("claude"));
        let before = ledger.clone();
        for owner in ["subagent:a", "subagent:b"] {
            assert_eq!(
                ledger.claim("do [#a]", owner, NOW + TTL * 5, TTL),
                ClaimOutcome::QuotaBlocked
            );
        }
        assert_eq!(ledger, before);
    }

    #[test]
    fn an_expired_lost_attempt_that_came_back_exhausted_is_blocked_not_lost() {
        let mut ledger = running_ledger();
        ledger.prune_detailed(NOW + TTL * 2, None);
        assert_eq!(ledger.claims[0].state, QueueClaimState::Lost);
        ledger
            .block_on_quota("do [#a]", Some(1), block(Some("claude")))
            .expect("block a lost attempt");
        assert_eq!(ledger.claims[0].state, QueueClaimState::QuotaBlocked);
        assert_eq!(ledger.claims[0].lost_from, None);
        assert_eq!(
            ledger.claim("do [#a]", "subagent:b", NOW + TTL * 3, TTL),
            ClaimOutcome::QuotaBlocked
        );
    }

    #[test]
    fn a_reported_attempt_is_not_blocked_by_a_late_notice() {
        let mut ledger = running_ledger();
        ledger.report("do [#a]", Some(1)).expect("report");
        assert!(
            ledger
                .block_on_quota("do [#a]", Some(1), block(None))
                .is_err()
        );
        assert_eq!(ledger.claims[0].state, QueueClaimState::Reported);
    }

    #[test]
    fn repeating_a_block_keeps_the_first_time_and_named_work() {
        let mut ledger = blocked_ledger(Some("claude"));
        let mut again = block(Some("claude"));
        again.blocked_at_secs = NOW + 500;
        again.branch = None;
        again.worktree = None;
        ledger
            .block_on_quota("do [#a]", Some(1), again)
            .expect("repeat");
        let stored = ledger.claims[0].quota_block.as_ref().unwrap();
        assert_eq!(stored.blocked_at_secs, NOW + 1);
        assert_eq!(stored.branch.as_deref(), Some("fix/partial"));
    }

    #[test]
    fn a_stale_generation_cannot_block_the_replacement_attempt() {
        let mut ledger = running_ledger();
        assert!(
            ledger
                .block_on_quota("do [#a]", Some(9), block(None))
                .is_err()
        );
        assert_eq!(ledger.claims[0].state, QueueClaimState::Running);
    }

    #[test]
    fn resuming_the_same_worker_returns_it_to_running_with_a_fresh_ttl() {
        let mut ledger = blocked_ledger(Some("claude"));
        let resumed_at = NOW + TTL * 50;
        let claim = ledger
            .start(
                "do [#a]",
                "subagent:a",
                "worker-1",
                resumed_at,
                TTL,
                Some(1),
            )
            .expect("resume");
        assert_eq!(claim.state, QueueClaimState::Running);
        assert_eq!(claim.quota_block, None);
        assert!(!claim.is_expired(resumed_at));
    }

    #[test]
    fn a_different_worker_cannot_take_over_a_blocked_attempt() {
        let mut ledger = blocked_ledger(Some("claude"));
        assert!(matches!(
            ledger.start("do [#a]", "subagent:a", "worker-2", NOW + 2, TTL, Some(1)),
            Err(QueueClaimStartRefused::AttemptAlreadyBound { .. })
        ));
    }

    #[test]
    fn a_blocked_worker_cannot_heartbeat() {
        let mut ledger = blocked_ledger(Some("claude"));
        assert!(
            ledger
                .refresh("do [#a]", "subagent:a", NOW + 2, TTL)
                .is_err()
        );
    }

    #[test]
    fn a_resumed_worker_can_still_report_and_clears_the_block() {
        let mut ledger = blocked_ledger(Some("claude"));
        let claim = ledger.report("do [#a]", Some(1)).expect("report");
        assert_eq!(claim.state, QueueClaimState::Reported);
        assert_eq!(claim.quota_block, None);
    }

    #[test]
    fn a_blocked_attempt_frees_its_pool_slot_without_reading_as_frozen() {
        let ledger = blocked_ledger(Some("claude"));
        let claim = &ledger.claims[0];
        let later = NOW + TTL * 1_000;
        assert!(!crate::subagent_pool::occupies_pool_slot(claim, later, TTL));
        assert_eq!(crate::subagent_pool::worker_frozen(claim, later, TTL), None);
        assert!(!crate::subagent_pool::heartbeat_due(claim, later, TTL));
    }

    #[test]
    fn the_pool_gate_refuses_only_the_exhausted_harness() {
        let ledger = blocked_ledger(Some("claude"));
        let refused = pool_refill_quota_block(&ledger.claims, Some("claude"));
        assert_eq!(refused.map(|claim| claim.generation), Some(1));
        assert!(pool_refill_quota_block(&ledger.claims, Some("codex")).is_none());
    }

    #[test]
    fn the_pool_gate_fails_closed_on_an_unknown_harness() {
        let unnamed = blocked_ledger(None);
        assert!(pool_refill_quota_block(&unnamed.claims, Some("codex")).is_some());
        let named = blocked_ledger(Some("claude"));
        assert!(pool_refill_quota_block(&named.claims, None).is_some());
    }

    #[test]
    fn claude_code_is_the_same_exhausted_harness_as_claude() {
        let ledger = blocked_ledger(Some("claude-code"));
        assert!(pool_refill_quota_block(&ledger.claims, Some("claude")).is_some());
        assert!(pool_refill_quota_block(&ledger.claims, Some("CLAUDE")).is_some());
        assert!(pool_refill_quota_block(&ledger.claims, Some("codex")).is_none());
        assert_eq!(quota_harness_key(" grok-build "), "grok");
        assert_eq!(parse_worker_harness("claude-code").unwrap(), "claude");
        assert!(parse_worker_harness("not-a-harness").is_err());
    }

    #[test]
    fn the_pool_gate_ignores_a_resumed_attempt() {
        let mut ledger = blocked_ledger(Some("claude"));
        ledger
            .start("do [#a]", "subagent:a", "worker-1", NOW + 2, TTL, Some(1))
            .expect("resume");
        assert!(pool_refill_quota_block(&ledger.claims, Some("claude")).is_none());
    }

    #[test]
    fn the_next_step_names_cause_evidence_and_both_ways_forward() {
        let ledger = blocked_ledger(Some("claude"));
        let next = quota_blocked_next_step("tasks/a.md", &ledger.claims[0]);
        for needle in [
            "session limit of claude",
            "blocked by quota",
            "branch `fix/partial` in worktree `/work/tree`",
            "resume the same worker `worker-1`",
            "queue start tasks/a.md",
            "--generation 1 --worker worker-1",
            "--requeue",
            "--harness",
            "do not start another worker on claude",
        ] {
            assert!(next.contains(needle), "missing {needle:?} in {next}");
        }
    }

    #[test]
    fn the_requeued_brief_names_the_partial_branch() {
        let handoff = crate::goal_preset_terminal::GoalLoopHandoff {
            identity: claim_identity("do [#a]"),
            item_text: "do [#a]".to_string(),
            owner: "subagent:a".to_string(),
            worker_id: Some("worker-1".to_string()),
            generation: 3,
            final_states: Vec::new(),
            response: String::new(),
            recorded_at_secs: NOW,
            quota_block: Some(block(Some("claude"))),
        };
        let section = crate::goal_preset_terminal::prior_attempt_brief_section(&handoff);
        assert!(section.contains("branch `fix/partial`"), "{section}");
        assert!(section.contains("generation 3"), "{section}");
        assert!(!section.contains("declared final state"), "{section}");
    }

    #[test]
    fn a_ledger_without_the_block_field_still_decodes() {
        let ledger = running_ledger();
        let payload = serde_json::to_string(&ledger).unwrap();
        assert!(!payload.contains("quota_block"));
        let decoded: QueueClaimLedger = serde_json::from_str(&payload).unwrap();
        assert_eq!(decoded, ledger);
    }

    #[test]
    fn the_blocked_state_round_trips_through_the_ledger_payload() {
        let ledger = blocked_ledger(Some("claude"));
        let payload = serde_json::to_string(&ledger).unwrap();
        assert!(payload.contains("\"quota_blocked\""));
        let decoded: QueueClaimLedger = serde_json::from_str(&payload).unwrap();
        assert_eq!(decoded, ledger);
    }
}

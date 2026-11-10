//! `#subagentworkpool`: heads added while the owner turn is still running.
//!
//! A hookless harness (Grok Build) cannot inject PostToolUse stdout into the
//! turn that is already running. The pool checkpoint still computes the refill,
//! and the hook watermark advances, so the idle wake never sees the new heads.
//! They sat unclaimed until the next command the coordinator happened to run
//! (infra.md, 2026-11-10: two prompts added beside one running worker, dispatched
//! only at the following `report-complete`).
//!
//! This decision is the paste: when free slots have heads or unstarted claims,
//! and the pane capture proves the owner turn is live, hand that refill to the
//! turn already running. An idle pane is not typed into — a submit there would
//! start a second turn. A harness whose hook already injects is left alone.

/// Why a running turn is not handed a pool refill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolRunningTurnFollowUpSkip {
    /// No unstarted claim and no free-slot head.
    NothingToDispatch,
    /// This exact head set was already pasted into the live turn.
    AlreadyDelivered,
    /// An operator pause holds every unattended injection.
    QueuePaused,
    /// `PostToolUse` injects the refill; a second paste would dispatch twice.
    HookInjects,
    /// The pane is not in a turn. Submitting would start a new one.
    TurnNotLive,
}

impl PoolRunningTurnFollowUpSkip {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NothingToDispatch => "nothing_to_dispatch",
            Self::AlreadyDelivered => "already_delivered",
            Self::QueuePaused => "queue_paused",
            Self::HookInjects => "hook_injects",
            Self::TurnNotLive => "turn_not_live",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolRunningTurnFollowUpDecision {
    Skip(PoolRunningTurnFollowUpSkip),
    FollowUp,
}

/// Everything the paste decides from. `turn_live` is a pane-capture fact, not
/// the actor record: a registry `ready` flag has been wrong while the turn
/// was still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolRunningTurnFollowUpObservation {
    pub next_heads: usize,
    pub unstarted: usize,
    pub delivered: bool,
    pub queue_paused: bool,
    pub hook_injects: bool,
    pub turn_live: bool,
}

pub fn pool_running_turn_follow_up_decision(
    observation: PoolRunningTurnFollowUpObservation,
) -> PoolRunningTurnFollowUpDecision {
    use PoolRunningTurnFollowUpDecision::Skip;
    use PoolRunningTurnFollowUpSkip::*;
    if observation.next_heads == 0 && observation.unstarted == 0 {
        return Skip(NothingToDispatch);
    }
    if observation.delivered {
        return Skip(AlreadyDelivered);
    }
    if observation.queue_paused {
        return Skip(QueuePaused);
    }
    if observation.hook_injects {
        return Skip(HookInjects);
    }
    if !observation.turn_live {
        return Skip(TurnNotLive);
    }
    PoolRunningTurnFollowUpDecision::FollowUp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_grok(next_heads: usize) -> PoolRunningTurnFollowUpObservation {
        PoolRunningTurnFollowUpObservation {
            next_heads,
            unstarted: 0,
            delivered: false,
            queue_paused: false,
            hook_injects: false,
            turn_live: true,
        }
    }

    /// infra.md: one worker running, two prompts added, free slots, Grok turn
    /// still live. The refill is a follow-up, not a new turn and not silence.
    #[test]
    fn heads_added_during_a_live_hookless_turn_are_a_follow_up() {
        assert_eq!(
            pool_running_turn_follow_up_decision(live_grok(2)),
            PoolRunningTurnFollowUpDecision::FollowUp
        );
    }

    #[test]
    fn an_unstarted_claim_is_also_a_follow_up() {
        let mut observation = live_grok(0);
        observation.unstarted = 1;
        assert_eq!(
            pool_running_turn_follow_up_decision(observation),
            PoolRunningTurnFollowUpDecision::FollowUp
        );
    }

    #[test]
    fn the_same_heads_are_not_pasted_twice() {
        let mut observation = live_grok(2);
        observation.delivered = true;
        assert_eq!(
            pool_running_turn_follow_up_decision(observation),
            PoolRunningTurnFollowUpDecision::Skip(PoolRunningTurnFollowUpSkip::AlreadyDelivered)
        );
    }

    #[test]
    fn a_hook_harness_keeps_the_post_tool_use_injection() {
        let mut observation = live_grok(2);
        observation.hook_injects = true;
        assert_eq!(
            pool_running_turn_follow_up_decision(observation),
            PoolRunningTurnFollowUpDecision::Skip(PoolRunningTurnFollowUpSkip::HookInjects)
        );
    }

    #[test]
    fn an_idle_pane_is_not_given_a_second_turn() {
        let mut observation = live_grok(2);
        observation.turn_live = false;
        assert_eq!(
            pool_running_turn_follow_up_decision(observation),
            PoolRunningTurnFollowUpDecision::Skip(PoolRunningTurnFollowUpSkip::TurnNotLive)
        );
    }

    #[test]
    fn a_full_pool_is_not_a_follow_up() {
        assert_eq!(
            pool_running_turn_follow_up_decision(live_grok(0)),
            PoolRunningTurnFollowUpDecision::Skip(PoolRunningTurnFollowUpSkip::NothingToDispatch)
        );
    }
}

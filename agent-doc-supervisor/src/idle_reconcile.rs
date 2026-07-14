//! Pure supervisor idle/reconcile policy.
//!
//! This module owns decisions that reconcile stale supervisor actor projections
//! from observed pane facts. It does not capture panes, inspect harness output,
//! mutate actor state, write logs, or dispatch queue work.

pub const QUEUED_DRAFT_BLOCKER_REASON: &str = "queued draft in composer";

pub fn recoverable_ready_busy_blocker_reason(reason: &str) -> bool {
    matches!(reason, QUEUED_DRAFT_BLOCKER_REASON)
}

pub fn ready_busy_conflict_reconcile_decision(
    actor_ready: bool,
    blocker_reason: Option<&str>,
    clear_cooldown_active: bool,
    consecutive_ready_busy_ticks: u32,
    required_ticks: u32,
) -> bool {
    actor_ready
        && !clear_cooldown_active
        && blocker_reason.is_some_and(recoverable_ready_busy_blocker_reason)
        && consecutive_ready_busy_ticks >= required_ticks
}

pub fn stale_busy_idle_reconcile_decision(
    actor_busy: bool,
    pane_has_busy_cue: bool,
    turn_active: bool,
    dispatch_grace_active: bool,
    clear_cooldown_active: bool,
    consecutive_idle_busy_ticks: u32,
    required_ticks: u32,
) -> bool {
    actor_busy
        && !pane_has_busy_cue
        && !turn_active
        && !dispatch_grace_active
        && !clear_cooldown_active
        && consecutive_idle_busy_ticks >= required_ticks
}

pub fn reconcile_stale_busy_idle_queue_state(
    last_dispatched: Option<String>,
    idle_busy_ticks: &mut u32,
) -> Option<String> {
    *idle_busy_ticks = 0;
    last_dispatched
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUIRED_TICKS: u32 = 4;

    #[test]
    fn stale_busy_reconcile_fires_after_debounce_over_idle_pane() {
        assert!(stale_busy_idle_reconcile_decision(
            true,
            false,
            false,
            false,
            false,
            REQUIRED_TICKS,
            REQUIRED_TICKS
        ));
    }

    #[test]
    fn stale_busy_reconcile_waits_for_full_debounce() {
        for ticks in 0..REQUIRED_TICKS {
            assert!(
                !stale_busy_idle_reconcile_decision(
                    true,
                    false,
                    false,
                    false,
                    false,
                    ticks,
                    REQUIRED_TICKS
                ),
                "should not reconcile after only {ticks} idle ticks"
            );
        }
    }

    #[test]
    fn stale_busy_reconcile_skips_when_pane_busy() {
        assert!(!stale_busy_idle_reconcile_decision(
            true,
            true,
            false,
            false,
            false,
            REQUIRED_TICKS + 10,
            REQUIRED_TICKS
        ));
    }

    #[test]
    fn stale_busy_reconcile_skips_when_actor_ready() {
        assert!(!stale_busy_idle_reconcile_decision(
            false,
            false,
            false,
            false,
            false,
            REQUIRED_TICKS,
            REQUIRED_TICKS
        ));
    }

    #[test]
    fn stale_busy_reconcile_skips_during_clear_cooldown() {
        assert!(!stale_busy_idle_reconcile_decision(
            true,
            false,
            false,
            false,
            true,
            REQUIRED_TICKS,
            REQUIRED_TICKS
        ));
    }

    #[test]
    fn stale_busy_reconcile_skips_while_owned_turn_is_active() {
        assert!(!stale_busy_idle_reconcile_decision(
            true,
            false,
            true,
            false,
            false,
            REQUIRED_TICKS + 10,
            REQUIRED_TICKS
        ));
    }

    #[test]
    fn stale_busy_reconcile_skips_during_fresh_dispatch_grace() {
        assert!(!stale_busy_idle_reconcile_decision(
            true,
            false,
            false,
            true,
            false,
            REQUIRED_TICKS + 10,
            REQUIRED_TICKS
        ));
    }

    #[test]
    fn stale_busy_reconcile_resets_idle_counter_and_preserves_last_dispatch() {
        let mut idle_busy_ticks = REQUIRED_TICKS;
        let last_dispatched = reconcile_stale_busy_idle_queue_state(
            Some("do [#learn-ohio-duplicate-gate]".to_string()),
            &mut idle_busy_ticks,
        );

        assert_eq!(idle_busy_ticks, 0);
        assert_eq!(
            last_dispatched.as_deref(),
            Some("do [#learn-ohio-duplicate-gate]")
        );
    }

    #[test]
    fn ready_busy_conflict_reconcile_debounces_stale_queue_draft() {
        for ticks in 0..REQUIRED_TICKS {
            assert!(
                !ready_busy_conflict_reconcile_decision(
                    true,
                    Some(QUEUED_DRAFT_BLOCKER_REASON),
                    false,
                    ticks,
                    REQUIRED_TICKS,
                ),
                "tick {ticks} should still wait for the bounded re-probe"
            );
        }
        assert!(ready_busy_conflict_reconcile_decision(
            true,
            Some(QUEUED_DRAFT_BLOCKER_REASON),
            false,
            REQUIRED_TICKS,
            REQUIRED_TICKS,
        ));
    }

    #[test]
    fn ready_busy_conflict_reconcile_protects_active_turns() {
        assert!(!ready_busy_conflict_reconcile_decision(
            true,
            Some("active codex turn"),
            false,
            REQUIRED_TICKS + 10,
            REQUIRED_TICKS,
        ));
        assert!(!ready_busy_conflict_reconcile_decision(
            true,
            Some("active permission prompt"),
            false,
            REQUIRED_TICKS + 10,
            REQUIRED_TICKS,
        ));
    }

    #[test]
    fn ready_busy_conflict_reconcile_skips_during_clear_cooldown() {
        assert!(!ready_busy_conflict_reconcile_decision(
            true,
            Some(QUEUED_DRAFT_BLOCKER_REASON),
            true,
            REQUIRED_TICKS + 10,
            REQUIRED_TICKS,
        ));
    }
}

//! Pure supervisor terminal/detection policy.
//!
//! Callers provide already-collected output and primitive supervisor facts.
//! This module does not capture panes, read shared buffers, lock actor state,
//! mutate latch state, or dispatch work.

use agent_doc_harness::HarnessConfig;
use lazily::ReadinessCore;

use crate::idle_reconcile::recoverable_ready_busy_blocker_reason;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleQueuePromptVisibility {
    Visible,
    Hidden,
    NeedsLivePaneDispatchReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTriggerPromptSource {
    CurrentChildPty,
    StableOwnedPane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTriggerPromptDecision {
    Dispatch(AutoTriggerPromptSource),
    CancelHelpScreen,
    Wait { live_pane_ready_ticks: u32 },
}

/// Decide whether a replacement child is ready for its one-shot document trigger.
///
/// The child-owned PTY prompt remains the primary proof. A managed tmux pane is a
/// fallback only after this child generation has emitted output, the actor has
/// independently reconciled to ready, and the pane has remained dispatch-ready
/// for a bounded number of consecutive polls. This coalesces duplicate readiness
/// signals without allowing stale pane history to trigger a newly spawned child.
pub fn auto_trigger_prompt_decision(
    current_child_prompt_visible: bool,
    current_child_output_observed: bool,
    actor_ready: bool,
    live_pane_dispatch_ready: Option<bool>,
    live_pane_ready_ticks: u32,
    required_live_pane_ready_ticks: u32,
    help_screen_visible: bool,
) -> AutoTriggerPromptDecision {
    if help_screen_visible {
        return AutoTriggerPromptDecision::CancelHelpScreen;
    }
    if current_child_prompt_visible {
        return AutoTriggerPromptDecision::Dispatch(AutoTriggerPromptSource::CurrentChildPty);
    }

    // Compose independent generation/readiness proofs through lazily's service
    // readiness primitive. All conditions are explicit so an empty probe set can
    // never accidentally authorize dispatch.
    let mut fallback_readiness = ReadinessCore::new();
    fallback_readiness.set("current_child_output", current_child_output_observed);
    fallback_readiness.set("actor_ready", actor_ready);
    fallback_readiness.set(
        "owned_pane_dispatch_ready",
        live_pane_dispatch_ready == Some(true),
    );
    let fallback_ready = fallback_readiness.ready();
    if !fallback_ready {
        return AutoTriggerPromptDecision::Wait {
            live_pane_ready_ticks: 0,
        };
    }

    let next_ticks = live_pane_ready_ticks.saturating_add(1);
    if next_ticks >= required_live_pane_ready_ticks.max(1) {
        AutoTriggerPromptDecision::Dispatch(AutoTriggerPromptSource::StableOwnedPane)
    } else {
        AutoTriggerPromptDecision::Wait {
            live_pane_ready_ticks: next_ticks,
        }
    }
}

pub fn prompt_visible_requires_ready_transition(
    first_prompt_for_child: bool,
    actor_known_non_ready: bool,
) -> bool {
    first_prompt_for_child || actor_known_non_ready
}

pub fn idle_queue_prompt_visibility(
    output: &str,
    harness: &HarnessConfig,
    actor_ready: bool,
) -> IdleQueuePromptVisibility {
    if harness.dispatch_blocker_reason(output).is_some() {
        return IdleQueuePromptVisibility::Hidden;
    }
    if actor_ready {
        return IdleQueuePromptVisibility::Visible;
    }
    if !harness.output_prompt_visible(output) {
        return IdleQueuePromptVisibility::Hidden;
    }
    IdleQueuePromptVisibility::NeedsLivePaneDispatchReady
}

pub fn idle_queue_prompt_visible_after_live_pane_dispatch_ready(
    live_pane_dispatch_ready: Option<bool>,
) -> bool {
    live_pane_dispatch_ready.unwrap_or(true)
}

pub fn ready_busy_blocker_reason(output: &str, harness: &HarnessConfig) -> Option<String> {
    harness
        .dispatch_blocker_reason(output)
        .filter(|reason| recoverable_ready_busy_blocker_reason(reason))
}

pub fn help_screen_visible(output: &str, harness: &HarnessConfig) -> bool {
    harness.is_help_screen_output(output)
}

pub fn pane_dispatch_ready(content: &str, harness: &HarnessConfig) -> bool {
    agent_doc_harness::ready_prompt_candidate(content, harness).is_some()
}

pub fn pane_has_busy_cue(content: &str, harness: &HarnessConfig) -> bool {
    harness.has_busy_cue(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idle_reconcile::QUEUED_DRAFT_BLOCKER_REASON;

    #[test]
    fn prompt_visible_ready_transition_requires_first_prompt_or_non_ready_actor() {
        assert!(prompt_visible_requires_ready_transition(true, false));
        assert!(prompt_visible_requires_ready_transition(true, true));
        assert!(prompt_visible_requires_ready_transition(false, true));
        assert!(!prompt_visible_requires_ready_transition(false, false));
    }

    #[test]
    fn auto_trigger_prompt_dispatches_immediately_from_current_child_pty() {
        assert_eq!(
            auto_trigger_prompt_decision(true, true, false, None, 0, 2, false),
            AutoTriggerPromptDecision::Dispatch(AutoTriggerPromptSource::CurrentChildPty)
        );
    }

    #[test]
    fn auto_trigger_prompt_coalesces_stable_owned_pane_readiness() {
        assert_eq!(
            auto_trigger_prompt_decision(false, true, true, Some(true), 0, 2, false),
            AutoTriggerPromptDecision::Wait {
                live_pane_ready_ticks: 1
            }
        );
        assert_eq!(
            auto_trigger_prompt_decision(false, true, true, Some(true), 1, 2, false),
            AutoTriggerPromptDecision::Dispatch(AutoTriggerPromptSource::StableOwnedPane)
        );
    }

    #[test]
    fn auto_trigger_prompt_rejects_stale_or_unstable_pane_evidence() {
        for decision in [
            auto_trigger_prompt_decision(false, false, true, Some(true), 1, 2, false),
            auto_trigger_prompt_decision(false, true, false, Some(true), 1, 2, false),
            auto_trigger_prompt_decision(false, true, true, Some(false), 1, 2, false),
            auto_trigger_prompt_decision(false, true, true, None, 1, 2, false),
        ] {
            assert_eq!(
                decision,
                AutoTriggerPromptDecision::Wait {
                    live_pane_ready_ticks: 0
                }
            );
        }
    }

    #[test]
    fn auto_trigger_prompt_help_screen_fails_closed_over_other_ready_signals() {
        assert_eq!(
            auto_trigger_prompt_decision(true, true, true, Some(true), 2, 2, true),
            AutoTriggerPromptDecision::CancelHelpScreen
        );
    }

    #[test]
    fn idle_queue_prompt_visibility_trusts_ready_actor_over_stale_renderer_tail() {
        let harness = agent_doc_harness::HarnessConfig::claude();

        assert_eq!(
            idle_queue_prompt_visibility(
                "turn committed, renderer tail has no composer\n",
                &harness,
                true,
            ),
            IdleQueuePromptVisibility::Visible
        );
    }

    #[test]
    fn idle_queue_prompt_visibility_keeps_blocker_over_ready_actor() {
        let harness = agent_doc_harness::HarnessConfig::claude();
        let output = concat!(
            "✶ Generating… (3s · esc to interrupt)\n",
            "❯\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host\n",
            "  ⏵⏵ bypass permissions on · 1 shell\n",
        );

        assert_eq!(
            idle_queue_prompt_visibility(output, &harness, true),
            IdleQueuePromptVisibility::Hidden
        );
    }

    #[test]
    fn idle_queue_prompt_visibility_requests_live_probe_for_non_ready_prompt_signal() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert_eq!(
            idle_queue_prompt_visibility("resumed child ready\n❯\n", &harness, false),
            IdleQueuePromptVisibility::NeedsLivePaneDispatchReady
        );
    }

    #[test]
    fn idle_queue_prompt_visible_after_live_probe_preserves_fallback_semantics() {
        assert!(idle_queue_prompt_visible_after_live_pane_dispatch_ready(
            Some(true)
        ));
        assert!(!idle_queue_prompt_visible_after_live_pane_dispatch_ready(
            Some(false)
        ));
        assert!(idle_queue_prompt_visible_after_live_pane_dispatch_ready(
            None
        ));
    }

    #[test]
    fn ready_busy_blocker_reason_filters_to_recoverable_queue_draft() {
        let harness = agent_doc_harness::HarnessConfig::codex();
        let queued_draft = "\
›
tab to queue message
gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used
";

        assert_eq!(
            ready_busy_blocker_reason(queued_draft, &harness).as_deref(),
            Some(QUEUED_DRAFT_BLOCKER_REASON)
        );

        let active_turn = "\
• Working (1m 34s • esc to interrupt)

› Write tests
gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used
";
        assert_eq!(ready_busy_blocker_reason(active_turn, &harness), None);
    }

    #[test]
    fn help_screen_visible_detects_opencode_help() {
        let harness = agent_doc_harness::HarnessConfig::opencode();
        let help_output = "\
opencode [project]           start opencode tui
opencode run [message..]     run opencode with a message
opencode debug               debugging and troubleshooting tools
";
        assert!(help_screen_visible(help_output, &harness));
    }

    #[test]
    fn help_screen_visible_rejects_normal_opencode_output() {
        let harness = agent_doc_harness::HarnessConfig::opencode();
        assert!(!help_screen_visible("some normal output\n>\n", &harness));
    }

    #[test]
    fn pane_dispatch_ready_uses_harness_ready_prompt_candidate() {
        let harness = agent_doc_harness::HarnessConfig::codex();
        let ready_output = "\
Previous turn output
gpt-5.5 high · ~/work/btakita/agent-loop · Context 45% used
";
        let busy_output = "\
Working (12s - esc to interrupt)
gpt-5.5 high · ~/work/btakita/agent-loop · Context 45% used
";

        assert!(pane_dispatch_ready(ready_output, &harness));
        assert!(!pane_dispatch_ready(busy_output, &harness));
    }

    #[test]
    fn pane_has_busy_cue_uses_harness_dispatch_blocker() {
        let harness = agent_doc_harness::HarnessConfig::codex();

        assert!(pane_has_busy_cue(
            "Working (12s - esc to interrupt)\n",
            &harness
        ));
        assert!(!pane_has_busy_cue(
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 45% used\n",
            &harness
        ));
    }
}

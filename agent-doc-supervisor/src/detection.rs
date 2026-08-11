//! Pure supervisor terminal/detection policy.
//!
//! Callers provide already-collected output and primitive supervisor facts.
//! This module does not capture panes, read shared buffers, lock actor state,
//! mutate latch state, or dispatch work.

use agent_doc_harness::{HarnessConfig, PaneComposerProjection, project_pane_composer};
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

/// Name the first fallback-readiness gate that never opened, for the
/// `no_prompt` startup-miss diagnostic (`#startupmissgates`).
///
/// [`auto_trigger_prompt_decision`]'s pane fallback requires all three of
/// `current_child_output`, `actor_ready`, and `owned_pane_dispatch_ready`.
/// When the deadline expires, the operator needs to know *which* one stayed
/// shut: they point at a dead PTY relay, a stuck actor projection, and a prompt
/// the pane detector cannot recognize respectively. Reported in gate order so
/// the earliest unmet precondition is the one named.
pub fn auto_trigger_blocking_gate(
    current_child_output_observed: bool,
    actor_ready: bool,
    live_pane_dispatch_ready: Option<bool>,
) -> &'static str {
    if !current_child_output_observed {
        return "current_child_output";
    }
    if !actor_ready {
        return "actor_ready";
    }
    match live_pane_dispatch_ready {
        None => "owned_pane_unavailable",
        Some(false) => "owned_pane_dispatch_ready",
        // All gates open: the deadline expired while coalescing consecutive
        // stable-pane ticks, not because a precondition was unmet.
        Some(true) => "none_coalescing",
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
    // `#idlequeuedraftinject`: only a PROVABLY EMPTY composer is dispatchable.
    //
    // The busy cue above only covers an *active* turn. A completed-turn marker
    // (`✻ Brewed for 7m 59s`) raises no cue, so `actor_ready` used to win and
    // return `Visible` without anyone looking at the composer — and the watch
    // injected `agent-doc <FILE>` on top of the operator's own unsent text. The
    // payload then sat unsubmitted and logged `dispatch_start_unproven` on a
    // loop. `dispatch_payload_pending_in_current_input` did not catch it either:
    // that asks whether *this payload* is already pending, not whether the
    // composer is occupied at all.
    //
    // Deliberately rejects `OperatorDraft` only, NOT "anything that is not
    // `ReadyEmpty`". `Absent` is a legitimate stale/partial renderer tail that
    // carries no composer at all, and
    // `idle_queue_prompt_visibility_trusts_ready_actor_over_stale_renderer_tail`
    // pins that a ready actor still wins there — tightening this to require
    // `ReadyEmpty` stalls real drains.
    //
    // Only explicit operator steering may add and submit a prompt, so this is
    // checked before the `actor_ready` short-circuit rather than after it.
    let projection = project_pane_composer(output, harness);
    if matches!(projection, PaneComposerProjection::OperatorDraft { .. }) {
        return IdleQueuePromptVisibility::Hidden;
    }
    // `#qflood`: the stacked-draft shape is `Absent`, not `OperatorDraft`.
    // Once several `/agent-doc <FILE>` lines pile up, the bottom-most carries no
    // `❯` glyph, so `is_prompt_line` is false and no draft is projected. The
    // watch then appended another copy and pressed Enter — which in a
    // multi-line composer inserts a newline instead of submitting, so the draft
    // grew every cycle (`dispatch_start_unproven` on a loop).
    //
    // Discriminated by composer CHROME, because a bare `Absent` must stay
    // dispatchable: a stale/partial renderer tail carries no composer at all
    // and a ready actor legitimately wins there. When the frame IS rendered
    // (rule / status / permission footer) but the body is not an empty prompt,
    // the composer is occupied and must never be injected into.
    if composer_chrome_rendered(output, harness)
        && !matches!(projection, PaneComposerProjection::ReadyEmpty { .. })
    {
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

/// True when the bottom of the capture shows the harness's rendered composer
/// frame (box rule, status line, permission footer).
///
/// Distinguishes "the composer is on screen and occupied" from "this capture
/// has no composer at all" — a stale or partial renderer tail, where a ready
/// actor is still trusted.
fn composer_chrome_rendered(output: &str, harness: &HarnessConfig) -> bool {
    output
        .lines()
        .rev()
        .take(8)
        .filter(|line| !line.trim().is_empty())
        .any(|line| harness.is_ignorable_output_line(line))
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

pub fn pane_dispatch_ready_at_cursor(
    content: &str,
    harness: &HarnessConfig,
    cursor_y: Option<usize>,
) -> bool {
    agent_doc_harness::ready_prompt_candidate_at_cursor(content, harness, cursor_y).is_some()
}

pub fn pane_has_busy_cue(content: &str, harness: &HarnessConfig) -> bool {
    harness.has_busy_cue(content)
}

/// (`#unrenderedframestorm`) Whether a pane capture is a rendered harness frame
/// at all, and may therefore be read as evidence about the composer.
///
/// The live failure, operator-reported 2026-08-09 on
/// `src/haiven-dev/tasks/haiven-dev.md`: pane-layout churn moved pane `%20`
/// between windows, and `capture-pane` sampled it mid-render. The entire capture
/// was 22 bytes — `ESC[38;5;246m❯ ESC[39m` — a bare prompt glyph with no
/// composer box, no footer, and no scrollback.
///
/// From those 22 bytes the idle-queue drain concluded THREE wrong things at
/// once: the composer is empty (`payload_already_pending=false`), the prompt is
/// dispatch-ready (`dispatch_ready=true`), and by omission that no turn was
/// running. It injected, and 31s later sampled the same unrenderable frame and
/// injected again — eleven times, stacking eleven identical triggers into a
/// composer whose pane was in fact 19 minutes into an active turn.
///
/// A successful capture is not the same as an informative one. `capture-pane`
/// returning `Ok` only proves tmux answered, and `#idlerevisionreactive` applies
/// to a pane exactly as it does to a controller probe: "I looked and the
/// composer is empty" and "the frame I got cannot contain a composer" must stay
/// distinct.
///
/// The discriminator is the observed one and needs no chrome matching: a
/// rendered harness surface always has something besides the prompt line — a
/// composer border, a footer, a status line, or prior output. A frame whose only
/// non-empty line IS the prompt candidate answers nothing. Deliberately not
/// keyed on harness footer text, which varies per harness and per permission
/// mode and is what `#panedraftunblocker` already had to repair twice.
///
/// Deferring costs one poll interval; injecting over an active turn costs the
/// operator a wedged session, so an ambiguous frame defers.
pub fn pane_frame_answers_composer_state(content: &str) -> bool {
    content
        .lines()
        .filter(|line| {
            !agent_doc_turn_executor_tmux::prompt::strip_ansi(line)
                .trim()
                .is_empty()
        })
        .nth(1)
        .is_some()
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

    /// `#startupmissgates`: each unmet precondition must be named distinctly,
    /// and the reported gate must agree with the decision actually taken.
    /// `#idlequeuedraftinject`: the idle-queue watch must never inject over a
    /// composer that already holds operator text.
    ///
    /// Captured live from `tasks/haiven-dev.md` pane `%20`: a completed-turn
    /// marker (`✻ Brewed for 7m 59s`, which is past tense and so raises no busy
    /// cue) above a composer holding the operator's own `keep going`. Because
    /// `idle_queue_prompt_visibility` short-circuits on `actor_ready`, it never
    /// consulted the composer projection and reported the pane dispatchable —
    /// so the watch injected `agent-doc <FILE>` on top of that text, which then
    /// sat unsubmitted and logged `dispatch_start_unproven` every cycle.
    ///
    /// Only explicit operator steering may add and submit a prompt.
    #[test]
    fn idle_queue_does_not_inject_over_an_operator_draft() {
        let harness = HarnessConfig::claude();
        let pane = concat!(
            "● Loop armed. Interrupt any time to stop or redirect.\n",
            "✻ Brewed for 7m 59s\n",
            "───────────────────────────────────────\n",
            "❯ keep going\n",
            "───────────────────────────────────────\n",
            "  Opus 5 ctx:26% ~/…/src/haiven-dev docs/fpe brian@cachyos-x8664\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · PR #20 · ← 1 agent\n",
        );

        // The precondition that made this reachable: no busy cue on a completed turn.
        assert!(
            harness.dispatch_blocker_reason(pane).is_none(),
            "completed-turn marker must not raise a busy cue (that is why actor_ready wins)"
        );

        assert_eq!(
            idle_queue_prompt_visibility(pane, &harness, true),
            IdleQueuePromptVisibility::Hidden,
            "a composer holding operator text must never be dispatchable"
        );
        assert_eq!(
            idle_queue_prompt_visibility(pane, &harness, false),
            IdleQueuePromptVisibility::Hidden,
            "and the non-ready path must agree"
        );
    }

    /// `#qflood`: the observed flood shape — several `/agent-doc <FILE>` lines
    /// stacked in the composer. The bottom-most carries no `❯` glyph, so
    /// `is_prompt_line` is false and the projection is `Absent`, not
    /// `OperatorDraft`. Rejecting only `OperatorDraft` would still inject here,
    /// append another copy, and press Enter — which in a multi-line composer
    /// inserts a newline instead of submitting, growing the draft every cycle.
    ///
    /// Captured live from `tasks/sdk.md` pane `%926`.
    #[test]
    fn idle_queue_does_not_inject_into_a_stacked_multiline_draft() {
        let harness = HarnessConfig::claude();
        let pane = concat!(
            "❯ /agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md\n",
            "  /agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md\n",
            "  /agent-doc /home/brian/work/btakita/agent-loop/src/haiven-dev/tasks/sdk.md\n",
            "───────────────────────────────────────\n",
            "  Opus 5 ctx:22% ~/…/src/haiven-dev docs/fpe brian@cachyos-x8664\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · PR #1\n",
        );

        assert_ne!(
            project_pane_composer(pane, &harness),
            PaneComposerProjection::ReadyEmpty {
                evidence: agent_doc_harness::PaneComposerReadinessEvidence::Prompt {
                    rendered: "❯".to_string()
                }
            },
            "a stacked draft must not project as an empty composer"
        );
        assert_eq!(
            idle_queue_prompt_visibility(pane, &harness, true),
            IdleQueuePromptVisibility::Hidden,
            "a stacked multi-line draft must never be dispatchable"
        );
    }

    /// The guard must not block a genuinely empty composer, or the queue stalls.
    #[test]
    fn idle_queue_still_dispatches_into_an_empty_composer() {
        let harness = HarnessConfig::claude();
        let pane = concat!(
            "✻ Brewed for 7m 59s\n",
            "───────────────────────────────────────\n",
            "❯ \n",
            "───────────────────────────────────────\n",
            "  Opus 5 ctx:26% ~/…/src/haiven-dev docs/fpe brian@cachyos-x8664\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · PR #20 · ← 1 agent\n",
        );

        assert_eq!(
            idle_queue_prompt_visibility(pane, &harness, true),
            IdleQueuePromptVisibility::Visible,
            "an empty composer on a ready actor must stay dispatchable"
        );
    }

    #[test]
    fn auto_trigger_blocking_gate_names_the_unmet_precondition() {
        assert_eq!(
            auto_trigger_blocking_gate(false, true, Some(true)),
            "current_child_output"
        );
        assert_eq!(
            auto_trigger_blocking_gate(true, false, Some(true)),
            "actor_ready"
        );
        assert_eq!(
            auto_trigger_blocking_gate(true, true, None),
            "owned_pane_unavailable"
        );
        assert_eq!(
            auto_trigger_blocking_gate(true, true, Some(false)),
            "owned_pane_dispatch_ready"
        );
        assert_eq!(
            auto_trigger_blocking_gate(true, true, Some(true)),
            "none_coalescing"
        );
    }

    /// A named gate is only useful if it explains the decision. Whenever the
    /// gate says a precondition is unmet, the decision must be `Wait` with the
    /// tick counter reset; whenever it says `none_coalescing`, the fallback is
    /// genuinely progressing toward dispatch.
    #[test]
    fn auto_trigger_blocking_gate_agrees_with_the_prompt_decision() {
        for (output, ready, pane) in [
            (false, true, Some(true)),
            (true, false, Some(true)),
            (true, true, None),
            (true, true, Some(false)),
        ] {
            let gate = auto_trigger_blocking_gate(output, ready, pane);
            assert_ne!(gate, "none_coalescing", "expected a blocked gate");
            assert_eq!(
                auto_trigger_prompt_decision(false, output, ready, pane, 0, 2, false),
                AutoTriggerPromptDecision::Wait {
                    live_pane_ready_ticks: 0
                },
                "gate {gate} must correspond to a reset Wait"
            );
        }

        assert_eq!(
            auto_trigger_blocking_gate(true, true, Some(true)),
            "none_coalescing"
        );
        assert_eq!(
            auto_trigger_prompt_decision(false, true, true, Some(true), 1, 2, false),
            AutoTriggerPromptDecision::Dispatch(AutoTriggerPromptSource::StableOwnedPane)
        );
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

    /// `#unrenderedframestorm`: the EXACT 22 bytes tmux returned for pane `%20`
    /// on 2026-08-09, preserved verbatim from
    /// `route-submit/1786246751175-idle_queue_payload_observation-claude-_20-46843cd9ff07.txt`.
    ///
    /// This frame is why the storm happened, so it is pinned as bytes rather than
    /// paraphrased: a rendered-frame check written against a paraphrase would not
    /// have caught it.
    const UNRENDERED_CLAUDE_FRAME: &str = "\u{1b}[38;5;246m❯\u{a0}\u{1b}[39m\n";

    #[test]
    fn an_unrendered_prompt_glyph_frame_answers_nothing_about_the_composer() {
        assert_eq!(
            UNRENDERED_CLAUDE_FRAME.len(),
            22,
            "the live capture was 22 bytes"
        );
        assert!(!pane_frame_answers_composer_state(UNRENDERED_CLAUDE_FRAME));

        // And the two values the drain used to trust are both derivable from it,
        // which is exactly why the frame check has to gate them.
        let harness = agent_doc_harness::HarnessConfig::claude();
        assert!(
            pane_dispatch_ready_at_cursor(UNRENDERED_CLAUDE_FRAME, &harness, Some(0)),
            "the bare glyph still reads as dispatch-ready -- the frame check is \
             the only thing that can reject it"
        );
        assert!(
            !pane_has_busy_cue(UNRENDERED_CLAUDE_FRAME, &harness),
            "and the active turn is invisible in it"
        );
    }

    #[test]
    fn a_rendered_frame_still_answers_the_composer_question() {
        // A real claude pane always renders something besides the prompt line.
        let rendered = "\u{1b}[38;5;246m❯\u{a0}\u{1b}[39m\n  ⏵⏵ bypass permissions on · 2 shells\n";
        assert!(pane_frame_answers_composer_state(rendered));
        // Prior output above the prompt counts too.
        assert!(pane_frame_answers_composer_state("completed response\n❯\n"));
        // An ANSI-only second line is not content.
        assert!(!pane_frame_answers_composer_state(
            "\u{1b}[38;5;246m❯\u{1b}[39m\n\u{1b}[39m\n"
        ));
        assert!(!pane_frame_answers_composer_state(""));
    }

    #[test]
    fn pane_dispatch_ready_at_cursor_ignores_custom_status_suffix() {
        let harness = agent_doc_harness::HarnessConfig::claude();
        let content = "completed response\n❯\narbitrary status output\nanother status line\n";

        assert!(pane_dispatch_ready_at_cursor(content, &harness, Some(1)));
        assert!(!pane_dispatch_ready(content, &harness));
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

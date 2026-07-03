//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_controller::dispatch::dispatch_payload_pending_in_current_input;
use agent_doc_supervisor::detection as supervisor_detection;

pub(crate) fn record_recent_output(shared: &SupervisorShared, bytes: &[u8]) {
    shared.output.record_recent_output(bytes);
}

pub(crate) fn record_terminal_screen(shared: &SupervisorShared, bytes: &[u8]) {
    shared.output.record_terminal_screen(bytes);
}

pub(crate) fn reset_terminal_screen(shared: &SupervisorShared, size: PtySize) {
    shared.output.reset_terminal_screen(size);
}

pub(crate) fn child_output_for_detection(shared: &SupervisorShared) -> String {
    shared.output.child_output_for_detection()
}

pub(crate) fn prompt_visible_requires_ready_transition(shared: &SupervisorShared) -> bool {
    let first_prompt_for_child = !shared.prompt_visible_once.swap(true, Ordering::Relaxed);
    let actor_known_non_ready = shared
        .actor_state
        .lock()
        .unwrap()
        .is_some_and(|state| state != agent_doc_sqlite::state_store::ActorState::Ready);
    supervisor_detection::prompt_visible_requires_ready_transition(
        first_prompt_for_child,
        actor_known_non_ready,
    )
}

pub(crate) fn current_child_prompt_visible(
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
) -> bool {
    let output = child_output_for_detection(shared);
    harness.output_prompt_visible(&output)
}

pub(crate) fn idle_queue_prompt_visible(
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
) -> bool {
    let output = child_output_for_detection(shared);
    let actor_ready = actor_state_is_ready(shared);
    match supervisor_detection::idle_queue_prompt_visibility(&output, harness, actor_ready) {
        supervisor_detection::IdleQueuePromptVisibility::Visible => true,
        supervisor_detection::IdleQueuePromptVisibility::Hidden => false,
        // `#runexitrestart`: when the actor is not yet Ready but the
        // edge-triggered PTY buffer shows a weak prompt glyph, re-check the live
        // tmux pane for a dispatch-ready prompt before letting the idle queue
        // drain. Failed/absent capture falls back to the prior PTY signal.
        supervisor_detection::IdleQueuePromptVisibility::NeedsLivePaneDispatchReady => {
            supervisor_detection::idle_queue_prompt_visible_after_live_pane_dispatch_ready(
                supervisor_pane_dispatch_ready(shared, harness),
            )
        }
    }
}

pub(crate) fn actor_state_is_ready(shared: &SupervisorShared) -> bool {
    shared
        .actor_state
        .lock()
        .unwrap()
        .is_some_and(|state| state == agent_doc_sqlite::state_store::ActorState::Ready)
}

pub(crate) fn ready_busy_blocker_reason(
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
) -> Option<String> {
    let output = child_output_for_detection(shared);
    supervisor_detection::ready_busy_blocker_reason(&output, harness)
}

/// `#runexitrestart`: fresh-tmux-capture dispatch-ready evidence for the
/// supervisor idle-watch drain gate. Captures the owned pane live (mirroring
/// [`supervisor_pane_has_busy_cue`], not the edge-triggered pty `terminal_screen`
/// buffer that can miss a restarted composer's redraw) and reports whether it
/// proves a harness dispatch-ready prompt via the same
/// [`agent_doc_harness::ready_prompt_candidate`] predicate the route /
/// cold-start gates use (latest prompt is `is_dispatch_ready_prompt_line` — a
/// genuinely empty, submit-ready composer — with no busy cue). `Some(true)` =
/// submit-ready; `Some(false)` = a prompt glyph but not yet submit-ready (still
/// starting / drafted composer); `None` = no pane id or the capture failed, so
/// the caller must never let unreadable evidence suppress a legitimate drain.
pub(crate) fn supervisor_pane_dispatch_ready(
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
) -> Option<bool> {
    let pane = shared
        .inject_pane
        .clone()
        .or_else(|| shared.actor_runtime.as_ref().map(|r| r.pane_id.clone()))?;
    let tmux = tmux_router::Tmux::default_server();
    let content = agent_doc_tmux_io::capture_pane(&tmux, &pane).ok()?;
    Some(supervisor_detection::pane_dispatch_ready(&content, harness))
}

/// Whether the authoritative in-memory actor state is `busy` or `starting` —
/// the two non-dispatchable states a stale projection can wedge in.
pub(crate) fn actor_state_is_busy_or_starting(shared: &SupervisorShared) -> bool {
    shared.actor_state.lock().unwrap().is_some_and(|state| {
        matches!(
            state,
            agent_doc_sqlite::state_store::ActorState::Busy
                | agent_doc_sqlite::state_store::ActorState::Starting
        )
    })
}

/// Direct pane evidence for the stale-busy reconcile: capture the supervisor's
/// owned pane fresh via tmux and report whether the harness shows a busy cue.
/// Returns `None` when no pane id is known or the capture fails, so the caller
/// treats unreadable evidence as "not proven idle" and never reconciles on it.
///
/// This deliberately reads the live pane (the same `has_busy_cue` test
/// `route.rs` uses for its stale-busy repairs) rather than the supervisor's
/// edge-triggered pty `terminal_screen` buffer — that buffer is exactly what
/// missed the composer redraw and left the actor wedged.
pub(crate) fn supervisor_pane_has_busy_cue(
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
) -> Option<bool> {
    let pane = shared
        .inject_pane
        .clone()
        .or_else(|| shared.actor_runtime.as_ref().map(|r| r.pane_id.clone()))?;
    let tmux = tmux_router::Tmux::default_server();
    let content = agent_doc_tmux_io::capture_pane(&tmux, &pane).ok()?;
    Some(supervisor_detection::pane_has_busy_cue(&content, harness))
}

/// `#qflood2` pre-send dedup: capture the supervisor's owned pane fresh and
/// report whether the routed drain payload (trigger or `/clear`) is already
/// pending/visible in the harness composer. Returns `None` when no pane id is
/// known or the capture fails, so the caller treats unreadable evidence as "not
/// proven pending" and dispatches normally — a failed capture must never
/// suppress a legitimate dispatch; only a positive match dedups.
///
pub(crate) fn supervisor_pane_payload_already_pending(
    shared: &SupervisorShared,
    payload: &str,
    harness: &agent_doc_harness::HarnessConfig,
) -> Option<bool> {
    let pane = shared
        .inject_pane
        .clone()
        .or_else(|| shared.actor_runtime.as_ref().map(|r| r.pane_id.clone()))?;
    let tmux = tmux_router::Tmux::default_server();
    let content = agent_doc_tmux_io::capture_pane(&tmux, &pane).ok()?;
    Some(dispatch_payload_pending_in_current_input(
        &content,
        payload,
        |line| harness.is_dispatch_ready_prompt_line(line),
        |line| harness.is_prompt_line(line),
    ))
}

/// Normalize raw stdin bytes while OpenCode's permission prompt is active.
/// Orchestration owns the live buffers; the deterministic prompt/key logic
/// lives in `agent-doc-turn-executor-tmux`.
pub(crate) fn normalize_stdin_for_harness_permission_prompt(
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
    data: &[u8],
) -> Option<Vec<u8>> {
    if harness.binary != "opencode" {
        return None;
    }
    let output = child_output_for_detection(shared);
    shared.output.with_recent_output(|raw| {
        agent_doc_turn_executor_tmux::prompt::normalize_opencode_permission_stdin(
            &output, raw, data,
        )
    })
}

pub(crate) fn is_help_screen_visible(
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
) -> bool {
    supervisor_detection::help_screen_visible(&child_output_for_detection(shared), harness)
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_config::Config;
    use agent_doc_frontmatter::frontmatter::Frontmatter;
    use agent_doc_hooks_io::fire_doc_hooks;
    use agent_doc_project_config_io as project_config_io;
    use std::collections::HashMap;
    use tempfile::TempDir;
    use tmux_router::IsolatedTmux;
    #[test]
    fn stale_busy_reconcile_preserves_already_dispatched_head_dedup() {
        let mut idle_busy_ticks = STALE_BUSY_RECONCILE_TICKS;
        let last_dispatched =
            agent_doc_supervisor::idle_reconcile::reconcile_stale_busy_idle_queue_state(
                Some("do [#learn-ohio-duplicate-gate]".to_string()),
                &mut idle_busy_ticks,
            );

        assert_eq!(idle_busy_ticks, 0);
        assert_eq!(
            idle_queue_drain_decision(
                false,
                true,
                false,
                false,
                false,
                Some("do [#learn-ohio-duplicate-gate]"),
                last_dispatched.as_deref(),
            ),
            IdleQueueDrainDecision::SkipAlreadyDispatched
        );
    }
    #[test]
    fn idle_queue_prompt_visible_trusts_ready_actor_over_stale_renderer_tail() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::claude();
        *shared.actor_state.lock().unwrap() =
            Some(agent_doc_sqlite::state_store::ActorState::Ready);
        record_recent_output(&shared, b"turn committed, renderer tail has no composer\n");

        assert!(
            !current_child_prompt_visible(&shared, &harness),
            "stale output alone should not prove an idle composer"
        );
        assert!(
            idle_queue_prompt_visible(&shared, &harness),
            "the supervisor's ready actor state should let the idle queue drain"
        );
    }
    #[test]
    fn idle_queue_prompt_visible_keeps_blocker_over_ready_actor() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::claude();
        *shared.actor_state.lock().unwrap() =
            Some(agent_doc_sqlite::state_store::ActorState::Ready);
        record_recent_output(
            &shared,
            concat!(
                "✶ Generating… (3s · esc to interrupt)\n",
                "❯\n",
                "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host\n",
                "  ⏵⏵ bypass permissions on · 1 shell\n",
            )
            .as_bytes(),
        );

        assert!(
            !idle_queue_prompt_visible(&shared, &harness),
            "active-turn blockers must win over a stale ready actor state"
        );
    }
    #[test]
    fn route_owned_live_pane_busy_requires_idle_prompt_before_reap() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        shared.running.store(true, Ordering::Relaxed);
        record_recent_output(&shared, b"exploring repository\n");

        let reason = route_owned_live_pane_busy_reason(&shared, &harness)
            .expect("running child without prompt should block route-owned reap");

        assert!(reason.contains("live_pane_busy_no_idle_prompt"));
        assert!(reason.contains("exploring repository"));
    }
    #[test]
    fn route_owned_live_pane_busy_trusts_ready_actor_over_stale_renderer_tail() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        shared.running.store(true, Ordering::Relaxed);
        *shared.actor_state.lock().unwrap() =
            Some(agent_doc_sqlite::state_store::ActorState::Ready);
        record_recent_output(&shared, "›\n".as_bytes());
        record_recent_output(&shared, b"Working...\n");
        record_recent_output(
            &shared,
            "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n".as_bytes(),
        );

        assert_eq!(route_owned_live_pane_busy_reason(&shared, &harness), None);
    }
    #[test]
    fn route_owned_live_pane_busy_keeps_ready_actor_for_blocking_prompt_state() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        shared.running.store(true, Ordering::Relaxed);
        *shared.actor_state.lock().unwrap() =
            Some(agent_doc_sqlite::state_store::ActorState::Ready);
        record_recent_output(&shared, "›\n".as_bytes());
        record_recent_output(&shared, b"tab to queue message\n");
        record_recent_output(
            &shared,
            "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n".as_bytes(),
        );

        let reason = route_owned_live_pane_busy_reason(&shared, &harness)
            .expect("queued composer state must still block route-owned reap");

        assert!(reason.contains("live_pane_busy_blocked_prompt"));
        assert!(reason.contains("queued draft in composer"));
    }
    #[test]
    fn ready_busy_blocker_reason_filters_to_recoverable_queue_draft() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        record_recent_output(&shared, "›\n".as_bytes());
        record_recent_output(&shared, b"tab to queue message\n");
        record_recent_output(
            &shared,
            "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n".as_bytes(),
        );

        assert_eq!(
            ready_busy_blocker_reason(&shared, &harness).as_deref(),
            Some("queued draft in composer")
        );

        let active_shared = SupervisorShared::new("test", "test-instance".to_string());
        record_recent_output(
            &active_shared,
            "• Working (1m 34s • esc to interrupt)\n\n› Write tests\n".as_bytes(),
        );
        record_recent_output(
            &active_shared,
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used\n".as_bytes(),
        );
        assert_eq!(ready_busy_blocker_reason(&active_shared, &harness), None);
    }
    #[test]
    fn route_owned_live_pane_busy_allows_idle_prompt_reap() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::codex();
        shared.running.store(true, Ordering::Relaxed);
        record_recent_output(&shared, b"done\n");
        record_recent_output(&shared, "›\n".as_bytes());

        assert_eq!(route_owned_live_pane_busy_reason(&shared, &harness), None);
    }
    #[test]
    fn is_help_screen_visible_detects_opencode_help() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::opencode();
        record_recent_output(
            &shared,
            b"opencode [project]           start opencode tui\n",
        );
        record_recent_output(
            &shared,
            b"opencode run [message..]     run opencode with a message\n",
        );
        record_recent_output(
            &shared,
            b"opencode debug               debugging and troubleshooting tools\n",
        );
        assert!(is_help_screen_visible(&shared, &harness));
    }
    #[test]
    fn is_help_screen_visible_rejects_normal_opencode_output() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        let harness = agent_doc_harness::HarnessConfig::opencode();
        record_recent_output(&shared, b"some normal output\n");
        record_recent_output(&shared, b">\n");
        assert!(!is_help_screen_visible(&shared, &harness));
    }
    #[test]
    fn prompt_visible_requires_ready_transition_on_first_prompt() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        assert!(prompt_visible_requires_ready_transition(&shared));
        assert!(
            !prompt_visible_requires_ready_transition(&shared),
            "a repeated prompt without an intervening busy transition should not retrigger ready"
        );
    }
    #[test]
    fn prompt_visible_requires_ready_transition_after_busy_dispatch() {
        let shared = SupervisorShared::new("test", "test-instance".to_string());
        shared.prompt_visible_once.store(true, Ordering::Relaxed);
        *shared.actor_state.lock().unwrap() = Some(agent_doc_sqlite::state_store::ActorState::Busy);
        assert!(
            prompt_visible_requires_ready_transition(&shared),
            "a busy actor that surfaces the prompt again must return to ready"
        );
    }
}

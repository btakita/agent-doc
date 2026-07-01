//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_controller::dispatch::{
    recent_lines_contain_trigger, route_trigger_visible_in_current_draft,
};
use agent_doc_supervisor::idle_reconcile::recoverable_ready_busy_blocker_reason;
use agent_doc_turn_executor_tmux::context_clear::context_clear_command_visible_in_active_input;

pub(crate) fn record_recent_output(shared: &SupervisorShared, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let mut recent = shared.recent_output.lock().unwrap();
    recent.extend_from_slice(bytes);
    if recent.len() > AUTO_TRIGGER_OUTPUT_BYTES_MAX {
        let overflow = recent.len() - AUTO_TRIGGER_OUTPUT_BYTES_MAX;
        recent.drain(..overflow);
    }
}

pub(crate) fn record_terminal_screen(shared: &SupervisorShared, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    shared.terminal_screen.lock().unwrap().push(bytes);
}

pub(crate) fn reset_terminal_screen(shared: &SupervisorShared, size: PtySize) {
    shared.terminal_screen.lock().unwrap().reset(size);
}

pub(crate) fn child_output_for_detection(shared: &SupervisorShared) -> String {
    let screen = shared.terminal_screen.lock().unwrap().visible_text();
    if screen.trim().is_empty() {
        let recent = shared.recent_output.lock().unwrap();
        String::from_utf8_lossy(&recent).into_owned()
    } else {
        screen
    }
}

pub(crate) fn prompt_visible_requires_ready_transition(shared: &SupervisorShared) -> bool {
    let first_prompt_for_child = !shared.prompt_visible_once.swap(true, Ordering::Relaxed);
    if first_prompt_for_child {
        return true;
    }
    shared
        .actor_state
        .lock()
        .unwrap()
        .is_some_and(|state| state != agent_doc_sqlite::state_store::ActorState::Ready)
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
    if harness.dispatch_blocker_reason(&output).is_some() {
        return false;
    }
    if shared
        .actor_state
        .lock()
        .unwrap()
        .is_some_and(|state| state == agent_doc_sqlite::state_store::ActorState::Ready)
    {
        return true;
    }
    // `#runexitrestart`: the actor is NOT yet `Ready` (e.g. `Starting` right
    // after a session restart / "Press Enter to restart" / supervisor re-exec).
    // The edge-triggered pty `terminal_screen` buffer can show a prompt *glyph*
    // (`matches_prompt`) while the freshly-restarted harness composer is not yet
    // submit-ready — the same cold-start dispatch race the route auto-start gate
    // (`#jbtsiftnosub`) closes, but here in the supervisor idle-watch drain loop.
    // Treating that transient glyph as dispatchable lets the per-tick drain
    // re-inject the `agent-doc <FILE>` trigger into a not-yet-ready composer (the
    // Enter never submits), and each idle tick stacks another un-submitted copy
    // (the operator-observed ~7 duplicate triggers with no submit). Before the
    // weak pty-buffer signal is trusted off the `Ready` fast path, re-verify
    // against a *fresh* tmux capture using the same `agent_doc_harness::ready_prompt_candidate`
    // dispatch-ready predicate the route gate uses. A fresh capture that cannot
    // prove a submit-ready prompt fails closed (defers the drain this tick); a
    // failed/absent capture (`None`) falls back to the pty-buffer signal so an
    // unreadable pane never permanently suppresses a legitimate drain.
    if !harness.output_prompt_visible(&output) {
        return false;
    }
    // A fresh capture proving submit-ready dispatches; one that is only a
    // not-yet-ready glyph fails closed (defers); an unreadable/absent capture
    // (`None`) conservatively falls back to the prior pty-buffer signal.
    supervisor_pane_dispatch_ready(shared, harness).unwrap_or(true)
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
    harness
        .dispatch_blocker_reason(&output)
        .filter(|reason| recoverable_ready_busy_blocker_reason(reason))
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
    let content = crate::sessions::capture_pane(&tmux, &pane).ok()?;
    Some(agent_doc_harness::ready_prompt_candidate(&content, harness).is_some())
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
    let content = crate::sessions::capture_pane(&tmux, &pane).ok()?;
    Some(harness.has_busy_cue(&content))
}

/// `#qflood2` pre-send dedup: capture the supervisor's owned pane fresh and
/// report whether the routed drain payload (trigger or `/clear`) is already
/// pending/visible in the harness composer. Returns `None` when no pane id is
/// known or the capture fails, so the caller treats unreadable evidence as "not
/// proven pending" and dispatches normally — a failed capture must never
/// suppress a legitimate dispatch; only a positive match dedups.
///
/// Reuses `agent_doc_controller::dispatch::recent_lines_contain_trigger`, the same
/// still-in-composer detector the route acceptance poll uses, so the dedup is
/// keyed off the harness composer rather than scrollback far above it. For
/// `agent-doc <path>` triggers, also reuse route's draft equivalence matcher so
/// a visible relative-path draft dedups an absolute trigger for the same file.
pub(crate) fn supervisor_pane_payload_pending_in_content(
    content: &str,
    payload: &str,
    harness: &agent_doc_harness::HarnessConfig,
) -> bool {
    if agent_doc_queue::queue_command::is_context_clear_command(payload) {
        return context_clear_command_visible_in_active_input(content, payload, |line| {
            harness.is_dispatch_ready_prompt_line(line)
        });
    }
    recent_lines_contain_trigger(content, payload)
        || route_trigger_visible_in_current_draft(content, payload, |line| {
            harness.is_prompt_line(line)
        })
}

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
    let content = crate::sessions::capture_pane(&tmux, &pane).ok()?;
    Some(supervisor_pane_payload_pending_in_content(
        &content, payload, harness,
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
    let raw = shared.recent_output.lock().unwrap();
    agent_doc_turn_executor_tmux::prompt::normalize_opencode_permission_stdin(&output, &raw, data)
}

pub(crate) fn is_help_screen_visible(
    shared: &SupervisorShared,
    harness: &agent_doc_harness::HarnessConfig,
) -> bool {
    harness.is_help_screen_output(&child_output_for_detection(shared))
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::hooks::fire_doc_hooks;
    use agent_doc_config::Config;
    use agent_doc_frontmatter::frontmatter::Frontmatter;
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
    fn supervisor_pending_payload_matches_relative_codex_agent_doc_draft() {
        let harness = agent_doc_harness::HarnessConfig::codex();
        let payload =
            "agent-doc /home/brian/work/btakita/agent-loop/src/sample-app/tasks/sampleorders.md";
        let content = "\
› agent-doc tasks/sampleorders.md
agent-doc tasks/sampleorders.md
agent-doc tasks/sampleorders.md
agent-doc tasks/sampleorders.md
gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/sample-app · Context 0% use
";

        assert!(
            supervisor_pane_payload_pending_in_content(content, payload, &harness),
            "idle-queue dedupe must recognize equivalent relative Codex drafts before appending another trigger"
        );
    }

    #[test]
    fn supervisor_pending_payload_detects_codex_context_clear_draft() {
        let harness = agent_doc_harness::HarnessConfig::codex();
        let content = concat!(
            "older output\n",
            "› /clear\n",
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used\n",
        );

        assert!(
            supervisor_pane_payload_pending_in_content(content, "/clear", &harness),
            "idle-queue recovery must see a visible Codex /clear draft and resubmit Enter"
        );
    }

    #[test]
    fn supervisor_pending_payload_ignores_submitted_context_clear_scrollback() {
        let harness = agent_doc_harness::HarnessConfig::claude();
        let content = concat!(
            "✶ Generating... (3s · esc to interrupt)\n",
            "  ❯ /clear\n",
            "────────────────────\n",
            "❯ Press up to edit queued messages\n",
            "────────────────────\n",
            "  Opus 4.8 ctx:10% ~/work/btakita/agent-loop main brian@host\n",
        );

        assert!(
            !supervisor_pane_payload_pending_in_content(content, "/clear", &harness),
            "a prior submitted /clear in scrollback must not suppress or resubmit the next drain"
        );
    }

    #[test]
    fn supervisor_pending_payload_detects_opencode_new_palette_row() {
        let harness = agent_doc_harness::HarnessConfig::opencode();
        let content = concat!(
            "older output\n",
            "/new        New session\n",
            "/models     Select model\n",
            "> /new\n",
        );

        assert!(supervisor_pane_payload_pending_in_content(
            content, "/new", &harness
        ));
    }

    #[test]
    fn supervisor_pending_payload_detects_opencode_selected_new_session_command() {
        let harness = agent_doc_harness::HarnessConfig::opencode();
        let content = concat!(
            "older output\n",
            "> New session\n",
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n",
        );

        assert!(
            supervisor_pane_payload_pending_in_content(content, "/new", &harness),
            "OpenCode can replace `/new` with the selected command label before the final submit Enter"
        );

        let structured = concat!(
            "older output\n",
            "> session_new\n",
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n",
        );
        assert!(
            supervisor_pane_payload_pending_in_content(structured, "/new", &harness),
            "OpenCode can also surface the selected command id before submission"
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

//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

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
        .is_some_and(|state| state != crate::session_actor::ActorState::Ready)
}

pub(crate) fn current_child_prompt_visible(
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
) -> bool {
    let output = child_output_for_detection(shared);
    child_output_prompt_visible(harness, &output)
}

pub(crate) fn child_output_prompt_visible(harness: &crate::harness::HarnessConfig, output: &str) -> bool {
    // #opencode-idle-detection-post-turn: for OpenCode, check only the bottom
    // N lines for idle chrome instead of requiring the entire scrollback to be
    // ignorable chrome. After a turn completes the pane keeps completed-turn
    // output in scrollback above the idle bottom chrome; the all-lines
    // is_idle_chrome_only_output returns false for those non-ignorable
    // scrollback lines, preventing idle detection. The bottom-N approach
    // mirrors dispatch_blocker_reason's strategy.
    if harness.binary == "opencode" && harness.is_bottom_idle_chrome(output, 12) {
        return true;
    }
    if harness.is_idle_chrome_only_output(output) {
        return true;
    }
    let Some(line) = harness.last_prompt_candidate(output) else {
        return false;
    };
    let stripped = crate::prompt::strip_ansi(&line);
    harness.matches_prompt(stripped.trim())
}

pub(crate) fn idle_queue_prompt_visible(
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
) -> bool {
    let output = child_output_for_detection(shared);
    if harness.dispatch_blocker_reason(&output).is_some() {
        return false;
    }
    if shared
        .actor_state
        .lock()
        .unwrap()
        .is_some_and(|state| state == crate::session_actor::ActorState::Ready)
    {
        return true;
    }
    child_output_prompt_visible(harness, &output)
}

/// Whether the authoritative in-memory actor state is `busy` or `starting` —
/// the two non-dispatchable states a stale projection can wedge in.
pub(crate) fn actor_state_is_busy_or_starting(shared: &SupervisorShared) -> bool {
    shared.actor_state.lock().unwrap().is_some_and(|state| {
        matches!(
            state,
            crate::session_actor::ActorState::Busy | crate::session_actor::ActorState::Starting
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
    harness: &crate::harness::HarnessConfig,
) -> Option<bool> {
    let pane = shared
        .inject_pane
        .clone()
        .or_else(|| shared.actor_runtime.as_ref().map(|r| r.pane_id.clone()))?;
    let tmux = crate::sessions::Tmux::default_server();
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
/// Reuses `route::cycle_ack::recent_lines_contain_trigger`, the same
/// still-in-composer detector the route acceptance poll uses, so the dedup is
/// keyed off the harness composer rather than scrollback far above it.
pub(crate) fn supervisor_pane_payload_already_pending(
    shared: &SupervisorShared,
    payload: &str,
) -> Option<bool> {
    let pane = shared
        .inject_pane
        .clone()
        .or_else(|| shared.actor_runtime.as_ref().map(|r| r.pane_id.clone()))?;
    let tmux = crate::sessions::Tmux::default_server();
    let content = crate::sessions::capture_pane(&tmux, &pane).ok()?;
    Some(crate::route::recent_lines_contain_trigger(&content, payload))
}

/// Decide whether the idle-queue watch should reconcile a stale-busy actor back
/// to ready (`#stale-busy-after-auto-inject-no-clear`).
///
/// The supervisor's one-shot pty completion transition (busy→ready on
/// `prompt_ready`, see the pty→stdout thread) is edge-triggered on the latest
/// output chunk. When an injected turn returns but its composer redraw lands
/// split so the final chunk carries no detectable prompt, the actor can stay
/// wedged `busy` over an idle pane with no further bytes to retrigger the
/// transition. The session then presents as "truly stuck" and a pane kill +
/// restart re-enters the same state.
///
/// This polling backstop self-heals it: reconcile only when the actor is
/// projected busy/starting, the live pane shows no busy cue, no clear cooldown
/// is pausing the loop, and the idle-over-busy condition has held for
/// `STALE_BUSY_RECONCILE_TICKS` consecutive polls (debounce so a turn that is
/// still spinning up is never cut short). Pure for unit testing the gate.
pub(crate) fn stale_busy_idle_reconcile_decision(
    actor_busy: bool,
    pane_has_busy_cue: bool,
    clear_cooldown_active: bool,
    consecutive_idle_busy_ticks: u32,
) -> bool {
    actor_busy
        && !pane_has_busy_cue
        && !clear_cooldown_active
        && consecutive_idle_busy_ticks >= STALE_BUSY_RECONCILE_TICKS
}

pub(crate) fn reconcile_stale_busy_idle_queue_state(
    last_dispatched: Option<String>,
    idle_busy_ticks: &mut u32,
) -> Option<String> {
    *idle_busy_ticks = 0;
    last_dispatched
}

pub(crate) fn opencode_permission_prompt_active(shared: &SupervisorShared) -> bool {
    // Primary: parse terminal screen for the structured permission dialog
    let output = child_output_for_detection(shared);
    let prompt = crate::prompt::parse_prompt(&output);
    if prompt.active
        && prompt.options.as_ref().is_some_and(|options| {
            options.iter().any(|option| option.label == "Allow once")
                && options.iter().any(|option| option.label == "Allow always")
                && options.iter().any(|option| option.label == "Reject")
        })
    {
        return true;
    }
    // Fallback: detect via the orange selection highlight in raw output bytes.
    // OpenCode uses ANSI 48;2;245;167;66 (amber) to mark the selected permission
    // option. This fires even when the footer text changes across OpenCode versions.
    let raw = shared.recent_output.lock().unwrap();
    let raw_str = String::from_utf8_lossy(&raw);
    raw_str.contains("\x1b[48;2;245;167;66m")
        && (raw_str.contains("Allow once")
            || raw_str.contains("Allow always")
            || raw_str.contains("Reject"))
}

pub(crate) fn translate_opencode_permission_arrow_keys(data: &[u8]) -> Option<Vec<u8>> {
    let mut translated = Vec::with_capacity(data.len());
    let mut changed = false;
    let mut i = 0;
    while i < data.len() {
        let replacement = if data[i..].starts_with(b"\x1b[C")
            || data[i..].starts_with(b"\x1b[B")
            || data[i..].starts_with(b"\x1bOC")
            || data[i..].starts_with(b"\x1bOB")
        {
            Some((&b"\t"[..], 3))
        } else if data[i..].starts_with(b"\x1b[D")
            || data[i..].starts_with(b"\x1b[A")
            || data[i..].starts_with(b"\x1bOD")
            || data[i..].starts_with(b"\x1bOA")
        {
            Some((&b"\x1b[Z"[..], 3))
        } else {
            None
        };

        if let Some((bytes, consumed)) = replacement {
            translated.extend_from_slice(bytes);
            i += consumed;
            changed = true;
        } else {
            translated.push(data[i]);
            i += 1;
        }
    }

    changed.then_some(translated)
}

pub(crate) fn normalize_stdin_for_harness_permission_prompt(
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
    data: &[u8],
) -> Option<Vec<u8>> {
    if harness.binary != "opencode" || !opencode_permission_prompt_active(shared) {
        return None;
    }
    translate_opencode_permission_arrow_keys(data)
}

pub(crate) fn is_help_screen_visible(
    shared: &SupervisorShared,
    harness: &crate::harness::HarnessConfig,
) -> bool {
    harness.is_help_screen_output(&child_output_for_detection(shared))
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
use crate::config::Config;
use crate::frontmatter::Frontmatter;
use crate::hooks::fire_doc_hooks;
use crate::project_config;
use crate::sessions::IsolatedTmux;
use std::collections::HashMap;
use tempfile::TempDir;
#[test]
fn stale_busy_reconcile_fires_after_debounce_over_idle_pane() {
    // Actor wedged busy, live pane shows no busy cue, cooldown clear, and the
    // idle-over-busy condition has held for the full debounce window: reconcile.
    assert!(stale_busy_idle_reconcile_decision(
        true,
        false,
        false,
        STALE_BUSY_RECONCILE_TICKS
    ));
}
#[test]
fn stale_busy_reconcile_preserves_already_dispatched_head_dedup() {
    let mut idle_busy_ticks = STALE_BUSY_RECONCILE_TICKS;
    let last_dispatched = reconcile_stale_busy_idle_queue_state(
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
            Some("do [#learn-ohio-duplicate-gate]"),
            last_dispatched.as_deref(),
        ),
        IdleQueueDrainDecision::SkipAlreadyDispatched
    );
}
#[test]
fn stale_busy_reconcile_waits_for_full_debounce() {
    // One idle observation is not enough — a turn spinning up briefly shows no
    // busy cue. Hold off until the debounce window is satisfied.
    for ticks in 0..STALE_BUSY_RECONCILE_TICKS {
        assert!(
            !stale_busy_idle_reconcile_decision(true, false, false, ticks),
            "should not reconcile after only {ticks} idle ticks"
        );
    }
}
#[test]
fn stale_busy_reconcile_skips_when_pane_busy() {
    // The pane shows a busy cue — a turn is genuinely running. Never reconcile,
    // regardless of how many ticks elapsed.
    assert!(!stale_busy_idle_reconcile_decision(
        true,
        true,
        false,
        STALE_BUSY_RECONCILE_TICKS + 10
    ));
}
#[test]
fn stale_busy_reconcile_skips_when_actor_ready() {
    // Actor already dispatchable: nothing to reconcile.
    assert!(!stale_busy_idle_reconcile_decision(
        false,
        false,
        false,
        STALE_BUSY_RECONCILE_TICKS
    ));
}
#[test]
fn stale_busy_reconcile_skips_during_clear_cooldown() {
    // A non-interrupting operator clear paused the loop; do not race it by
    // flipping the actor ready underneath the deferred clear.
    assert!(!stale_busy_idle_reconcile_decision(
        true,
        false,
        true,
        STALE_BUSY_RECONCILE_TICKS
    ));
}
#[test]
fn opencode_permission_prompt_translates_legacy_arrows_to_tab_controls() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::opencode();
    record_recent_output(
        &shared,
        b"\x1b[48;2;245;167;66mAllow once\x1b[0m Allow always Reject ctrl+f fullscreen \xe2\x87\x86 select enter confirm\n",
    );

    let translated = normalize_stdin_for_harness_permission_prompt(
        &shared,
        &harness,
        b"\x1b[C\x1b[C\x1b[D\x1b[Atext",
    )
    .expect("OpenCode permission prompt should translate legacy arrow escapes");

    assert_eq!(translated, b"\t\t\x1b[Z\x1b[Ztext");
}
#[test]
fn opencode_permission_prompt_translation_is_gated_to_permission_dialog() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::opencode();
    record_recent_output(&shared, b"Ask anything...\n");

    assert!(
        normalize_stdin_for_harness_permission_prompt(&shared, &harness, b"\x1b[C").is_none(),
        "normal OpenCode prompt editing must keep arrow keys unchanged"
    );

    let codex = crate::harness::HarnessConfig::codex();
    record_recent_output(
        &shared,
        b"\x1b[48;2;245;167;66mAllow once\x1b[0m Allow always Reject ctrl+f fullscreen \xe2\x87\x86 select enter confirm\n",
    );
    assert!(
        normalize_stdin_for_harness_permission_prompt(&shared, &codex, b"\x1b[C").is_none(),
        "non-OpenCode harnesses must not receive OpenCode permission key translation"
    );
}
#[test]
fn opencode_permission_prompt_fallback_detects_orange_highlight_without_footer() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::opencode();
    // Simulate a newer OpenCode version where the footer text changed but the
    // orange selection highlight (48;2;245;167;66) is still present.
    record_recent_output(
        &shared,
        b"\x1b[48;2;245;167;66mAllow once\x1b[0m Allow always Reject\n",
    );
    let translated = normalize_stdin_for_harness_permission_prompt(
        &shared, &harness, b"\x1b[C",
    )
    .expect("fallback detection must translate arrows even without the standard footer text");
    assert_eq!(translated, b"\t");
}
#[test]
fn opencode_permission_prompt_fallback_requires_allow_or_reject_label() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::opencode();
    // Orange highlight alone (no permission labels) must not trigger translation.
    record_recent_output(
        &shared,
        b"\x1b[48;2;245;167;66msome other highlighted text\x1b[0m\n",
    );
    assert!(
        normalize_stdin_for_harness_permission_prompt(&shared, &harness, b"\x1b[C").is_none(),
        "orange highlight without permission labels must not trigger arrow translation"
    );
}
#[test]
fn current_child_prompt_visible_uses_latest_nonempty_line() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::codex();
    record_recent_output(&shared, b"old output\n");
    record_recent_output(&shared, "❯\n".as_bytes());
    record_recent_output(&shared, b"resumed child still printing\n");
    assert!(
        !current_child_prompt_visible(&shared, &harness),
        "an earlier prompt in the current child transcript should not count once newer non-prompt output follows it"
    );
}
#[test]
fn current_child_prompt_visible_accepts_prompt_from_current_child_output() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::codex();
    record_recent_output(&shared, b"resumed child ready\n");
    record_recent_output(&shared, "❯\n".as_bytes());
    assert!(current_child_prompt_visible(&shared, &harness));
}
#[test]
fn current_child_prompt_visible_handles_suffix_prompt_line() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::codex();
    record_recent_output(&shared, "/tmp/project ❯\n".as_bytes());
    assert!(current_child_prompt_visible(&shared, &harness));
}
#[test]
fn current_child_prompt_visible_skips_codex_footer_line() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::codex();
    record_recent_output(&shared, "›\n".as_bytes());
    record_recent_output(
        &shared,
        "gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used\n".as_bytes(),
    );
    assert!(current_child_prompt_visible(&shared, &harness));
}
#[test]
fn current_child_prompt_visible_rejects_busy_output_above_codex_footer() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::codex();
    record_recent_output(&shared, "›\n".as_bytes());
    record_recent_output(&shared, b"resumed child still printing\n");
    record_recent_output(
        &shared,
        "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n".as_bytes(),
    );
    assert!(!current_child_prompt_visible(&shared, &harness));
}
#[test]
fn current_child_prompt_visible_accepts_opencode_status_chrome_without_proof_output() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::opencode();
    record_recent_output(
        &shared,
        "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n".as_bytes(),
    );
    assert!(current_child_prompt_visible(&shared, &harness));
}
#[test]
fn current_child_prompt_visible_accepts_opencode_idle_splash_without_prompt_glyph() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::opencode();
    record_recent_output(
        &shared,
        "\
                                                                                                 ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▄ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀
                                                                               ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                               ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                               ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                               tab agents  ctrl+p commands
                                                                                    ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
"
        .as_bytes(),
    );
    assert!(current_child_prompt_visible(&shared, &harness));
}
#[test]
fn current_child_prompt_visible_detects_opencode_post_turn_idle() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::opencode();
    record_recent_output(
        &shared,
        "\
$ cargo test -p agent-doc-orchestration
   Compiling agent-doc-orchestration
Finished test profile
 Running unittests src/lib.rs
test result: ok. 2219 passed; 0 failed
Thought: 7.6s
Click to expand
The change is complete and all tests pass.
src/harness.rs: added is_bottom_idle_chrome method
src/harness.rs: tests for is_bottom_idle_chrome
src/start.rs: updated child_output_prompt_visible
src/start.rs: test for post-turn idle detection
cargo test -p agent-doc-orchestration — 2219 passed
cargo check --bin agent-doc — clean
cargo install — installed agent-doc 0.34.0
                                                                               ┃
                                                                               ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                               ┃
                                                                               ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                               ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                tab agents  ctrl+p commands
                                                                                     ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
"
        .as_bytes(),
    );
    assert!(
        current_child_prompt_visible(&shared, &harness),
        "post-turn OpenCode pane with idle bottom chrome must be detected as prompt-visible"
    );
}
#[test]
fn idle_queue_prompt_visible_trusts_ready_actor_over_stale_renderer_tail() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::claude();
    *shared.actor_state.lock().unwrap() = Some(crate::session_actor::ActorState::Ready);
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
    let harness = crate::harness::HarnessConfig::claude();
    *shared.actor_state.lock().unwrap() = Some(crate::session_actor::ActorState::Ready);
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
    let harness = crate::harness::HarnessConfig::codex();
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
    let harness = crate::harness::HarnessConfig::codex();
    shared.running.store(true, Ordering::Relaxed);
    *shared.actor_state.lock().unwrap() = Some(crate::session_actor::ActorState::Ready);
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
    let harness = crate::harness::HarnessConfig::codex();
    shared.running.store(true, Ordering::Relaxed);
    *shared.actor_state.lock().unwrap() = Some(crate::session_actor::ActorState::Ready);
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
fn route_owned_live_pane_busy_allows_idle_prompt_reap() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::codex();
    shared.running.store(true, Ordering::Relaxed);
    record_recent_output(&shared, b"done\n");
    record_recent_output(&shared, "›\n".as_bytes());

    assert_eq!(route_owned_live_pane_busy_reason(&shared, &harness), None);
}
#[test]
fn is_help_screen_visible_detects_opencode_help() {
    let shared = SupervisorShared::new("test", "test-instance".to_string());
    let harness = crate::harness::HarnessConfig::opencode();
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
    let harness = crate::harness::HarnessConfig::opencode();
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
    *shared.actor_state.lock().unwrap() = Some(crate::session_actor::ActorState::Busy);
    assert!(
        prompt_visible_requires_ready_transition(&shared),
        "a busy actor that surfaces the prompt again must return to ready"
    );
}
}

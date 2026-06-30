//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

use agent_doc_controller::dispatch::{
    compact_trigger_text, line_contains_trigger, recent_lines_contain_trigger,
    shares_trigger_prefix, strip_leading_prompt_prefix,
};
use agent_doc_supervisor::lifecycle::recycle_interrupted_resubmit_should_wait;
use agent_doc_tmux::pane_current_command_is_bare_shell;

/// Outcome of one direct-pane submit-acceptance poll window.
pub(crate) struct DirectPaneAcceptance {
    status: CommandDispatchStatus,
    elapsed: Duration,
    /// Whether the trigger text was still visible in the pane when the window
    /// closed (only meaningful when `status == TimedOut`).
    trigger_visible: bool,
    /// The trigger NEVER landed in the composer (the send silently no-op'd into a
    /// not-ready pane): the composer stayed empty the whole window AND the pane is
    /// sitting at an idle dispatch-ready prompt — so the empty composer is NOT a
    /// fast submit, it's a non-dispatch. The caller re-sends the FULL trigger
    /// (text+Enter), not a bare Enter. (#jbrundispatch directive 2 — "detect if the
    /// prompt was not dispatched into the session, and send + submit it".)
    not_dispatched: bool,
    diagnostic_path: Option<PathBuf>,
}

fn protected_prompt_draft_preview(harness: &HarnessConfig, content: &str) -> Option<String> {
    let candidate = harness.last_prompt_candidate(content)?;
    let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(&candidate);
    let redacted = agent_doc_secret_redact::redact(stripped.trim());
    let preview = redacted.trim();
    if preview.is_empty() {
        return None;
    }
    const MAX_CHARS: usize = 160;
    let mut chars = preview.chars();
    let shortened: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        Some(format!("{shortened}..."))
    } else {
        Some(shortened)
    }
}

/// `#rdypoll` (§D / img_52): process-global count of REAL trigger injections into
/// a harness composer for this `agent-doc route` invocation. A single dispatch
/// should inject the `agent-doc <FILE>` trigger exactly once; a multi-inject
/// regression (the ~7 stacked un-submitted copies the operator saw after a
/// restart) shows up as `attempt=2`, `attempt=3`, … in ops.log. The route process
/// is short-lived (one logical dispatch per invocation), so a monotonic counter
/// makes "did this dispatch type the trigger more than once?" provable from logs.
static DISPATCH_INJECT_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Record one real trigger injection and emit the `dispatch_inject attempt=N`
/// marker. `transport` distinguishes the direct-pane text+Enter send from the
/// supervisor-IPC inject so a regression can be attributed to the right path.
fn log_dispatch_inject(file: &Path, pane: &str, harness: &HarnessConfig, transport: &str) {
    let attempt = DISPATCH_INJECT_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    crate::ops_log::log_op(
        file,
        &format!(
            "dispatch_inject file={} pane={} harness={} transport={} attempt={}",
            file.display(),
            pane,
            harness.binary,
            transport,
            attempt
        ),
    );
}

/// Max bare-Enter resubmits while the trigger is still visible (drafted, not
/// submitted) — the supervisor "retry until the prompt is submitted" budget
/// (`#jbclaudesubmit`). Each resubmit re-polls for the 1s acceptance window, so
/// a visibly drafted trigger gets another submit key at least once/second.
/// Raised from 3 → 30 (and made env-tunable via
/// `AGENT_DOC_DIRECT_PANE_MAX_ENTER_RESUBMITS`) because a slow-to-ready Claude Code
/// composer — which has no submit-proof hook, so dispatch is accepted-only — could
/// exhaust the old 3-nudge budget before the pane focused and consumed the Enter,
/// leaving the Run-Agent-Doc trigger sitting unsent ("doesn't submit to Claude Code").
/// (Claude dispatch is accepted-only — text+Enter delivered without a submit-proof
/// hook.) The loop still exits the moment the trigger is consumed (submitted), so the
/// higher cap only costs extra wall-clock on a genuinely stuck pane.
fn direct_pane_max_enter_resubmits() -> usize {
    std::env::var("AGENT_DOC_DIRECT_PANE_MAX_ENTER_RESUBMITS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT)
}

/// Poll the pane capture until the trigger text is consumed or the acceptance
/// window expires, logging the resulting submit observation. Pure detection —
/// it never sends input — so callers can re-run it after a re-submit attempt.
pub(crate) fn poll_direct_pane_acceptance(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &HarnessConfig,
    trigger: &str,
    phase: &str,
) -> DirectPaneAcceptance {
    let start = std::time::Instant::now();
    let timeout = direct_pane_submit_acceptance_timeout();
    // `#run-agent-doc-latency`: capture-then-sleep, not sleep-then-capture. A pane
    // that consumes the trigger quickly is detected on the first capture (~capture
    // overhead) instead of paying a full poll interval before the first check, and
    // a tighter poll shortens the acceptance floor for slower panes.
    let poll_interval = DIRECT_PANE_SUBMIT_ACCEPTANCE_POLL_INTERVAL;
    let mut last_capture: Option<(bool, usize, String, String)> = None;
    let mut poll_state = DirectPaneAcceptancePollState::default();
    let mut capture_failed = false;
    while start.elapsed() < timeout {
        match sessions::capture_pane(tmux, pane) {
            Ok(content) => {
                let elapsed = start.elapsed();
                let cmd_still_in_input = recent_lines_contain_trigger(&content, trigger);
                let capture_hash = short_content_hash(&content);
                let capture_len = content.len();
                last_capture = Some((cmd_still_in_input, capture_len, capture_hash, content));

                if direct_pane_acceptance_poll_status(&mut poll_state, elapsed, cmd_still_in_input)
                    .is_some()
                {
                    let capture_hash = last_capture.as_ref().map(|(_, _, hash, _)| hash.as_str());
                    log_route_submit_observation(RouteSubmitObservationLogFacts {
                        file,
                        pane,
                        harness,
                        phase,
                        observation: RouteSubmitObservation::Accepted,
                        trigger_visible: Some(false),
                        elapsed,
                        capture_len: Some(capture_len),
                        capture_hash,
                        proof: None,
                    });
                    // #jbrundispatch directive 2: an empty composer is normally a
                    // submit — UNLESS the trigger was NEVER observed in the composer
                    // (so we can't prove it was typed) AND the pane is now sitting at
                    // an idle dispatch-ready prompt. For an agent-doc trigger that
                    // starts a turn, a genuine submit leaves the pane PROCESSING (not
                    // idle), so empty+idle+never-seen means the send no-op'd into a
                    // not-ready pane — the prompt was not dispatched.
                    let not_dispatched = !poll_state.saw_trigger_visible()
                        && last_capture
                            .as_ref()
                            .map(|(_, _, _, content)| pane_idle_dispatch_ready(content, harness))
                            .unwrap_or(false);
                    return DirectPaneAcceptance {
                        status: CommandDispatchStatus::Accepted,
                        elapsed,
                        trigger_visible: false,
                        not_dispatched,
                        diagnostic_path: None,
                    };
                }
            }
            Err(_) => {
                capture_failed = true;
            }
        }
        std::thread::sleep(poll_interval);
    }
    let elapsed = start.elapsed();
    let trigger_visible = last_capture
        .as_ref()
        .map(|(visible, _, _, _)| *visible)
        .unwrap_or(false);
    let mut diagnostic_path = None;
    if let Some((visible, capture_len, capture_hash, content)) = last_capture.as_ref() {
        if *visible {
            diagnostic_path =
                preserve_route_pane_snapshot(file, pane, harness, phase, content).path;
        }
        log_route_submit_observation(RouteSubmitObservationLogFacts {
            file,
            pane,
            harness,
            phase,
            observation: if *visible {
                RouteSubmitObservation::TriggerStillVisible
            } else {
                RouteSubmitObservation::Accepted
            },
            trigger_visible: Some(*visible),
            elapsed,
            capture_len: Some(*capture_len),
            capture_hash: Some(capture_hash.as_str()),
            proof: None,
        });
    } else if capture_failed {
        log_route_submit_observation(RouteSubmitObservationLogFacts {
            file,
            pane,
            harness,
            phase,
            observation: RouteSubmitObservation::CaptureFailed,
            trigger_visible: None,
            elapsed,
            capture_len: None,
            capture_hash: None,
            proof: None,
        });
    }
    DirectPaneAcceptance {
        status: CommandDispatchStatus::TimedOut,
        elapsed,
        trigger_visible,
        // A timed-out window with the trigger still drafted is "submit didn't fire",
        // handled by the bare-Enter resubmit — not a non-dispatch.
        not_dispatched: false,
        diagnostic_path,
    }
}

/// True when the pane's last prompt candidate is an idle, dispatch-ready harness
/// prompt (composer empty and waiting for input) — i.e. NOT processing a turn.
/// Used to tell a genuine fast submit (pane now processing) from a send that never
/// landed (pane still idle). (#jbrundispatch directive 2)
fn pane_idle_dispatch_ready(content: &str, harness: &HarnessConfig) -> bool {
    harness
        .last_prompt_candidate(content)
        .map(|line| harness.is_dispatch_ready_prompt_line(&line))
        .unwrap_or(false)
}

pub(crate) fn direct_pane_existing_draft_visible(
    content: &str,
    trigger: &str,
    harness: &HarnessConfig,
) -> bool {
    let recent_lines: Vec<String> = content
        .lines()
        .rev()
        .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
        .filter(|line| !line.trim().is_empty())
        .take(16)
        .collect();
    let lines: Vec<&String> = recent_lines.iter().rev().collect();
    for start in 0..lines.len() {
        if !line_contains_trigger(lines[start], trigger)
            && !line_contains_equivalent_agent_doc_path_trigger(lines[start], trigger)
            && !wrapped_trigger_starts_at_line(&lines, start, trigger)
        {
            continue;
        }
        let later_has_prompt = lines
            .iter()
            .skip(start + 1)
            .any(|line| harness.is_prompt_line(line));
        return !later_has_prompt;
    }
    false
}

fn line_contains_equivalent_agent_doc_path_trigger(line: &str, trigger: &str) -> bool {
    let Some(trigger_path) = single_agent_doc_path_arg(trigger) else {
        return false;
    };
    let stripped = strip_leading_prompt_prefix(line);
    let tokens: Vec<&str> = stripped.split_whitespace().collect();
    for pair in tokens.windows(2) {
        let [command, path_arg] = pair else {
            continue;
        };
        if is_agent_doc_command_token(command)
            && agent_doc_path_args_equivalent(path_arg, trigger_path)
        {
            return true;
        }
    }
    false
}

fn single_agent_doc_path_arg(command_line: &str) -> Option<&str> {
    let stripped = strip_leading_prompt_prefix(command_line);
    let mut tokens = stripped.split_whitespace();
    let command = tokens.next()?;
    if !is_agent_doc_command_token(command) {
        return None;
    }
    let path_arg = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    Some(path_arg)
}

fn is_agent_doc_command_token(token: &str) -> bool {
    let token = token.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
    token == "agent-doc" || token == "/agent-doc"
}

#[derive(Debug, PartialEq, Eq)]
struct AgentDocPathArg {
    absolute: bool,
    components: Vec<String>,
}

fn agent_doc_path_arg(token: &str) -> Option<AgentDocPathArg> {
    let trimmed = token.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }
    let slash_normalized = trimmed.replace('\\', "/");
    let mut components = Vec::new();
    for component in slash_normalized.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return None;
        }
        components.push(component.to_string());
    }
    if components.is_empty() {
        return None;
    }
    Some(AgentDocPathArg {
        absolute: slash_normalized.starts_with('/'),
        components,
    })
}

fn agent_doc_path_args_equivalent(visible: &str, trigger: &str) -> bool {
    let Some(visible) = agent_doc_path_arg(visible) else {
        return false;
    };
    let Some(trigger) = agent_doc_path_arg(trigger) else {
        return false;
    };
    if visible.components == trigger.components {
        return true;
    }
    if visible.absolute == trigger.absolute {
        return false;
    }
    let (absolute, relative) = if visible.absolute {
        (&visible, &trigger)
    } else {
        (&trigger, &visible)
    };
    absolute.components.ends_with(&relative.components)
}

fn wrapped_trigger_starts_at_line(lines: &[&String], start: usize, trigger: &str) -> bool {
    let compact_trigger = compact_trigger_text(trigger);
    if compact_trigger.is_empty() {
        return false;
    }
    let first = compact_trigger_text(strip_leading_prompt_prefix(lines[start]));
    if first.is_empty() || !shares_trigger_prefix(&first, &compact_trigger) {
        return false;
    }
    let mut joined = first;
    if joined.contains(&compact_trigger) {
        return true;
    }
    for next in lines.iter().skip(start + 1).take(3) {
        joined.push_str(&compact_trigger_text(next));
        if joined.contains(&compact_trigger) {
            return true;
        }
        if joined.len() > compact_trigger.len() + 32 {
            break;
        }
    }
    false
}

fn send_direct_pane_enter_resubmit(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &HarnessConfig,
    trigger: &str,
    phase: &str,
    attempt: usize,
) -> DirectPaneAcceptance {
    let submit_key = agent_doc_tmux_commands::tmux_submit_key_for_harness(&harness.binary);
    crate::input_diag::log_text_submit(
        Some(file),
        "route.direct_pane_resubmit",
        &format!("pane:{pane}"),
        "",
        Some(&harness.binary),
        "routed_resubmit_submit_key",
        submit_key,
    );
    if let Err(e) =
        crate::sessions::send_submitted_text_for_harness(tmux, pane, "", &harness.binary)
    {
        eprintln!(
            "[route] warning: {} resubmit {} failed for pane {}: {}",
            harness.binary, submit_key, pane, e
        );
    }
    let second = poll_direct_pane_acceptance(tmux, pane, file, harness, trigger, phase);
    let file_display = file.display().to_string();
    let editor_attempt_id = editor_route_attempt_id();
    crate::ops_log::log_op(
        file,
        &direct_pane_resubmit_proof_line(DirectPaneResubmitProofFacts {
            file_display: &file_display,
            pane,
            harness_binary: &harness.binary,
            submit_key,
            status: second.status,
            elapsed_ms: second.elapsed.as_millis(),
            attempt,
            editor_attempt_id: editor_attempt_id.as_deref(),
        }),
    );
    second
}

fn send_direct_pane_enter_resubmit_until_stable(
    tmux: &Tmux,
    pane: &str,
    file: &Path,
    harness: &HarnessConfig,
    trigger: &str,
    phase: &str,
    initial: DirectPaneAcceptance,
) -> DirectPaneAcceptance {
    let mut status = initial.status;
    let mut trigger_visible = initial.trigger_visible;
    let mut elapsed = initial.elapsed;
    let mut diagnostic_path = initial.diagnostic_path;
    let mut attempts_sent = 0usize;
    let profile_allows_pending_draft_enter_resubmit =
        agent_doc_tmux_commands::tmux_submit_profile_for_harness(&harness.binary)
            .pending_draft_enter_resubmit();
    let max_attempts = direct_pane_max_enter_resubmits();

    while direct_pane_can_continue_enter_resubmit(DirectPaneEnterResubmitAttemptFacts {
        profile_allows_pending_draft_enter_resubmit,
        status,
        trigger_visible,
        attempts_sent,
        max_attempts,
    }) {
        attempts_sent += 1;
        let retry = send_direct_pane_enter_resubmit(
            tmux,
            pane,
            file,
            harness,
            trigger,
            phase,
            attempts_sent,
        );
        elapsed += retry.elapsed;
        status = retry.status;
        trigger_visible = retry.trigger_visible;
        if retry.diagnostic_path.is_some() {
            diagnostic_path = retry.diagnostic_path;
        }
    }

    DirectPaneAcceptance {
        status,
        elapsed,
        trigger_visible,
        // The bare-Enter resubmit path only handles a drafted (visible) trigger, so a
        // non-dispatch is never produced here.
        not_dispatched: false,
        diagnostic_path,
    }
}

/// #1vhn: positively verify the pane still hosts a live harness immediately
/// before sending the routed trigger. When the harness has crashed/exited to a
/// bare interactive shell, `#{pane_current_command}` reports the shell and the
/// captured pane shows no harness dispatch-ready prompt — typing the trigger
/// there would leave `agent-doc <FILE>` as un-run shell text (or, worse, run a
/// stray process in the shell). Returns the shell command name when route must
/// fail closed instead of dispatching into a dead shell.
///
/// Both signals are required so a harness that briefly spawns a subshell, or a
/// momentary `#{pane_current_command}` read while the harness composer is still
/// the visible prompt, does not trip a false positive: the pane must report a
/// bare shell foreground command AND show no harness dispatch-ready prompt.
pub(crate) fn dead_harness_shell_dispatch_block(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
) -> Option<String> {
    let current_command = super::pane_display_value(tmux, pane, "#{pane_current_command}")
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())?;
    if !pane_current_command_is_bare_shell(&current_command) {
        return None;
    }
    let pane_shows_harness_prompt = sessions::capture_pane(tmux, pane)
        .ok()
        .and_then(|content| harness.last_prompt_candidate(&content))
        .map(|line| harness.is_dispatch_ready_prompt_line(&line))
        .unwrap_or(false);
    if pane_shows_harness_prompt {
        return None;
    }
    Some(current_command)
}

/// Why a `#jbtsiftnosub` cold-start re-verify refuses an auto-start dispatch. The
/// distinction matters for diagnostics: a `StartingPane` is a freshly created
/// pane whose harness is still booting (the composer accepts keystrokes but is
/// not yet submit-ready), while a `DeadShell` is the issue-A case where the
/// harness already crashed/exited to a bare interactive shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AutoStartDispatchBlock {
    /// The pane is still cold-starting: no harness dispatch-ready prompt yet, but
    /// the foreground process is NOT a bare shell (the harness is coming up).
    StartingPane,
    /// The harness crashed/exited to a bare interactive shell (issue A). Carries
    /// the shell command name reported by `#{pane_current_command}`.
    DeadShell(String),
}

/// `#jbtsiftnosub`: re-verify, immediately before an auto-start send, that the
/// freshly created pane has reached a harness dispatch-ready prompt. The
/// cold-start race is that `wait_for_agent_ready` proved a transient dispatch-ready
/// prompt while the harness TUI was still coming up, but by send time the composer
/// is not yet submit-ready, so the trigger keystrokes land without a real submit.
///
/// Returns `None` when the pane shows a harness dispatch-ready prompt (the send
/// may proceed). Returns `Some(DeadShell)` when the harness exited to a bare shell
/// (the issue-A guard), or `Some(StartingPane)` when the harness is the foreground
/// process but no dispatch-ready prompt is visible yet (still cold-starting).
pub(crate) fn auto_start_dispatch_ready_block(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
) -> Option<AutoStartDispatchBlock> {
    // A visible harness dispatch-ready prompt means the composer is submit-ready;
    // let the send proceed.
    let pane_shows_dispatch_ready_prompt = sessions::capture_pane(tmux, pane)
        .ok()
        .and_then(|content| super::startup::ready_prompt_candidate(&content, harness))
        .is_some();
    if pane_shows_dispatch_ready_prompt {
        return None;
    }
    // No dispatch-ready prompt: distinguish a dead bare shell (issue A) from a
    // still-starting harness composer (issue C) by the foreground command.
    match super::pane_display_value(tmux, pane, "#{pane_current_command}")
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
    {
        Some(cmd) if pane_current_command_is_bare_shell(&cmd) => {
            Some(AutoStartDispatchBlock::DeadShell(cmd))
        }
        _ => Some(AutoStartDispatchBlock::StartingPane),
    }
}

/// `#jbtsiftnosub`: gate an auto-start send behind a bounded re-verify that the
/// freshly created pane has reached a harness dispatch-ready prompt. Polls
/// [`auto_start_dispatch_ready_block`] up to `timeout`; a clear (`None`) result
/// lets the caller proceed with the normal send. If the bound elapses while the
/// pane is still cold-starting (or has dropped to a dead shell), fails closed with
/// claim/restart guidance and records `dispatch_into_starting_pane` /
/// `dispatch_into_shell` in ops.log instead of typing into a not-yet-submit-ready
/// composer.
pub(crate) fn reverify_auto_start_dispatch_ready(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    let poll_interval = Duration::from_millis(150);
    let last_block = loop {
        match auto_start_dispatch_ready_block(tmux, pane, harness) {
            None => {
                // #jbtsiftnosub / #j9ja: SUCCESS marker. The pane reached a harness
                // dispatch-ready prompt within the bound, so the auto-start send may
                // proceed. The fail-closed arms below log `dispatch_into_starting_pane`
                // / `dispatch_into_shell`; emit the positive counterpart so a live
                // operator test of this gate is provable/disprovable from ops.log
                // (auto-verify resolves the gate via `--pending-set-verify
                // verify=ops_log:auto_start_dispatch_ready_confirmed`).
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "auto_start_dispatch_ready_confirmed file={} pane={} harness={} elapsed_secs={} #jbtsiftnosub",
                        file.display(),
                        pane,
                        harness.binary,
                        start.elapsed().as_secs()
                    ),
                );
                return Ok(());
            }
            Some(block) if start.elapsed() >= timeout => break block,
            Some(_) => {}
        }
        std::thread::sleep(poll_interval);
    };
    match last_block {
        AutoStartDispatchBlock::StartingPane => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "dispatch_into_starting_pane file={} pane={} harness={} timeout_secs={} reason=harness_not_dispatch_ready_before_auto_start_send",
                    file.display(),
                    pane,
                    harness.binary,
                    timeout.as_secs()
                ),
            );
            anyhow::bail!(
                "route refusing to dispatch {} into pane {}: harness '{}' is still starting (no dispatch-ready prompt after {}s). The cold-start composer is not yet submit-ready — re-run `agent-doc route`/`Run Agent Doc` once the {} prompt is up, or claim/restart the harness.",
                harness.trigger_command(file_path),
                pane,
                harness.binary,
                timeout.as_secs(),
                harness.binary,
            );
        }
        AutoStartDispatchBlock::DeadShell(shell) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "dispatch_into_shell file={} pane={} harness={} pane_current_command={} timeout_secs={} reason=harness_exited_to_bare_shell_before_auto_start_send",
                    file.display(),
                    pane,
                    harness.binary,
                    shell,
                    timeout.as_secs()
                ),
            );
            anyhow::bail!(
                "route refusing to dispatch {} into pane {}: harness '{}' is not running (pane is a bare '{}' shell). The harness crashed/exited during cold-start — claim/restart the harness before routing.",
                harness.trigger_command(file_path),
                pane,
                harness.binary,
                shell,
            );
        }
    }
}

pub(crate) fn send_command_unchecked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<CommandDispatchResult> {
    let file = Path::new(file_path);
    // #1vhn: re-verify a live harness owns the pane immediately before sending.
    // Closes the crash-mid-dispatch race where the harness was dispatch-ready at
    // the readiness check but exited to a bare shell before the send.
    if let Some(shell) = dead_harness_shell_dispatch_block(tmux, pane, harness) {
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_into_dead_shell_blocked file={} pane={} harness={} pane_current_command={} reason=harness_exited_to_bare_shell",
                file.display(),
                pane,
                harness.binary,
                shell
            ),
        );
        anyhow::bail!(
            "route refusing to dispatch {} into pane {}: harness '{}' is not running (pane is a bare '{}' shell). The harness crashed/exited — claim/restart the harness before routing.",
            harness.trigger_command(file_path),
            pane,
            harness.binary,
            shell
        );
    }
    let trigger = harness.trigger_command(file_path);
    let payload = trigger.to_string();
    if let Some(rejection) = routed_trigger_payload_rejection(RoutedTriggerPayloadFacts {
        harness_binary: &harness.binary,
        trigger: &trigger,
        payload: &payload,
    }) {
        anyhow::bail!("{rejection}");
    }
    let mut existing_draft_diagnostic_path = None;
    let mut protected_prompt_input = None;
    let existing_draft_visible = match sessions::capture_pane(tmux, pane) {
        Ok(content) => {
            let visible = direct_pane_existing_draft_visible(&content, &trigger, harness);
            if visible {
                existing_draft_diagnostic_path = preserve_route_pane_snapshot(
                    file,
                    pane,
                    harness,
                    "direct_pane_existing_draft_visible",
                    &content,
                )
                .path;
            } else if let Some(reason) = harness.protected_prompt_input_reason(&content) {
                let diagnostic_path = preserve_route_pane_snapshot(
                    file,
                    pane,
                    harness,
                    "direct_pane_protected_prompt_input",
                    &content,
                )
                .path;
                let draft_preview = protected_prompt_draft_preview(harness, &content);
                protected_prompt_input = Some((reason, diagnostic_path, draft_preview));
            }
            visible
        }
        Err(e) => {
            eprintln!(
                "[route] warning: failed to capture pane {} before direct submit: {}",
                pane, e
            );
            false
        }
    };
    if let Some((reason, diagnostic_path, draft_preview)) = protected_prompt_input {
        let draft_preview_field = draft_preview
            .as_deref()
            .map(|preview| format!(" draft_preview={preview:?}"))
            .unwrap_or_default();
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_direct_pane_blocked file={} pane={} harness={} protected_input={}{}",
                file.display(),
                pane,
                harness.binary,
                reason,
                draft_preview_field,
            ),
        );
        let diagnostic = diagnostic_path
            .as_ref()
            .map(|path| format!(" snapshot_path={}", path.display()))
            .unwrap_or_default();
        anyhow::bail!(
            "route refusing to dispatch {} into pane {} for {} because the composer contains protected prompt input ({}){}; clear or submit that draft, then rerun agent-doc route{}",
            harness.trigger_command(file_path),
            pane,
            file.display(),
            reason,
            draft_preview_field,
            diagnostic,
        );
    }
    if direct_pane_can_enter_existing_draft(DirectPaneExistingDraftSubmitFacts {
        profile_allows_pending_draft_enter_resubmit:
            agent_doc_tmux_commands::tmux_submit_profile_for_harness(&harness.binary)
                .pending_draft_enter_resubmit(),
        trigger_visible: existing_draft_visible,
    }) {
        let first = send_direct_pane_enter_resubmit_until_stable(
            tmux,
            pane,
            file,
            harness,
            &trigger,
            "direct_pane_existing_draft_acceptance",
            DirectPaneAcceptance {
                status: CommandDispatchStatus::TimedOut,
                elapsed: Duration::ZERO,
                trigger_visible: true,
                not_dispatched: false,
                diagnostic_path: existing_draft_diagnostic_path,
            },
        );
        return Ok(CommandDispatchResult {
            status: first.status,
            elapsed: first.elapsed,
            diagnostic_path: first.diagnostic_path,
        });
    }

    let trigger = send_command_once_unchecked(tmux, pane, file_path, harness)?;
    let mut acceptance = poll_direct_pane_acceptance(
        tmux,
        pane,
        file,
        harness,
        &trigger,
        "direct_pane_acceptance",
    );

    // `#jbdisprecycle` R3: the trigger was just injected (dispatch_inject
    // attempt=1) but the submit has not been accepted. If the project supervisor
    // is mid-`execve` recycle (lib-install auto-recycle / operator restart), the
    // submit keystroke was dropped across the hot-reload boundary. Wait for the
    // recycle to settle and re-poll ONCE, so the budgeted resubmit/re-type loops
    // below run against the settled supervisor and land the submit exactly once —
    // never burning the budget on input the recycle silently drops (which would
    // re-type the trigger N times: the #rdypoll restack symptom).
    if acceptance.status != CommandDispatchStatus::Accepted
        && recycle_interrupted_resubmit_should_wait(
            true,
            crate::recycle_inflight::recycle_inflight_pending(file_path),
        )
    {
        let settled = crate::recycle_inflight::wait_for_recycle_settle(
            file_path,
            crate::recycle_inflight::RECYCLE_SETTLE_WAIT,
            crate::recycle_inflight::RECYCLE_SETTLE_POLL,
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_submit_recycle_settle file={} pane={} harness={} settled={} action=submit_once_after_settle",
                file.display(),
                pane,
                harness.binary,
                settled
            ),
        );
        acceptance = poll_direct_pane_acceptance(
            tmux,
            pane,
            file,
            harness,
            &trigger,
            "direct_pane_post_recycle_acceptance",
        );
    }

    // #jbrundispatch directive 2: "detect if the prompt was not dispatched into the
    // session, and send the prompt and submit the prompt." When the trigger never
    // landed in the composer (`not_dispatched`) — the classic pane-kill+restart
    // "Run Agent Doc stalled" case where the send no-op'd into a not-ready pane —
    // re-send the FULL trigger (text+Enter), not a bare Enter, until it lands or the
    // budget is exhausted. The bare-Enter resubmit below cannot recover this: there
    // is no drafted text in the composer to submit.
    let mut full_resends = 0usize;
    while acceptance.not_dispatched && full_resends < direct_pane_max_enter_resubmits() {
        full_resends += 1;
        crate::ops_log::log_op(
            file,
            &format!(
                "route_redispatch_not_landed file={} pane={} attempt={} harness={}",
                file.display(),
                pane,
                full_resends,
                harness.binary
            ),
        );
        let resent = send_command_once_unchecked(tmux, pane, file_path, harness)?;
        acceptance = poll_direct_pane_acceptance(
            tmux,
            pane,
            file,
            harness,
            &resent,
            "direct_pane_redispatch_acceptance",
        );
    }

    if acceptance.not_dispatched {
        // Budget exhausted and the trigger still never landed — report a genuine
        // non-dispatch (TimedOut) instead of a false Accepted, so the caller knows
        // the dispatch did not reach the session.
        return Ok(CommandDispatchResult {
            status: CommandDispatchStatus::TimedOut,
            elapsed: acceptance.elapsed,
            diagnostic_path: acceptance.diagnostic_path,
        });
    }
    if acceptance.status == CommandDispatchStatus::Accepted {
        return Ok(CommandDispatchResult {
            status: acceptance.status,
            elapsed: acceptance.elapsed,
            diagnostic_path: acceptance.diagnostic_path,
        });
    }

    let second = send_direct_pane_enter_resubmit_until_stable(
        tmux,
        pane,
        file,
        harness,
        &trigger,
        "direct_pane_resubmit_acceptance",
        acceptance,
    );

    Ok(CommandDispatchResult {
        status: second.status,
        elapsed: second.elapsed,
        diagnostic_path: second.diagnostic_path,
    })
}

pub(crate) fn send_command_once_unchecked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<String> {
    let short_name = std::path::Path::new(file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    let trigger = harness.trigger_command(file_path);
    let payload = trigger.to_string();
    if let Some(rejection) = routed_trigger_payload_rejection(RoutedTriggerPayloadFacts {
        harness_binary: &harness.binary,
        trigger: &trigger,
        payload: &payload,
    }) {
        anyhow::bail!("{rejection}");
    }
    let flash_msg = format!("⏳ {}", harness.trigger_command(&short_name));
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", pane, "-d", "2000", &flash_msg])
        .status()
    {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    let transform = agent_doc_tmux_commands::tmux_submit_transform_for_harness(&harness.binary);
    let submit_key = agent_doc_tmux_commands::tmux_submit_key_for_harness(&harness.binary);
    crate::input_diag::log_text_submit(
        Some(Path::new(file_path)),
        "route.direct_pane_submit",
        &format!("pane:{pane}"),
        &payload,
        Some(&harness.binary),
        transform,
        submit_key,
    );
    log_dispatch_inject(Path::new(file_path), pane, harness, "direct_pane");
    crate::sessions::send_submitted_text_for_harness(tmux, pane, &payload, &harness.binary)?;
    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!("[route] Sent {} → pane {}", trigger, pane);
    Ok(trigger)
}

pub(crate) fn dispatch_via_supervisor_ipc_with_mode(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
    options: SupervisorIpcDispatchOptions,
) -> Result<RoutedDispatchStartProof> {
    let Some(sock) = supervisor_socket_path(file, session_id) else {
        anyhow::bail!(
            "authoritative actor for {} has no supervisor socket; run `agent-doc start {}` to recover",
            file.display(),
            file.display()
        );
    };
    let short_name = std::path::Path::new(file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    let trigger = harness.trigger_command(file_path);
    let payload = trigger.to_string();
    if let Some(rejection) = routed_trigger_payload_rejection(RoutedTriggerPayloadFacts {
        harness_binary: &harness.binary,
        trigger: &trigger,
        payload: &payload,
    }) {
        anyhow::bail!("{rejection}");
    }
    let flash_msg = format!("⏳ {}", harness.trigger_command(&short_name));
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", pane, "-d", "2000", &flash_msg])
        .status()
    {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    let tracker =
        build_routed_dispatch_start_tracker(file, file_path, harness, Some(tmux), Some(pane))?;
    let _route_submit_guard =
        crate::route_in_flight::begin_route_submit(file, pane, &harness.binary)?;
    let method = IpcMethod::Inject {
        bytes: agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(&payload)
            .to_string(),
    };
    crate::input_diag::log_text_submit(
        Some(file),
        "route.supervisor_ipc",
        &format!("socket:{}:pane:{pane}", sock.display()),
        &payload,
        Some(&harness.binary),
        "supervisor_ipc_inject",
        "Inject",
    );
    log_dispatch_inject(file, pane, harness, "supervisor_ipc");
    let submit_start = Instant::now();
    let response = crate::supervisor::ipc::send_command(&sock, &method).with_context(|| {
        format!(
            "failed to dispatch authoritative actor trigger for {} via supervisor IPC",
            file.display()
        )
    })?;
    log_route_latency(
        file,
        "supervisor_ipc_submit",
        submit_start.elapsed(),
        Duration::from_millis(500),
        pane,
        harness,
        if response.ok { "accepted" } else { "rejected" },
    );
    if !response.ok {
        let message = response
            .error
            .unwrap_or_else(|| "unknown supervisor error".to_string());
        anyhow::bail!(
            "authoritative actor for {} rejected routed trigger in pane {}: {}",
            file.display(),
            pane,
            message
        );
    }

    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!(
        "[route] Dispatched {} via supervisor IPC → pane {}",
        trigger, pane
    );

    if !options.await_start_proof {
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    }

    let Some(tracker) = tracker else {
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    };

    let timeout =
        routed_dispatch_start_timeout_for_binary(Some(harness.binary.as_str()), cfg!(test));
    let proof_start = Instant::now();
    if let Some(proof) = wait_for_routed_dispatch_start(tmux, file, &tracker, harness, timeout)? {
        log_route_latency(
            file,
            "dispatch_start_proof",
            proof_start.elapsed(),
            timeout,
            pane,
            harness,
            proof.dispatch_stage_label(),
        );
        log_route_submit_observation(RouteSubmitObservationLogFacts {
            file,
            pane,
            harness,
            phase: "supervisor_dispatch_start_proof",
            observation: RouteSubmitObservation::DispatchStartProven,
            trigger_visible: None,
            elapsed: proof_start.elapsed(),
            capture_len: None,
            capture_hash: None,
            proof: Some(proof),
        });
        crate::ops_log::log_op(
            file,
            &format!(
                "route_actor_dispatch_start_proven file={} pane={} harness={} proof={} timeout_secs={}",
                file.display(),
                pane,
                harness.binary,
                proof.dispatch_stage_label(),
                timeout.as_secs()
            ),
        );
        return Ok(proof);
    }

    log_route_latency(
        file,
        "dispatch_start_proof",
        proof_start.elapsed(),
        timeout,
        pane,
        harness,
        "unproven_but_accepted",
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "route_actor_dispatch_start_unproven_but_accepted file={} pane={} harness={} timeout_secs={}",
            file.display(),
            pane,
            harness.binary,
            timeout.as_secs()
        ),
    );
    let diagnostic_path = match sessions::capture_pane(tmux, pane) {
        Ok(content) => {
            let snapshot = preserve_route_pane_snapshot(
                file,
                pane,
                harness,
                "supervisor_dispatch_start_unproven",
                &content,
            );
            print_route_pane_snapshot_hint(
                file,
                pane,
                harness,
                "supervisor_dispatch_start_unproven",
                &snapshot,
            );
            snapshot.path
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to capture pane {} after unproven supervisor dispatch: {}",
                pane, err
            );
            None
        }
    };
    log_route_submit_observation(RouteSubmitObservationLogFacts {
        file,
        pane,
        harness,
        phase: "supervisor_dispatch_start_proof",
        observation: RouteSubmitObservation::AcceptedWithoutDispatchProof,
        trigger_visible: None,
        elapsed: proof_start.elapsed(),
        capture_len: None,
        capture_hash: None,
        proof: None,
    });
    file_route_dispatch_bug_report(RouteDispatchBugReportFacts {
        file,
        pane,
        harness,
        phase: "supervisor_dispatch_start_proof",
        issue: "accepted_without_dispatch_start_proof",
        result: RouteSubmitObservation::AcceptedWithoutDispatchProof.label(),
        elapsed: proof_start.elapsed(),
        proof: None,
        diagnostic_path: diagnostic_path.as_deref(),
    });
    if options.print_unproven_progress {
        eprintln!(
            "[route] authoritative actor accepted the {} reopen for {} in pane {}, but no routed submission proof appeared after {}s",
            harness.binary,
            file.display(),
            pane,
            timeout.as_secs()
        );
    }
    Ok(RoutedDispatchStartProof::DispatchStartUnproven)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SupervisorIpcDispatchOptions {
    pub(crate) await_start_proof: bool,
    pub(crate) print_unproven_progress: bool,
}

pub(crate) fn dispatch_via_supervisor_ipc(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<RoutedDispatchStartProof> {
    dispatch_via_supervisor_ipc_with_mode(
        tmux,
        file,
        pane,
        session_id,
        file_path,
        harness,
        SupervisorIpcDispatchOptions {
            await_start_proof: true,
            print_unproven_progress: true,
        },
    )
}

pub(crate) fn authoritative_actor_dispatch_recovery_hint(
    state: agent_doc_sqlite::state_store::ActorState,
    file: &Path,
) -> String {
    actor_recovery_hint(actor_dispatch_state(state), &file.display().to_string())
}

#[cfg(test)]
pub(crate) fn authoritative_actor_dispatch_can_queue_optimistically(
    state: agent_doc_sqlite::state_store::ActorState,
) -> bool {
    agent_doc_controller::dispatch::actor_can_queue_optimistically(actor_dispatch_state(state))
}

pub(crate) fn canonical_dispatch_file(path: &std::path::Path) -> std::path::PathBuf {
    let resolved = crate::git::resolve_absolute_file_path(path);
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

pub(crate) fn canonical_registered_file(entry: &tmux_router::RegistryEntry) -> std::path::PathBuf {
    let path = std::path::Path::new(&entry.file);
    let resolved = if path.is_absolute() || entry.cwd.is_empty() {
        path.to_path_buf()
    } else {
        std::path::Path::new(&entry.cwd).join(path)
    };
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

pub(crate) fn registry_base_dir_for_dispatch(file_path: &str) -> std::path::PathBuf {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    agent_doc_fs::find_project_root(&requested)
        .or_else(|| requested.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
}

pub(crate) fn lookup_dispatch_registration(
    file_path: &str,
    session_id: &str,
) -> Result<Option<String>> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    sessions::lookup_in(&base_dir, session_id)
}

pub(crate) fn load_dispatch_registry(file_path: &str) -> Result<tmux_router::Registry> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    sessions::load_in(&base_dir)
}

pub(crate) fn deregister_dispatch_registration(file_path: &str, session_id: &str) -> Result<bool> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let registry_path = sessions::registry_path_in(&base_dir);
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = sessions::load_in(&base_dir)?;
    let removed_key = registry.iter().find_map(|(key, entry)| {
        ((entry.session_id == session_id) || (entry.session_id.is_empty() && key == session_id))
            .then(|| key.clone())
    });
    let removed = removed_key.and_then(|key| registry.remove(&key)).is_some();
    if removed {
        sessions::save_in(&base_dir, &registry)?;
    }
    Ok(removed)
}

pub(crate) fn register_dispatch_target(
    tmux: &Tmux,
    session_id: &str,
    pane_id: &str,
    file_path: &str,
) -> Result<()> {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    let requested_str = requested.to_string_lossy().to_string();
    let base_dir = registry_base_dir_for_dispatch(&requested_str);
    ensure_dispatch_target_can_bind_file(tmux, &base_dir, pane_id, &requested_str)?;
    let window = sessions::pane_window(pane_id).unwrap_or_default();
    let cwd = base_dir.to_string_lossy().to_string();
    sessions::register_full_with_cwd_in(
        &base_dir,
        session_id,
        pane_id,
        &requested_str,
        std::process::id(),
        &window,
        &cwd,
    )
}

pub(crate) fn ensure_dispatch_target_can_bind_file(
    tmux: &Tmux,
    base_dir: &Path,
    pane: &str,
    file_path: &str,
) -> Result<()> {
    let registry = sessions::load_in(base_dir).with_context(|| {
        format!(
            "failed to load route registry before dispatch registration from {}",
            base_dir.display()
        )
    })?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        let registered = canonical_registered_file(entry);
        let registered_is_live_owner = !entry.session_id.is_empty()
            && crate::sync::find_normal_path_owner_pane(tmux, &registered, &entry.session_id)
                .as_deref()
                == Some(pane);
        if !registered_is_live_owner {
            return Ok(());
        }
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            registered.display(),
            requested.display()
        );
    }

    Ok(())
}

pub(crate) fn pane_registration_matches_file(
    registry: &tmux_router::Registry,
    pane: &str,
    file_path: &str,
) -> bool {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    registry
        .values()
        .find(|entry| entry.pane == pane)
        .map(|entry| canonical_registered_file(entry) == requested)
        .unwrap_or(false)
}

pub(crate) fn ensure_dispatch_target_matches_file(pane: &str, file_path: &str) -> Result<()> {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let registry = sessions::load_in(&registry_base_dir).with_context(|| {
        format!(
            "failed to load route registry before dispatch validation from {}",
            registry_base_dir.display()
        )
    })?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            canonical_registered_file(entry).display(),
            requested.display()
        );
    }

    anyhow::bail!(
        "route dispatch target {} is not registered for {}; refusing unbound dispatch",
        pane,
        requested.display()
    );
}

pub(crate) fn resolve_fresh_dispatch_target_after_ready_wait(
    tmux: &Tmux,
    session_id: &str,
    pane: &str,
    file_path: &str,
    _startup_miss_handoff_blocked_pane: Option<&str>,
) -> Result<String> {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let registry = sessions::load_in(&registry_base_dir).with_context(|| {
        format!(
            "failed to load route registry before fresh-dispatch validation from {}",
            registry_base_dir.display()
        )
    })?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(pane.to_string());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    let handoff_target = registry
        .values()
        .find(|entry| {
            entry.session_id == session_id
                && !entry.pane.is_empty()
                && entry.pane != pane
                && canonical_registered_file(entry) == requested
        })
        .map(|entry| entry.pane.clone());
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        if let Some(handoff_pane) = handoff_target {
            eprintln!(
                "[route] fresh restart re-bound {} away from pane {} and onto authoritative pane {} before retry",
                file_path, pane, handoff_pane
            );
            return Ok(handoff_pane);
        }
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            canonical_registered_file(entry).display(),
            requested.display()
        );
    }

    // A fresh route already created `pane` deliberately. If some concurrent
    // sync/layout path rebinds the same document session back to another pane
    // during the ready wait, keep the fresh pane authoritative instead of
    // handing dispatch back to the older pane and making the new pane disposable.
    register_dispatch_target(tmux, session_id, pane, file_path)?;
    Ok(pane.to_string())
}

pub(crate) fn send_command_checked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<CommandDispatchResult> {
    ensure_dispatch_target_matches_file(pane, file_path)?;
    send_command_unchecked(tmux, pane, file_path, harness)
}

fn try_late_direct_pane_enter_resubmit_after_unproven_dispatch(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    tracker: &RoutedDispatchStartTracker,
    timeout: Duration,
) -> Result<Option<RoutedDispatchStartProof>> {
    let trigger = harness.trigger_command(file_path);
    let visible = match sessions::capture_pane(tmux, pane) {
        Ok(content) => {
            let visible = direct_pane_existing_draft_visible(&content, &trigger, harness);
            if visible {
                preserve_route_pane_snapshot(
                    file,
                    pane,
                    harness,
                    "dispatch_start_unproven_late_draft_visible",
                    &content,
                );
            }
            visible
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to capture pane {} before late direct-submit retry: {}",
                pane, err
            );
            false
        }
    };
    if !direct_pane_can_enter_existing_draft(DirectPaneExistingDraftSubmitFacts {
        profile_allows_pending_draft_enter_resubmit:
            agent_doc_tmux_commands::tmux_submit_profile_for_harness(&harness.binary)
                .pending_draft_enter_resubmit(),
        trigger_visible: visible,
    }) {
        return Ok(None);
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "route_submit_late_resubmit file={} pane={} harness={} cause=dispatch_start_unproven_prompt_visible",
            file.display(),
            pane,
            harness.binary,
        ),
    );
    let retry = send_direct_pane_enter_resubmit(
        tmux,
        pane,
        file,
        harness,
        &trigger,
        "dispatch_start_unproven_late_draft_acceptance",
        1,
    );
    let proof_start = Instant::now();
    if let Some(proof) = wait_for_routed_dispatch_start(tmux, file, tracker, harness, timeout)? {
        log_route_latency(
            file,
            "direct_pane_late_resubmit",
            retry.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(retry.status, Some(proof)),
        );
        log_route_latency(
            file,
            "dispatch_start_proof_after_late_resubmit",
            proof_start.elapsed(),
            timeout,
            pane,
            harness,
            proof.dispatch_stage_label(),
        );
        log_route_submit_observation(RouteSubmitObservationLogFacts {
            file,
            pane,
            harness,
            phase: "dispatch_start_proof_after_late_resubmit",
            observation: RouteSubmitObservation::DispatchStartProven,
            trigger_visible: None,
            elapsed: proof_start.elapsed(),
            capture_len: None,
            capture_hash: None,
            proof: Some(proof),
        });
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_start_late_resubmit_proven file={} pane={} harness={} dispatch_stage={} timeout_secs={} retry=late_enter",
                file.display(),
                pane,
                harness.binary,
                proof.dispatch_stage_label(),
                timeout.as_secs()
            ),
        );
        return Ok(Some(proof));
    }

    log_route_latency(
        file,
        "direct_pane_late_resubmit",
        retry.elapsed,
        direct_pane_submit_acceptance_budget(),
        pane,
        harness,
        direct_pane_submit_outcome(retry.status, None),
    );
    log_route_latency(
        file,
        "dispatch_start_proof_after_late_resubmit",
        proof_start.elapsed(),
        timeout,
        pane,
        harness,
        "late_resubmit_unproven",
    );
    Ok(None)
}

pub(crate) fn dispatch_existing_managed_reopen(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<RoutedDispatchStartProof> {
    dispatch_via_supervisor_ipc(tmux, file, pane, session_id, file_path, harness)
}

pub(crate) fn dispatch_routed_reopen(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<RoutedDispatchStartProof> {
    dispatch_routed_reopen_with_mode(
        tmux,
        file,
        pane,
        file_path,
        harness,
        DirectPaneDispatchOptions {
            await_start_proof: true,
            print_unproven_progress: true,
        },
    )
}

pub(crate) fn dispatch_routed_reopen_with_mode(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    options: DirectPaneDispatchOptions,
) -> Result<RoutedDispatchStartProof> {
    let tracker =
        build_routed_dispatch_start_tracker(file, file_path, harness, Some(tmux), Some(pane))?;
    let _route_submit_guard =
        crate::route_in_flight::begin_route_submit(file, pane, &harness.binary)?;
    let submit_result = send_command_checked(tmux, pane, file_path, harness)?;
    let Some(tracker) = tracker else {
        log_route_latency(
            file,
            "direct_pane_submit",
            submit_result.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(submit_result.status, None),
        );
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    };
    if !direct_pane_should_await_dispatch_start_proof(DirectPaneDispatchStartProofFacts {
        await_start_proof: options.await_start_proof,
        submit_status: submit_result.status,
    }) {
        log_route_latency(
            file,
            "direct_pane_submit",
            submit_result.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(submit_result.status, None),
        );
        return Ok(RoutedDispatchStartProof::CommandAcceptedOnly);
    }

    let timeout =
        routed_dispatch_start_timeout_for_binary(Some(harness.binary.as_str()), cfg!(test));
    let proof_start = Instant::now();
    if let Some(proof) = wait_for_routed_dispatch_start(tmux, file, &tracker, harness, timeout)? {
        log_route_latency(
            file,
            "direct_pane_submit",
            submit_result.elapsed,
            direct_pane_submit_acceptance_budget(),
            pane,
            harness,
            direct_pane_submit_outcome(submit_result.status, Some(proof)),
        );
        log_route_latency(
            file,
            "dispatch_start_proof",
            proof_start.elapsed(),
            timeout,
            pane,
            harness,
            proof.dispatch_stage_label(),
        );
        log_route_submit_observation(RouteSubmitObservationLogFacts {
            file,
            pane,
            harness,
            phase: "dispatch_start_proof",
            observation: RouteSubmitObservation::DispatchStartProven,
            trigger_visible: None,
            elapsed: proof_start.elapsed(),
            capture_len: None,
            capture_hash: None,
            proof: Some(proof),
        });
        crate::ops_log::log_op(
            file,
            &format!(
                "route_dispatch_start_proven file={} pane={} harness={} proof={} timeout_secs={}",
                file.display(),
                pane,
                harness.binary,
                proof.dispatch_stage_label(),
                timeout.as_secs()
            ),
        );
        return Ok(proof);
    }

    log_route_latency(
        file,
        "direct_pane_submit",
        submit_result.elapsed,
        direct_pane_submit_acceptance_budget(),
        pane,
        harness,
        direct_pane_submit_outcome(submit_result.status, None),
    );
    match submit_result.status {
        CommandDispatchStatus::Accepted => {
            if let Some(proof) = try_late_direct_pane_enter_resubmit_after_unproven_dispatch(
                tmux, file, pane, file_path, harness, &tracker, timeout,
            )? {
                return Ok(proof);
            }
            log_route_latency(
                file,
                "dispatch_start_proof",
                proof_start.elapsed(),
                timeout,
                pane,
                harness,
                "unproven_but_accepted",
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_dispatch_start_unproven_but_accepted file={} pane={} harness={} timeout_secs={}",
                    file.display(),
                    pane,
                    harness.binary,
                    timeout.as_secs()
                ),
            );
            let diagnostic_path = match sessions::capture_pane(tmux, pane) {
                Ok(content) => {
                    let snapshot = preserve_route_pane_snapshot(
                        file,
                        pane,
                        harness,
                        "direct_pane_dispatch_start_unproven",
                        &content,
                    );
                    print_route_pane_snapshot_hint(
                        file,
                        pane,
                        harness,
                        "direct_pane_dispatch_start_unproven",
                        &snapshot,
                    );
                    snapshot.path
                }
                Err(err) => {
                    eprintln!(
                        "[route] warning: failed to capture pane {} after unproven direct dispatch: {}",
                        pane, err
                    );
                    None
                }
            };
            log_route_submit_observation(RouteSubmitObservationLogFacts {
                file,
                pane,
                harness,
                phase: "dispatch_start_proof",
                observation: RouteSubmitObservation::AcceptedWithoutDispatchProof,
                trigger_visible: None,
                elapsed: proof_start.elapsed(),
                capture_len: None,
                capture_hash: None,
                proof: None,
            });
            file_route_dispatch_bug_report(RouteDispatchBugReportFacts {
                file,
                pane,
                harness,
                phase: "dispatch_start_proof",
                issue: "accepted_without_dispatch_start_proof",
                result: RouteSubmitObservation::AcceptedWithoutDispatchProof.label(),
                elapsed: proof_start.elapsed(),
                proof: None,
                diagnostic_path: diagnostic_path.as_deref(),
            });
            if options.print_unproven_progress {
                eprintln!(
                    "[route] bare {} reopen for {} was accepted in pane {}, but no routed submission proof appeared after {}s",
                    harness.binary,
                    file.display(),
                    pane,
                    timeout.as_secs()
                );
            }
            Ok(RoutedDispatchStartProof::DispatchStartUnproven)
        }
        CommandDispatchStatus::TimedOut => {
            log_route_latency(
                file,
                "dispatch_start_proof",
                proof_start.elapsed(),
                timeout,
                pane,
                harness,
                "submit_timed_out_without_proof",
            );
            file_route_dispatch_bug_report(RouteDispatchBugReportFacts {
                file,
                pane,
                harness,
                phase: "direct_pane_submit_final",
                issue: "prompt_not_submitted",
                result: "submit_timed_out_without_proof",
                elapsed: proof_start.elapsed(),
                proof: None,
                diagnostic_path: submit_result.diagnostic_path.as_deref(),
            });
            anyhow::bail!(
                "routed {} trigger for {} left the bare reopen drafted in pane {} and still showed no routed submission proof after waiting {}s",
                harness.binary,
                file.display(),
                pane,
                timeout.as_secs()
            )
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DirectPaneDispatchOptions {
    pub(crate) await_start_proof: bool,
    pub(crate) print_unproven_progress: bool,
}

pub(crate) fn apply_plain_trigger_override(harness: &mut HarnessConfig) {
    harness.trigger_command_template = "agent-doc {file}".to_string();
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};
    use agent_doc_controller::dispatch::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    #[test]
    fn authoritative_actor_starting_hint_names_reroute_and_restart() {
        let file = std::path::Path::new("/tmp/session.md");
        let hint = authoritative_actor_dispatch_recovery_hint(
            agent_doc_sqlite::state_store::ActorState::Starting,
            file,
        );
        assert!(
            hint.contains("rerun `agent-doc /tmp/session.md`"),
            "starting actor hint should tell the user how to retry: {hint}"
        );
        assert!(
            hint.contains("prompt_ready=true"),
            "starting actor hint should name the dispatch-ready wait state: {hint}"
        );
        assert!(
            hint.contains("agent-doc start /tmp/session.md"),
            "starting actor hint should name the owner restart recovery: {hint}"
        );
    }
    #[test]
    fn protected_prompt_draft_preview_redacts_and_bounds_latest_draft() {
        let harness = HarnessConfig::codex();
        let content = format!(
            "\
history
› {}
gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% used
",
            format!(
                "Implement feature using OPENAI_API_KEY=sk-proj-{} and then {}",
                "a".repeat(32),
                "continue ".repeat(40)
            )
        );

        let preview = protected_prompt_draft_preview(&harness, &content).unwrap();

        assert!(preview.starts_with("› Implement feature"), "{preview}");
        assert!(
            preview.contains("OPENAI_API_KEY=[REDACTED]"),
            "preview must redact secrets before surfacing draft text: {preview}"
        );
        assert!(
            !preview.contains("sk-proj-"),
            "raw secret must not leak into route diagnostics: {preview}"
        );
        assert!(preview.ends_with("..."), "{preview}");
        assert!(
            preview.chars().count() <= 163,
            "preview should be bounded plus ellipsis: {preview}"
        );
    }

    #[test]
    fn pane_idle_dispatch_ready_distinguishes_non_dispatch_from_fast_submit() {
        // #jbrundispatch directive 2: an empty composer at an idle prompt means the
        // trigger never landed (re-send the full trigger); a processing pane means a
        // genuine fast submit (do NOT re-send, or the agent runs twice).
        let h = HarnessConfig::claude();
        assert!(
            pane_idle_dispatch_ready("prior output\n\n❯\n", &h),
            "empty composer at an idle prompt is a non-dispatch"
        );
        assert!(
            !pane_idle_dispatch_ready("❯ agent-doc tasks/x.md\n", &h),
            "a drafted trigger in the composer is not idle"
        );
        assert!(
            !pane_idle_dispatch_ready("Working… (esc to interrupt)\n", &h),
            "a processing pane is not idle — a fast submit must not be re-sent"
        );
    }

    #[test]
    fn direct_pane_existing_draft_detection_enters_only_current_codex_draft() {
        let harness = HarnessConfig::codex();
        let trigger = "agent-doc tasks/agent-doc/agent-doc-bugs2.md";

        let drafted = "\
history line
› agent-doc tasks/agent-doc/agent-doc-bugs2.md
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        assert!(
            direct_pane_existing_draft_visible(drafted, trigger, &harness),
            "visible Codex composer draft should be eligible for append-free Enter"
        );

        let accumulated = "\
› agent-doc tasks/agent-doc/agent-doc-bugs2.md agent-doc tasks/agent-doc/agent-doc-bugs2.md
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        assert!(
            direct_pane_existing_draft_visible(accumulated, trigger, &harness),
            "accumulated duplicate drafts must still be treated as current input"
        );

        let stale_scrollback = "\
› agent-doc tasks/agent-doc/agent-doc-bugs2.md
preflight complete
›
";
        assert!(
            !direct_pane_existing_draft_visible(stale_scrollback, trigger, &harness),
            "an idle prompt below the trigger means it is scrollback, not the active draft"
        );

        let interrupted_with_new_draft = "\
╭─────────────────────────────────────────────╮
│ >_ OpenAI Codex (v0.142.0)                  │
╰─────────────────────────────────────────────╯

› agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/sampleportal.md

■ Conversation interrupted - tell the model what to do differently.

› Use /skills to list available skills

gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% used
";
        assert!(
            !direct_pane_existing_draft_visible(
                interrupted_with_new_draft,
                "agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/sampleportal.md",
                &harness
            ),
            "a cancelled route trigger in scrollback must not receive Enter when a newer composer draft exists"
        );
    }
    #[test]
    fn direct_pane_existing_draft_detection_handles_wrapped_codex_path() {
        let harness = HarnessConfig::codex();
        let trigger =
            "agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md";
        let content = "\
› agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-
doc-bugs2.md
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";

        assert!(
            direct_pane_existing_draft_visible(content, trigger, &harness),
            "wrapped current drafts should be submitted with the profile submit key rather than appended again"
        );
    }

    #[test]
    fn direct_pane_existing_draft_detection_ignores_codex_blank_padding() {
        let harness = HarnessConfig::codex();
        let trigger =
            "agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md";
        let content = "\
╭─────────────────────────────────────────────╮
│ >_ OpenAI Codex (v0.141.0)                  │
╰─────────────────────────────────────────────╯

  Tip: Use /side to start a side conversation in a temporary fork without polluting the main thread.


› agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md


  gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% used








";

        assert!(
            direct_pane_existing_draft_visible(content, trigger, &harness),
            "blank-padded Codex composer captures should still expose the current draft for late Enter retry"
        );
    }

    #[test]
    fn direct_pane_existing_draft_detection_matches_relative_codex_path() {
        let harness = HarnessConfig::codex();
        let trigger =
            "agent-doc /home/brian/work/btakita/agent-loop/src/sample-app/tasks/sampleorders.md";
        let drafted = "\
› agent-doc tasks/sampleorders.md
gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/sample-app · Context 0% used
";

        assert!(
            direct_pane_existing_draft_visible(drafted, trigger, &harness),
            "a visible relative-path Codex draft for the same target should receive Enter instead of an appended absolute trigger"
        );

        let stale_scrollback = "\
› agent-doc tasks/sampleorders.md
preflight complete
›
";
        assert!(
            !direct_pane_existing_draft_visible(stale_scrollback, trigger, &harness),
            "an idle prompt below an equivalent relative-path draft still proves scrollback"
        );

        let different_target = "\
› agent-doc tasks/sampleportal.md
gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/sample-app · Context 0% used
";
        assert!(
            !direct_pane_existing_draft_visible(different_target, trigger, &harness),
            "relative-path equivalence must not collapse different document names"
        );
    }
}

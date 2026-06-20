//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

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
}

const DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR: Duration = Duration::from_millis(900);

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
    let attempt =
        DISPATCH_INJECT_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
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

/// Default bare-Enter resubmit cap when the routed trigger stays drafted in the
/// composer (not yet submitted).
const DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT: usize = 6;

/// Max bare-Enter resubmits while the trigger is still visible (drafted, not
/// submitted) — the supervisor "retry until the prompt is submitted" budget
/// (`#jbclaudesubmit`). Each resubmit re-polls for a full acceptance window, so the
/// wall-clock budget is roughly this count times `direct_pane_submit_acceptance_timeout`.
/// Raised from 3 → 6 (and made env-tunable via
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

#[derive(Debug, Default)]
struct DirectPaneAcceptancePollState {
    saw_trigger_visible: bool,
    first_empty_capture_at: Option<Duration>,
}

fn direct_pane_acceptance_poll_status(
    state: &mut DirectPaneAcceptancePollState,
    elapsed: Duration,
    trigger_visible: bool,
) -> Option<CommandDispatchStatus> {
    if trigger_visible {
        state.saw_trigger_visible = true;
        state.first_empty_capture_at = None;
        return None;
    }

    if state.saw_trigger_visible {
        return Some(CommandDispatchStatus::Accepted);
    }

    let first_empty_at = state.first_empty_capture_at.get_or_insert(elapsed);
    if elapsed.saturating_sub(*first_empty_at) >= DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR {
        Some(CommandDispatchStatus::Accepted)
    } else {
        None
    }
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
                    log_route_submit_observation(RouteSubmitObservationFacts {
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
                    let not_dispatched = !poll_state.saw_trigger_visible
                        && last_capture
                            .as_ref()
                            .map(|(_, _, _, content)| {
                                pane_idle_dispatch_ready(content, harness)
                            })
                            .unwrap_or(false);
                    return DirectPaneAcceptance {
                        status: CommandDispatchStatus::Accepted,
                        elapsed,
                        trigger_visible: false,
                        not_dispatched,
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
    if let Some((visible, capture_len, capture_hash, content)) = last_capture.as_ref() {
        if *visible {
            preserve_route_pane_snapshot(file, pane, harness, phase, content);
        }
        log_route_submit_observation(RouteSubmitObservationFacts {
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
        log_route_submit_observation(RouteSubmitObservationFacts {
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

/// `#jbcodexsubmit` / `#jbclaudesubmit`: decide whether a timed-out direct-pane
/// submit warrants a bare submit-key re-submit. This is recovery for a visibly
/// drafted trigger left by an older submit path or a missed submit key; the
/// harness-specific eligibility lives in the shared tmux submit profile.
/// Only re-send when the attempt timed out with the trigger still visible
/// (an empty composer must not receive a stray bare submit key).
pub(crate) fn direct_pane_needs_enter_resubmit(
    harness_binary: &str,
    status: CommandDispatchStatus,
    trigger_visible: bool,
) -> bool {
    crate::sessions::tmux_submit_profile_for_harness(harness_binary).pending_draft_enter_resubmit()
        && status == CommandDispatchStatus::TimedOut
        && trigger_visible
}

fn direct_pane_can_continue_enter_resubmit(
    harness_binary: &str,
    status: CommandDispatchStatus,
    trigger_visible: bool,
    attempts_sent: usize,
) -> bool {
    attempts_sent < direct_pane_max_enter_resubmits()
        && direct_pane_needs_enter_resubmit(harness_binary, status, trigger_visible)
}

/// A route reopen may already be drafted in the composer from a prior failed
/// editor dispatch. In that case, append-free recovery is a single profile
/// submit key.
pub(crate) fn direct_pane_can_enter_existing_draft(
    harness_binary: &str,
    trigger_visible: bool,
) -> bool {
    crate::sessions::tmux_submit_profile_for_harness(harness_binary).pending_draft_enter_resubmit()
        && trigger_visible
}

pub(crate) fn direct_pane_existing_draft_visible(
    content: &str,
    trigger: &str,
    harness: &HarnessConfig,
) -> bool {
    let recent_lines: Vec<String> = content
        .lines()
        .rev()
        .take(8)
        .map(crate::prompt::strip_ansi)
        .collect();
    let lines: Vec<&String> = recent_lines.iter().rev().collect();
    for start in 0..lines.len() {
        if !line_contains_trigger(lines[start], trigger)
            && !line_contains_equivalent_agent_doc_path_trigger(lines[start], trigger)
            && !wrapped_trigger_starts_at_line(&lines, start, trigger)
        {
            continue;
        }
        let later_has_idle_prompt = lines
            .iter()
            .skip(start + 1)
            .any(|line| harness.is_dispatch_ready_prompt_line(line));
        return !later_has_idle_prompt;
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

/// `accepted` when the re-submit consumed the drafted trigger, `still_visible`
/// when the bare submit key did not submit either. Kept pure for proof-line tests.
pub(crate) fn resubmit_result_label(second_status: CommandDispatchStatus) -> &'static str {
    if second_status == CommandDispatchStatus::Accepted {
        "accepted"
    } else {
        "still_visible"
    }
}

/// Build the operator-greppable `route_submit_resubmit` proof line. The live
/// test asserts on exactly this shape in `ops.log`.
pub(crate) fn route_submit_resubmit_proof_line(
    file: &Path,
    pane: &str,
    harness_binary: &str,
    second_status: CommandDispatchStatus,
    elapsed: Duration,
    attempt: usize,
) -> String {
    let submit_key = crate::sessions::tmux_submit_key_for_harness(harness_binary);
    let mut message = format!(
        "route_submit_resubmit file={} pane={} harness={} action=submit_key key={} result={} elapsed_ms={} attempt={}",
        file.display(),
        pane,
        harness_binary,
        submit_key,
        resubmit_result_label(second_status),
        elapsed.as_millis(),
        attempt
    );
    append_editor_route_attempt(&mut message);
    message
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
    let submit_key = crate::sessions::tmux_submit_key_for_harness(&harness.binary);
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
    crate::ops_log::log_op(
        file,
        &route_submit_resubmit_proof_line(
            file,
            pane,
            &harness.binary,
            second.status,
            second.elapsed,
            attempt,
        ),
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
    let mut attempts_sent = 0usize;

    while direct_pane_can_continue_enter_resubmit(
        &harness.binary,
        status,
        trigger_visible,
        attempts_sent,
    ) {
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
    }

    DirectPaneAcceptance {
        status,
        elapsed,
        trigger_visible,
        // The bare-Enter resubmit path only handles a drafted (visible) trigger, so a
        // non-dispatch is never produced here.
        not_dispatched: false,
    }
}

/// Foreground program names tmux reports via `#{pane_current_command}` when a
/// pane has fallen back to a bare interactive shell because the harness
/// crashed/exited. A login shell can show a leading `-` (for example `-zsh`).
pub(crate) fn pane_current_command_is_bare_shell(cmd: &str) -> bool {
    let name = cmd.trim().trim_start_matches('-');
    matches!(
        name,
        "zsh" | "bash" | "sh" | "fish" | "dash" | "ksh" | "tcsh" | "csh"
    )
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
            None => return Ok(()),
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
    let payload = routed_trigger_payload(&trigger);
    validate_routed_trigger_payload(harness, &trigger, &payload)?;
    let existing_draft_visible = match sessions::capture_pane(tmux, pane) {
        Ok(content) => {
            let visible = direct_pane_existing_draft_visible(&content, &trigger, harness);
            if visible {
                preserve_route_pane_snapshot(
                    file,
                    pane,
                    harness,
                    "direct_pane_existing_draft_visible",
                    &content,
                );
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
    if direct_pane_can_enter_existing_draft(&harness.binary, existing_draft_visible) {
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
            },
        );
        return Ok(CommandDispatchResult {
            status: first.status,
            elapsed: first.elapsed,
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
        });
    }
    if acceptance.status == CommandDispatchStatus::Accepted {
        return Ok(CommandDispatchResult {
            status: acceptance.status,
            elapsed: acceptance.elapsed,
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
    let payload = routed_trigger_payload(&trigger);
    validate_routed_trigger_payload(harness, &trigger, &payload)?;
    let flash_msg = format!("⏳ {}", harness.trigger_command(&short_name));
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", pane, "-d", "2000", &flash_msg])
        .status()
    {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    let (transform, submit_key) = routed_trigger_submit_diagnostic(&harness.binary);
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
    let payload = routed_trigger_payload(&trigger);
    validate_routed_trigger_payload(harness, &trigger, &payload)?;
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
        bytes: routed_trigger_submit_payload(&payload),
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

    let timeout = routed_dispatch_start_timeout(harness);
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
        log_route_submit_observation(RouteSubmitObservationFacts {
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
    log_route_submit_observation(RouteSubmitObservationFacts {
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
    match sessions::capture_pane(tmux, pane) {
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
        }
        Err(err) => {
            eprintln!(
                "[route] warning: failed to capture pane {} after unproven supervisor dispatch: {}",
                pane, err
            );
        }
    }
    if options.print_unproven_progress {
        eprintln!(
            "[route] authoritative actor accepted the {} reopen for {} in pane {}, but no routed submission proof appeared after {}s",
            harness.binary,
            file.display(),
            pane,
            timeout.as_secs()
        );
    }
    Ok(RoutedDispatchStartProof::CommandAcceptedOnly)
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
    state: crate::session_actor::ActorState,
    file: &Path,
) -> String {
    actor_recovery_hint(actor_dispatch_state(state), &file.display().to_string())
}

#[cfg(test)]
pub(crate) fn authoritative_actor_dispatch_can_queue_optimistically(
    state: crate::session_actor::ActorState,
) -> bool {
    crate::flow::routed_reopen::actor_can_queue_optimistically(actor_dispatch_state(state))
}

pub(crate) fn canonical_dispatch_file(path: &std::path::Path) -> std::path::PathBuf {
    let resolved = crate::git::resolve_absolute_file_path(path);
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

pub(crate) fn canonical_registered_file(entry: &sessions::SessionEntry) -> std::path::PathBuf {
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
    crate::snapshot::find_project_root(&requested)
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

pub(crate) fn load_dispatch_registry(file_path: &str) -> Result<sessions::SessionRegistry> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    sessions::load_in(&base_dir)
}

pub(crate) fn deregister_dispatch_registration(file_path: &str, session_id: &str) -> Result<bool> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let registry_path = sessions::registry_path_in(&base_dir);
    let _lock = sessions::RegistryLock::acquire(&registry_path)?;
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
    registry: &sessions::SessionRegistry,
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
    if !direct_pane_can_enter_existing_draft(&harness.binary, visible) {
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
        log_route_submit_observation(RouteSubmitObservationFacts {
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
    dispatch_routed_reopen_with_mode(tmux, file, pane, file_path, harness, true)
}

pub(crate) fn dispatch_routed_reopen_with_mode(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
    print_unproven_progress: bool,
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

    let timeout = routed_dispatch_start_timeout(harness);
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
        log_route_submit_observation(RouteSubmitObservationFacts {
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
            log_route_submit_observation(RouteSubmitObservationFacts {
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
            match sessions::capture_pane(tmux, pane) {
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
                }
                Err(err) => {
                    eprintln!(
                        "[route] warning: failed to capture pane {} after unproven direct dispatch: {}",
                        pane, err
                    );
                }
            }
            if print_unproven_progress {
                eprintln!(
                    "[route] bare {} reopen for {} was accepted in pane {}, but no routed submission proof appeared after {}s",
                    harness.binary,
                    file.display(),
                    pane,
                    timeout.as_secs()
                );
            }
            Ok(RoutedDispatchStartProof::CommandAcceptedOnly)
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

pub(crate) fn routed_trigger_payload(trigger: &str) -> String {
    trigger.to_string()
}

pub(crate) fn apply_plain_trigger_override(harness: &mut HarnessConfig) {
    harness.trigger_command_template = "agent-doc {file}".to_string();
}

pub(crate) fn routed_trigger_submit_payload(payload: &str) -> String {
    crate::supervisor::ipc::normalize_submit_text(payload)
}

pub(crate) fn validate_routed_trigger_payload(
    harness: &HarnessConfig,
    trigger: &str,
    payload: &str,
) -> Result<()> {
    if harness.binary == "codex"
        && (payload != trigger || payload.contains('\n') || payload.contains('\r'))
    {
        anyhow::bail!(
            "internal route bug: Codex reroute payload must stay the bare `agent-doc <FILE>` reopen; refusing to inject {:?}",
            payload
        );
    }
    Ok(())
}

fn routed_trigger_submit_diagnostic(harness_binary: &str) -> (&'static str, &'static str) {
    let profile = crate::sessions::tmux_submit_profile_for_harness(harness_binary);
    (profile.transform(), profile.submit_key())
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::flow::routed_reopen::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};
    #[test]
    fn authoritative_actor_starting_hint_names_reroute_and_restart() {
        let file = std::path::Path::new("/tmp/session.md");
        let hint = authoritative_actor_dispatch_recovery_hint(
            crate::session_actor::ActorState::Starting,
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
    fn pane_current_command_is_bare_shell_classifies_shells() {
        // #1vhn: shells (with or without a login `-` prefix) indicate the harness
        // crashed/exited to a bare prompt; harness/agent processes do not.
        for shell in [
            "zsh", "bash", "sh", "fish", "dash", "ksh", "tcsh", "csh", "-zsh", "-bash", " zsh ",
        ] {
            assert!(
                pane_current_command_is_bare_shell(shell),
                "{shell:?} should classify as a bare shell"
            );
        }
        for not_shell in [
            "claude",
            "node",
            "codex",
            "opencode",
            "bun",
            "agent-doc",
            "sleep",
            "cat",
            "vim",
            "",
        ] {
            assert!(
                !pane_current_command_is_bare_shell(not_shell),
                "{not_shell:?} should NOT classify as a bare shell"
            );
        }
    }
    #[test]
    fn routed_trigger_submit_diagnostic_names_codex_enter_key() {
        assert_eq!(
            routed_trigger_submit_diagnostic("codex"),
            ("tmux_text_enter", "Enter")
        );
        assert_eq!(
            routed_trigger_submit_diagnostic("opencode"),
            ("tmux_text_enter", "Enter")
        );
        assert_eq!(
            routed_trigger_submit_diagnostic("claude"),
            ("tmux_text_enter", "Enter")
        );
    }
    #[test]
    fn direct_pane_acceptance_waits_for_stable_empty_capture() {
        let mut state = DirectPaneAcceptancePollState::default();
        assert_eq!(
            direct_pane_acceptance_poll_status(&mut state, Duration::from_millis(0), false),
            None
        );
        assert_eq!(
            direct_pane_acceptance_poll_status(
                &mut state,
                DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR - Duration::from_millis(1),
                false
            ),
            None
        );
        assert_eq!(
            direct_pane_acceptance_poll_status(
                &mut state,
                DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR,
                false
            ),
            Some(CommandDispatchStatus::Accepted)
        );
    }
    #[test]
    fn direct_pane_acceptance_accepts_after_visible_draft_disappears() {
        let mut state = DirectPaneAcceptancePollState::default();
        assert_eq!(
            direct_pane_acceptance_poll_status(&mut state, Duration::from_millis(0), false),
            None
        );
        assert_eq!(
            direct_pane_acceptance_poll_status(&mut state, Duration::from_millis(150), true),
            None
        );
        assert_eq!(
            direct_pane_acceptance_poll_status(&mut state, Duration::from_millis(300), false),
            Some(CommandDispatchStatus::Accepted)
        );
    }
    #[test]
    fn direct_pane_resubmit_only_on_timeout_with_trigger_visible() {
        // `#jbcodexsubmit` / `#jbclaudesubmit`: a direct-pane submit that times out
        // with the trigger still drafted in the composer earns a guarded blank-text
        // Enter re-submit. The operator reported the non-submit on BOTH Codex and
        // Claude panes; both use the same text+Enter submit path.
        assert!(direct_pane_needs_enter_resubmit(
            "codex",
            CommandDispatchStatus::TimedOut,
            true
        ));
        assert!(direct_pane_needs_enter_resubmit(
            "claude",
            CommandDispatchStatus::TimedOut,
            true
        ));
        // Trigger consumed → no re-submit even on a timeout report (a stray bare
        // Enter into an empty composer could fire an unintended submit).
        assert!(!direct_pane_needs_enter_resubmit(
            "codex",
            CommandDispatchStatus::TimedOut,
            false
        ));
        assert!(!direct_pane_needs_enter_resubmit(
            "claude",
            CommandDispatchStatus::TimedOut,
            false
        ));
        // Accepted → already submitted, never re-send.
        assert!(!direct_pane_needs_enter_resubmit(
            "codex",
            CommandDispatchStatus::Accepted,
            true
        ));
        assert!(!direct_pane_needs_enter_resubmit(
            "claude",
            CommandDispatchStatus::Accepted,
            true
        ));
        // OpenCode shares the same text + profile submit-key path, so a visible
        // draft after timeout earns the same guarded Enter recovery.
        assert!(direct_pane_needs_enter_resubmit(
            "opencode",
            CommandDispatchStatus::TimedOut,
            true
        ));
    }
    #[test]
    fn direct_pane_resubmit_is_scoped_to_timed_out_visible_enter_harnesses() {
        // #jbcodexsubmit / #jbclaudesubmit / #efscodexsubmit: a direct-pane submit
        // that timed out with the trigger STILL VISIBLE (the composer left the routed
        // prompt drafted) warrants bounded blank-text Enter re-submits. Scoped to the
        // harnesses that submit via the text+Enter path.
        for harness in ["codex", "claude", "opencode"] {
            assert!(
                direct_pane_needs_enter_resubmit(harness, CommandDispatchStatus::TimedOut, true),
                "{harness} timed-out-with-visible-trigger must earn one Enter re-submit"
            );
            // Already accepted ⇒ the prompt submitted, nothing to re-send.
            assert!(!direct_pane_needs_enter_resubmit(
                harness,
                CommandDispatchStatus::Accepted,
                true
            ));
            // Timed out but the trigger was consumed (not visible) ⇒ not a non-submit;
            // a bare submit key here could fire an unintended empty submit, so don't re-send.
            assert!(!direct_pane_needs_enter_resubmit(
                harness,
                CommandDispatchStatus::TimedOut,
                false
            ));
        }
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
    fn direct_pane_enter_resubmit_is_bounded_while_trigger_remains_visible() {
        let cap = direct_pane_max_enter_resubmits();
        assert!(
            cap >= DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT,
            "default resubmit budget should honor `retry until submitted` (#jbclaudesubmit)"
        );
        for attempts_sent in 0..cap {
            assert!(
                direct_pane_can_continue_enter_resubmit(
                    "codex",
                    CommandDispatchStatus::TimedOut,
                    true,
                    attempts_sent,
                ),
                "attempt {attempts_sent} should still be eligible while the trigger remains visible"
            );
        }
        assert!(!direct_pane_can_continue_enter_resubmit(
            "codex",
            CommandDispatchStatus::TimedOut,
            true,
            cap,
        ));
        assert!(!direct_pane_can_continue_enter_resubmit(
            "codex",
            CommandDispatchStatus::Accepted,
            true,
            0,
        ));
        assert!(!direct_pane_can_continue_enter_resubmit(
            "codex",
            CommandDispatchStatus::TimedOut,
            false,
            0,
        ));
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

        assert!(direct_pane_can_enter_existing_draft("codex", true));
        assert!(direct_pane_can_enter_existing_draft("claude", true));
        assert!(direct_pane_can_enter_existing_draft("opencode", true));
        assert!(!direct_pane_can_enter_existing_draft("codex", false));
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
    fn direct_pane_existing_draft_detection_matches_relative_codex_path() {
        let harness = HarnessConfig::codex();
        let trigger = "agent-doc /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md";
        let drafted = "\
› agent-doc tasks/monsterrodholders.md
gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/boost-client · Context 0% used
";

        assert!(
            direct_pane_existing_draft_visible(drafted, trigger, &harness),
            "a visible relative-path Codex draft for the same target should receive Enter instead of an appended absolute trigger"
        );

        let stale_scrollback = "\
› agent-doc tasks/monsterrodholders.md
preflight complete
›
";
        assert!(
            !direct_pane_existing_draft_visible(stale_scrollback, trigger, &harness),
            "an idle prompt below an equivalent relative-path draft still proves scrollback"
        );

        let different_target = "\
› agent-doc tasks/equityfundingsource.md
gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/boost-client · Context 0% used
";
        assert!(
            !direct_pane_existing_draft_visible(different_target, trigger, &harness),
            "relative-path equivalence must not collapse different document names"
        );
    }
    #[test]
    fn route_submit_resubmit_proof_line_is_operator_greppable_for_both_harnesses() {
        // #jbcodexsubmit / #jbclaudesubmit: the operator's live test greps ops.log
        // for `route_submit_resubmit ... action=submit_key key=Enter result=accepted|still_visible`.
        // Assert the exact shape the binary emits for bounded re-submit attempts on
        // harnesses that travel the text+Enter submit path.
        let file = std::path::Path::new("/tmp/plan.md");
        for harness in ["codex", "claude", "opencode"] {
            // First re-submit consumed the drafted trigger ⇒ result=accepted.
            let accepted = route_submit_resubmit_proof_line(
                file,
                "%42",
                harness,
                CommandDispatchStatus::Accepted,
                Duration::from_millis(120),
                1,
            );
            assert_eq!(
                accepted,
                format!(
                    "route_submit_resubmit file=/tmp/plan.md pane=%42 harness={harness} action=submit_key key=Enter result=accepted elapsed_ms=120 attempt=1"
                )
            );
            // The submit key still did not submit ⇒ result=still_visible, with the
            // attempt number telling the operator where the bounded loop ended.
            let still = route_submit_resubmit_proof_line(
                file,
                "%42",
                harness,
                CommandDispatchStatus::TimedOut,
                Duration::from_millis(300),
                3,
            );
            assert!(
                still.contains("action=submit_key key=Enter result=still_visible"),
                "unsubmitted re-poll must report still_visible: {still}"
            );
            assert!(
                still.contains("attempt=3"),
                "unsubmitted re-poll must report the bounded attempt: {still}"
            );
        }
        assert_eq!(
            resubmit_result_label(CommandDispatchStatus::Accepted),
            "accepted"
        );
        assert_eq!(
            resubmit_result_label(CommandDispatchStatus::TimedOut),
            "still_visible"
        );
    }
}

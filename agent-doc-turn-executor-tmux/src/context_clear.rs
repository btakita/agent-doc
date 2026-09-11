use std::fmt::Display;
use std::path::Path;
use std::time::Duration;

/// (`#clearsubmitlabel`) The deadline can expire for two unrelated reasons, and
/// they need different operator actions:
///
/// - [`Self::StillVisible`] — the clear command was in the composer at the last
///   capture. It was delivered and NOT consumed; the pane is wedged and the fix
///   is to restore an idle prompt (this is also the only shape an Enter-resubmit
///   may retry).
/// - [`Self::Unobserved`] — the deadline expired with no evidence in EITHER
///   direction: the command was never seen in the composer and the pane never
///   changed. Whether the clear happened is unknown.
///
/// One `TimedOut` variant used to carry both, reporting
/// `result=command_still_visible` alongside `command_visible=false` — a line
/// that states the opposite of what was observed. The acceptance rule is
/// deliberately unchanged: neither is `Accepted`, both still fail closed through
/// `require_context_clear_submit_accepted`. Only the report is split, because a
/// clear that may not have run must never be reported as done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextClearSubmitStatus {
    Accepted,
    /// (`#clearsubmitunobserved`) No transition was observed, but the pane's
    /// scrollback proves it holds no conversation — the cleared state that
    /// `/clear` exists to produce. See [`Self::is_accepted`].
    AcceptedClearedState,
    StillVisible,
    Unobserved,
    /// (`#clearqueuedcomposer`) The harness is mid-turn and QUEUED the command
    /// instead of executing it. The clear did not run, and it is not unknown
    /// whether it ran — waiting or interrupting is what unblocks it.
    HarnessQueuedInput,
    CaptureFailed,
    Unrendered,
}

impl ContextClearSubmitStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::AcceptedClearedState => "accepted_cleared_state",
            Self::StillVisible => "command_still_visible",
            Self::Unobserved => "submission_unobserved",
            Self::HarnessQueuedInput => "harness_queued_input",
            Self::CaptureFailed => "capture_failed",
            Self::Unrendered => "pane_not_rendered",
        }
    }

    /// Whether this outcome satisfies the clear. Callers must use this rather
    /// than `== Accepted`, or the state-proven variant silently fails closed.
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted | Self::AcceptedClearedState)
    }

    pub const fn issue(self) -> Option<&'static str> {
        match self {
            Self::StillVisible => Some("prompt_not_submitted"),
            Self::Unobserved => Some("submit_unobserved"),
            Self::HarnessQueuedInput => Some("clear_queued_during_busy_turn"),
            Self::CaptureFailed => Some("submit_unverified_capture_failed"),
            Self::Unrendered => Some("submit_unverified_unrendered"),
            Self::Accepted | Self::AcceptedClearedState => None,
        }
    }

    /// The exact unblocker for this outcome. `StillVisible` is a wedged composer;
    /// `Unobserved` is an unknown, where telling the operator to "restore an idle
    /// prompt" would be a guess — the prompt may already be idle.
    pub const fn unblocker(self) -> &'static str {
        match self {
            Self::StillVisible => "clear_command_not_consumed",
            Self::Unobserved => "clear_submission_unobserved",
            Self::HarnessQueuedInput => "clear_queued_behind_busy_turn",
            Self::CaptureFailed => "clear_submit_capture_failed",
            Self::Unrendered => "wait_for_pane_render",
            Self::Accepted | Self::AcceptedClearedState => "none",
        }
    }

    pub const fn next_action(self) -> &'static str {
        match self {
            Self::StillVisible => "restore_idle_prompt_and_retry",
            Self::Unobserved | Self::CaptureFailed => "verify_pane_state_then_retry",
            Self::HarnessQueuedInput => "wait_for_turn_or_interrupt_then_retry",
            Self::Unrendered => "wait_for_pane_render",
            Self::Accepted | Self::AcceptedClearedState => "none",
        }
    }
}

/// (`#clearqueuedcomposer`) Does the capture show a harness composer that is
/// QUEUEING operator input rather than executing it?
///
/// Observed live 2026-09-11 on `src/haiven-dev/tasks/sdk.md`, pane `%1`: a
/// Claude Code session mid-turn (`❯ Press up to edit queued messages`, two
/// subagents running) took a `/clear`, queued it, and the acceptance poll
/// reported `submission_unobserved` — "whether the clear ran is unknown" about a
/// pane whose state was fully legible. Both the initial attempt and the command
/// repair resend landed in the same queue, 2s apart.
///
/// A queued clear is still NOT accepted: this changes the label and the
/// unblocker, never the acceptance rule (same contract as `#clearsubmitlabel`).
pub fn context_clear_capture_shows_queued_input(
    capture: &str,
    is_queued_input_placeholder_line: impl Fn(&str) -> bool,
) -> bool {
    capture
        .lines()
        .map(crate::prompt::strip_ansi)
        .any(|line| is_queued_input_placeholder_line(line.trim()))
}

/// Scrollback lines a pane may retain and still count as cleared.
///
/// A cleared Claude Code / Codex / OpenCode pane shows a banner, a few hint
/// lines, and the prompt — tens of lines. A pane holding a real conversation
/// carries hundreds to thousands. The threshold sits far above the first and far
/// below the second on purpose: it must never call a pane with retained context
/// "cleared", and a generous margin costs nothing because the two populations are
/// orders of magnitude apart.
pub const CONTEXT_CLEAR_CLEARED_STATE_MAX_HISTORY_LINES: usize = 200;

/// (`#clearsubmitunobserved`) Does the pane's scrollback prove it is in the
/// cleared state?
///
/// `/clear` is **idempotent**: its contract is a state ("this pane holds no
/// conversation"), not a transition. The poll loop can only observe transitions,
/// so when a clear succeeds on a pane that had nothing to clear — the normal case
/// immediately after a cold start — it sees no command in the composer and no
/// content change, and reports `Unobserved` for a clear that did exactly what was
/// asked. Asking the state question directly resolves it.
///
/// This never weakens the guard. A pane whose clear was genuinely lost still
/// holds its conversation, so its scrollback is far over the threshold and it
/// stays blocked. The only outcome this changes is the one where the desired end
/// state demonstrably already holds.
///
/// Fails closed on the ambiguous inputs: a composer still showing the command is
/// an unconsumed draft regardless of scrollback, and a capture with no
/// dispatch-ready prompt line is not a settled pane.
pub fn context_clear_history_proves_cleared_state(
    history: &str,
    command: &str,
    max_history_lines: usize,
    is_dispatch_ready_prompt_line: impl Fn(&str) -> bool + Copy,
) -> bool {
    if context_clear_command_visible_in_active_input(
        history,
        command,
        is_dispatch_ready_prompt_line,
    ) {
        return false;
    }
    let retained: Vec<String> = history
        .lines()
        .map(crate::prompt::strip_ansi)
        .filter(|line| !line.trim().is_empty())
        .collect();
    if retained.len() > max_history_lines {
        return false;
    }
    retained
        .iter()
        .any(|line| is_dispatch_ready_prompt_line(line.trim()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextClearSubmitObservation {
    pub status: ContextClearSubmitStatus,
    pub elapsed: Duration,
    pub command_visible: bool,
    /// Whether the pane capture ever differed from its pre-delivery hash
    /// (`#cleardoublesend`).
    ///
    /// The poll loop already computes this per frame to recognize acceptance,
    /// but used to discard it on timeout — so the retry decision could only see
    /// that acceptance was not *proven* and had to guess why. `true` means input
    /// visibly reached the pane and a resend would be a second submit; `false`
    /// means the pane never moved, which is the only state a lost delivery can
    /// produce.
    pub content_changed_since_delivery: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContextClearSubmitPollState {
    saw_command_visible: bool,
    saw_changed_absent: bool,
}

impl ContextClearSubmitPollState {
    pub const fn saw_submission_evidence(self) -> bool {
        self.saw_command_visible || self.saw_changed_absent
    }
}

/// A blank repaint is absence of terminal evidence, never a submitted clear.
/// Keep the ordinary stuck-draft budget short, but let an unrendered terminal
/// finish drawing before deciding to resend a destructive control command.
pub fn context_clear_observation_budget(
    content: &str,
    ordinary: Duration,
    rendering: Duration,
) -> Duration {
    if content
        .lines()
        .all(|line| crate::prompt::strip_ansi(line).trim().is_empty())
    {
        rendering
    } else {
        ordinary
    }
}

/// Only a settled prompt can prove a command disappeared. Blank redraws,
/// spinners and unrelated output changes cannot acknowledge the clear.
pub fn context_clear_submit_frame_status(
    state: &mut ContextClearSubmitPollState,
    command_visible: bool,
    content_changed_since_delivery: bool,
    prompt_ready: bool,
) -> Option<ContextClearSubmitStatus> {
    if !command_visible && !prompt_ready {
        return None;
    }
    context_clear_submit_poll_status(state, command_visible, content_changed_since_delivery)
}

pub fn context_clear_submit_poll_status(
    state: &mut ContextClearSubmitPollState,
    command_visible: bool,
    content_changed_since_delivery: bool,
) -> Option<ContextClearSubmitStatus> {
    if command_visible {
        state.saw_command_visible = true;
        return None;
    }
    if state.saw_command_visible {
        return Some(ContextClearSubmitStatus::Accepted);
    }
    if content_changed_since_delivery {
        state.saw_changed_absent = true;
        return Some(ContextClearSubmitStatus::Accepted);
    }
    None
}

pub fn context_clear_command_visible_in_active_input(
    content: &str,
    command: &str,
    is_dispatch_ready_prompt_line: impl Fn(&str) -> bool,
) -> bool {
    let recent_lines: Vec<String> = content
        .lines()
        .rev()
        .take(8)
        .map(crate::prompt::strip_ansi)
        .collect();
    let lines: Vec<&String> = recent_lines.iter().rev().collect();
    for start in 0..lines.len() {
        if !line_shows_context_clear_command_input(lines[start], command) {
            continue;
        }
        let later_has_idle_prompt = lines.iter().skip(start + 1).any(|line| {
            is_dispatch_ready_prompt_line(line.trim())
                || line_starts_with_context_clear_prompt_prefix(line)
        });
        if later_has_idle_prompt {
            continue;
        }
        return true;
    }
    false
}

pub fn context_clear_submit_needs_enter_resubmit(
    observation: &ContextClearSubmitObservation,
    pending_draft_enter_resubmit: bool,
) -> bool {
    pending_draft_enter_resubmit
        && observation.status == ContextClearSubmitStatus::StillVisible
        && observation.command_visible
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextClearSubmitRetryFacts {
    pub observation: ContextClearSubmitObservation,
    pub pending_draft_enter_resubmit: bool,
    pub attempts_sent: usize,
    pub max_attempts: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextClearSubmitRetryAction {
    /// The command is still visible in a split-submit composer; retry only the
    /// harness submit key so the draft is not duplicated.
    SubmitKey,
    /// Delivery produced no observable command or transition while retained
    /// history proves the clear did not take effect. `/clear`-style commands
    /// are idempotent, so repair the lost delivery with the full command.
    ResendCommand,
}

impl ContextClearSubmitRetryAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SubmitKey => "submit_key",
            Self::ResendCommand => "resend_command",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextClearSubmitRetryProofFacts<'a> {
    pub action: ContextClearSubmitRetryAction,
    pub submit_key: &'a str,
    pub attempt: usize,
    pub max_attempts: usize,
    pub observation: ContextClearSubmitObservation,
}

pub fn context_clear_submit_retry_action(
    facts: ContextClearSubmitRetryFacts,
) -> Option<ContextClearSubmitRetryAction> {
    if facts.attempts_sent >= facts.max_attempts {
        return None;
    }
    if context_clear_submit_needs_enter_resubmit(
        &facts.observation,
        facts.pending_draft_enter_resubmit,
    ) {
        return Some(ContextClearSubmitRetryAction::SubmitKey);
    }
    // `#cleardoublesend`: `Unobserved` is "I looked and got no answer", NOT "the
    // delivery was lost" — the same collapse `#idlerevisionreactive` forbids,
    // and it inverts behavior at the worst moment. Resending the full command
    // text is a SECOND submit, and the observation window is exactly the case
    // where we cannot tell whether the first one landed.
    //
    // Live 2026-09-11 on `src/haiven-dev/tasks/backend.md`, pane `%12`: the
    // supervisor-IPC clear reported `submission_unobserved`, the repair retyped
    // `/clear`+Enter, and the pane capture grew `1157 -> 1183` bytes across it —
    // the resend visibly landed, so the pane had been accepting input all along.
    // The operator got two clears and a `blocked` verdict.
    //
    // A pane whose capture never moved since delivery is the one state a lost
    // delivery does produce, so that is what earns the resend.
    (facts.observation.status == ContextClearSubmitStatus::Unobserved
        && !facts.observation.command_visible
        && !facts.observation.content_changed_since_delivery)
        .then_some(ContextClearSubmitRetryAction::ResendCommand)
}

pub fn context_clear_submit_can_enter_resubmit(facts: ContextClearSubmitRetryFacts) -> bool {
    context_clear_submit_retry_action(facts) == Some(ContextClearSubmitRetryAction::SubmitKey)
}

pub fn context_clear_submit_observation_line(
    file: impl Display,
    pane: &str,
    harness: &str,
    phase: &str,
    observation: ContextClearSubmitObservation,
    capture_len: Option<usize>,
    capture_hash: Option<&str>,
) -> String {
    let mut line = format!(
        "session_clear_submit_observation file={} pane={} harness={} phase={} result={} elapsed_ms={} command_visible={}",
        file,
        pane,
        harness,
        phase,
        observation.status.as_str(),
        observation.elapsed.as_millis(),
        observation.command_visible
    );
    if let Some(capture_len) = capture_len {
        line.push_str(&format!(" capture_len={capture_len}"));
    }
    if let Some(capture_hash) = capture_hash {
        line.push_str(&format!(" capture_hash={capture_hash}"));
    }
    if let Some(issue) = observation.status.issue() {
        line.push_str(&format!(" issue={issue}"));
    }
    line
}

pub fn context_clear_submit_resubmit_proof_line(
    file: impl Display,
    pane: &str,
    harness: &str,
    facts: ContextClearSubmitRetryProofFacts<'_>,
) -> String {
    let result = match facts.observation.status {
        ContextClearSubmitStatus::Accepted => "accepted",
        ContextClearSubmitStatus::AcceptedClearedState => "accepted_cleared_state",
        ContextClearSubmitStatus::StillVisible => "still_visible",
        ContextClearSubmitStatus::Unobserved => "unobserved",
        ContextClearSubmitStatus::HarnessQueuedInput => "harness_queued_input",
        ContextClearSubmitStatus::CaptureFailed => "capture_failed",
        ContextClearSubmitStatus::Unrendered => "pane_not_rendered",
    };
    format!(
        "session_clear_submit_resubmit file={} pane={} harness={} action={} key={} attempt={} max_attempts={} result={} elapsed_ms={}",
        file,
        pane,
        harness,
        facts.action.as_str(),
        facts.submit_key,
        facts.attempt,
        facts.max_attempts,
        result,
        facts.observation.elapsed.as_millis()
    )
}

pub fn context_clear_submit_blocked_line(
    file: impl Display,
    pane: &str,
    harness: &str,
    command: &str,
    phase: &str,
    observation: ContextClearSubmitObservation,
) -> String {
    format!(
        "session_clear_submit_blocked file={} pane={} harness={} phase={} command={} result={} elapsed_ms={} command_visible={} issue={} ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action={} unblocker={}",
        file,
        pane,
        harness,
        phase,
        command,
        observation.status.as_str(),
        observation.elapsed.as_millis(),
        observation.command_visible,
        observation.status.issue().unwrap_or("submit_not_accepted"),
        observation.status.next_action(),
        observation.status.unblocker()
    )
}

pub fn context_clear_submit_blocked_message(
    file: impl Display,
    pane: &str,
    harness: &str,
    command: &str,
    phase: &str,
    observation: ContextClearSubmitObservation,
) -> String {
    // `#clearsubmitlabel`: the remedy sentence must match what was actually
    // observed. "Restore an idle prompt" is right for a composer still holding
    // the command; for an unobserved submit the prompt may already be idle and
    // the real question is whether the clear ran at all.
    let remedy = match observation.status {
        ContextClearSubmitStatus::Unrendered => format!(
            "The {harness} pane stayed blank while awaiting its repaint. No duplicate clear was sent; wait for the prompt to render and check its context"
        ),
        ContextClearSubmitStatus::Unobserved => format!(
            "No submission evidence was seen in either direction, so whether the clear ran is unknown. Check the {harness} pane before retrying — run Clear Session Context again if the context is still there"
        ),
        _ => format!(
            "Restore an idle {harness} prompt or restart the session, then run Clear Session Context again"
        ),
    };
    format!(
        "session_clear {harness} command `{command}` for {} was not proven submitted in pane {pane} after {phase} (result={}, command_visible={}); treating Clear Session Context as not submitted. ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action={} unblocker={}. {remedy}",
        file,
        observation.status.as_str(),
        observation.command_visible,
        observation.status.next_action(),
        observation.status.unblocker()
    )
}

/// User-facing message when a non-interrupting clear pauses an active auto-loop
/// and defers (`#autoloop-command-preemption` Phase 2). The loop is paused via
/// the clear cooldown so the pane reaches an idle gap; the operator can re-run
/// `session clear` there, and the destructive path stays explicit.
pub fn busy_clear_deferred_message(file: &Path, pane_id: Option<&str>) -> String {
    let pane = pane_id.unwrap_or("unknown");
    format!(
        "session_clear deferred for {} — pane {} is alive-busy under an active `agent:queue auto` loop, so the clear cannot run mid-turn without discarding in-flight work. Queued one clear for automatic delivery at the next idle prompt; the loop resumes after the clear settles. Run `agent-doc session interrupt-clear {}` to interrupt the turn and clear now.",
        file.display(),
        pane,
        file.display()
    )
}

pub fn busy_clear_already_deferred_message(file: &Path, pane_id: Option<&str>) -> String {
    let pane = pane_id.unwrap_or("unknown");
    format!(
        "session_clear already deferred for {} — pane {} still has a queued clear waiting for the next idle prompt. Not sending another clear into the active turn. Run `agent-doc session interrupt-clear {}` to interrupt the turn and clear now.",
        file.display(),
        pane,
        file.display()
    )
}

pub fn protected_clear_refusal_message(
    file: &Path,
    pane_id: Option<&str>,
    source: &str,
    current_command: Option<&str>,
    tail: Option<&str>,
    reason: &str,
) -> String {
    let pane = pane_id.unwrap_or("unknown");
    let command = current_command.unwrap_or("unknown");
    let tail = tail.unwrap_or("unknown");
    format!(
        "session_clear refused for {} because pane {} contains protected prompt input (reason={}, source={}, current_command={}, tail={:?}). Clear the prompt input manually, or run `agent-doc session interrupt-clear {}` to intentionally interrupt the pane and clear context.",
        file.display(),
        pane,
        reason,
        source,
        command,
        tail,
        file.display()
    )
}

pub fn busy_clear_refusal_message(
    file: &Path,
    pane_id: Option<&str>,
    source: &str,
    current_command: Option<&str>,
    tail: Option<&str>,
    reason: &str,
) -> String {
    let pane = pane_id.unwrap_or("unknown");
    let command = current_command.unwrap_or("unknown");
    let tail = tail.unwrap_or("unknown");
    format!(
        "session_clear refused for {} because pane {} is alive-busy (reason={}, source={}, current_command={}, tail={:?}). Wait for an idle prompt, or run `agent-doc session interrupt-clear {}` to intentionally interrupt the pane and clear context.",
        file.display(),
        pane,
        reason,
        source,
        command,
        tail,
        file.display()
    )
}

#[derive(Clone, Copy, Debug)]
pub struct InterruptClearTimeoutFacts<'a> {
    pub file: &'a Path,
    pub pane: &'a str,
    pub state: &'a str,
    pub source: &'a str,
    pub current_command: Option<&'a str>,
    pub prompt_ready: Option<bool>,
    pub tail: Option<&'a str>,
    pub editor_recovery_attempted: bool,
}

pub fn interrupt_clear_timeout_message(facts: InterruptClearTimeoutFacts<'_>) -> String {
    let command = facts.current_command.unwrap_or("unknown");
    let tail = facts.tail.unwrap_or("unknown");
    let prompt_ready = facts
        .prompt_ready
        .map(|ready| ready.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    if facts.editor_recovery_attempted {
        return format!(
            "session_interrupt_clear timed out for {} because pane {} stayed {} after interrupt and forced editor recovery (source={}, current_command={}, prompt_ready={}, tail={:?}). Inspect the pane, exit any editor prompt with `:qa!`, then run `agent-doc session status {}` before retrying.",
            facts.file.display(),
            facts.pane,
            facts.state,
            facts.source,
            command,
            prompt_ready,
            tail,
            facts.file.display()
        );
    }
    format!(
        "session_interrupt_clear timed out for {} because pane {} stayed {} after interrupt (source={}, current_command={}, prompt_ready={}, tail={:?}). Run `agent-doc session status {}` before retrying.",
        facts.file.display(),
        facts.pane,
        facts.state,
        facts.source,
        command,
        prompt_ready,
        tail,
        facts.file.display()
    )
}

pub fn terminal_editor_command(command: &str) -> bool {
    let name = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
        .trim();
    matches!(
        name,
        "vi" | "view" | "vim" | "vim.basic" | "vimdiff" | "nvim" | "nvimdiff"
    )
}

/// Ordered interrupt keys for an operator interrupt-clear / force-restart on a
/// live pane. `codex_shell_search` is true only when the Codex pane is in a
/// shell `reverse-i-search` / history-search state — the one place `C-g` is
/// safe and useful (it aborts the search). In the normal Codex TUI composer
/// `C-g` opens the external editor (`$EDITOR`, e.g. nvim), so it must be omitted
/// there and the interrupt falls through to `Escape` + `C-c`
/// (#codex-interrupt-clear-ctrl-g-opens-editor).
pub fn operator_interrupt_key_plan(harness: &str, codex_shell_search: bool) -> Vec<&'static str> {
    match harness {
        "opencode" => vec!["Escape", "Escape"],
        "codex" if codex_shell_search => vec!["C-g", "Escape", "C-c"],
        "codex" => vec!["Escape", "C-c"],
        _ => vec!["C-c"],
    }
}

pub fn operator_interrupt_step_delay(harness: &str) -> Duration {
    match harness {
        "opencode" => Duration::from_millis(200),
        _ => Duration::from_millis(100),
    }
}

fn line_shows_context_clear_command_input(line: &str, command: &str) -> bool {
    let trimmed = line.trim();
    context_clear_command_candidate_visible(trimmed, command)
        || context_clear_command_candidate_visible(
            strip_context_clear_prompt_prefix(trimmed).trim(),
            command,
        )
}

fn context_clear_command_candidate_visible(candidate: &str, command: &str) -> bool {
    if candidate == command {
        return true;
    }
    if command == "/new" && matches!(candidate, "New session" | "session_new") {
        return true;
    }
    command == "/new"
        && candidate
            .strip_prefix("/new")
            .map(|rest| {
                let label = rest.trim_start();
                label.starts_with("New session") || label.starts_with("session_new")
            })
            .unwrap_or(false)
}

fn line_starts_with_context_clear_prompt_prefix(line: &str) -> bool {
    matches!(line.trim_start().chars().next(), Some('>' | '›' | '❯'))
}

fn strip_context_clear_prompt_prefix(line: &str) -> &str {
    line.trim_start_matches(|ch: char| matches!(ch, '>' | '›' | '❯') || ch.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_dispatch_ready_prompt_line(line: &str) -> bool {
        matches!(line.trim(), ">" | "›" | "❯")
    }

    #[test]
    fn a_pane_that_visibly_moved_since_delivery_is_never_resent_the_full_command() {
        // `#cleardoublesend`. Live 2026-09-11, `src/haiven-dev/tasks/backend.md`
        // pane `%12`: the supervisor-IPC clear reported `submission_unobserved`,
        // the repair retyped `/clear` + Enter, and the pane capture grew
        // `1157 -> 1183` bytes across that resend — so the pane had been
        // accepting input the whole time and the operator got TWO clears before
        // the command still reported `blocked`.
        //
        // `Unobserved` means "the window closed with no evidence either way",
        // which is not the same claim as "the delivery was lost". Only a pane
        // that never moved can have lost it.
        let moved = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::Unobserved,
            elapsed: Duration::from_millis(918),
            command_visible: false,
            content_changed_since_delivery: true,
        };
        let never_moved = ContextClearSubmitObservation {
            content_changed_since_delivery: false,
            ..moved
        };
        let facts = |observation| ContextClearSubmitRetryFacts {
            observation,
            pending_draft_enter_resubmit: false,
            attempts_sent: 0,
            max_attempts: 1,
        };

        assert_eq!(
            context_clear_submit_retry_action(facts(moved)),
            None,
            "input visibly reached the pane, so resending the command text is a second submit"
        );
        assert_eq!(
            context_clear_submit_retry_action(facts(never_moved)),
            Some(ContextClearSubmitRetryAction::ResendCommand),
            "a pane that never moved is the one state a lost delivery produces, \
             and it must still be repaired"
        );
    }

    #[test]
    fn a_visible_stuck_draft_still_earns_a_submit_key_even_after_the_pane_moved() {
        // The guard above must narrow only the full-command resend. A command
        // sitting visibly in a split-submit composer is proven un-submitted, so
        // pressing the harness submit key duplicates nothing and must keep
        // working regardless of what else repainted.
        let facts = ContextClearSubmitRetryFacts {
            observation: ContextClearSubmitObservation {
                status: ContextClearSubmitStatus::StillVisible,
                elapsed: Duration::from_millis(900),
                command_visible: true,
                content_changed_since_delivery: true,
            },
            pending_draft_enter_resubmit: true,
            attempts_sent: 0,
            max_attempts: 1,
        };
        assert_eq!(
            context_clear_submit_retry_action(facts),
            Some(ContextClearSubmitRetryAction::SubmitKey)
        );
    }

    #[test]
    fn blank_repaint_waits_for_ready_clear_receipt() {
        let ordinary = Duration::from_millis(900);
        let rendering = Duration::from_secs(10);
        let mut state = ContextClearSubmitPollState::default();
        assert_eq!(
            context_clear_observation_budget("\n", ordinary, rendering),
            rendering
        );
        assert_eq!(
            context_clear_submit_frame_status(&mut state, false, true, false),
            None
        );
        assert!(!state.saw_submission_evidence());
        assert_eq!(
            context_clear_submit_frame_status(&mut state, false, true, true),
            Some(ContextClearSubmitStatus::Accepted)
        );
        assert_eq!(
            context_clear_observation_budget("› /clear", ordinary, rendering),
            ordinary
        );
    }

    #[test]
    fn unrendered_clear_never_retries_or_reports_success() {
        let observation = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::Unrendered,
            elapsed: Duration::from_secs(10),
            command_visible: false,
            content_changed_since_delivery: false,
        };
        assert!(!observation.status.is_accepted());
        assert_eq!(
            context_clear_submit_retry_action(ContextClearSubmitRetryFacts {
                observation,
                pending_draft_enter_resubmit: true,
                attempts_sent: 0,
                max_attempts: 4,
            }),
            None
        );
        assert!(
            context_clear_submit_blocked_message(
                "session.md",
                "%1",
                "codex",
                "/clear",
                "test",
                observation
            )
            .contains("No duplicate clear was sent")
        );
    }

    #[test]
    fn visible_command_then_blank_is_not_submission_proof() {
        let mut state = ContextClearSubmitPollState::default();
        assert_eq!(
            context_clear_submit_frame_status(&mut state, true, true, true),
            None
        );
        assert_eq!(
            context_clear_submit_frame_status(&mut state, false, true, false),
            None
        );
        assert_eq!(
            context_clear_submit_frame_status(&mut state, false, false, true),
            Some(ContextClearSubmitStatus::Accepted)
        );
    }

    #[test]
    fn context_clear_command_visible_detects_codex_active_composer() {
        let content = concat!(
            "older output\n",
            "› /clear\n",
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used\n",
        );

        assert!(context_clear_command_visible_in_active_input(
            content,
            "/clear",
            is_dispatch_ready_prompt_line,
        ));
    }

    #[test]
    fn context_clear_command_visible_treats_empty_composer_as_submitted() {
        let content = concat!(
            "older output\n",
            "› Ask Codex to do anything\n",
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used\n",
        );

        assert!(!context_clear_command_visible_in_active_input(
            content,
            "/clear",
            is_dispatch_ready_prompt_line,
        ));
    }

    #[test]
    fn context_clear_command_visible_detects_opencode_new_palette_row() {
        let content = concat!(
            "older output\n",
            "/new        New session\n",
            "/models     Select model\n",
            "> /new\n",
        );

        assert!(context_clear_command_visible_in_active_input(
            content,
            "/new",
            is_dispatch_ready_prompt_line,
        ));
    }

    #[test]
    fn context_clear_command_visible_detects_opencode_selected_new_session_command() {
        let content = concat!(
            "older output\n",
            "> New session\n",
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n",
        );

        assert!(
            context_clear_command_visible_in_active_input(
                content,
                "/new",
                is_dispatch_ready_prompt_line,
            ),
            "OpenCode can replace `/new` with the selected command label before the final submit Enter"
        );

        let structured = concat!(
            "older output\n",
            "> session_new\n",
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n",
        );
        assert!(
            context_clear_command_visible_in_active_input(
                structured,
                "/new",
                is_dispatch_ready_prompt_line,
            ),
            "OpenCode can also surface the selected command id before submission"
        );
    }

    /// `#clearqueuedcomposer`: a Claude Code pane mid-turn QUEUES operator input
    /// instead of executing it, and its composer says so.
    ///
    /// Observed live 2026-09-11 on `src/haiven-dev/tasks/sdk.md`, pane `%1`: two
    /// subagents running, `❯ Press up to edit queued messages` in the composer,
    /// and a `/clear` plus its 2s-later repair resend both landing in that queue.
    /// The acceptance poll reported `submission_unobserved` — "whether the clear
    /// ran is unknown" — about a pane whose state was fully legible, sending the
    /// operator to check a pane the binary had already read.
    #[test]
    fn a_queued_composer_is_recognized_as_the_harness_queueing_input() {
        let is_queued = |line: &str| line.trim() == "\u{276f} Press up to edit queued messages";

        let busy_with_queued_clear = concat!(
            "  \u{276f} /agent-doc src/haiven-dev/tasks/sdk.md\n",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n",
            "\u{276f} Press up to edit queued messages\n",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n",
            "  Opus 5 (1M context) ctx:17% ~/src/haiven-sdk brian@host\n",
            "  \u{23f5}\u{23f5} bypass permissions on (shift+tab to cycle) \u{b7} \u{2190} 2 agents\n",
        );
        assert!(context_clear_capture_shows_queued_input(
            busy_with_queued_clear,
            is_queued
        ));

        // An idle composer is not queueing. Neither is Claude's OTHER empty-
        // composer placeholder, which renders on a fresh, idle pane.
        let idle = concat!(
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n",
            "\u{276f}\n",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n",
        );
        assert!(!context_clear_capture_shows_queued_input(idle, is_queued));
        let fresh = "\u{276f} describe a task for a new session\n";
        assert!(!context_clear_capture_shows_queued_input(fresh, is_queued));
    }

    /// `#clearqueuedcomposer`: a queued clear did NOT run, so the acceptance rule
    /// is unchanged — only the label and the unblocker are. Same contract
    /// `#clearsubmitlabel` set: never report a clear that may not have run as
    /// done, and never hand the operator a guess when the state is known.
    #[test]
    fn a_queued_clear_stays_unaccepted_but_names_its_real_unblocker() {
        let queued = ContextClearSubmitStatus::HarnessQueuedInput;

        assert!(!queued.is_accepted(), "a queued clear has not run");
        assert_eq!(queued.as_str(), "harness_queued_input");
        assert_eq!(queued.issue(), Some("clear_queued_during_busy_turn"));
        assert_eq!(queued.unblocker(), "clear_queued_behind_busy_turn");
        assert_eq!(queued.next_action(), "wait_for_turn_or_interrupt_then_retry");

        // The unknown it replaces sent the operator to inspect the pane; this one
        // names what actually unblocks it, and must not read as the same thing.
        assert_ne!(
            queued.unblocker(),
            ContextClearSubmitStatus::Unobserved.unblocker()
        );
        assert_ne!(
            queued.next_action(),
            ContextClearSubmitStatus::Unobserved.next_action()
        );
    }

    #[test]
    fn context_clear_command_visible_ignores_stale_scrollback_before_idle_prompt() {
        let content = concat!(
            "✶ Generating... (3s · esc to interrupt)\n",
            "  ❯ /clear\n",
            "────────────────────\n",
            "❯ Press up to edit queued messages\n",
            "────────────────────\n",
            "  Opus 4.8 ctx:10% ~/work/btakita/agent-loop main brian@host\n",
            "  bypass permissions on (shift+tab to cycle)\n",
        );

        assert!(!context_clear_command_visible_in_active_input(
            content,
            "/clear",
            is_dispatch_ready_prompt_line,
        ));
    }

    #[test]
    fn context_clear_submit_poll_requires_consumption_or_content_change() {
        let mut state = ContextClearSubmitPollState::default();
        assert_eq!(
            context_clear_submit_poll_status(&mut state, false, false),
            None,
            "an unchanged empty composer is not submit proof"
        );
        assert!(!state.saw_submission_evidence());

        let mut consumed = ContextClearSubmitPollState::default();
        assert_eq!(
            context_clear_submit_poll_status(&mut consumed, true, false),
            None
        );
        assert_eq!(
            context_clear_submit_poll_status(&mut consumed, false, false),
            Some(ContextClearSubmitStatus::Accepted),
            "a visible clear command disappearing proves consumption"
        );
        assert!(consumed.saw_submission_evidence());

        let mut changed = ContextClearSubmitPollState::default();
        assert_eq!(
            context_clear_submit_poll_status(&mut changed, false, true),
            Some(ContextClearSubmitStatus::Accepted),
            "fast clear can be proven by post-delivery pane content change"
        );
        assert!(changed.saw_submission_evidence());
    }

    #[test]
    fn context_clear_submit_retry_is_scoped_to_visible_enter_profile_drafts() {
        let visible_timeout = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::StillVisible,
            elapsed: Duration::from_millis(250),
            command_visible: true,
            content_changed_since_delivery: false,
        };
        let accepted = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::Accepted,
            elapsed: Duration::from_millis(20),
            command_visible: false,
            content_changed_since_delivery: false,
        };
        let stale_or_empty_timeout = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::Unobserved,
            elapsed: Duration::from_millis(250),
            command_visible: false,
            content_changed_since_delivery: false,
        };

        assert!(context_clear_submit_needs_enter_resubmit(
            &visible_timeout,
            true,
        ));
        assert!(!context_clear_submit_needs_enter_resubmit(
            &visible_timeout,
            false,
        ));
        assert!(!context_clear_submit_needs_enter_resubmit(&accepted, true));
        assert!(!context_clear_submit_needs_enter_resubmit(
            &stale_or_empty_timeout,
            true,
        ));
        assert!(context_clear_submit_can_enter_resubmit(
            ContextClearSubmitRetryFacts {
                observation: visible_timeout,
                pending_draft_enter_resubmit: true,
                attempts_sent: 0,
                max_attempts: 2,
            }
        ));
        assert!(!context_clear_submit_can_enter_resubmit(
            ContextClearSubmitRetryFacts {
                observation: visible_timeout,
                pending_draft_enter_resubmit: true,
                attempts_sent: 2,
                max_attempts: 2,
            }
        ));
        assert_eq!(
            context_clear_submit_retry_action(ContextClearSubmitRetryFacts {
                observation: stale_or_empty_timeout,
                pending_draft_enter_resubmit: true,
                attempts_sent: 0,
                max_attempts: 2,
            }),
            Some(ContextClearSubmitRetryAction::ResendCommand),
            "an unobserved clear with retained history repairs the full idempotent command"
        );
        assert_eq!(
            context_clear_submit_retry_action(ContextClearSubmitRetryFacts {
                observation: ContextClearSubmitObservation {
                    status: ContextClearSubmitStatus::CaptureFailed,
                    elapsed: Duration::from_millis(250),
                    command_visible: false,
                    content_changed_since_delivery: false,
                },
                pending_draft_enter_resubmit: true,
                attempts_sent: 0,
                max_attempts: 2,
            }),
            None,
            "capture failure cannot prove that a full-command resend is safe"
        );
        assert_eq!(
            context_clear_submit_retry_action(ContextClearSubmitRetryFacts {
                observation: stale_or_empty_timeout,
                pending_draft_enter_resubmit: true,
                attempts_sent: 2,
                max_attempts: 2,
            }),
            None,
            "full-command repair shares the bounded retry budget"
        );
    }

    /// `#clearsubmitunobserved`: the reported failure. A cold-started pane has
    /// nothing to clear, so a successful `/clear` changes nothing and the poll
    /// loop reports `Unobserved` for a clear that did exactly what was asked.
    #[test]
    fn cleared_pane_history_proves_the_cleared_state_without_an_observed_transition() {
        let is_ready = |line: &str| line.starts_with('>');
        let fresh_pane = "\
Welcome to Claude Code

  /help for help

> \
";
        assert!(
            context_clear_history_proves_cleared_state(fresh_pane, "/clear", 200, is_ready),
            "a pane holding no conversation is already in the cleared state"
        );
    }

    /// The half that must never regress: a clear that was genuinely lost leaves
    /// the conversation in scrollback, so the pane stays blocked.
    #[test]
    fn retained_conversation_history_keeps_an_unobserved_clear_blocked() {
        let is_ready = |line: &str| line.starts_with('>');
        let mut busy_pane = String::from("Welcome to Claude Code\n");
        for turn in 0..300 {
            busy_pane.push_str(&format!("> question {turn}\nassistant answer {turn}\n"));
        }
        busy_pane.push_str("> ");
        assert!(
            !context_clear_history_proves_cleared_state(&busy_pane, "/clear", 200, is_ready),
            "retained conversation must never read as cleared"
        );
    }

    /// Both ambiguous shapes stay closed: an unconsumed command in the composer
    /// is a draft regardless of scrollback, and a capture with no settled prompt
    /// is not a settled pane.
    #[test]
    fn ambiguous_pane_shapes_never_prove_the_cleared_state() {
        let is_ready = |line: &str| line.trim() == ">";
        let unconsumed = "Welcome to Claude Code\n> /clear";
        assert!(
            !context_clear_history_proves_cleared_state(unconsumed, "/clear", 200, is_ready),
            "a composer still holding the command is an unconsumed draft"
        );
        let no_prompt = "Welcome to Claude Code\nstarting up...";
        assert!(
            !context_clear_history_proves_cleared_state(no_prompt, "/clear", 200, is_ready),
            "a pane with no dispatch-ready prompt is not settled"
        );
    }

    /// The state-proven variant must satisfy the gate. Callers compare through
    /// `is_accepted()`; a caller that wrote `== Accepted` would silently keep
    /// failing closed.
    #[test]
    fn cleared_state_acceptance_reports_as_accepted_with_no_issue() {
        assert!(ContextClearSubmitStatus::AcceptedClearedState.is_accepted());
        assert!(ContextClearSubmitStatus::Accepted.is_accepted());
        assert!(!ContextClearSubmitStatus::Unobserved.is_accepted());
        assert!(!ContextClearSubmitStatus::StillVisible.is_accepted());
        assert!(!ContextClearSubmitStatus::CaptureFailed.is_accepted());
        assert_eq!(
            ContextClearSubmitStatus::AcceptedClearedState.as_str(),
            "accepted_cleared_state"
        );
        assert_eq!(ContextClearSubmitStatus::AcceptedClearedState.issue(), None);
        assert_eq!(
            ContextClearSubmitStatus::AcceptedClearedState.unblocker(),
            "none"
        );
    }

    #[test]
    fn context_clear_submit_proof_lines_report_prompt_issue_and_retry_outcome() {
        let observation = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::StillVisible,
            elapsed: Duration::from_millis(5123),
            command_visible: true,
            content_changed_since_delivery: false,
        };
        let issue = context_clear_submit_observation_line(
            "/tmp/doc.md",
            "%7",
            "codex",
            "direct_pane_acceptance",
            observation,
            Some(2048),
            Some("abc123"),
        );
        assert!(
            issue.contains("session_clear_submit_observation"),
            "{issue}"
        );
        assert!(issue.contains("issue=prompt_not_submitted"), "{issue}");
        assert!(issue.contains("command_visible=true"), "{issue}");

        let retry = context_clear_submit_resubmit_proof_line(
            "/tmp/doc.md",
            "%7",
            "codex",
            ContextClearSubmitRetryProofFacts {
                action: ContextClearSubmitRetryAction::SubmitKey,
                submit_key: "Enter",
                attempt: 2,
                max_attempts: 30,
                observation: ContextClearSubmitObservation {
                    status: ContextClearSubmitStatus::Accepted,
                    elapsed: Duration::from_millis(150),
                    command_visible: false,
                    content_changed_since_delivery: false,
                },
            },
        );
        assert!(retry.contains("session_clear_submit_resubmit"), "{retry}");
        assert!(retry.contains("action=submit_key key=Enter"), "{retry}");
        assert!(retry.contains("attempt=2"), "{retry}");
        assert!(retry.contains("max_attempts=30"), "{retry}");
        assert!(retry.contains("result=accepted"), "{retry}");

        let repair = context_clear_submit_resubmit_proof_line(
            "/tmp/doc.md",
            "%7",
            "codex",
            ContextClearSubmitRetryProofFacts {
                action: ContextClearSubmitRetryAction::ResendCommand,
                submit_key: "Enter",
                attempt: 1,
                max_attempts: 1,
                observation: ContextClearSubmitObservation {
                    status: ContextClearSubmitStatus::Accepted,
                    elapsed: Duration::from_millis(150),
                    command_visible: false,
                    content_changed_since_delivery: false,
                },
            },
        );
        assert!(repair.contains("action=resend_command"), "{repair}");
    }

    #[test]
    fn context_clear_submit_blocked_lines_name_command_and_unblocker() {
        let observation = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::StillVisible,
            elapsed: Duration::from_millis(2001),
            command_visible: true,
            content_changed_since_delivery: false,
        };
        let line = context_clear_submit_blocked_line(
            "/tmp/doc.md",
            "%12",
            "opencode",
            "/new",
            "direct_pane_resubmit_acceptance",
            observation,
        );
        assert!(line.contains("session_clear_submit_blocked"), "{line}");
        assert!(line.contains("command=/new"), "{line}");
        assert!(
            line.contains("ui_outcome=blocked_with_exact_unblocker"),
            "{line}"
        );
        assert!(
            line.contains("unblocker=clear_command_not_consumed"),
            "{line}"
        );

        let message = context_clear_submit_blocked_message(
            "/tmp/doc.md",
            "%12",
            "opencode",
            "/new",
            "direct_pane_resubmit_acceptance",
            observation,
        );
        assert!(message.contains("command `/new`"), "{message}");
        assert!(
            message.contains("treating Clear Session Context as not submitted"),
            "{message}"
        );
        assert!(
            message.contains("ui_outcome=blocked_with_exact_unblocker"),
            "{message}"
        );
    }

    /// `#clearsubmitlabel`: the operator's report was
    /// `result=command_still_visible` on a line that also carried
    /// `command_visible=false` — the label contradicted the observation, so it
    /// could not be used to tell a wedged composer from an unproven submit.
    #[test]
    fn unobserved_clear_submit_is_not_reported_as_still_visible() {
        let still_visible = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::StillVisible,
            elapsed: Duration::from_millis(2000),
            command_visible: true,
            content_changed_since_delivery: false,
        };
        let unobserved = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::Unobserved,
            elapsed: Duration::from_millis(2000),
            command_visible: false,
            content_changed_since_delivery: false,
        };

        // The contradiction itself: an unobserved submit must never claim the
        // command is still visible.
        assert_eq!(
            ContextClearSubmitStatus::Unobserved.as_str(),
            "submission_unobserved"
        );
        assert_ne!(
            ContextClearSubmitStatus::Unobserved.as_str(),
            ContextClearSubmitStatus::StillVisible.as_str(),
            "the two deadlines must be distinguishable in ops.log"
        );

        for (observation, expect_result, expect_issue, expect_unblocker) in [
            (
                still_visible,
                "command_still_visible",
                "prompt_not_submitted",
                "clear_command_not_consumed",
            ),
            (
                unobserved,
                "submission_unobserved",
                "submit_unobserved",
                "clear_submission_unobserved",
            ),
        ] {
            let line = context_clear_submit_blocked_line(
                "/tmp/doc.md",
                "%12",
                "claude",
                "/clear",
                "direct_pane_acceptance",
                observation,
            );
            assert!(line.contains(&format!("result={expect_result}")), "{line}");
            assert!(line.contains(&format!("issue={expect_issue}")), "{line}");
            assert!(
                line.contains(&format!("unblocker={expect_unblocker}")),
                "{line}"
            );
            assert!(
                line.contains(&format!("command_visible={}", observation.command_visible)),
                "{line}"
            );
        }

        // The remedy sentence must match the observation too: telling an operator
        // to "restore an idle prompt" is a guess when nothing was observed.
        let unobserved_message = context_clear_submit_blocked_message(
            "/tmp/doc.md",
            "%12",
            "claude",
            "/clear",
            "direct_pane_acceptance",
            unobserved,
        );
        assert!(
            unobserved_message.contains("whether the clear ran is unknown"),
            "{unobserved_message}"
        );
        assert!(
            !unobserved_message.contains("Restore an idle"),
            "{unobserved_message}"
        );

        // ACCEPTANCE IS UNCHANGED: splitting the label must not turn an
        // unproven clear into a successful one.
        assert_ne!(unobserved.status, ContextClearSubmitStatus::Accepted);
        assert!(unobserved.status.issue().is_some());
        // Only the wedged-composer shape may drive an Enter resubmit; an
        // unobserved submit must not blind-resend into an unknown pane.
        assert!(context_clear_submit_needs_enter_resubmit(
            &still_visible,
            true
        ));
        assert!(!context_clear_submit_needs_enter_resubmit(
            &unobserved,
            true
        ));
    }

    #[test]
    fn protected_clear_refusal_points_to_interrupt_clear() {
        let message = protected_clear_refusal_message(
            Path::new("/tmp/doc.md"),
            Some("%7"),
            "authoritative_actor",
            Some("agent-doc"),
            Some("gpt-5.5 high · ~/work/btakita/agent-loop · Context 85% used"),
            "drafted prompt input",
        );

        assert!(message.contains("session_clear refused"));
        assert!(message.contains("pane %7 contains protected prompt input"));
        assert!(message.contains("reason=drafted prompt input"));
        assert!(message.contains("agent-doc session interrupt-clear /tmp/doc.md"));
    }

    #[test]
    fn busy_clear_refusal_points_to_interrupt_clear() {
        let message = busy_clear_refusal_message(
            Path::new("/tmp/doc.md"),
            Some("%7"),
            "authoritative_actor",
            Some("agent-doc"),
            Some("Working..."),
            "active codex turn",
        );
        assert!(message.contains("session_clear refused"));
        assert!(message.contains("pane %7 is alive-busy"));
        assert!(message.contains("reason=active codex turn"));
        assert!(message.contains("agent-doc session interrupt-clear /tmp/doc.md"));
    }

    #[test]
    fn busy_clear_deferred_message_names_automatic_single_delivery() {
        let message = busy_clear_deferred_message(Path::new("/tmp/doc.md"), Some("%7"));

        assert!(message.contains("session_clear deferred"));
        assert!(message.contains("Queued one clear for automatic delivery"));
        assert!(!message.contains("retry `agent-doc session clear"));
        assert!(message.contains("agent-doc session interrupt-clear /tmp/doc.md"));
    }

    #[test]
    fn busy_clear_already_deferred_message_refuses_duplicate_clear() {
        let message = busy_clear_already_deferred_message(Path::new("/tmp/doc.md"), Some("%7"));

        assert!(message.contains("session_clear already deferred"));
        assert!(message.contains("Not sending another clear into the active turn"));
        assert!(message.contains("agent-doc session interrupt-clear /tmp/doc.md"));
    }

    #[test]
    fn terminal_editor_command_detects_vim_family_processes() {
        for command in [
            "vi",
            "view",
            "vim",
            "vim.basic",
            "vimdiff",
            "nvim",
            "nvimdiff",
        ] {
            assert!(
                terminal_editor_command(command),
                "{command} should trigger interrupt-clear editor recovery"
            );
        }
        assert!(terminal_editor_command("/usr/bin/nvim"));
        assert!(!terminal_editor_command("codex"));
        assert!(!terminal_editor_command("agent-doc"));
        assert!(!terminal_editor_command("vim-addon-manager"));
    }

    #[test]
    fn operator_interrupt_key_plan_omits_ctrl_g_for_codex_composer() {
        // #codex-interrupt-clear-ctrl-g-opens-editor: C-g opens the external
        // editor (nvim) in the Codex composer, so the normal interrupt path must
        // not send it — Escape + C-c is the safe interrupt.
        assert_eq!(
            operator_interrupt_key_plan("codex", false),
            vec!["Escape", "C-c"]
        );
        assert!(!operator_interrupt_key_plan("codex", false).contains(&"C-g"));
    }

    #[test]
    fn operator_interrupt_key_plan_sends_ctrl_g_only_for_codex_shell_search() {
        // C-g is safe (aborts the search) only when the Codex pane is in a shell
        // reverse-i-search / history-search state.
        assert_eq!(
            operator_interrupt_key_plan("codex", true),
            vec!["C-g", "Escape", "C-c"]
        );
    }

    #[test]
    fn operator_interrupt_key_plan_unchanged_for_other_harnesses() {
        // The codex_shell_search flag is codex-scoped and must not perturb other
        // harnesses' interrupt sequences.
        assert_eq!(
            operator_interrupt_key_plan("opencode", false),
            vec!["Escape", "Escape"]
        );
        assert_eq!(
            operator_interrupt_key_plan("opencode", true),
            vec!["Escape", "Escape"]
        );
        assert_eq!(operator_interrupt_key_plan("claude", false), vec!["C-c"]);
        assert_eq!(operator_interrupt_key_plan("claude", true), vec!["C-c"]);
    }

    #[test]
    fn operator_interrupt_step_delay_uses_longer_opencode_gap() {
        assert_eq!(
            operator_interrupt_step_delay("opencode"),
            Duration::from_millis(200)
        );
        assert_eq!(
            operator_interrupt_step_delay("codex"),
            Duration::from_millis(100)
        );
    }

    #[test]
    fn interrupt_clear_timeout_message_reports_editor_recovery() {
        let message = interrupt_clear_timeout_message(InterruptClearTimeoutFacts {
            file: Path::new("/tmp/doc.md"),
            pane: "%7",
            state: "alive-busy",
            source: "authoritative_actor",
            current_command: Some("vim"),
            prompt_ready: Some(false),
            tail: Some("-- INSERT --"),
            editor_recovery_attempted: true,
        });

        assert!(message.contains("forced editor recovery"));
        assert!(message.contains("stayed alive-busy"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=vim"));
        assert!(message.contains("prompt_ready=false"));
        assert!(message.contains("tail=\"-- INSERT --\""));
        assert!(message.contains(":qa!"));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
    }

    #[test]
    fn interrupt_clear_timeout_message_reports_last_command_without_editor_recovery() {
        let message = interrupt_clear_timeout_message(InterruptClearTimeoutFacts {
            file: Path::new("/tmp/doc.md"),
            pane: "%7",
            state: "alive-busy",
            source: "authoritative_actor",
            current_command: Some("codex"),
            prompt_ready: Some(false),
            tail: Some("⏵⏵ bypass permissions on"),
            editor_recovery_attempted: false,
        });

        assert!(!message.contains("forced editor recovery"));
        assert!(message.contains("stayed alive-busy"));
        assert!(message.contains("source=authoritative_actor"));
        assert!(message.contains("current_command=codex"));
        assert!(message.contains("prompt_ready=false"));
        assert!(message.contains("tail=\"⏵⏵ bypass permissions on\""));
        assert!(message.contains("agent-doc session status /tmp/doc.md"));
    }
}

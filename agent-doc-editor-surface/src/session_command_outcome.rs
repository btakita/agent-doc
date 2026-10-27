//! What a Clear Session Context or Restart session command's output means for
//! the operator, behind the hot-reloaded native boundary (`#jbsessioncmdoutcome`,
//! the GH #326 lever, the same move `route_outcome` made).
//!
//! The JetBrains plugin runs `agent-doc session clear` and
//! `agent-doc session restart-{agent,supervisor} [--force]` and gets back the
//! command's output text and whether it exited zero. Reading that text was a
//! Kotlin rule set in `TerminalUtil`: whether a clear was queued behind a live
//! turn (or a settling prompt, or an accepted run), deferred under a busy
//! auto-loop, or refused because the pane is busy, holds protected prompt
//! input, or runs a turn no supervisor owns; whether a restart was refused
//! because a terminal editor holds the pane, the pane is busy, or the actor is
//! still starting; the details each of those carries; and the notification
//! text for each. The wording is the binary's, so every wording change used to
//! need a plugin generation as well, and on the exact-263 modular package that
//! is an IDE restart.
//!
//! Those rules live here now, as the `session_command_outcome` derivation. The
//! plugin sends the command, whether it succeeded, whether it was forced, the
//! output, and the document's relative path, and executes the answer: the
//! notification type, its actions, and the restart telemetry read stay in
//! Kotlin. A new outcome *kind* still needs a plugin generation (it has no
//! effect there); a new wording or a new output shape for an existing kind does
//! not.
//!
//! This is a byte-for-byte port of the pre-move Kotlin rules: `java.util.regex`
//! ASCII `\s`, `\S`, `\d`, and `\b`, Kotlin `trim`/`isBlank` whitespace, and
//! Kotlin's `substringBefore`, `removeSuffix`, and `removeSurrounding`. The
//! plugin keeps a frozen Kotlin copy for a library that cannot answer. One
//! parity fixture
//! (`editors/jetbrains/src/test/resources/editor-derivations/session_command_outcome.json`)
//! is answered by both. New rules land here, never there.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::route_outcome::{java_regex, kotlin_is_blank, kotlin_trim};

/// What one finished session command reports. Every field is optional on the
/// wire: after a hot reload this library is newer than the plugin generation
/// calling it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCommandOutcomeEvidence {
    /// `clear` or `restart` (agent or supervisor; the wording is shared).
    #[serde(default)]
    pub command: Option<String>,
    /// Whether the command exited zero.
    #[serde(default)]
    pub succeeded: Option<bool>,
    /// Whether a restart was `--force` (Interrupt and restart).
    #[serde(default)]
    pub force: Option<bool>,
    /// The command's combined output, trimmed, exactly as the plugin read it.
    #[serde(default)]
    pub output: Option<String>,
    /// The document's project-relative path, for the notification text.
    #[serde(default)]
    pub relative_path: Option<String>,
}

/// The kind of outcome a session command output reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionCommandOutcomeKind {
    /// No shape this derivation reads: the adapter's generic handling (the
    /// command-failure notice, or a restart's telemetry-based success notice).
    Generic,
    /// The clear ran now.
    Cleared,
    /// The clear was queued behind a live turn, a settling prompt, or an
    /// accepted Run Agent Doc dispatch.
    ClearQueued,
    /// The clear was deferred under a busy queue auto-loop.
    ClearDeferred,
    /// The clear was refused (busy pane, protected prompt input, or a live turn
    /// no supervisor owns).
    ClearRefused,
    /// The restart was refused because a terminal editor holds the pane.
    RestartRefusedEditorHoldsPane,
    /// The restart was refused because the pane is busy.
    RestartRefusedBusy,
    /// The restart was refused because the authoritative actor is still starting.
    RestartRefusedStarting,
}

/// A clear queued behind a turn, a settling prompt, or an accepted run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionClearQueued {
    pub file: String,
    pub pane: String,
    pub evidence: String,
    pub evidence_age_secs: Option<i64>,
    pub rearmed: bool,
    pub settles_in_secs: Option<i64>,
    pub pre_turn_grace_secs: Option<i64>,
}

/// A refused clear.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionClearRefused {
    pub file: String,
    pub pane: String,
    pub source: String,
    pub current_command: String,
    pub tail: String,
    pub protected_reason: String,
    pub live_turn_evidence: String,
}

/// A restart refused because the pane is busy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRestartRefusedBusy {
    pub file: String,
    pub pane: String,
    pub source: String,
    pub current_command: String,
    pub tail: String,
}

/// A restart refused because the authoritative actor is still starting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRestartRefusedStarting {
    pub file: String,
    pub reason: String,
}

/// A restart refused because a terminal editor holds the pane.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRestartRefusedEditorHoldsPane {
    pub file: String,
    pub pane: String,
    pub editor: String,
    pub source: String,
    pub current_command: String,
    pub tail: String,
}

/// The `session_command_outcome` answer. Exactly the detail field its kind
/// names is set; every other detail is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCommandOutcome {
    pub kind: SessionCommandOutcomeKind,
    /// The notification (or hint) text for this kind, or `None` for `generic`.
    pub summary: Option<String>,
    pub clear_queued: Option<SessionClearQueued>,
    pub clear_refused: Option<SessionClearRefused>,
    pub restart_refused_busy: Option<SessionRestartRefusedBusy>,
    pub restart_refused_starting: Option<SessionRestartRefusedStarting>,
    pub restart_refused_editor_holds_pane: Option<SessionRestartRefusedEditorHoldsPane>,
}

const PENDING_RUN_CLEAR_EVIDENCE: &str = "dispatch_turn_pending";
const DEFERRED_QUEUE_PREEMPT_CLEAR_MARKER: &str = "session_clear deferred for";

static BUSY_CLEAR_REFUSAL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    java_regex(
        r"(?s)session_clear refused for (.+?) because pane (\S+) is (?:alive-busy|active_agent_doc|busy)",
    )
});
// #clearqueuedliveturn: a harness turn is proven live and no supervisor owns
// the pane to queue the clear behind it.
static LIVE_TURN_CLEAR_REFUSAL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    java_regex(
        r"(?s)session_clear refused for (.+?) because a turn is live in pane (\S+) \(harness_turn_live=true evidence=([^,\s)]+)",
    )
});
// #clearctxslow / #clearaftrun: queued behind a live turn, a prompt that has
// not started a turn, or a Run Agent Doc dispatch that has not started its turn.
static LIVE_TURN_CLEAR_QUEUED: LazyLock<Regex> = LazyLock::new(|| {
    java_regex(
        r"(?s)session_clear queued for (.+?) — (?:(?:a turn is live|the last prompt) in|Run Agent Doc for) pane (\S+)",
    )
});
static LIVE_TURN_CLEAR_QUEUED_SETTLES_IN: LazyLock<Regex> =
    LazyLock::new(|| java_regex(r"\bsettles_in_secs=(\d+)"));
static LIVE_TURN_CLEAR_QUEUED_PRE_TURN_GRACE: LazyLock<Regex> =
    LazyLock::new(|| java_regex(r"\bpre_turn_grace_secs=(\d+)"));
static LIVE_TURN_CLEAR_QUEUED_EVIDENCE: LazyLock<Regex> =
    LazyLock::new(|| java_regex(r"\bevidence=([^,\s)]+)"));
static LIVE_TURN_CLEAR_QUEUED_AGE: LazyLock<Regex> =
    LazyLock::new(|| java_regex(r"\bevidence_age_secs=(\d+)"));
static LIVE_TURN_CLEAR_QUEUED_REARMED: LazyLock<Regex> =
    LazyLock::new(|| java_regex(r"\brearmed=(true|false)\b"));
static PROTECTED_CLEAR_REFUSAL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    java_regex(
        r"(?s)session_clear refused for (.+?) because pane (\S+) contains protected prompt input",
    )
});
static BUSY_RESTART_REFUSAL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    java_regex(r"(?s)session_restart refused for (.+?) because pane (\S+) is alive-busy")
});
static STARTING_RESTART_REFUSAL: LazyLock<Regex> = LazyLock::new(|| {
    java_regex(
        r"(?s)session_restart refused for (.+?) because the authoritative actor is still starting and (.+?)\. Wait for a dispatch-ready prompt",
    )
});
// #hj7s: a terminal editor (e.g. Claude Code `ctrl+g` edit-in-nvim) owns the
// pane TTY, so restart is refused and the operator must close the editor.
static EDITOR_RESTART_REFUSAL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    java_regex(r"(?s)session_restart refused for (.+?) because pane (\S+) is held by editor (\S+)")
});
static PROTECTED_CLEAR_REASON: LazyLock<Regex> = LazyLock::new(|| java_regex(r"reason=([^,)]+)"));
static BUSY_CLEAR_SOURCE: LazyLock<Regex> = LazyLock::new(|| java_regex(r"source=([^,)]+)"));
static BUSY_CLEAR_COMMAND: LazyLock<Regex> =
    LazyLock::new(|| java_regex(r"current_command=([^,)]+)"));

/// Kotlin `ifBlank { fallback }`.
fn if_blank(value: String, fallback: &str) -> String {
    if kotlin_is_blank(&value) {
        fallback.to_string()
    } else {
        value
    }
}

/// Kotlin `substringBefore(delimiter)`: the whole string when it is absent.
fn substring_before<'a>(value: &'a str, delimiter: &str) -> &'a str {
    value.find(delimiter).map_or(value, |index| &value[..index])
}

/// Kotlin `removeSuffix(suffix)`.
fn remove_suffix<'a>(value: &'a str, suffix: &str) -> &'a str {
    value.strip_suffix(suffix).unwrap_or(value)
}

/// Kotlin `removeSurrounding(delimiter)`: only when the value is at least two
/// delimiters long and both starts and ends with it.
fn remove_surrounding<'a>(value: &'a str, delimiter: &str) -> &'a str {
    if value.len() >= delimiter.len() * 2
        && value.starts_with(delimiter)
        && value.ends_with(delimiter)
    {
        &value[delimiter.len()..value.len() - delimiter.len()]
    } else {
        value
    }
}

/// The first capture group of `regex`'s first match in `text`, or empty.
fn group_one(regex: &Regex, text: &str) -> String {
    regex
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
        .unwrap_or_default()
}

/// Kotlin `toLongOrNull()` of the first capture group.
fn group_one_long(regex: &Regex, text: &str) -> Option<i64> {
    regex
        .captures(text)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<i64>().ok())
}

fn extract_busy_clear_tail(detail: &str) -> String {
    let marker = "tail=";
    let Some(start) = detail.find(marker) else {
        return String::new();
    };
    let mut raw = &detail[start + marker.len()..];
    for delimiter in [
        "). Run `agent-doc session status",
        "). Run agent-doc session status",
        "). Clear the prompt input manually",
        "). Wait for an idle prompt",
    ] {
        raw = substring_before(raw, delimiter);
    }
    let raw = kotlin_trim(raw);
    let raw = remove_suffix(raw, ").");
    let raw = remove_suffix(raw, ")");
    let raw = kotlin_trim(raw);
    remove_surrounding(raw, "\"")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

fn is_deferred_queue_preempt_clear(output: &str) -> bool {
    output.contains(DEFERRED_QUEUE_PREEMPT_CLEAR_MARKER)
}

fn parse_clear_queued(output: &str) -> Option<SessionClearQueued> {
    let captures = LIVE_TURN_CLEAR_QUEUED.captures(output)?;
    let detail = &output[captures.get(0)?.end()..];
    Some(SessionClearQueued {
        file: captures[1].to_string(),
        pane: captures[2].to_string(),
        evidence: group_one(&LIVE_TURN_CLEAR_QUEUED_EVIDENCE, detail),
        evidence_age_secs: group_one_long(&LIVE_TURN_CLEAR_QUEUED_AGE, detail),
        rearmed: LIVE_TURN_CLEAR_QUEUED_REARMED
            .captures(detail)
            .and_then(|captures| captures.get(1))
            .is_some_and(|value| value.as_str() == "true"),
        settles_in_secs: group_one_long(&LIVE_TURN_CLEAR_QUEUED_SETTLES_IN, detail),
        pre_turn_grace_secs: group_one_long(&LIVE_TURN_CLEAR_QUEUED_PRE_TURN_GRACE, detail),
    })
}

fn parse_clear_refused(output: &str) -> Option<SessionClearRefused> {
    if let Some(captures) = LIVE_TURN_CLEAR_REFUSAL_HEADER.captures(output) {
        let detail = &output[captures.get(0)?.end()..];
        return Some(SessionClearRefused {
            file: captures[1].to_string(),
            pane: captures[2].to_string(),
            source: if_blank(group_one(&BUSY_CLEAR_SOURCE, detail), "unknown"),
            current_command: "unknown".to_string(),
            tail: "unknown".to_string(),
            protected_reason: String::new(),
            live_turn_evidence: captures[3].to_string(),
        });
    }
    if let Some(captures) = PROTECTED_CLEAR_REFUSAL_HEADER.captures(output) {
        let detail = &output[captures.get(0)?.end()..];
        return Some(SessionClearRefused {
            file: captures[1].to_string(),
            pane: captures[2].to_string(),
            source: if_blank(group_one(&BUSY_CLEAR_SOURCE, detail), "unknown"),
            current_command: if_blank(group_one(&BUSY_CLEAR_COMMAND, detail), "unknown"),
            tail: extract_busy_clear_tail(detail),
            protected_reason: if_blank(
                group_one(&PROTECTED_CLEAR_REASON, detail),
                "protected prompt input",
            ),
            live_turn_evidence: String::new(),
        });
    }
    let captures = BUSY_CLEAR_REFUSAL_HEADER.captures(output)?;
    let detail = &output[captures.get(0)?.end()..];
    Some(SessionClearRefused {
        file: captures[1].to_string(),
        pane: captures[2].to_string(),
        source: if_blank(group_one(&BUSY_CLEAR_SOURCE, detail), "unknown"),
        current_command: if_blank(group_one(&BUSY_CLEAR_COMMAND, detail), "unknown"),
        tail: extract_busy_clear_tail(detail),
        protected_reason: String::new(),
        live_turn_evidence: String::new(),
    })
}

fn parse_restart_refused_busy(output: &str) -> Option<SessionRestartRefusedBusy> {
    let captures = BUSY_RESTART_REFUSAL_HEADER.captures(output)?;
    let detail = &output[captures.get(0)?.end()..];
    Some(SessionRestartRefusedBusy {
        file: captures[1].to_string(),
        pane: captures[2].to_string(),
        source: if_blank(group_one(&BUSY_CLEAR_SOURCE, detail), "unknown"),
        current_command: if_blank(group_one(&BUSY_CLEAR_COMMAND, detail), "unknown"),
        tail: extract_busy_clear_tail(detail),
    })
}

fn parse_restart_refused_starting(output: &str) -> Option<SessionRestartRefusedStarting> {
    let captures = STARTING_RESTART_REFUSAL.captures(output)?;
    Some(SessionRestartRefusedStarting {
        file: captures[1].to_string(),
        reason: kotlin_trim(&captures[2]).to_string(),
    })
}

fn parse_restart_refused_editor_holds_pane(
    output: &str,
) -> Option<SessionRestartRefusedEditorHoldsPane> {
    let captures = EDITOR_RESTART_REFUSAL_HEADER.captures(output)?;
    let detail = &output[captures.get(0)?.end()..];
    Some(SessionRestartRefusedEditorHoldsPane {
        file: captures[1].to_string(),
        pane: captures[2].to_string(),
        editor: captures[3].to_string(),
        source: if_blank(group_one(&BUSY_CLEAR_SOURCE, detail), "unknown"),
        current_command: if_blank(group_one(&BUSY_CLEAR_COMMAND, detail), "unknown"),
        tail: extract_busy_clear_tail(detail),
    })
}

/// Kotlin `formatEvidenceAge`.
fn format_evidence_age(secs: i64) -> String {
    if secs < 120 {
        format!("{secs}s")
    } else if secs < 7200 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// #clearnoticeverify: the first line states the outcome and when the clear
/// runs; the second carries the re-arm and the live-turn evidence and its age.
fn clear_queued_message(relative_path: &str, queued: &SessionClearQueued) -> String {
    if let Some(settles_in) = queued.settles_in_secs {
        return cancelled_prompt_clear_queued_message(relative_path, queued, settles_in);
    }
    if queued.evidence == PENDING_RUN_CLEAR_EVIDENCE {
        return pending_run_clear_queued_message(relative_path, queued);
    }
    let mut message = String::new();
    if queued.rearmed {
        message.push_str("Clear Session Context re-armed for ");
        message.push_str(relative_path);
        message.push_str(": still one clear, it runs when the turn in pane ");
    } else {
        message.push_str("Clear Session Context queued for ");
        message.push_str(relative_path);
        message.push_str(": it runs when the turn in pane ");
    }
    message.push_str(&queued.pane);
    message.push_str(" ends.\n");
    message.push_str("Nothing was typed into the running turn");
    if !kotlin_is_blank(&queued.evidence) {
        message.push_str(" (live-turn evidence: ");
        message.push_str(&queued.evidence);
        if let Some(secs) = queued.evidence_age_secs {
            message.push_str(", ");
            message.push_str(&format_evidence_age(secs));
            message.push_str(" old");
        }
        message.push(')');
    }
    message.push_str(". The clear is dropped if a new prompt starts first. ");
    message.push_str("Use Interrupt and clear to clear immediately instead.");
    message
}

/// #clearctxslow: queued behind a prompt that never started a turn.
fn cancelled_prompt_clear_queued_message(
    relative_path: &str,
    queued: &SessionClearQueued,
    settles_in_secs: i64,
) -> String {
    let mut message = String::from(if queued.rearmed {
        "Clear Session Context is already queued for "
    } else {
        "Clear Session Context queued for "
    });
    message.push_str(relative_path);
    message.push_str(": it runs automatically in about ");
    message.push_str(&format_evidence_age(settles_in_secs));
    message.push_str(".\n");
    message.push_str("The last prompt in pane ");
    message.push_str(&queued.pane);
    message.push_str(
        " never started a turn (it looks cancelled); agent-doc waits for that to be confirmed before ",
    );
    message.push_str("typing /clear. Pressing Clear again does not make it sooner. ");
    message.push_str("Use Interrupt and clear to clear immediately instead.");
    message
}

/// #clearaftrun: queued behind an accepted Run Agent Doc dispatch that has not
/// started its turn.
fn pending_run_clear_queued_message(relative_path: &str, queued: &SessionClearQueued) -> String {
    let mut message = String::from(if queued.rearmed {
        "Clear Session Context is already queued for "
    } else {
        "Clear Session Context queued for "
    });
    message.push_str(relative_path);
    message.push_str(": it runs after the Run Agent Doc ");
    match queued.evidence_age_secs {
        Some(age) => {
            message.push_str("accepted ");
            message.push_str(&format_evidence_age(age));
            message.push_str(" ago");
        }
        None => message.push_str("already accepted"),
    }
    message.push_str(" finishes.\n");
    message.push_str("That run has not started its turn in pane ");
    message.push_str(&queued.pane);
    message.push_str(
        " yet, so nothing is running and nothing was typed; a clear pressed after Run Agent Doc runs after that run, ",
    );
    message.push_str("and that run does not cancel it.");
    if let Some(grace) = queued.pre_turn_grace_secs {
        let remaining = (grace - queued.evidence_age_secs.unwrap_or(0)).max(0);
        message.push_str(" If the run never starts, the clear runs within about ");
        message.push_str(&format_evidence_age(remaining));
        message.push('.');
    }
    message.push_str(" Pressing Clear again or Interrupt and clear does not make it sooner.");
    message
}

fn clear_deferred_message(relative_path: &str) -> String {
    format!(
        "Clear Session Context deferred for {relative_path}.\nThe pane is busy under an active queue auto-loop, so the loop was paused and the clear will run automatically at the next idle gap, then the loop resumes. Use Interrupt and clear to clear immediately instead."
    )
}

fn shown_value(tail: &str) -> Option<&str> {
    (!kotlin_is_blank(tail) && tail != "unknown").then_some(tail)
}

fn clear_refused_message(relative_path: &str, refusal: &SessionClearRefused) -> String {
    let mut message = String::new();
    if !kotlin_is_blank(&refusal.protected_reason) {
        message.push_str("Clear Session Context is blocked for ");
        message.push_str(relative_path);
        message.push_str(".\nPane ");
        message.push_str(&refusal.pane);
        message.push_str(" contains protected prompt input");
        message.push_str(" (");
        message.push_str(&refusal.protected_reason);
        message.push_str(
            "). Use Interrupt and clear to discard the prompt input, or Show status to inspect the session.",
        );
        if let Some(tail) = shown_value(&refusal.tail) {
            message.push_str("\nLatest pane output: ");
            message.push_str(tail);
        }
        return message;
    }
    if !kotlin_is_blank(&refusal.live_turn_evidence) {
        message.push_str("Clear Session Context was not queued for ");
        message.push_str(relative_path);
        message.push_str(".\nA turn is running in pane ");
        message.push_str(&refusal.pane);
        message.push_str(" (");
        message.push_str(&refusal.live_turn_evidence);
        message.push_str(
            "), and no agent-doc supervisor owns that pane to run the clear when the turn ends.",
        );
        message.push_str(" Nothing was sent. Retry Clear Session Context after the turn finishes, or use Interrupt and clear to discard the running turn.");
        return message;
    }
    message.push_str("Session is still running for ");
    message.push_str(relative_path);
    message.push_str(".\nPane ");
    message.push_str(&refusal.pane);
    message.push_str(" is busy");
    if let Some(command) = shown_value(&refusal.current_command) {
        message.push_str(" (");
        message.push_str(command);
        message.push(')');
    }
    message.push_str(". Wait for the turn to finish, then retry Clear Session Context.");
    message.push_str(" Use Refresh and retry if the pane has returned to an idle prompt, or Interrupt and clear to discard the running turn.");
    if let Some(tail) = shown_value(&refusal.tail) {
        message.push_str("\nLatest pane output: ");
        message.push_str(tail);
    }
    message
}

fn restart_refused_busy_message(
    relative_path: &str,
    refusal: &SessionRestartRefusedBusy,
) -> String {
    let mut message = String::from("Restart Supervisor is blocked for ");
    message.push_str(relative_path);
    message.push_str(".\nPane ");
    message.push_str(&refusal.pane);
    message.push_str(" is busy");
    if let Some(command) = shown_value(&refusal.current_command) {
        message.push_str(" (");
        message.push_str(command);
        message.push(')');
    }
    message.push_str(". Use Interrupt and restart to stop the running turn and restart the supervisor, or Show status to inspect the session.");
    if let Some(tail) = shown_value(&refusal.tail) {
        message.push_str("\nLatest pane output: ");
        message.push_str(tail);
    }
    message
}

fn restart_refused_starting_message(
    relative_path: &str,
    refusal: &SessionRestartRefusedStarting,
) -> String {
    let mut message = String::from("Restart Supervisor is blocked for ");
    message.push_str(relative_path);
    message.push_str(".\nThe authoritative actor is still starting");
    if !kotlin_is_blank(&refusal.reason) {
        message.push_str(" and ");
        message.push_str(&refusal.reason);
    }
    message.push_str(". Use Interrupt and restart to stop the current supervisor generation and restart anyway, or Show status to inspect the session.");
    message
}

fn restart_refused_editor_holds_pane_message(
    relative_path: &str,
    refusal: &SessionRestartRefusedEditorHoldsPane,
) -> String {
    let mut message = String::from("Restart Supervisor is blocked for ");
    message.push_str(relative_path);
    message.push_str(".\nA terminal editor");
    let editor = if kotlin_is_blank(&refusal.editor) {
        refusal.current_command.as_str()
    } else {
        refusal.editor.as_str()
    };
    if let Some(editor) = shown_value(editor) {
        message.push_str(" (");
        message.push_str(editor);
        message.push(')');
    }
    message.push_str(" holds pane ");
    message.push_str(&refusal.pane);
    message.push_str(" — e.g. Claude Code `ctrl+g` edit-in-nvim. Close the editor (for example `:wq` in nvim) and retry; Interrupt and restart will not bypass it. Use Show status to inspect the session.");
    if let Some(tail) = shown_value(&refusal.tail) {
        message.push_str("\nLatest pane output: ");
        message.push_str(tail);
    }
    message
}

fn generic() -> SessionCommandOutcome {
    SessionCommandOutcome {
        kind: SessionCommandOutcomeKind::Generic,
        summary: None,
        clear_queued: None,
        clear_refused: None,
        restart_refused_busy: None,
        restart_refused_starting: None,
        restart_refused_editor_holds_pane: None,
    }
}

/// An outcome of `kind` with `summary` and no details; a refusal adds its own.
fn with_kind(kind: SessionCommandOutcomeKind, summary: String) -> SessionCommandOutcome {
    SessionCommandOutcome {
        kind,
        summary: Some(summary),
        ..generic()
    }
}

fn clear_outcome(output: &str, relative_path: &str, succeeded: bool) -> SessionCommandOutcome {
    if succeeded {
        if let Some(queued) = parse_clear_queued(output) {
            return {
                let summary = clear_queued_message(relative_path, &queued);
                SessionCommandOutcome {
                    clear_queued: Some(queued),
                    ..with_kind(SessionCommandOutcomeKind::ClearQueued, summary)
                }
            };
        }
        if is_deferred_queue_preempt_clear(output) {
            return with_kind(
                SessionCommandOutcomeKind::ClearDeferred,
                clear_deferred_message(relative_path),
            );
        }
        let hint = if kotlin_is_blank(output) {
            format!("Cleared session context for {relative_path}")
        } else {
            output.to_string()
        };
        return with_kind(SessionCommandOutcomeKind::Cleared, hint);
    }
    match parse_clear_refused(output) {
        Some(refusal) => {
            let summary = clear_refused_message(relative_path, &refusal);
            SessionCommandOutcome {
                clear_refused: Some(refusal),
                ..with_kind(SessionCommandOutcomeKind::ClearRefused, summary)
            }
        }
        None => generic(),
    }
}

fn restart_outcome(
    output: &str,
    relative_path: &str,
    succeeded: bool,
    force: bool,
) -> SessionCommandOutcome {
    if succeeded {
        return generic();
    }
    // #hj7s: an editor holding the pane refuses both Restart and --force
    // Interrupt and restart, so it is read before the force check.
    if let Some(refusal) = parse_restart_refused_editor_holds_pane(output) {
        return {
            let summary = restart_refused_editor_holds_pane_message(relative_path, &refusal);
            SessionCommandOutcome {
                restart_refused_editor_holds_pane: Some(refusal),
                ..with_kind(
                    SessionCommandOutcomeKind::RestartRefusedEditorHoldsPane,
                    summary,
                )
            }
        };
    }
    if force {
        return generic();
    }
    if let Some(refusal) = parse_restart_refused_busy(output) {
        return {
            let summary = restart_refused_busy_message(relative_path, &refusal);
            SessionCommandOutcome {
                restart_refused_busy: Some(refusal),
                ..with_kind(SessionCommandOutcomeKind::RestartRefusedBusy, summary)
            }
        };
    }
    if let Some(refusal) = parse_restart_refused_starting(output) {
        return {
            let summary = restart_refused_starting_message(relative_path, &refusal);
            SessionCommandOutcome {
                restart_refused_starting: Some(refusal),
                ..with_kind(SessionCommandOutcomeKind::RestartRefusedStarting, summary)
            }
        };
    }
    generic()
}

/// Derive what one session command output means.
pub fn derive_session_command_outcome(
    evidence: &SessionCommandOutcomeEvidence,
) -> SessionCommandOutcome {
    let output = evidence.output.as_deref().unwrap_or("");
    let relative_path = evidence.relative_path.as_deref().unwrap_or("");
    let succeeded = evidence.succeeded.unwrap_or(false);
    match evidence.command.as_deref() {
        Some("clear") => clear_outcome(output, relative_path, succeeded),
        Some("restart") => restart_outcome(
            output,
            relative_path,
            succeeded,
            evidence.force.unwrap_or(false),
        ),
        _ => generic(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kotlin_string_helpers_match_their_stdlib_edges() {
        assert_eq!(substring_before("a). Run b", "). Run"), "a");
        assert_eq!(substring_before("abc", "x"), "abc");
        assert_eq!(remove_surrounding("\"", "\""), "\"");
        assert_eq!(remove_surrounding("\"\"", "\""), "");
        assert_eq!(remove_surrounding("\"x", "\""), "\"x");
        assert_eq!(remove_suffix("x).", ")."), "x");
        assert_eq!(format_evidence_age(119), "119s");
        assert_eq!(format_evidence_age(4000), "66m 40s");
        assert_eq!(format_evidence_age(7200), "2h 0m");
    }

    #[test]
    fn a_pid_too_large_for_a_long_is_absent_like_to_long_or_null() {
        let queued = parse_clear_queued(
            "session_clear queued for /r/doc.md — a turn is live in pane %1 (evidence=x evidence_age_secs=99999999999999999999)",
        )
        .unwrap();
        assert_eq!(queued.evidence_age_secs, None);
    }

    #[test]
    fn an_unknown_command_is_generic() {
        let outcome = derive_session_command_outcome(&SessionCommandOutcomeEvidence {
            command: Some("interrupt-clear".to_string()),
            succeeded: Some(false),
            ..Default::default()
        });
        assert_eq!(outcome, generic());
    }
}

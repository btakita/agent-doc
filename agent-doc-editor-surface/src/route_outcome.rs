//! What a Run Agent Doc route's output means for the operator, behind the
//! hot-reloaded native boundary (`#jbshrinkrestarts`, the GH #326 lever).
//!
//! The JetBrains plugin runs a route through the controller and gets back its
//! output text and exit code. Reading that text was a Kotlin rule set in
//! `TerminalUtil`: which outcome it is (still running, queued behind the owner,
//! steering delivered, paused queue, deferred harness switch, unproven dispatch,
//! protected prompt input, or a persistent failure), the details a paused queue
//! or a deferred switch carries, the notification text for each outcome, the
//! route snapshot path, and the two success notices (another lane owns the pane;
//! the pane is not visible). The route's wording is the binary's, so every
//! wording change used to need a plugin generation as well, and on the
//! exact-263 modular package that is an IDE restart.
//!
//! Those rules live here now, as the `route_outcome` derivation. The plugin
//! sends the output and the document's relative path and executes the answer:
//! the notification type, its actions, the ops-log line, and the state
//! projection stay in Kotlin. A new outcome *kind* still needs a plugin
//! generation (it has no effect there); a new wording or a new output shape for
//! an existing kind does not.
//!
//! This is a byte-for-byte port of the pre-move Kotlin rules. They used
//! `java.util.regex` with ASCII `\s`, `\S`, `\d`, and `\b`, and Kotlin's
//! `trim`/`isBlank` (`Char.isWhitespace`), which differs from Rust's
//! `char::is_whitespace`; both are reproduced here. The plugin keeps a frozen
//! Kotlin copy for a library that cannot answer. One parity fixture
//! (`editors/jetbrains/src/test/resources/editor-derivations/route_outcome.json`)
//! is answered by both. New rules land here, never there.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// What one finished route reports. Every field is optional on the wire: after
/// a hot reload this library is newer than the plugin generation calling it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteOutcomeEvidence {
    /// The route's combined output, exactly as the route client returned it.
    #[serde(default)]
    pub output: Option<String>,
    /// The document's project-relative path, for the notification text.
    #[serde(default)]
    pub relative_path: Option<String>,
}

/// The kind of outcome a route output reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteOutcomeKind {
    Persistent,
    StartupNotReady,
    BusyRunning,
    SteeringDelivered,
    QueuedPending,
    QueuePaused,
    AgentSwitchDeferred,
    DispatchStartUnproven,
    ProtectedPromptInput,
}

/// A route refused because the document's queue is paused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteQueuePaused {
    pub reason: String,
    pub restart_supervisor_redirect: bool,
    pub stale_pid: String,
}

/// A route that found the actor on another harness than the frontmatter names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteAgentSwitchDeferred {
    pub previous_harness: String,
    pub target_harness: String,
    pub queue_paused: bool,
    pub force_required: bool,
    pub supervisor_unavailable: bool,
    pub restart_in_flight: bool,
}

/// The `route_outcome` answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteOutcome {
    pub kind: RouteOutcomeKind,
    /// The notification text for this kind, or `None` for a persistent failure
    /// (the plugin's persistent-failure notice saves the output and links it).
    pub summary: Option<String>,
    /// The paused-queue details, when the output names a paused queue.
    pub queue_paused: Option<RouteQueuePaused>,
    /// The deferred harness switch, when the output names one.
    pub agent_switch_deferred: Option<RouteAgentSwitchDeferred>,
    /// The latest `snapshot_path=` the output names, or empty.
    pub snapshot_path: String,
    /// The `[agent-doc] owned_by_other_lane:` notice, without its prefix.
    pub owned_by_other_lane_notice: Option<String>,
    /// The `[route] pane-not-visible:` line (GH #243), trimmed.
    pub pane_not_visible_notice: Option<String>,
}

const UI_OUTCOME_QUEUED_BEHIND_OWNER: &str = "queued_behind_owner";
const UI_OUTCOME_STEERING_DELIVERED_TO_OWNER: &str = "steering_delivered_to_owner";
const UI_OUTCOME_RECOVERED_AND_RETRIED: &str = "recovered_and_retried";
const SUPERVISOR_RESTART_REDIRECT_MARKER: &str = "supervisor_restart_redirect";
const OWNED_BY_OTHER_LANE_NOTICE_PREFIX: &str = "[agent-doc] owned_by_other_lane:";
const PANE_NOT_VISIBLE_MARKER: &str = "[route] pane-not-visible:";
const QUEUE_PAUSED_REASON_MAX_UTF16: usize = 600;

/// `java.util.regex` `\s` (ASCII): space, tab, newline, vertical tab, form feed, carriage return.
const JAVA_SPACE: &str = r"[ \t\n\x0B\x0C\r]";
/// `java.util.regex` `\S` (ASCII).
const JAVA_NON_SPACE: &str = r"[^ \t\n\x0B\x0C\r]";

pub(crate) fn java_regex(pattern: &str) -> Regex {
    let pattern = pattern
        .replace(r"\s", JAVA_SPACE)
        .replace(r"\S", JAVA_NON_SPACE)
        .replace(r"\d", "[0-9]")
        .replace(r"\b", r"(?-u:\b)");
    Regex::new(&pattern).expect("route outcome pattern compiles")
}

static QUEUE_PAUSED_REASON: LazyLock<Regex> =
    LazyLock::new(|| java_regex(r"(?s)failed_stage=queue_paused\s+reason=(.*?)\s+receipt_id="));
static QUEUE_PAUSED_STALE_PID: LazyLock<Regex> = LazyLock::new(|| java_regex(r"\bstale_pid=(\d+)"));
static AGENT_SWITCH_DEFERRED: LazyLock<Regex> = LazyLock::new(|| {
    java_regex(
        r"(?s)authoritative actor record for (.+?) is running harness ([^,\s]+), but frontmatter now resolves to ([^;\s]+); deferring to boundary agent restart instead of replacing live pane",
    )
});
static EDITOR_ATTEMPT: LazyLock<Regex> = LazyLock::new(|| java_regex(r"\beditor_attempt_id=(\S+)"));
static SNAPSHOT_PATH: LazyLock<Regex> = LazyLock::new(|| java_regex(r"\bsnapshot_path=(\S+)"));
static DRAFT_PREVIEW: LazyLock<Regex> =
    LazyLock::new(|| java_regex(r#"\bdraft_preview="([^"]*)""#));

/// Kotlin's `Char.isWhitespace` on the JVM: `Character.isWhitespace(c) ||
/// Character.isSpaceChar(c)`, i.e. the Zs/Zl/Zp separators plus the ASCII
/// controls tab through carriage return and U+001C..U+001F. Unlike
/// `char::is_whitespace` it excludes U+0085.
pub(crate) fn kotlin_is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{0B}'
            | '\u{0C}'
            | '\r'
            | '\u{1C}'..='\u{1F}'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// Kotlin `String.trim()`.
pub(crate) fn kotlin_trim(value: &str) -> &str {
    value.trim_matches(kotlin_is_whitespace)
}

/// Kotlin `CharSequence.isBlank()`.
pub(crate) fn kotlin_is_blank(value: &str) -> bool {
    value.chars().all(kotlin_is_whitespace)
}

/// Kotlin `String.lines()` / `lineSequence()`: split at CRLF, LF, and CR.
pub(crate) fn kotlin_lines(value: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = value.as_bytes();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                lines.push(&value[start..index]);
                index += 1;
                start = index;
            }
            b'\r' => {
                lines.push(&value[start..index]);
                index += if bytes.get(index + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = index;
            }
            _ => index += 1,
        }
    }
    lines.push(&value[start..]);
    lines
}

/// Kotlin `String.take(n)` over UTF-16 units, never splitting a code point.
fn take_utf16(value: &str, max_units: usize) -> &str {
    let mut units = 0;
    for (index, c) in value.char_indices() {
        let width = c.len_utf16();
        if units + width > max_units {
            return &value[..index];
        }
        units += width;
    }
    value
}

/// Kotlin `ifBlank { fallback }`.
fn if_blank<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if kotlin_is_blank(value) {
        fallback
    } else {
        value
    }
}

fn has_user_facing_outcome(lowercase_output: &str, outcome: &str) -> bool {
    lowercase_output.contains(&format!("ui_outcome={outcome}"))
}

fn is_starting_actor_route_failure(output: &str) -> bool {
    output.contains("authoritative actor generation")
        && output.contains("route will not inject a new trigger")
        && output.contains("the authoritative actor is still starting")
}

fn is_protected_prompt_input_route_failure(lower: &str) -> bool {
    lower.contains("route refusing to dispatch")
        && lower.contains("composer contains protected prompt input")
}

fn is_dispatch_start_unproven(lower: &str) -> bool {
    lower.contains("accepted_without_dispatch_start_proof")
        || lower.contains("route_dispatch_only_submit_unproven")
        || (lower.contains("dispatch-only")
            && lower.contains("only pane-input acceptance proof")
            && lower.contains("dispatch-start proof"))
}

fn is_latest_run_still_booting_shape(lower: &str) -> bool {
    lower.contains("dispatch-only")
        && lower.contains("latest run is still booting")
        && lower.contains("never reached a dispatch-ready prompt")
}

fn is_dispatch_only_active_turn_blocked(lower: &str) -> bool {
    let has_active_turn_cue = lower.contains("opencode active turn")
        || lower.contains("active codex turn")
        || lower.contains("active claude turn");
    lower.contains("dispatch-only")
        && has_active_turn_cue
        && (lower.contains("pane still shows") || lower.contains("pane is busy on an active"))
}

fn is_dispatch_only_busy_actor_wait_timeout(lower: &str) -> bool {
    lower.contains("dispatch-only route will not inject a new trigger")
        && lower.contains("the authoritative actor is busy")
        && lower.contains("did not return to a dispatch-ready prompt")
}

fn is_route_queued(lower: &str) -> bool {
    has_user_facing_outcome(lower, UI_OUTCOME_QUEUED_BEHIND_OWNER)
        || lower.contains("queued pending dispatch")
            && (lower.contains("active agent:queue") || lower.contains("agent:queue auto"))
}

fn is_steering_delivered(lower: &str) -> bool {
    has_user_facing_outcome(lower, UI_OUTCOME_STEERING_DELIVERED_TO_OWNER)
        && lower.contains("steering_delivered=")
}

fn parse_queue_paused(output: &str, lower: &str) -> Option<RouteQueuePaused> {
    if !lower.contains("failed_stage=queue_paused") {
        return None;
    }
    let reason = QUEUE_PAUSED_REASON
        .captures(output)
        .and_then(|captures| captures.get(1))
        .map(|reason| kotlin_trim(reason.as_str()).to_string())
        .unwrap_or_default();
    let stale_pid = QUEUE_PAUSED_STALE_PID
        .captures(output)
        .and_then(|captures| captures.get(1))
        .map(|pid| pid.as_str().to_string())
        .unwrap_or_default();
    let restart_supervisor_redirect = lower.contains(SUPERVISOR_RESTART_REDIRECT_MARKER)
        || has_user_facing_outcome(lower, UI_OUTCOME_RECOVERED_AND_RETRIED)
        || lower.contains("next_action=restart_supervisor_once_and_retry");
    Some(RouteQueuePaused {
        reason,
        restart_supervisor_redirect,
        stale_pid,
    })
}

fn parse_agent_switch_deferred(output: &str, lower: &str) -> Option<RouteAgentSwitchDeferred> {
    let captures = AGENT_SWITCH_DEFERRED.captures(output)?;
    let group = |index: usize| {
        captures
            .get(index)
            .map(|value| value.as_str().to_string())
            .unwrap_or_default()
    };
    let restart_in_flight = lower.contains("harness restart is already in flight");
    // `#actorswitchdeferbusyself`: an in-flight restart never asks for
    // `--force`, so it can never be reported as force-required.
    let force_required =
        !restart_in_flight && lower.contains("restart-supervisor") && lower.contains("--force");
    Some(RouteAgentSwitchDeferred {
        previous_harness: group(2),
        target_harness: group(3),
        queue_paused: lower.contains("queue is paused"),
        force_required,
        supervisor_unavailable: lower.contains("supervisor is unreachable")
            || lower.contains("supervisor is unhealthy")
            || lower.contains("supervisor is paused"),
        restart_in_flight,
    })
}

fn classify(output: &str, lower: &str) -> RouteOutcomeKind {
    if is_steering_delivered(lower) {
        RouteOutcomeKind::SteeringDelivered
    } else if is_route_queued(lower) {
        RouteOutcomeKind::QueuedPending
    } else if parse_queue_paused(output, lower).is_some() {
        RouteOutcomeKind::QueuePaused
    } else if parse_agent_switch_deferred(output, lower).is_some() {
        RouteOutcomeKind::AgentSwitchDeferred
    } else if is_dispatch_only_active_turn_blocked(lower)
        || is_dispatch_only_busy_actor_wait_timeout(lower)
        || (is_latest_run_still_booting_shape(lower) && lower.contains("(active codex turn)"))
    {
        RouteOutcomeKind::BusyRunning
    } else if is_dispatch_start_unproven(lower) {
        RouteOutcomeKind::DispatchStartUnproven
    } else if is_protected_prompt_input_route_failure(lower) {
        RouteOutcomeKind::ProtectedPromptInput
    } else if (is_latest_run_still_booting_shape(lower) && lower.contains("(timed_out)"))
        || is_starting_actor_route_failure(output)
    {
        RouteOutcomeKind::StartupNotReady
    } else {
        RouteOutcomeKind::Persistent
    }
}

/// The value of the first match of `regex` on the latest line that has a non-blank one.
fn latest_regex_value(lines: &[&str], regex: &Regex) -> String {
    for line in lines.iter().rev() {
        if let Some(value) = regex.captures(line).and_then(|captures| captures.get(1))
            && !kotlin_is_blank(value.as_str())
        {
            return value.as_str().to_string();
        }
    }
    String::new()
}

fn still_running_message(relative_path: &str) -> String {
    format!(
        "Agent Doc is still running for {relative_path}.\nThe Codex pane has an active turn and is not ready for another routed follow-up yet. Wait for the turn to finish, then run Agent Doc again."
    )
}

fn queue_paused_message(relative_path: &str, paused: &RouteQueuePaused) -> String {
    let mut message = format!("Agent Doc queue is paused for {relative_path}.\n");
    if paused.restart_supervisor_redirect {
        message.push_str("This pause looks like stale-supervisor churn. Restart Supervisor and resume the queue, then run Agent Doc again.");
        if !kotlin_is_blank(&paused.stale_pid) {
            message.push_str("\nStale supervisor pid: ");
            message.push_str(&paused.stale_pid);
        }
    } else {
        message.push_str("Resume the queue when unattended dispatch should continue, or use Show status to inspect it first.");
    }
    if !kotlin_is_blank(&paused.reason) {
        message.push_str("\nReason: ");
        message.push_str(take_utf16(&paused.reason, QUEUE_PAUSED_REASON_MAX_UTF16));
    }
    message
}

fn agent_switch_deferred_message(
    relative_path: &str,
    deferred: &RouteAgentSwitchDeferred,
) -> String {
    let mut message = format!(
        "Agent Doc did not switch harnesses for {relative_path}.\nCurrent actor is {}; frontmatter now resolves to {}. ",
        if_blank(&deferred.previous_harness, "the previous harness"),
        if_blank(&deferred.target_harness, "the new harness"),
    );
    message.push_str(if deferred.queue_paused {
        "The queue is paused, so the boundary restart will not fire. Restart Supervisor and resume the queue, then run Agent Doc again."
    } else if deferred.restart_in_flight {
        "The harness restart is already in flight and completes the switch at the next boundary. Wait for it, then run Agent Doc again — do not interrupt or force a restart."
    } else if deferred.force_required {
        "The pane is not at a dispatch-ready boundary. Use Interrupt and restart to force the harness switch."
    } else if deferred.supervisor_unavailable {
        "The supervisor is not healthy enough to reach the boundary restart. Restart Supervisor, then run Agent Doc again."
    } else {
        "The supervisor will switch at the next idle boundary. Use Restart Supervisor to switch now."
    });
    message
}

fn dispatch_unproven_message(relative_path: &str, lines: &[&str]) -> String {
    let attempt_id = latest_regex_value(lines, &EDITOR_ATTEMPT);
    let snapshot_path = latest_regex_value(lines, &SNAPSHOT_PATH);
    let mut message = format!(
        "Agent Doc did not start for {relative_path}.\nPane input was accepted, but dispatch-start proof did not appear."
    );
    if !kotlin_is_blank(&attempt_id) {
        message.push_str("\nAttempt: ");
        message.push_str(&attempt_id);
    }
    if !kotlin_is_blank(&snapshot_path) {
        message.push_str("\nRoute snapshot: ");
        message.push_str(&snapshot_path);
    }
    message.push_str("\nWait for an idle prompt or restart the session, then run Agent Doc again.");
    message
}

fn protected_prompt_input_message(relative_path: &str, lines: &[&str]) -> String {
    let snapshot_path = latest_regex_value(lines, &SNAPSHOT_PATH);
    let draft_preview = latest_regex_value(lines, &DRAFT_PREVIEW);
    let mut message = format!(
        "Agent Doc did not start for {relative_path}.\nThe target pane has unsent prompt text. Clear or submit that draft, then run Agent Doc again."
    );
    if !kotlin_is_blank(&draft_preview) {
        message.push_str("\nDraft preview: ");
        message.push_str(&draft_preview);
    }
    if !kotlin_is_blank(&snapshot_path) {
        message.push_str("\nRoute snapshot: ");
        message.push_str(&snapshot_path);
    }
    message
}

fn owned_by_other_lane_notice(lines: &[&str]) -> Option<String> {
    let line = lines
        .iter()
        .map(|line| kotlin_trim(line))
        .find(|line| line.starts_with(OWNED_BY_OTHER_LANE_NOTICE_PREFIX))?;
    let notice = kotlin_trim(&line[OWNED_BY_OTHER_LANE_NOTICE_PREFIX.len()..]);
    (!notice.is_empty()).then(|| notice.to_string())
}

fn pane_not_visible_notice(lines: &[&str]) -> Option<String> {
    lines
        .iter()
        .map(|line| kotlin_trim(line))
        .find(|line| line.starts_with(PANE_NOT_VISIBLE_MARKER))
        .map(str::to_string)
}

/// Derive what one route output means.
pub fn derive_route_outcome(evidence: &RouteOutcomeEvidence) -> RouteOutcome {
    let output = evidence.output.as_deref().unwrap_or("");
    let relative_path = evidence.relative_path.as_deref().unwrap_or("");
    let lower = output.to_lowercase();
    let lines = kotlin_lines(output);
    let kind = classify(output, &lower);
    let queue_paused = parse_queue_paused(output, &lower);
    let agent_switch_deferred = parse_agent_switch_deferred(output, &lower);
    let summary = match kind {
        RouteOutcomeKind::Persistent => None,
        RouteOutcomeKind::BusyRunning | RouteOutcomeKind::StartupNotReady => {
            Some(still_running_message(relative_path))
        }
        RouteOutcomeKind::QueuedPending => Some(format!(
            "Agent Doc is queued behind the active owner for {relative_path}.\nIt should run when that turn drains."
        )),
        RouteOutcomeKind::SteeringDelivered => Some(format!(
            "Sent your update to the Agent Doc turn already running for {relative_path}.\nThat turn will handle it; no second turn was started."
        )),
        RouteOutcomeKind::QueuePaused => Some(queue_paused_message(
            relative_path,
            &queue_paused.clone().unwrap_or_default(),
        )),
        RouteOutcomeKind::AgentSwitchDeferred => Some(agent_switch_deferred_message(
            relative_path,
            &agent_switch_deferred.clone().unwrap_or_default(),
        )),
        RouteOutcomeKind::DispatchStartUnproven => {
            Some(dispatch_unproven_message(relative_path, &lines))
        }
        RouteOutcomeKind::ProtectedPromptInput => {
            Some(protected_prompt_input_message(relative_path, &lines))
        }
    };
    RouteOutcome {
        kind,
        summary,
        queue_paused,
        agent_switch_deferred,
        snapshot_path: latest_regex_value(&lines, &SNAPSHOT_PATH),
        owned_by_other_lane_notice: owned_by_other_lane_notice(&lines),
        pane_not_visible_notice: pane_not_visible_notice(&lines),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(output: &str) -> RouteOutcome {
        derive_route_outcome(&RouteOutcomeEvidence {
            output: Some(output.to_string()),
            relative_path: Some("plan.md".to_string()),
        })
    }

    #[test]
    fn java_ascii_classes_do_not_match_unicode_space_or_digits() {
        // `\s` is ASCII in java.util.regex: a no-break space does not end the reason.
        let paused = outcome(
            "failed_stage=queue_paused reason=held\u{A0}by operator receipt_id=1 stale_pid=\u{0661}2",
        );
        let details = paused.queue_paused.unwrap();
        assert_eq!(details.reason, "held\u{A0}by operator");
        // `\d` is ASCII: an Arabic-Indic digit is not a pid digit.
        assert_eq!(details.stale_pid, "");
    }

    #[test]
    fn kotlin_trim_keeps_nel_and_strips_unit_separator() {
        assert_eq!(kotlin_trim("\u{85}x\u{1F}"), "\u{85}x");
        assert!(kotlin_is_blank("\u{2007}\u{3000}\t"));
        assert!(!kotlin_is_blank("\u{85}"));
    }

    #[test]
    fn kotlin_lines_split_crlf_lf_and_cr() {
        assert_eq!(kotlin_lines("a\r\nb\rc\nd"), vec!["a", "b", "c", "d"]);
        assert_eq!(kotlin_lines("a\n"), vec!["a", ""]);
    }

    #[test]
    fn a_long_reason_is_cut_at_600_utf16_units_without_splitting_a_code_point() {
        let reason = format!("{}\u{1F600}tail", "x".repeat(599));
        assert_eq!(take_utf16(&reason, 600), "x".repeat(599));
        assert_eq!(take_utf16("short", 600), "short");
    }
}

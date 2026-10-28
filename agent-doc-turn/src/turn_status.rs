//! Pure turn-status vocabulary and pane-title policy.
//!
//! Callers own tmux commands, project-root resolution, and state.db IO.

use serde::{Deserialize, Serialize};

/// Legacy pane-border title shown while a turn is in flight when no registered
/// document name is available.
pub const TURN_ACTIVE_PANE_TITLE: &str = "⟳ agent-doc: turn in progress";

/// Status suffix appended after the document name while a turn is in flight.
pub const TURN_ACTIVE_PANE_SUFFIX: &str = " — turn in progress";

/// Leading marker prepended to the pane title when the route-owned supervisor is
/// running a stale binary.
pub const STALE_SUPERVISOR_PANE_MARKER: &str = "⚠ STALE SUPERVISOR";

/// `#stalecontrollerhotupdate` (3): a stale-supervisor marker is evidence only
/// while the supervisor that wrote it is still running.
///
/// The marker is keyed by tmux pane id alone. Pane ids are reused across tmux
/// server restarts, and a supervisor that died stale never cleared its row:
/// observed live (2026-10-15) a fresh, current supervisor's pane `%180` was
/// titled `⚠ STALE SUPERVISOR` from a 2026-09-09 row whose writer (pid 261721)
/// had been gone for five weeks. `holder_started_at_secs` is the start time of
/// the process now holding `holder_pid` (`None` when no such process exists);
/// a process that started after the marker was written cannot be its writer
/// (pid reuse). The one-second slack absorbs start-time rounding.
pub fn stale_supervisor_marker_holder_is_live(
    holder_pid: Option<u32>,
    holder_started_at_secs: Option<u64>,
    written_at_secs: u64,
) -> bool {
    holder_pid.is_some()
        && holder_started_at_secs.is_some_and(|started| started <= written_at_secs.saturating_add(1))
}

/// Self-expiry window. A missed idle hook must not wedge the session busy.
pub const TURN_ACTIVE_TTL_SECS: u64 = 3600;

/// Projected turn-state contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnActiveMarker {
    /// The tmux pane the turn is running in (`$TMUX_PANE`), best-effort.
    pub pane: String,
    /// Unix seconds the turn went active, used for self-expiry.
    pub written_at: u64,
}

/// True when a turn-active fact is inside the freshness window.
pub fn turn_active_marker_is_fresh(marker: &TurnActiveMarker, now: u64) -> bool {
    now.saturating_sub(marker.written_at) < TURN_ACTIVE_TTL_SECS
}

/// Newest heartbeat that reads as expired at `now`: a turn-active lease whose
/// heartbeat is at or before this cutoff fails [`turn_active_marker_is_fresh`]
/// and may be deleted by an age-based sweep (GH #135). `None` while `now` is
/// still inside the first TTL window, where no heartbeat can be expired.
pub fn turn_active_expiry_cutoff(now: u64) -> Option<u64> {
    now.checked_sub(TURN_ACTIVE_TTL_SECS)
}

/// True when a turn-active fact belongs to `pane`.
pub fn turn_active_marker_matches_pane(marker: &TurnActiveMarker, pane: &str) -> bool {
    marker.pane == pane
}

/// Title to set for a turn state. A registered document name is retained in
/// both active and idle states; the legacy fallback stays available for panes
/// that have not been registered yet.
pub fn pane_title_for_state(document_name: Option<&str>, active: bool) -> String {
    let document_name = document_name.map(str::trim).filter(|name| !name.is_empty());
    match (document_name, active) {
        (Some(name), true) => format!("⟳ {name}{TURN_ACTIVE_PANE_SUFFIX}"),
        (Some(name), false) => name.to_string(),
        (None, true) => TURN_ACTIVE_PANE_TITLE.to_string(),
        (None, false) => String::new(),
    }
}

/// Single-marker title for a busy pane whose supervisor is stale (GH #124).
///
/// A pane title holds ONE marker. The pre-#124 composition welded the stale
/// marker onto the busy marker (`⚠ STALE SUPERVISOR ⟳ agent-doc: turn in
/// progress`), so a stale pane carried the live turn's own `⟳` marker as well
/// as the warning. The stale verdict replaces the busy glyph instead: the title
/// leads with the one warning marker and names the busy state as plain text.
pub const STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE: &str = "⚠ STALE SUPERVISOR: turn in progress";

/// Compose the pane-border title for a turn state, decorated with the stale
/// supervisor marker when `stale` is true. A known document name is present in
/// every state and the title always carries at most one status marker.
pub fn pane_title_for_status(document_name: Option<&str>, active: bool, stale: bool) -> String {
    compose_pane_title(&pane_title_for_state(document_name, active), stale)
}

/// The undecorated title under any stale-supervisor decoration: the busy title,
/// empty (idle), or an operator-owned custom title. Reads every shape agent-doc
/// has written, including the pre-#124 welded `⚠ STALE SUPERVISOR ⟳ …` form.
pub fn undecorated_pane_title(title: &str) -> &str {
    if title == STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE {
        return TURN_ACTIVE_PANE_TITLE;
    }
    match title.strip_prefix(STALE_SUPERVISOR_PANE_MARKER) {
        Some(rest) => rest
            .strip_prefix(" — ")
            .or_else(|| rest.strip_prefix(' '))
            .unwrap_or(rest),
        None => title,
    }
}

fn compose_pane_title(base: &str, stale: bool) -> String {
    match (stale, base) {
        (false, _) => base.to_string(),
        (true, "") => STALE_SUPERVISOR_PANE_MARKER.to_string(),
        (true, TURN_ACTIVE_PANE_TITLE) => STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE.to_string(),
        (true, active) if active.starts_with("⟳ ") => format!(
            "{STALE_SUPERVISOR_PANE_MARKER} — {}",
            active.trim_start_matches("⟳ ")
        ),
        (true, custom) => format!("{STALE_SUPERVISOR_PANE_MARKER} — {custom}"),
    }
}

fn pane_title_is_active(title: &str) -> bool {
    title == TURN_ACTIVE_PANE_TITLE
        || title == STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE
        || title.ends_with(TURN_ACTIVE_PANE_SUFFIX)
}

/// Refresh only the supervisor decoration. When registration supplies the
/// document name, regenerate the complete title so legacy/empty titles also
/// converge to the document-bearing form.
pub fn pane_title_with_freshness(title: &str, document_name: Option<&str>, stale: bool) -> String {
    match document_name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => pane_title_for_status(Some(name), pane_title_is_active(title), stale),
        None => compose_pane_title(undecorated_pane_title(title), stale),
    }
}

/// Number of agent-doc status markers (`⚠` stale, `⟳` busy) in a pane title.
/// GH #124: an agent-doc-composed title carries at most one.
pub fn pane_title_status_marker_count(title: &str) -> usize {
    title.matches('⚠').count() + title.matches('⟳').count()
}

// ---------------------------------------------------------------------------
// `#staleharnessturnlive`: interrupted turns retire their lease.
//
// Claude Code does not run the `Stop` hook when the operator interrupts a turn
// (Esc / Ctrl-C). The `UserPromptSubmit` hook wrote the turn-active lease, the
// matching `turn-status idle` never runs, and the lease outlived the harness
// turn until the next prompt's `Stop` or the one-hour TTL. Every consumer that
// treats the lease as an unconditional live-turn veto (`#reclaimliveturn`)
// then deferred recovery for an owner pane sitting idle at its prompt.
//
// The harness does record the interrupt: it appends a user record whose text
// is `[Request interrupted by user]` (or `... for tool use]`) to the session
// transcript the hook named in its `transcript_path`. That record is
// harness-authored turn-boundary evidence, unlike a scraped ready prompt, which
// a harness redraws between tool calls inside a live turn.
//
// `#devclear`: Codex behaves the same way. Its `UserPromptSubmit` hook writes
// the lease bound to the rollout JSONL named in `transcript_path`, and an
// interrupted turn runs no `Stop` hook. Codex records the boundary in the
// rollout as `{"type":"event_msg","payload":{"type":"turn_aborted",
// "reason":"interrupted",...}}`; every new Codex turn writes `task_started`,
// `turn_context` and `response_item` records after it, so the same
// newest-decisive-record rule applies.
//
// `#ctrlcrerun`: Grok Build behaves the same way. It runs the Claude-compatible
// `UserPromptSubmit` hook (`turn-status active`) with `transcript_path` naming
// the session's `updates.jsonl`, and a Ctrl+C cancel runs NO `stop` hook. Grok
// records the boundary as `{"timestamp":<unix secs>,"method":
// "_x.ai/session/update","params":{"update":{"sessionUpdate":"turn_completed",
// "stop_reason":"cancelled",...}}}`. Every new Grok turn writes a
// `user_prompt_submit` `hook_execution` and a `user_message_chunk` after it, so
// the same newest-decisive-record rule applies. Before this the probe could not
// read Grok records at all, so an interrupted Grok turn's lease read as a live
// turn and route reported steering "delivered to the agent already running"
// into a pane sitting at its idle prompt.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// `#runctrlclaude`: harness turn-end receipts.
//
// Clearing a lease only says "no turn is known to be live". Reclaiming an
// empty preflight without waiting out the stall deadline needs the stronger,
// POSITIVE fact that the harness reached a boundary after the cycle opened —
// and it must survive the lease row being deleted. So boundaries that prove
// the conversation which ran preflight can no longer answer it leave a durable
// per-pane receipt. Only two kinds qualify:
//
// - `SessionStart` with source `clear`, `startup` or `resume`: the harness is
//   at an idle prompt in a new or reloaded conversation. `compact` is excluded
//   because auto-compaction fires `SessionStart` in the middle of a live turn.
// - A settled `[Request interrupted by user…]` transcript record, or a Codex
//   rollout `turn_aborted` (reason `interrupted`) record (above).
//
// `Stop` is deliberately NOT a receipt: a Stop hook may answer
// `decision: "block"` (open cycle, queue continuation), and the harness then
// keeps generating in the same turn.
// ---------------------------------------------------------------------------

/// Receipt reason for an interrupt record that retired a lease.
pub const TURN_END_REASON_INTERRUPT: &str = "interrupt";

/// The receipt reason a harness hook event proves, or `None` when the event
/// does not prove the previous turn ended.
pub fn harness_turn_end_reason(
    hook_event_name: Option<&str>,
    source: Option<&str>,
) -> Option<&'static str> {
    match (hook_event_name?, source?) {
        ("SessionStart", "clear") => Some("session_start_clear"),
        ("SessionStart", "startup") => Some("session_start_startup"),
        ("SessionStart", "resume") => Some("session_start_resume"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// `#clauderesumeafterclear`: Claude conversation-id capture from `SessionStart`.
//
// A pane-launched Claude Code session never reports its conversation id back to
// agent-doc except through hooks. An operator `/clear` (and any non-resume
// startup) begins a NEW conversation, and unless the document's `resume.claude`
// follows it, every later restart resumes the pre-clear conversation. The
// capture is fail-closed: only `SessionStart` `clear`/`startup` qualify
// (`resume` reloads the same id, `compact` keeps it), the payload's own
// transcript must name the reported id, and the hook must be proven to come
// from the document supervisor's direct harness child (see
// [`hook_harness_child_of_supervisor`]) — a nested `claude -p` run from inside
// the pane inherits `$TMUX_PANE` and must never move the document's pointer.
// ---------------------------------------------------------------------------

/// A `SessionStart` payload that reports a new Claude conversation id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeSessionStartResume {
    /// The new conversation id (`session_id`).
    pub session_id: String,
    /// `clear` or `startup`.
    pub source: &'static str,
}

/// Classify a hook payload's fields for `#clauderesumeafterclear`.
///
/// `Err` carries a stable skip reason. `transcript_stem` is the file stem of the
/// payload's `transcript_path`; Claude names the transcript after the
/// conversation, so a missing or mismatched stem fails closed.
pub fn claude_session_start_resume(
    hook_event_name: Option<&str>,
    source: Option<&str>,
    session_id: Option<&str>,
    transcript_stem: Option<&str>,
) -> Result<ClaudeSessionStartResume, &'static str> {
    if hook_event_name != Some("SessionStart") {
        return Err("not_session_start");
    }
    let source = match source {
        Some("clear") => "clear",
        Some("startup") => "startup",
        Some("resume") => return Err("source_resume_keeps_id"),
        Some("compact") => return Err("source_compact_keeps_id"),
        _ => return Err("source_unproven"),
    };
    let session_id = session_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or("session_id_missing")?;
    match transcript_stem.map(str::trim) {
        Some(stem) if stem == session_id => {}
        Some(_) => return Err("transcript_names_other_session"),
        None => return Err("transcript_path_missing"),
    }
    Ok(ClaudeSessionStartResume {
        session_id: session_id.to_string(),
        source,
    })
}

/// Shell names a harness may interpose between itself and a hook command
/// (`sh -c '<command>'`). Anything else between the hook and the harness child
/// (a nested `claude -p`, a test binary, a `cargo` run) breaks the proof.
const HOOK_WRAPPER_SHELLS: &[&str] = &[
    "sh", "bash", "dash", "zsh", "ash", "ksh", "mksh", "fish", "busybox",
];

/// Prove the hook process was spawned by the supervisor's DIRECT harness child.
///
/// `ancestors` is the hook process's ancestry as `(pid, comm)`, starting at
/// its parent and walking upward. The supervisor must appear in it; the entry
/// just below the supervisor is its harness child; every entry between the
/// hook and that child must be a wrapper shell. Returns the harness child pid,
/// or a stable skip reason.
pub fn hook_harness_child_of_supervisor(
    ancestors: &[(u32, String)],
    supervisor_pid: u32,
) -> Result<u32, &'static str> {
    let Some(index) = ancestors.iter().position(|(pid, _)| *pid == supervisor_pid) else {
        return Err("supervisor_not_an_ancestor");
    };
    if index == 0 {
        return Err("hook_spawned_by_supervisor_not_harness");
    }
    let harness_pid = ancestors[index - 1].0;
    let intermediaries_are_shells = ancestors[..index - 1]
        .iter()
        .all(|(_, comm)| HOOK_WRAPPER_SHELLS.contains(&comm.trim()));
    if !intermediaries_are_shells {
        return Err("hook_not_from_supervisor_harness_child");
    }
    Ok(harness_pid)
}

/// Whether a controller `harness_id` names Claude Code explicitly. Unknown
/// harness ids fail closed rather than defaulting to Claude.
pub fn harness_id_is_claude(harness_id: &str) -> bool {
    matches!(harness_id.trim(), "claude" | "claude-code")
}

/// `#sdkclearwedge`: true when a harness turn-end receipt proves the pane's
/// conversation was already reset after an operator clear was deferred at
/// `deferred_at`, so delivering the deferred `/clear` would wipe the newer
/// conversation instead. Only boundaries that start an empty conversation
/// qualify (`session_start_clear`, `session_start_startup`); `resume`
/// restores context and an interrupt / abandoned prompt keeps it. The receipt
/// must be strictly newer than the deferral: a same-second boundary is
/// ambiguous and keeps the deferred clear.
pub fn harness_receipt_satisfies_deferred_clear(
    receipt_reason: &str,
    receipt_ended_at: u64,
    deferred_at: u64,
) -> bool {
    matches!(
        receipt_reason,
        "session_start_clear" | "session_start_startup"
    ) && receipt_ended_at > deferred_at
}

/// Text prefix of the record Claude Code appends to the session transcript
/// when the operator interrupts a turn. Both observed forms share it:
/// `[Request interrupted by user]` and `[Request interrupted by user for tool use]`.
pub const HARNESS_INTERRUPT_MARKER_PREFIX: &str = "[Request interrupted by user";

/// Seconds an interrupt record must have settled before it retires a lease.
/// A prompt submitted in the same second as the interrupt writes its lease
/// before its own transcript record lands; the settle window lets that record
/// appear so a fresh turn is never mistaken for the interrupted one.
pub const TURN_INTERRUPT_SETTLE_SECS: u64 = 3;

/// `#sdkclearwedge`: seconds an orphaned prompt snapshot must have settled,
/// with nothing but inert bookkeeping after it, before it retires a lease. A
/// live prompt's record lands in the same flush as its snapshot (1213 of 1213
/// matched snapshots in the 2026-10-11 transcript audit); the longest gap seen
/// before any follow-up record of a resumed session was 12s, so the window
/// sits well above it.
pub const ABANDONED_PROMPT_SETTLE_SECS: u64 = 20;

/// Receipt reason for an abandoned-prompt snapshot that retired a lease.
pub const TURN_END_REASON_ABANDONED_PROMPT: &str = "abandoned_prompt";

/// Transcript record types that carry no turn activity. They may follow an
/// abandoned prompt snapshot without disproving the abandonment; any other
/// record type after the snapshot (an attachment, a queue operation, a
/// snapshot update, a system record, or a type this policy does not know)
/// keeps the lease live.
const INERT_TRANSCRIPT_RECORD_TYPES: &[&str] = &[
    "last-prompt",
    "ai-title",
    "custom-title",
    "summary",
    "cost-state",
    "bridge-session",
    "pr-link",
    "atis-latch",
    "mode",
    "permission-mode",
];

/// Bytes of transcript tail the interrupt probe reads. Bounded so a large
/// transcript costs one small read; a record longer than the window is never
/// seen whole and therefore reads as [`TranscriptTailEvidence::Unknown`].
pub const TRANSCRIPT_TAIL_PROBE_BYTES: u64 = 256 * 1024;

/// What the newest conversation record of a harness transcript proves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptTailEvidence {
    /// The newest conversation record is a harness interrupt record written at
    /// `at_secs` (Unix seconds, UTC).
    Interrupted { at_secs: u64 },
    /// `#sdkclearwedge`: the newest main-conversation record is a prompt
    /// snapshot (`file-history-snapshot`, written at `at_secs`) whose prompt
    /// record never landed. Claude Code writes the snapshot when a submitted
    /// prompt clears its `UserPromptSubmit` hooks and appends the prompt record
    /// in the same flush; an operator cancel (Esc) in that window drops the
    /// prompt back into the composer and persists neither the prompt, an
    /// interrupt record, nor a `Stop` hook. Nothing but inert bookkeeping (and
    /// background-subagent sidechain records) follows the snapshot.
    AbandonedPrompt { at_secs: u64 },
    /// The newest conversation record is ordinary turn activity (a prompt, an
    /// assistant message, a tool result). `at_secs` is the record timestamp
    /// when the harness supplied one; only timestamped activity can extend an
    /// otherwise-expired turn lease.
    Activity { at_secs: Option<u64> },
    /// Nothing provable: no complete conversation record in the window, an
    /// unparseable or still-being-written record, a record without a readable
    /// timestamp, or a turn abort whose reason is not an operator interrupt.
    Unknown,
}

/// Codex rollout `event_msg` payload type the harness writes when a turn ends
/// without completing (`#devclear`).
pub const CODEX_TURN_ABORTED_EVENT: &str = "turn_aborted";

/// The only Codex `turn_aborted` reason accepted as an interrupt. Every abort
/// observed in local rollouts carries it; any other reason proves nothing.
pub const CODEX_TURN_ABORTED_INTERRUPTED_REASON: &str = "interrupted";

/// Codex rollout `event_msg` payload types that are main-turn conversation
/// activity. Other `event_msg` types are bookkeeping (`token_count`,
/// `thread_settings_applied`, `thread_goal_updated`, `item_completed`) or
/// late completions of work an interrupted turn left running in the
/// background (`exec_command_end`, `mcp_tool_call_end`), and are skipped.
pub const CODEX_TURN_ACTIVITY_EVENTS: &[&str] = &[
    "task_started",
    "task_complete",
    "user_message",
    "agent_message",
    "agent_reasoning",
];

/// Grok Build `updates.jsonl` `sessionUpdate` kind the harness writes when a
/// turn ends, completed or not (`#ctrlcrerun`).
pub const GROK_TURN_COMPLETED_UPDATE: &str = "turn_completed";

/// The only Grok `turn_completed` `stop_reason` accepted as an interrupt: the
/// operator's Ctrl+C cancel (`cancellation_context.trigger = "ctrl_c"` in the
/// session's `events.jsonl`), which runs no `stop` hook. Every other stop
/// reason (`end_turn`, `error`, `rate_limit`, ...) is followed by the `stop`
/// hook that clears the lease, so it proves nothing here.
pub const GROK_TURN_CANCELLED_STOP_REASON: &str = "cancelled";

/// Grok `sessionUpdate` kinds that are main-turn conversation activity. Other
/// kinds are bookkeeping (`background_tasks`, `task_*`, `subagent_*`,
/// `memory_dream_*`, `auto_compact_*`, `compaction_checkpoint`,
/// `hook_annotation`, `retry_state`, `session_recap`) and are skipped.
pub const GROK_TURN_ACTIVITY_UPDATES: &[&str] = &[
    "user_message_chunk",
    "agent_message_chunk",
    "agent_thought_chunk",
    "tool_call",
    "tool_call_update",
    "plan",
];

/// Grok `hook_execution` events that only run inside a live turn. A
/// `user_prompt_submit` execution is the strongest proof a newer turn began;
/// `session_start` / `session_end` / `stop` executions are bookkeeping.
pub const GROK_TURN_ACTIVITY_HOOK_EVENTS: &[&str] =
    &["user_prompt_submit", "pre_tool_use", "post_tool_use"];

/// True when `text` is a harness interrupt record.
pub fn is_harness_interrupt_marker_text(text: &str) -> bool {
    text.trim_start()
        .starts_with(HARNESS_INTERRUPT_MARKER_PREFIX)
}

/// Parse `YYYY-MM-DDTHH:MM:SS[.fraction]Z` (UTC) into Unix seconds.
pub fn parse_rfc3339_utc_secs(value: &str) -> Option<u64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let time = time.split_once('.').map_or(time, |(whole, _)| whole);
    let mut time_parts = time.split(':');
    let hour: u64 = time_parts.next()?.parse().ok()?;
    let minute: u64 = time_parts.next()?.parse().ok()?;
    let second: u64 = time_parts.next()?.parse().ok()?;
    if time_parts.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = u64::try_from(days_from_civil(year, month, day)).ok()?;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Format Unix seconds as `YYYY-MM-DDTHH:MM:SS.000Z`, the shape Claude Code
/// writes into transcript `timestamp` fields.
pub fn format_rfc3339_utc_secs(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn record_text_is_interrupt(content: &serde_json::Value) -> bool {
    match content {
        serde_json::Value::String(text) => is_harness_interrupt_marker_text(text),
        serde_json::Value::Array(blocks) => blocks.iter().any(|block| {
            block.get("type").and_then(serde_json::Value::as_str) == Some("text")
                && block
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(is_harness_interrupt_marker_text)
        }),
        _ => false,
    }
}

fn record_timestamp_evidence(record: &serde_json::Value) -> TranscriptTailEvidence {
    record
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .and_then(parse_rfc3339_utc_secs)
        .map_or(TranscriptTailEvidence::Unknown, |at_secs| {
            TranscriptTailEvidence::Interrupted { at_secs }
        })
}

fn record_activity_evidence(record: &serde_json::Value) -> TranscriptTailEvidence {
    TranscriptTailEvidence::Activity {
        at_secs: record
            .get("timestamp")
            .and_then(serde_json::Value::as_str)
            .and_then(parse_rfc3339_utc_secs),
    }
}

/// `#devclear`: what one Codex rollout `event_msg` record proves, or `None`
/// when it is bookkeeping the scan skips.
fn classify_codex_event(record: &serde_json::Value) -> Option<TranscriptTailEvidence> {
    let payload = record.get("payload")?;
    let kind = payload.get("type").and_then(serde_json::Value::as_str)?;
    if kind == CODEX_TURN_ABORTED_EVENT {
        let reason = payload.get("reason").and_then(serde_json::Value::as_str);
        return Some(if reason == Some(CODEX_TURN_ABORTED_INTERRUPTED_REASON) {
            record_timestamp_evidence(record)
        } else {
            TranscriptTailEvidence::Unknown
        });
    }
    CODEX_TURN_ACTIVITY_EVENTS
        .contains(&kind)
        .then(|| record_activity_evidence(record))
}

/// `#ctrlcrerun`: the `params.update` object of a Grok Build `updates.jsonl`
/// record, or `None` for any other transcript shape.
fn grok_session_update(record: &serde_json::Value) -> Option<&serde_json::Value> {
    record.get("method").and_then(serde_json::Value::as_str)?;
    let update = record.get("params")?.get("update")?;
    update
        .get("sessionUpdate")
        .and_then(serde_json::Value::as_str)?;
    Some(update)
}

/// Grok writes `timestamp` as integer Unix seconds.
fn grok_record_secs(record: &serde_json::Value) -> Option<u64> {
    let value = record.get("timestamp")?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(parse_rfc3339_utc_secs))
}

/// `#ctrlcrerun`: what one Grok `updates.jsonl` record proves, or `None` when
/// it is bookkeeping the scan skips.
fn classify_grok_update(
    record: &serde_json::Value,
    update: &serde_json::Value,
) -> Option<TranscriptTailEvidence> {
    let kind = update
        .get("sessionUpdate")
        .and_then(serde_json::Value::as_str)?;
    let activity = || TranscriptTailEvidence::Activity {
        at_secs: grok_record_secs(record),
    };
    if kind == GROK_TURN_COMPLETED_UPDATE {
        let reason = update
            .get("stop_reason")
            .and_then(serde_json::Value::as_str);
        return Some(
            match (
                reason == Some(GROK_TURN_CANCELLED_STOP_REASON),
                grok_record_secs(record),
            ) {
                (true, Some(at_secs)) => TranscriptTailEvidence::Interrupted { at_secs },
                _ => TranscriptTailEvidence::Unknown,
            },
        );
    }
    if kind == "hook_execution" {
        let event = update.get("event_name").and_then(serde_json::Value::as_str);
        return event
            .is_some_and(|event| GROK_TURN_ACTIVITY_HOOK_EVENTS.contains(&event))
            .then(activity);
    }
    GROK_TURN_ACTIVITY_UPDATES.contains(&kind).then(activity)
}

/// Classify the newest conversation record in a transcript tail.
///
/// `tail` is the last bytes of a JSONL transcript; `starts_mid_file` is true
/// when the window does not begin at byte 0, so its first segment may be a
/// fragment and is discarded. Records are scanned newest first. Bookkeeping
/// records (attachments, links, system summaries, queue operations, file
/// history) and subagent sidechain records are skipped; the first `user` or
/// `assistant` record decides. For a Codex rollout (`#devclear`) the first
/// `response_item`, `turn_context`, conversation `event_msg`, or interrupted
/// `turn_aborted` record decides; other `event_msg` records are skipped. For a
/// Grok Build `updates.jsonl` (`#ctrlcrerun`) the first `turn_completed`,
/// conversation update, or in-turn `hook_execution` decides; `turn_completed`
/// is `Interrupted` only for stop reason `cancelled`, else `Unknown`; other
/// updates are skipped. Any unparseable segment met before that record,
/// including a trailing line still being written, yields
/// [`TranscriptTailEvidence::Unknown`]: the probe never reaches past a record
/// it cannot read to an older interrupt.
///
/// `#sdkclearwedge`: a prompt snapshot (`file-history-snapshot` with
/// `isSnapshotUpdate: false`) whose `messageId` names no `user` record in the
/// window, newer than every conversation record and followed only by
/// [`INERT_TRANSCRIPT_RECORD_TYPES`], is an abandoned prompt
/// ([`TranscriptTailEvidence::AbandonedPrompt`]). A snapshot whose prompt
/// record exists (before or after it) is ordinary bookkeeping. Any other
/// record newer than an orphaned snapshot makes the window
/// [`TranscriptTailEvidence::Unknown`], and so does an orphaned snapshot whose
/// preceding conversation never reached a boundary (no `turn_duration` /
/// `stop_hook_summary` system record and no interrupt record between it and
/// the newest conversation record): it may belong to input submitted while a
/// turn was still running.
pub fn classify_transcript_tail(tail: &str, starts_mid_file: bool) -> TranscriptTailEvidence {
    let mut segments: Vec<&str> = tail.split('\n').collect();
    if starts_mid_file && !segments.is_empty() {
        segments.remove(0);
    }
    let prompt_record_ids: std::collections::HashSet<String> = segments
        .iter()
        .filter_map(|segment| serde_json::from_str::<serde_json::Value>(segment.trim()).ok())
        .filter(|record| record.get("type").and_then(serde_json::Value::as_str) == Some("user"))
        .filter_map(|record| {
            record
                .get("uuid")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect();
    // Newest first: `active_after_snapshot` records whether a non-inert record
    // is newer than any orphaned snapshot found later in the scan.
    let mut active_after_snapshot = false;
    let mut abandoned_at: Option<u64> = None;
    let mut disqualified = false;
    let mut previous_turn_closed = false;
    for segment in segments.iter().rev() {
        let line = segment.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            return TranscriptTailEvidence::Unknown;
        };
        if record
            .get("isSidechain")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            continue;
        }
        // Grok Build `updates.jsonl` records (`#ctrlcrerun`) carry no `type`.
        if let Some(update) = grok_session_update(&record) {
            match classify_grok_update(&record, update) {
                Some(evidence) => return evidence,
                None => continue,
            }
        }
        let kind = record.get("type").and_then(serde_json::Value::as_str);
        match kind {
            Some("user") | Some("assistant") if disqualified => {
                return TranscriptTailEvidence::Unknown;
            }
            Some("user") | Some("assistant") if abandoned_at.is_some() => {
                // The conversation the snapshot follows must have reached a
                // boundary: a completed turn (`turn_duration` /
                // `stop_hook_summary`) or an interrupt record. A snapshot
                // newer than a still-open turn proves nothing about it.
                let interrupted = kind == Some("user")
                    && record
                        .get("message")
                        .and_then(|message| message.get("content"))
                        .is_some_and(record_text_is_interrupt);
                return match abandoned_at {
                    Some(at_secs) if previous_turn_closed || interrupted => {
                        TranscriptTailEvidence::AbandonedPrompt { at_secs }
                    }
                    _ => TranscriptTailEvidence::Unknown,
                };
            }
            Some("user") => {
                let content = record
                    .get("message")
                    .and_then(|message| message.get("content"));
                if content.is_some_and(record_text_is_interrupt) {
                    return record_timestamp_evidence(&record);
                }
                return record_activity_evidence(&record);
            }
            Some("assistant") => return record_activity_evidence(&record),
            // Codex rollout records (`#devclear`).
            Some("response_item") | Some("turn_context") => {
                return record_activity_evidence(&record);
            }
            Some("event_msg") => match classify_codex_event(&record) {
                Some(evidence) => return evidence,
                None => continue,
            },
            Some("system")
                if abandoned_at.is_some()
                    && matches!(
                        record.get("subtype").and_then(serde_json::Value::as_str),
                        Some("turn_duration" | "stop_hook_summary")
                    ) =>
            {
                previous_turn_closed = true;
            }
            _ if abandoned_at.is_some() || disqualified => continue,
            Some("file-history-snapshot") => {
                match orphaned_prompt_snapshot_secs(&record, &prompt_record_ids) {
                    Some(_) if active_after_snapshot => disqualified = true,
                    Some(at_secs) => abandoned_at = Some(at_secs),
                    None => {
                        if record
                            .get("isSnapshotUpdate")
                            .and_then(serde_json::Value::as_bool)
                            != Some(false)
                        {
                            active_after_snapshot = true;
                        }
                    }
                }
            }
            Some(kind) if INERT_TRANSCRIPT_RECORD_TYPES.contains(&kind) => continue,
            _ => active_after_snapshot = true,
        }
    }
    match abandoned_at {
        Some(at_secs) if !starts_mid_file && !disqualified => {
            TranscriptTailEvidence::AbandonedPrompt { at_secs }
        }
        _ => TranscriptTailEvidence::Unknown,
    }
}

/// The write time of a prompt snapshot whose prompt record never landed:
/// `isSnapshotUpdate` is explicitly `false`, its `messageId` names no `user`
/// record in the window, and its own timestamp parses. Anything else is `None`.
fn orphaned_prompt_snapshot_secs(
    record: &serde_json::Value,
    prompt_record_ids: &std::collections::HashSet<String>,
) -> Option<u64> {
    if record
        .get("isSnapshotUpdate")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        return None;
    }
    let message_id = record
        .get("messageId")
        .and_then(serde_json::Value::as_str)?;
    if message_id.is_empty() || prompt_record_ids.contains(message_id) {
        return None;
    }
    record
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("timestamp"))
        .and_then(serde_json::Value::as_str)
        .and_then(parse_rfc3339_utc_secs)
}

/// True when the harness transcript proves the turn that wrote `marker` was
/// interrupted and has not been followed by any newer conversation activity.
///
/// `#sdkclearwedge`: an abandoned prompt snapshot (see
/// [`TranscriptTailEvidence::AbandonedPrompt`]) ends the lease under the same
/// rules with [`ABANDONED_PROMPT_SETTLE_SECS`].
///
/// Requires all of: the newest conversation record is an interrupt record; the
/// interrupt is no older than the lease (an interrupt of an earlier turn says
/// nothing about this one); and the interrupt has settled for
/// [`TURN_INTERRUPT_SETTLE_SECS`]. Anything else keeps the lease live, so the
/// probe can only shorten a lease the harness itself already ended.
pub fn turn_lease_ended_by_interrupt(
    marker: &TurnActiveMarker,
    evidence: TranscriptTailEvidence,
    now: u64,
) -> bool {
    match evidence {
        TranscriptTailEvidence::Interrupted { at_secs } => {
            at_secs >= marker.written_at
                && now.saturating_sub(at_secs) >= TURN_INTERRUPT_SETTLE_SECS
        }
        // `#sdkclearwedge`: the snapshot is written after the prompt's
        // `UserPromptSubmit` hooks (which write the lease) finish, so the
        // abandoned prompt's snapshot is never older than its own lease.
        TranscriptTailEvidence::AbandonedPrompt { at_secs } => {
            at_secs >= marker.written_at
                && now.saturating_sub(at_secs) >= ABANDONED_PROMPT_SETTLE_SECS
        }
        TranscriptTailEvidence::Activity { .. } | TranscriptTailEvidence::Unknown => false,
    }
}

/// `#cancelturnprompt`: true when an abandoned prompt snapshot ends `marker`'s
/// turn *without* the [`ABANDONED_PROMPT_SETTLE_SECS`] settle window, because
/// agent-doc itself observed and withdrew the prompt.
///
/// The settle window exists for one ambiguity: a live prompt's record may
/// still land after its snapshot. That ambiguity is gone once the cancelled
/// prompt has been seen back in the composer (Claude Code restores a prompt
/// there only when it abandons it) and agent-doc has removed it. Callers must
/// prove both facts before asking; this rule then only checks that the
/// snapshot belongs to this lease. Interrupt records, activity and unknown
/// tails never qualify here; they keep the ordinary
/// [`turn_lease_ended_by_interrupt`] rules.
pub fn turn_lease_ended_by_withdrawn_prompt(
    marker: &TurnActiveMarker,
    evidence: TranscriptTailEvidence,
) -> bool {
    matches!(
        evidence,
        TranscriptTailEvidence::AbandonedPrompt { at_secs } if at_secs >= marker.written_at
    )
}

/// `#clearctxslow`: seconds until an abandoned prompt snapshot that belongs to
/// `marker`'s turn settles and [`turn_lease_ended_by_interrupt`] retires the
/// lease, or `None` when the tail is not such a snapshot (or it has settled).
///
/// Only reports the wait; it never shortens it. The operator-clear guard uses
/// it to tell the operator that no turn is running yet and when the queued
/// clear will run, instead of claiming a live turn.
pub fn abandoned_prompt_settle_remaining_secs(
    marker: &TurnActiveMarker,
    evidence: TranscriptTailEvidence,
    now: u64,
) -> Option<u64> {
    let TranscriptTailEvidence::AbandonedPrompt { at_secs } = evidence else {
        return None;
    };
    if at_secs < marker.written_at {
        return None;
    }
    let elapsed = now.saturating_sub(at_secs);
    (elapsed < ABANDONED_PROMPT_SETTLE_SECS).then(|| ABANDONED_PROMPT_SETTLE_SECS - elapsed)
}

/// The turn-end receipt reason a lease-retiring transcript boundary proves.
pub fn transcript_turn_end_reason(evidence: TranscriptTailEvidence) -> Option<&'static str> {
    match evidence {
        TranscriptTailEvidence::Interrupted { .. } => Some(TURN_END_REASON_INTERRUPT),
        TranscriptTailEvidence::AbandonedPrompt { .. } => Some(TURN_END_REASON_ABANDONED_PROMPT),
        TranscriptTailEvidence::Activity { .. } | TranscriptTailEvidence::Unknown => None,
    }
}

/// A harness turn remains live while either its original lease is fresh or its
/// bound transcript proves recent activity from that same turn.
///
/// Long-running Codex/Claude turns can exceed [`TURN_ACTIVE_TTL_SECS`] without
/// another lifecycle hook. Treating the original hook timestamp as the only
/// heartbeat abandons the open document cycle underneath a still-running turn.
/// Timestamped transcript activity extends liveness by the same bounded TTL;
/// unknown or untimestamped records never do, so a genuinely orphaned lease
/// still self-expires.
pub fn turn_active_marker_is_live_with_transcript(
    marker: &TurnActiveMarker,
    evidence: TranscriptTailEvidence,
    now: u64,
) -> bool {
    if turn_active_marker_is_fresh(marker, now) {
        return true;
    }
    matches!(
        evidence,
        TranscriptTailEvidence::Activity {
            at_secs: Some(at_secs)
        } if at_secs >= marker.written_at
            && now.saturating_sub(at_secs) < TURN_ACTIVE_TTL_SECS
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#clauderesumeafterclear`: only a `clear`/`startup` `SessionStart` whose
    /// transcript names the reported id is a capture; `resume` and `compact`
    /// keep the same conversation and never move the pointer.
    #[test]
    fn claude_session_start_resume_captures_only_new_conversations() {
        let ss = Some("SessionStart");
        assert_eq!(
            claude_session_start_resume(ss, Some("clear"), Some("e62ea89f"), Some("e62ea89f")),
            Ok(ClaudeSessionStartResume {
                session_id: "e62ea89f".into(),
                source: "clear",
            })
        );
        assert_eq!(
            claude_session_start_resume(ss, Some("startup"), Some(" n1 "), Some("n1"))
                .map(|c| (c.session_id, c.source)),
            Ok(("n1".to_string(), "startup"))
        );
        assert_eq!(
            claude_session_start_resume(ss, Some("resume"), Some("a"), Some("a")),
            Err("source_resume_keeps_id")
        );
        assert_eq!(
            claude_session_start_resume(ss, Some("compact"), Some("a"), Some("a")),
            Err("source_compact_keeps_id")
        );
        assert_eq!(
            claude_session_start_resume(ss, None, Some("a"), Some("a")),
            Err("source_unproven")
        );
        assert_eq!(
            claude_session_start_resume(Some("Stop"), Some("clear"), Some("a"), Some("a")),
            Err("not_session_start")
        );
        assert_eq!(
            claude_session_start_resume(ss, Some("clear"), Some("  "), Some("a")),
            Err("session_id_missing")
        );
        assert_eq!(
            claude_session_start_resume(ss, Some("clear"), Some("a"), None),
            Err("transcript_path_missing")
        );
        assert_eq!(
            claude_session_start_resume(ss, Some("clear"), Some("a"), Some("b")),
            Err("transcript_names_other_session")
        );
    }

    fn chain(entries: &[(u32, &str)]) -> Vec<(u32, String)> {
        entries
            .iter()
            .map(|(pid, comm)| (*pid, comm.to_string()))
            .collect()
    }

    /// `#clauderesumeafterclear`: the hook must come from the supervisor's
    /// direct harness child, through at most wrapper shells.
    #[test]
    fn hook_lineage_proves_the_supervisor_harness_child() {
        // claude (20) is the supervisor's (10) child and ran the hook directly.
        assert_eq!(
            hook_harness_child_of_supervisor(&chain(&[(20, "claude"), (10, "agent-doc")]), 10),
            Ok(20)
        );
        // ... or through an `sh -c` wrapper.
        assert_eq!(
            hook_harness_child_of_supervisor(
                &chain(&[
                    (30, "sh"),
                    (20, "claude"),
                    (10, "agent-doc"),
                    (1, "systemd")
                ]),
                10
            ),
            Ok(20)
        );
        // A nested `claude -p` launched from the main session's Bash tool
        // inherits `$TMUX_PANE`; its hook must never be attributed to the pane.
        assert_eq!(
            hook_harness_child_of_supervisor(
                &chain(&[
                    (40, "claude"),
                    (35, "zsh"),
                    (20, "claude"),
                    (10, "agent-doc")
                ]),
                10
            ),
            Err("hook_not_from_supervisor_harness_child")
        );
        assert_eq!(
            hook_harness_child_of_supervisor(
                &chain(&[
                    (50, "sh"),
                    (40, "claude"),
                    (20, "claude"),
                    (10, "agent-doc")
                ]),
                10
            ),
            Err("hook_not_from_supervisor_harness_child")
        );
        // The supervisor itself is not a harness.
        assert_eq!(
            hook_harness_child_of_supervisor(&chain(&[(10, "agent-doc")]), 10),
            Err("hook_spawned_by_supervisor_not_harness")
        );
        // A different (stale or foreign) supervisor proves nothing.
        assert_eq!(
            hook_harness_child_of_supervisor(&chain(&[(20, "claude"), (11, "agent-doc")]), 10),
            Err("supervisor_not_an_ancestor")
        );
    }

    #[test]
    fn harness_id_is_claude_fails_closed_on_unknown_ids() {
        assert!(harness_id_is_claude("claude-code"));
        assert!(harness_id_is_claude("claude"));
        assert!(!harness_id_is_claude("codex"));
        assert!(!harness_id_is_claude(""));
        assert!(!harness_id_is_claude("grok"));
    }

    #[test]
    fn stale_marker_counts_only_while_its_writer_runs() {
        // The writer is still running (started before it wrote the row).
        assert!(stale_supervisor_marker_holder_is_live(Some(7), Some(100), 200));
        // Start-time rounding slack.
        assert!(stale_supervisor_marker_holder_is_live(Some(7), Some(201), 200));
        // The writer is gone.
        assert!(!stale_supervisor_marker_holder_is_live(Some(7), None, 200));
        // The pid was reused by a process that started after the write.
        assert!(!stale_supervisor_marker_holder_is_live(Some(7), Some(5_000), 200));
        // No writer recorded.
        assert!(!stale_supervisor_marker_holder_is_live(None, Some(100), 200));
    }

    /// `#runctrlclaude`: only boundaries that end the conversation which ran
    /// preflight are receipts. `Stop` can be blocked into a continuation and
    /// `compact` fires mid-turn, so neither may vouch for an orphaned cycle.
    #[test]
    fn only_clear_startup_and_resume_session_starts_are_turn_end_receipts() {
        assert_eq!(
            harness_turn_end_reason(Some("SessionStart"), Some("clear")),
            Some("session_start_clear")
        );
        assert_eq!(
            harness_turn_end_reason(Some("SessionStart"), Some("startup")),
            Some("session_start_startup")
        );
        assert_eq!(
            harness_turn_end_reason(Some("SessionStart"), Some("resume")),
            Some("session_start_resume")
        );
        for (event, source) in [
            (Some("SessionStart"), Some("compact")),
            (Some("SessionStart"), None),
            (Some("Stop"), None),
            (Some("Stop"), Some("clear")),
            (Some("SubagentStop"), None),
            (None, Some("clear")),
            (None, None),
        ] {
            assert_eq!(
                harness_turn_end_reason(event, source),
                None,
                "{event:?}/{source:?}"
            );
        }
    }

    #[test]
    fn adoption_refresh_preserves_active_idle_and_custom_titles() {
        for title in ["", TURN_ACTIVE_PANE_TITLE, "custom title"] {
            let stale = pane_title_with_freshness(title, None, true);
            assert_eq!(pane_title_with_freshness(&stale, None, true), stale);
            assert_eq!(pane_title_with_freshness(&stale, None, false), title);
            assert_eq!(pane_title_with_freshness(title, None, false), title);
        }
    }

    #[test]
    fn pane_title_active_names_turn_in_progress() {
        assert_eq!(pane_title_for_state(None, true), TURN_ACTIVE_PANE_TITLE);
        assert!(pane_title_for_state(None, true).contains("turn in progress"));
    }

    #[test]
    fn pane_title_idle_without_registration_clears_to_default() {
        assert_eq!(pane_title_for_state(None, false), "");
    }

    #[test]
    fn document_name_is_present_in_every_status_state() {
        for active in [false, true] {
            for stale in [false, true] {
                let title = pane_title_for_status(Some("sample-session.md"), active, stale);
                assert!(title.contains("sample-session.md"), "{title}");
                assert_eq!(
                    pane_title_status_marker_count(&title),
                    usize::from(active || stale)
                );
            }
        }
    }

    #[test]
    fn turn_active_marker_self_expires_after_ttl() {
        let marker = TurnActiveMarker {
            pane: "%7".to_string(),
            written_at: 1000,
        };
        assert!(turn_active_marker_is_fresh(
            &marker,
            1000 + TURN_ACTIVE_TTL_SECS - 1
        ));
        assert!(!turn_active_marker_is_fresh(
            &marker,
            1000 + TURN_ACTIVE_TTL_SECS
        ));
    }

    #[test]
    fn recent_bound_transcript_activity_keeps_a_long_turn_live() {
        let marker = TurnActiveMarker {
            pane: "%7".to_string(),
            written_at: 1_000,
        };
        let now = marker.written_at + TURN_ACTIVE_TTL_SECS + 30;
        assert!(turn_active_marker_is_live_with_transcript(
            &marker,
            TranscriptTailEvidence::Activity {
                at_secs: Some(now - 1),
            },
            now,
        ));
        assert!(!turn_active_marker_is_live_with_transcript(
            &marker,
            TranscriptTailEvidence::Activity {
                at_secs: Some(now - TURN_ACTIVE_TTL_SECS),
            },
            now,
        ));
        assert!(!turn_active_marker_is_live_with_transcript(
            &marker,
            TranscriptTailEvidence::Activity {
                at_secs: Some(marker.written_at - 1),
            },
            now,
        ));
        assert!(!turn_active_marker_is_live_with_transcript(
            &marker,
            TranscriptTailEvidence::Activity { at_secs: None },
            now,
        ));
        assert!(!turn_active_marker_is_live_with_transcript(
            &marker,
            TranscriptTailEvidence::Interrupted { at_secs: now - 1 },
            now,
        ));
    }

    #[test]
    fn turn_active_expiry_cutoff_agrees_with_freshness() {
        assert_eq!(turn_active_expiry_cutoff(TURN_ACTIVE_TTL_SECS - 1), None);
        let now = 1000 + TURN_ACTIVE_TTL_SECS;
        let cutoff = turn_active_expiry_cutoff(now).unwrap();
        let at_cutoff = TurnActiveMarker {
            pane: "%7".to_string(),
            written_at: cutoff,
        };
        let after_cutoff = TurnActiveMarker {
            pane: "%7".to_string(),
            written_at: cutoff + 1,
        };
        assert!(!turn_active_marker_is_fresh(&at_cutoff, now));
        assert!(turn_active_marker_is_fresh(&after_cutoff, now));
    }

    #[test]
    fn turn_active_marker_matches_only_marker_pane() {
        let marker = TurnActiveMarker {
            pane: "%7".to_string(),
            written_at: 1000,
        };
        assert!(turn_active_marker_matches_pane(&marker, "%7"));
        assert!(!turn_active_marker_matches_pane(&marker, "%8"));
    }

    #[test]
    fn pane_title_active_stale_leads_with_warning() {
        let title = pane_title_for_status(Some("sample-session.md"), true, true);
        assert!(
            title.contains(STALE_SUPERVISOR_PANE_MARKER),
            "stale active title must contain the warning: {title}"
        );
        assert!(
            title.contains("turn in progress"),
            "stale active title must keep the turn-in-progress text: {title}"
        );
        assert!(
            title.starts_with(STALE_SUPERVISOR_PANE_MARKER),
            "warning must lead the title: {title}"
        );
    }

    #[test]
    fn gh124_stale_busy_title_holds_exactly_one_marker() {
        // A pane already carrying the stale marker that goes busy must not weld
        // the busy marker on (`⚠ STALE SUPERVISOR ⟳ agent-doc: turn in progress`).
        let busy_on_stale = pane_title_for_status(Some("sample-session.md"), true, true);
        assert_eq!(
            pane_title_status_marker_count(&busy_on_stale),
            1,
            "{busy_on_stale}"
        );
        assert!(
            !busy_on_stale.contains(TURN_ACTIVE_PANE_TITLE),
            "{busy_on_stale}"
        );
        let refreshed = pane_title_with_freshness(
            STALE_SUPERVISOR_PANE_MARKER,
            Some("sample-session.md"),
            true,
        );
        assert_eq!(pane_title_status_marker_count(&refreshed), 1, "{refreshed}");
        for active in [true, false] {
            for stale in [true, false] {
                let title = pane_title_for_status(Some("sample-session.md"), active, stale);
                assert!(pane_title_status_marker_count(&title) <= 1, "{title}");
            }
        }
    }

    #[test]
    fn gh124_legacy_welded_title_normalises_to_one_marker() {
        let welded = format!("{STALE_SUPERVISOR_PANE_MARKER} {TURN_ACTIVE_PANE_TITLE}");
        assert_eq!(undecorated_pane_title(&welded), TURN_ACTIVE_PANE_TITLE);
        let stale = pane_title_with_freshness(&welded, None, true);
        assert_eq!(stale, STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE);
        assert_eq!(pane_title_status_marker_count(&stale), 1);
        assert_eq!(
            pane_title_with_freshness(&welded, None, false),
            TURN_ACTIVE_PANE_TITLE
        );
        assert_eq!(
            pane_title_with_freshness(STALE_SUPERVISOR_TURN_ACTIVE_PANE_TITLE, None, false),
            TURN_ACTIVE_PANE_TITLE
        );
    }

    #[test]
    fn pane_title_active_fresh_has_no_warning() {
        let title = pane_title_for_status(Some("sample-session.md"), true, false);
        assert_eq!(title, "⟳ sample-session.md — turn in progress");
        assert!(!title.contains(STALE_SUPERVISOR_PANE_MARKER));
    }

    #[test]
    fn pane_title_idle_stale_still_warns() {
        assert_eq!(
            pane_title_for_status(Some("sample-session.md"), false, true),
            "⚠ STALE SUPERVISOR — sample-session.md"
        );
    }

    #[test]
    fn pane_title_idle_fresh_keeps_document_name() {
        assert_eq!(
            pane_title_for_status(Some("sample-session.md"), false, false),
            "sample-session.md"
        );
    }

    #[test]
    fn freshness_converges_legacy_titles_to_registered_document_name() {
        assert_eq!(
            pane_title_with_freshness(TURN_ACTIVE_PANE_TITLE, Some("sample-session.md"), true,),
            "⚠ STALE SUPERVISOR — sample-session.md — turn in progress"
        );
        assert_eq!(
            pane_title_with_freshness("", Some("sample-session.md"), false),
            "sample-session.md"
        );
    }

    fn interrupt_line(at: u64, text: &str) -> String {
        serde_json::json!({
            "type": "user",
            "isSidechain": false,
            "message": {"role": "user", "content": [{"type": "text", "text": text}]},
            "timestamp": format_rfc3339_utc_secs(at),
        })
        .to_string()
    }

    fn tool_result_line(at: u64) -> String {
        serde_json::json!({
            "type": "user",
            "isSidechain": false,
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": "ok"}]},
            "timestamp": format_rfc3339_utc_secs(at),
        })
        .to_string()
    }

    fn assistant_line(at: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "working"}]},
            "timestamp": format_rfc3339_utc_secs(at),
        })
        .to_string()
    }

    #[test]
    fn rfc3339_round_trips_and_matches_the_observed_interrupt() {
        // contracts.md pane %161 interrupt record, 2026-10-09.
        let at = parse_rfc3339_utc_secs("2026-10-09T22:11:18.947Z").unwrap();
        assert_eq!(at, 1_791_583_878);
        assert_eq!(format_rfc3339_utc_secs(at), "2026-10-09T22:11:18.000Z");
        for secs in [0, 951_782_400, 1_709_164_800, 1_791_583_878, 4_102_444_800] {
            assert_eq!(
                parse_rfc3339_utc_secs(&format_rfc3339_utc_secs(secs)),
                Some(secs)
            );
        }
        for bad in [
            "",
            "2026-10-09 22:11:18Z",
            "2026-13-09T22:11:18Z",
            "2026-10-09T22:11:18",
        ] {
            assert_eq!(parse_rfc3339_utc_secs(bad), None, "{bad}");
        }
    }

    #[test]
    fn transcript_tail_reports_interrupt_only_when_it_is_the_newest_conversation_record() {
        let tail = [
            tool_result_line(100),
            interrupt_line(101, "[Request interrupted by user for tool use]"),
            r#"{"type":"attachment"}"#.to_string(),
            r#"{"type":"pr-link"}"#.to_string(),
            String::new(),
        ]
        .join("\n");
        assert_eq!(
            classify_transcript_tail(&tail, false),
            TranscriptTailEvidence::Interrupted { at_secs: 101 }
        );
        let bare = interrupt_line(7, "[Request interrupted by user]");
        assert_eq!(
            classify_transcript_tail(&bare, false),
            TranscriptTailEvidence::Interrupted { at_secs: 7 }
        );

        // A newer prompt, assistant message, or tool result supersedes it.
        for newer in [assistant_line(102), tool_result_line(102)] {
            let tail = format!(
                "{}\n{newer}\n",
                interrupt_line(101, "[Request interrupted by user]")
            );
            assert_eq!(
                classify_transcript_tail(&tail, false),
                TranscriptTailEvidence::Activity { at_secs: Some(102) }
            );
        }
        // A subagent sidechain record after the interrupt is not main-turn activity.
        let sidechain = r#"{"type":"assistant","isSidechain":true}"#;
        let tail = format!(
            "{}\n{sidechain}\n",
            interrupt_line(101, "[Request interrupted by user]")
        );
        assert_eq!(
            classify_transcript_tail(&tail, false),
            TranscriptTailEvidence::Interrupted { at_secs: 101 }
        );
    }

    #[test]
    fn transcript_tail_never_reaches_past_an_unreadable_record() {
        let interrupt = interrupt_line(101, "[Request interrupted by user]");
        // A trailing record still being written.
        let tail = format!("{interrupt}\n{{\"type\":\"user\",\"mess");
        assert_eq!(
            classify_transcript_tail(&tail, false),
            TranscriptTailEvidence::Unknown
        );
        // A window that starts mid-file discards its leading fragment.
        let tail = format!("pe\":\"assistant\"}}\n{interrupt}\n");
        assert_eq!(
            classify_transcript_tail(&tail, true),
            TranscriptTailEvidence::Interrupted { at_secs: 101 }
        );
        // A record longer than the window leaves only its fragment: nothing provable.
        assert_eq!(
            classify_transcript_tail("tail of a huge tool result\"}\n", true),
            TranscriptTailEvidence::Unknown
        );
        // A Codex abort without a timestamp or an interrupt reason proves nothing.
        for codex in [
            r#"{"type":"event_msg","payload":{"type":"turn_aborted","reason":"interrupted"}}"#,
            r#"{"timestamp":"2026-10-11T00:10:28.372Z","type":"event_msg","payload":{"type":"turn_aborted"}}"#,
            r#"{"timestamp":"2026-10-11T00:10:28.372Z","type":"event_msg","payload":{"type":"turn_aborted","reason":"replaced"}}"#,
        ] {
            assert_eq!(
                classify_transcript_tail(codex, false),
                TranscriptTailEvidence::Unknown,
                "{codex}"
            );
        }
        assert_eq!(
            classify_transcript_tail("", false),
            TranscriptTailEvidence::Unknown
        );
    }

    fn codex_line(at: u64, kind: &str, payload: serde_json::Value) -> String {
        serde_json::json!({
            "timestamp": format_rfc3339_utc_secs(at),
            "type": kind,
            "payload": payload,
        })
        .to_string()
    }

    /// A Claude Code prompt snapshot record (`file-history-snapshot`).
    fn prompt_snapshot_line(message_id: &str, at: &str, update: bool) -> String {
        serde_json::json!({
            "type": "file-history-snapshot",
            "messageId": message_id,
            "snapshot": {"messageId": message_id, "trackedFileBackups": {}, "timestamp": at},
            "isSnapshotUpdate": update,
        })
        .to_string()
    }

    fn codex_event(at: u64, event: &str) -> String {
        codex_line(at, "event_msg", serde_json::json!({"type": event}))
    }

    fn codex_aborted(at: u64) -> String {
        codex_line(
            at,
            "event_msg",
            serde_json::json!({
                "type": "turn_aborted", "turn_id": "t1", "reason": "interrupted",
                "started_at": at - 2, "completed_at": at, "duration_ms": 1711,
            }),
        )
    }

    /// `#devclear` (live 2026-10-11, haiven-dev dev.md pane %127): the tail of
    /// the Codex rollout after the operator interrupted a dispatched turn two
    /// seconds in. The interrupt is the newest decisive record.
    #[test]
    fn codex_rollout_tail_reports_an_interrupted_turn() {
        let tail = [
            codex_line(
                1_791_677_426,
                "event_msg",
                serde_json::json!({"type": "thread_settings_applied"}),
            ),
            codex_event(1_791_677_426, "task_started"),
            codex_line(
                1_791_677_426,
                "response_item",
                serde_json::json!({"type": "message", "role": "user", "content": []}),
            ),
            codex_line(1_791_677_426, "world_state", serde_json::json!({})),
            codex_line(1_791_677_426, "turn_context", serde_json::json!({})),
            codex_line(
                1_791_677_428,
                "response_item",
                serde_json::json!({"type": "message", "role": "developer",
                    "content": [{"type": "input_text", "text": "<turn_aborted>\n..."}]}),
            ),
            codex_aborted(1_791_677_428),
            String::new(),
        ]
        .join("\n");
        assert_eq!(
            classify_transcript_tail(&tail, false),
            TranscriptTailEvidence::Interrupted {
                at_secs: 1_791_677_428
            }
        );
        // Bookkeeping and background completions after the abort are skipped.
        for late in [
            codex_event(1_791_677_429, "token_count"),
            codex_event(1_791_677_429, "exec_command_end"),
            codex_event(1_791_677_429, "mcp_tool_call_end"),
            codex_event(1_791_677_429, "item_completed"),
            codex_event(1_791_677_429, "thread_goal_updated"),
            codex_line(1_791_677_429, "token_usage_record", serde_json::json!({})),
        ] {
            let tail = format!("{tail}{late}\n");
            assert_eq!(
                classify_transcript_tail(&tail, false),
                TranscriptTailEvidence::Interrupted {
                    at_secs: 1_791_677_428
                },
                "{late}"
            );
        }
    }

    /// `#devclear`: any record of a newer Codex turn supersedes the abort.
    #[test]
    fn codex_rollout_newer_turn_activity_supersedes_the_abort() {
        let aborted = codex_aborted(100);
        for newer in [
            codex_event(101, "task_started"),
            codex_event(101, "user_message"),
            codex_event(101, "agent_message"),
            codex_event(101, "task_complete"),
            codex_line(101, "turn_context", serde_json::json!({})),
            codex_line(
                101,
                "response_item",
                serde_json::json!({"type": "function_call", "name": "exec_command"}),
            ),
        ] {
            let tail = format!("{aborted}\n{newer}\n");
            assert_eq!(
                classify_transcript_tail(&tail, false),
                TranscriptTailEvidence::Activity { at_secs: Some(101) },
                "{newer}"
            );
        }
        // A live Codex turn with no abort anywhere is activity, not unknown.
        let live = format!(
            "{}\n{}\n",
            codex_event(100, "task_started"),
            codex_event(101, "token_count")
        );
        assert_eq!(
            classify_transcript_tail(&live, false),
            TranscriptTailEvidence::Activity { at_secs: Some(100) }
        );
    }

    fn grok_update(at: u64, method: &str, update: serde_json::Value) -> String {
        serde_json::json!({
            "timestamp": at,
            "method": method,
            "params": {"sessionId": "01a180e1-d079-7520-be90-2be28053e294", "update": update},
        })
        .to_string()
    }

    fn grok_hook(at: u64, event: &str) -> String {
        grok_update(
            at,
            "_x.ai/session/update",
            serde_json::json!({"sessionUpdate": "hook_execution", "event_name": event, "runs": []}),
        )
    }

    fn grok_chunk(at: u64, kind: &str) -> String {
        grok_update(
            at,
            "session/update",
            serde_json::json!({"sessionUpdate": kind, "content": {"type": "text", "text": "x"}}),
        )
    }

    fn grok_turn_completed(at: u64, stop_reason: &str) -> String {
        grok_update(
            at,
            "_x.ai/session/update",
            serde_json::json!({"sessionUpdate": "turn_completed", "prompt_id": "dfd1eb06",
                "stop_reason": stop_reason, "elapsed_ms": 9169}),
        )
    }

    /// `#ctrlcrerun` (live 2026-10-28, haiven-dev game-sessions.md pane %16):
    /// Run Agent Doc auto-triggered Grok at 05:00:55; the operator pressed
    /// Ctrl+C at 05:00:57 (`turn_ended outcome=cancelled trigger=ctrl_c`) and
    /// Grok ran no `stop` hook. Record shapes are trimmed copies of session
    /// `01a180e1-…/updates.jsonl`.
    fn grok_ctrl_c_incident_tail() -> String {
        [
            grok_hook(1_793_163_647, "session_start"),
            grok_hook(1_793_163_655, "user_prompt_submit"),
            grok_chunk(1_793_163_655, "user_message_chunk"),
            grok_turn_completed(1_793_163_657, "cancelled"),
            String::new(),
        ]
        .join("\n")
    }

    #[test]
    fn grok_updates_tail_reports_a_ctrl_c_cancelled_turn() {
        assert_eq!(
            classify_transcript_tail(&grok_ctrl_c_incident_tail(), false),
            TranscriptTailEvidence::Interrupted {
                at_secs: 1_793_163_657
            }
        );
        // Bookkeeping after the cancel is skipped.
        for late in [
            grok_update(
                1_793_163_658,
                "_x.ai/session/update",
                serde_json::json!({"sessionUpdate": "background_tasks", "tasks": []}),
            ),
            grok_update(
                1_793_163_658,
                "_x.ai/session/update",
                serde_json::json!({"sessionUpdate": "subagent_finished", "subagent_id": "s"}),
            ),
            grok_update(
                1_793_163_658,
                "_x.ai/session/update",
                serde_json::json!({"sessionUpdate": "memory_dream_queued"}),
            ),
            grok_hook(1_793_163_658, "session_end"),
        ] {
            let tail = format!("{}{late}\n", grok_ctrl_c_incident_tail());
            assert_eq!(
                classify_transcript_tail(&tail, false),
                TranscriptTailEvidence::Interrupted {
                    at_secs: 1_793_163_657
                },
                "{late}"
            );
        }
    }

    /// `#ctrlcrerun`: the operator's next prompt (05:01:15 in the incident)
    /// supersedes the cancel, as does any other in-turn record.
    #[test]
    fn grok_updates_newer_turn_activity_supersedes_the_cancel() {
        let cancelled = grok_ctrl_c_incident_tail();
        for newer in [
            grok_hook(1_793_163_683, "user_prompt_submit"),
            grok_hook(1_793_163_683, "pre_tool_use"),
            grok_chunk(1_793_163_683, "user_message_chunk"),
            grok_chunk(1_793_163_683, "agent_thought_chunk"),
            grok_chunk(1_793_163_683, "agent_message_chunk"),
            grok_update(
                1_793_163_683,
                "session/update",
                serde_json::json!({"sessionUpdate": "tool_call", "toolCallId": "c", "title": "read_file"}),
            ),
        ] {
            let tail = format!("{cancelled}{newer}\n");
            assert_eq!(
                classify_transcript_tail(&tail, false),
                TranscriptTailEvidence::Activity {
                    at_secs: Some(1_793_163_683)
                },
                "{newer}"
            );
        }
    }

    /// `#ctrlcrerun`: only a `cancelled` turn end is an interrupt. A turn that
    /// completed any other way runs the `stop` hook, so its record proves
    /// nothing; an untimestamped cancel proves nothing either.
    #[test]
    fn grok_updates_only_a_timestamped_cancel_is_an_interrupt() {
        for stop_reason in ["end_turn", "error", "rate_limit", "interrupted"] {
            let tail = format!(
                "{}\n{}\n",
                grok_chunk(100, "agent_message_chunk"),
                grok_turn_completed(101, stop_reason)
            );
            assert_eq!(
                classify_transcript_tail(&tail, false),
                TranscriptTailEvidence::Unknown,
                "{stop_reason}"
            );
        }
        let untimestamped = serde_json::json!({
            "method": "_x.ai/session/update",
            "params": {"update": {"sessionUpdate": "turn_completed", "stop_reason": "cancelled"}},
        })
        .to_string();
        assert_eq!(
            classify_transcript_tail(&format!("{untimestamped}\n"), false),
            TranscriptTailEvidence::Unknown
        );
        // A live Grok turn with no turn end is activity.
        let live = format!(
            "{}\n{}\n",
            grok_chunk(100, "agent_thought_chunk"),
            grok_hook(101, "session_end")
        );
        assert_eq!(
            classify_transcript_tail(&live, false),
            TranscriptTailEvidence::Activity { at_secs: Some(100) }
        );
    }

    fn prompt_line(uuid: &str, at: &str, text: &str) -> String {
        serde_json::json!({
            "type": "user", "isSidechain": false, "uuid": uuid, "timestamp": at,
            "message": {"role": "user", "content": text},
        })
        .to_string()
    }

    /// `#sdkclearwedge` incident replay: haiven-dev `tasks/sdk.md`, pane %180,
    /// 2026-10-11. The previous turn ended at 04:26:44; the dispatched prompt's
    /// `UserPromptSubmit` hook wrote the lease at 04:30:21 and Claude Code wrote
    /// its snapshot at 04:30:23; the operator cancelled before the prompt
    /// record landed. Record shapes are trimmed copies of transcript
    /// `a07856e8-…`.
    fn sdk_incident_tail() -> String {
        [
            r#"{"type":"assistant","isSidechain":false,"uuid":"9c71d297","timestamp":"2026-10-11T04:26:44.284Z","message":{"role":"assistant","content":[{"type":"text","text":"The document write didn't complete."}]}}"#.to_string(),
            r#"{"type":"system","subtype":"stop_hook_summary","isSidechain":false,"timestamp":"2026-10-11T04:26:44.833Z"}"#.to_string(),
            r#"{"type":"system","subtype":"turn_duration","isSidechain":false,"durationMs":521371,"timestamp":"2026-10-11T04:26:44.837Z"}"#.to_string(),
            prompt_snapshot_line("7287902a-3e0f-4b9f-928e-496baf4cf009", "2026-10-11T04:30:23.234Z", false),
            String::new(),
        ]
        .join("\n")
    }

    #[test]
    fn sdkclearwedge_incident_tail_is_an_abandoned_prompt() {
        let at = parse_rfc3339_utc_secs("2026-10-11T04:30:23.234Z").unwrap();
        assert_eq!(
            classify_transcript_tail(&sdk_incident_tail(), false),
            TranscriptTailEvidence::AbandonedPrompt { at_secs: at }
        );
        // Inert bookkeeping and background-subagent sidechain records after
        // the snapshot do not disprove the abandonment.
        let tail = format!(
            "{}{}\n{}\n{}\n",
            sdk_incident_tail(),
            r#"{"type":"last-prompt","lastPrompt":"/agent-doc tasks/sdk.md"}"#,
            r#"{"type":"cost-state"}"#,
            r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-10-11T04:31:00.000Z"}"#,
        );
        assert_eq!(
            classify_transcript_tail(&tail, false),
            TranscriptTailEvidence::AbandonedPrompt { at_secs: at }
        );
    }

    #[test]
    fn sdkclearwedge_live_prompt_snapshots_are_never_abandoned() {
        let previous = assistant_line(100);
        // Snapshot written in the same flush as its prompt record (both orders
        // Claude Code produces), then the model thinks with no new record.
        for tail in [
            [
                previous.clone(),
                prompt_snapshot_line("p1", "1970-01-01T00:01:50.000Z", false),
                prompt_line("p1", "1970-01-01T00:01:20.000Z", "/agent-doc tasks/sdk.md"),
            ]
            .join("\n"),
            [
                previous.clone(),
                prompt_line(
                    "p1",
                    "1970-01-01T00:01:20.000Z",
                    "<command-message>agent-doc</command-message>",
                ),
                prompt_line(
                    "p2",
                    "1970-01-01T00:01:20.000Z",
                    "Base directory for this skill",
                ),
                r#"{"type":"attachment"}"#.to_string(),
                prompt_snapshot_line("p1", "1970-01-01T00:01:50.000Z", false),
            ]
            .join("\n"),
        ] {
            assert_eq!(
                classify_transcript_tail(&tail, false),
                TranscriptTailEvidence::Activity { at_secs: Some(80) },
                "{tail}"
            );
        }
        // Anything but inert bookkeeping after an orphaned snapshot keeps the
        // lease: an attachment, a queue operation (a queued/dequeued input may
        // start a turn), a snapshot update (tool activity), a system record.
        for newer in [
            r#"{"type":"attachment","timestamp":"2026-10-11T04:30:24.000Z"}"#,
            r#"{"type":"queue-operation","operation":"dequeue","timestamp":"2026-10-11T04:30:30.000Z"}"#,
            r#"{"type":"system","subtype":"stop_hook_summary"}"#,
            r#"{"type":"some-future-record"}"#,
        ] {
            let tail = format!("{}{newer}\n", sdk_incident_tail());
            assert_eq!(
                classify_transcript_tail(&tail, false),
                TranscriptTailEvidence::Unknown,
                "{newer}"
            );
        }
        let update = prompt_snapshot_line("x", "2026-10-11T04:30:40.000Z", true);
        let tail = format!("{}{update}\n", sdk_incident_tail());
        assert_eq!(
            classify_transcript_tail(&tail, false),
            TranscriptTailEvidence::Unknown
        );
        // A prompt record (even a different, newer prompt) after the orphan
        // snapshot is conversation activity.
        let tail = format!(
            "{}{}\n",
            sdk_incident_tail(),
            prompt_line("p9", "2026-10-11T04:30:25.000Z", "/agent-doc tasks/sdk.md")
        );
        assert!(matches!(
            classify_transcript_tail(&tail, false),
            TranscriptTailEvidence::Activity { at_secs: Some(_) }
        ));
        // An orphaned snapshot after a turn that never reached a boundary
        // (input submitted while the turn runs) proves nothing; after an
        // interrupt record it is abandoned like after a completed turn.
        let open_turn = [
            prompt_line("p0", "1970-01-01T00:01:00.000Z", "do the thing"),
            assistant_line(70),
            prompt_snapshot_line("queued", "1970-01-01T00:01:30.000Z", false),
        ]
        .join("\n");
        assert_eq!(
            classify_transcript_tail(&open_turn, false),
            TranscriptTailEvidence::Unknown
        );
        let after_interrupt = [
            interrupt_line(70, "[Request interrupted by user]"),
            prompt_snapshot_line("p1", "1970-01-01T00:01:30.000Z", false),
        ]
        .join("\n");
        assert_eq!(
            classify_transcript_tail(&after_interrupt, false),
            TranscriptTailEvidence::AbandonedPrompt { at_secs: 90 }
        );
        // A window starting mid-file with no conversation record proves nothing;
        // the same snapshot as the whole (fresh) transcript is abandoned.
        let only = prompt_snapshot_line("p1", "1970-01-01T00:01:40.000Z", false);
        assert_eq!(
            classify_transcript_tail(&format!("frag\n{only}\n"), true),
            TranscriptTailEvidence::Unknown
        );
        assert_eq!(
            classify_transcript_tail(&format!("{only}\n"), false),
            TranscriptTailEvidence::AbandonedPrompt { at_secs: 100 }
        );
    }

    #[test]
    fn sdkclearwedge_abandoned_prompt_retires_only_its_own_lease_after_settling() {
        let at = parse_rfc3339_utc_secs("2026-10-11T04:30:23.234Z").unwrap();
        let lease = TurnActiveMarker {
            pane: "%180".to_string(),
            written_at: parse_rfc3339_utc_secs("2026-10-11T04:30:21Z").unwrap(),
        };
        let evidence = TranscriptTailEvidence::AbandonedPrompt { at_secs: at };
        assert!(!turn_lease_ended_by_interrupt(
            &lease,
            evidence,
            at + ABANDONED_PROMPT_SETTLE_SECS - 1
        ));
        assert!(turn_lease_ended_by_interrupt(
            &lease,
            evidence,
            at + ABANDONED_PROMPT_SETTLE_SECS
        ));
        // A lease written after the snapshot belongs to a newer prompt.
        let newer = TurnActiveMarker {
            pane: "%180".to_string(),
            written_at: at + 1,
        };
        assert!(!turn_lease_ended_by_interrupt(&newer, evidence, at + 3_000));
        assert_eq!(
            transcript_turn_end_reason(evidence),
            Some(TURN_END_REASON_ABANDONED_PROMPT)
        );
        assert_eq!(
            transcript_turn_end_reason(TranscriptTailEvidence::Activity { at_secs: Some(at) }),
            None
        );
    }

    /// `#clearctxslow` incident replay: haiven-dev `tasks/sdk.md`, pane %180,
    /// 2026-10-15. Run Agent Doc's lease was written at 14:49:04, the prompt's
    /// snapshot landed at 14:49:06.904 with no prompt record, and Clear
    /// Session Context was pressed at 14:49:10 and 14:49:16. The lease retired
    /// at 14:49:26 (`turn_status_projection_repaired
    /// reason=harness_prompt_abandoned`). The settle window is unchanged; the
    /// guard only reports how long is left.
    #[test]
    fn clearctxslow_reports_the_remaining_abandoned_prompt_settle() {
        let lease = TurnActiveMarker {
            pane: "%180".to_string(),
            written_at: parse_rfc3339_utc_secs("2026-10-15T14:49:04Z").unwrap(),
        };
        let snapshot = parse_rfc3339_utc_secs("2026-10-15T14:49:06.904Z").unwrap();
        let evidence = TranscriptTailEvidence::AbandonedPrompt { at_secs: snapshot };
        let first_press = parse_rfc3339_utc_secs("2026-10-15T14:49:10Z").unwrap();
        let second_press = parse_rfc3339_utc_secs("2026-10-15T14:49:16Z").unwrap();
        assert_eq!(
            abandoned_prompt_settle_remaining_secs(&lease, evidence, first_press),
            Some(16)
        );
        assert_eq!(
            abandoned_prompt_settle_remaining_secs(&lease, evidence, second_press),
            Some(10)
        );
        // Settled: the lease retires instead, so there is nothing to wait for.
        let settled = snapshot + ABANDONED_PROMPT_SETTLE_SECS;
        assert_eq!(
            abandoned_prompt_settle_remaining_secs(&lease, evidence, settled),
            None
        );
        assert!(turn_lease_ended_by_interrupt(&lease, evidence, settled));
        assert!(!turn_lease_ended_by_interrupt(&lease, evidence, second_press));
        // A snapshot older than the lease belongs to an earlier prompt, and
        // any other tail proves no abandoned prompt.
        let newer = TurnActiveMarker {
            pane: "%180".to_string(),
            written_at: snapshot + 1,
        };
        assert_eq!(
            abandoned_prompt_settle_remaining_secs(&newer, evidence, first_press),
            None
        );
        for other in [
            TranscriptTailEvidence::Activity {
                at_secs: Some(snapshot),
            },
            TranscriptTailEvidence::Unknown,
            TranscriptTailEvidence::Interrupted { at_secs: snapshot },
        ] {
            assert_eq!(
                abandoned_prompt_settle_remaining_secs(&lease, other, first_press),
                None
            );
        }
    }

    /// `#cancelturnprompt` incident replay: agent-loop
    /// `tasks/agent-doc/agent-doc-bugs.md`, pane %149, 2026-10-11. The previous
    /// turn ended at 05:49:07. Run Agent Doc typed the trigger at 05:59:23 and
    /// Cancel Turn sent `C-c` at 05:59:30 while the `UserPromptSubmit`
    /// preflight hook was still running (snapshot 05:59:30.674, no prompt
    /// record). The operator ran again at 06:00:09 and cancelled again at
    /// 06:00:16 (snapshot 06:00:16.961, no prompt record). Trimmed copies of
    /// transcript `ba846c4c-…` lines 2836-2846.
    fn cancelturnprompt_incident_tail() -> String {
        [
            r#"{"type":"assistant","isSidechain":false,"uuid":"7dfb2cb3","timestamp":"2026-10-11T05:49:06.100Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#.to_string(),
            r#"{"type":"attachment","isSidechain":false,"attachment":{"type":"hook_success","hookName":"Stop"},"timestamp":"2026-10-11T05:49:07.424Z"}"#.to_string(),
            r#"{"type":"system","subtype":"stop_hook_summary","isSidechain":false,"timestamp":"2026-10-11T05:49:07.430Z"}"#.to_string(),
            r#"{"type":"system","subtype":"turn_duration","isSidechain":false,"durationMs":13593,"timestamp":"2026-10-11T05:49:07.433Z"}"#.to_string(),
            r#"{"type":"last-prompt","lastPrompt":"/agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md"}"#.to_string(),
            r#"{"type":"mode","mode":"normal"}"#.to_string(),
            r#"{"type":"permission-mode","permissionMode":"bypassPermissions"}"#.to_string(),
            r#"{"type":"atis-latch","atis":""}"#.to_string(),
            r#"{"type":"pr-link","prNumber":226,"timestamp":"2026-10-11T05:49:07.556Z"}"#.to_string(),
            prompt_snapshot_line("8f4af13e-f86f-454d-ac6c-06a2a0440680", "2026-10-11T05:59:30.674Z", false),
            prompt_snapshot_line("8b8cf978-ffc2-4e80-99ad-809591c714a0", "2026-10-11T06:00:16.961Z", false),
            String::new(),
        ]
        .join("\n")
    }

    #[test]
    fn cancelturnprompt_back_to_back_cancelled_prompts_are_abandoned_at_the_newest_snapshot() {
        let first = parse_rfc3339_utc_secs("2026-10-11T05:59:30.674Z").unwrap();
        let second = parse_rfc3339_utc_secs("2026-10-11T06:00:16.961Z").unwrap();
        let tail = cancelturnprompt_incident_tail();
        assert_eq!(
            classify_transcript_tail(&tail, false),
            TranscriptTailEvidence::AbandonedPrompt { at_secs: second }
        );
        // After only the first cancel the same tail names the first snapshot.
        let first_only = tail
            .lines()
            .filter(|line| !line.contains("8b8cf978"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            classify_transcript_tail(&first_only, false),
            TranscriptTailEvidence::AbandonedPrompt { at_secs: first }
        );
        // The queued trigger Claude Code enqueued at 06:00:46 may start a
        // turn: a queue operation after the snapshot fails closed.
        let queued = format!(
            "{tail}{}\n",
            r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-10-11T06:00:46.799Z","content":"/agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md"}"#
        );
        assert_eq!(
            classify_transcript_tail(&queued, false),
            TranscriptTailEvidence::Unknown
        );
    }

    /// `#cancelturnprompt`: once agent-doc saw the cancelled trigger back in
    /// the composer and withdrew it, the abandoned snapshot retires its own
    /// lease immediately; it still never retires a newer lease, and nothing
    /// but an abandoned snapshot qualifies.
    #[test]
    fn cancelturnprompt_withdrawn_prompt_retires_its_own_lease_without_settling() {
        let lease_at = parse_rfc3339_utc_secs("2026-10-11T05:59:23Z").unwrap();
        let snapshot_at = parse_rfc3339_utc_secs("2026-10-11T05:59:30.674Z").unwrap();
        let lease = TurnActiveMarker {
            pane: "%149".to_string(),
            written_at: lease_at,
        };
        let abandoned = TranscriptTailEvidence::AbandonedPrompt {
            at_secs: snapshot_at,
        };
        assert!(turn_lease_ended_by_withdrawn_prompt(&lease, abandoned));
        assert!(
            !turn_lease_ended_by_interrupt(&lease, abandoned, snapshot_at + 1),
            "without the withdrawal the 20s settle window still applies"
        );
        let newer = TurnActiveMarker {
            pane: "%149".to_string(),
            written_at: snapshot_at + 1,
        };
        assert!(!turn_lease_ended_by_withdrawn_prompt(&newer, abandoned));
        for evidence in [
            TranscriptTailEvidence::Interrupted {
                at_secs: snapshot_at,
            },
            TranscriptTailEvidence::Activity {
                at_secs: Some(snapshot_at),
            },
            TranscriptTailEvidence::Unknown,
        ] {
            assert!(!turn_lease_ended_by_withdrawn_prompt(&lease, evidence));
        }
    }

    /// `#sdkclearwedge`: the operator's deferred clear (04:30:30) was later
    /// satisfied by a hand-typed `/clear` (`SessionStart` clear receipt at
    /// 04:34:25); delivering it at the next idle gap would wipe the re-run.
    #[test]
    fn sdkclearwedge_only_a_newer_fresh_conversation_satisfies_a_deferred_clear() {
        let deferred = parse_rfc3339_utc_secs("2026-10-11T04:30:30Z").unwrap();
        let cleared = parse_rfc3339_utc_secs("2026-10-11T04:34:25Z").unwrap();
        assert!(harness_receipt_satisfies_deferred_clear(
            "session_start_clear",
            cleared,
            deferred
        ));
        assert!(harness_receipt_satisfies_deferred_clear(
            "session_start_startup",
            cleared,
            deferred
        ));
        for reason in [
            "session_start_resume",
            TURN_END_REASON_INTERRUPT,
            TURN_END_REASON_ABANDONED_PROMPT,
        ] {
            assert!(!harness_receipt_satisfies_deferred_clear(
                reason, cleared, deferred
            ));
        }
        assert!(!harness_receipt_satisfies_deferred_clear(
            "session_start_clear",
            deferred,
            deferred
        ));
        assert!(!harness_receipt_satisfies_deferred_clear(
            "session_start_clear",
            deferred - 60,
            deferred
        ));
    }

    #[test]
    fn interrupt_retires_only_the_lease_it_ended_after_settling() {
        let marker = TurnActiveMarker {
            pane: "%161".to_string(),
            written_at: 1_000,
        };
        let interrupted = TranscriptTailEvidence::Interrupted { at_secs: 1_011 };
        assert!(turn_lease_ended_by_interrupt(
            &marker,
            interrupted,
            1_011 + TURN_INTERRUPT_SETTLE_SECS
        ));
        // Not yet settled: a same-second prompt may still be landing.
        assert!(!turn_lease_ended_by_interrupt(
            &marker,
            interrupted,
            1_011 + TURN_INTERRUPT_SETTLE_SECS - 1
        ));
        // An interrupt of an earlier turn says nothing about this lease.
        assert!(!turn_lease_ended_by_interrupt(
            &marker,
            TranscriptTailEvidence::Interrupted { at_secs: 999 },
            5_000
        ));
        for evidence in [
            TranscriptTailEvidence::Activity {
                at_secs: Some(marker.written_at),
            },
            TranscriptTailEvidence::Unknown,
        ] {
            assert!(!turn_lease_ended_by_interrupt(&marker, evidence, 5_000));
        }
    }
}

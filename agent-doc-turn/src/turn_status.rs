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
// - A settled `[Request interrupted by user…]` transcript record (above).
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

/// Text prefix of the record Claude Code appends to the session transcript
/// when the operator interrupts a turn. Both observed forms share it:
/// `[Request interrupted by user]` and `[Request interrupted by user for tool use]`.
pub const HARNESS_INTERRUPT_MARKER_PREFIX: &str = "[Request interrupted by user";

/// Seconds an interrupt record must have settled before it retires a lease.
/// A prompt submitted in the same second as the interrupt writes its lease
/// before its own transcript record lands; the settle window lets that record
/// appear so a fresh turn is never mistaken for the interrupted one.
pub const TURN_INTERRUPT_SETTLE_SECS: u64 = 3;

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
    /// The newest conversation record is ordinary turn activity (a prompt, an
    /// assistant message, a tool result).
    Activity,
    /// Nothing provable: no complete conversation record in the window, an
    /// unparseable or still-being-written record, or a transcript shape this
    /// policy does not recognise (for example a Codex rollout).
    Unknown,
}

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

/// Classify the newest conversation record in a transcript tail.
///
/// `tail` is the last bytes of a JSONL transcript; `starts_mid_file` is true
/// when the window does not begin at byte 0, so its first segment may be a
/// fragment and is discarded. Records are scanned newest first. Bookkeeping
/// records (attachments, links, system summaries, queue operations, file
/// history) and subagent sidechain records are skipped; the first `user` or
/// `assistant` record decides. Any unparseable segment met before that record,
/// including a trailing line still being written, yields
/// [`TranscriptTailEvidence::Unknown`]: the probe never reaches past a record
/// it cannot read to an older interrupt.
pub fn classify_transcript_tail(tail: &str, starts_mid_file: bool) -> TranscriptTailEvidence {
    let mut segments: Vec<&str> = tail.split('\n').collect();
    if starts_mid_file && !segments.is_empty() {
        segments.remove(0);
    }
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
        match record.get("type").and_then(serde_json::Value::as_str) {
            Some("user") => {
                let content = record
                    .get("message")
                    .and_then(|message| message.get("content"));
                if content.is_some_and(record_text_is_interrupt) {
                    return record
                        .get("timestamp")
                        .and_then(serde_json::Value::as_str)
                        .and_then(parse_rfc3339_utc_secs)
                        .map_or(TranscriptTailEvidence::Unknown, |at_secs| {
                            TranscriptTailEvidence::Interrupted { at_secs }
                        });
                }
                return TranscriptTailEvidence::Activity;
            }
            Some("assistant") => return TranscriptTailEvidence::Activity,
            _ => continue,
        }
    }
    TranscriptTailEvidence::Unknown
}

/// True when the harness transcript proves the turn that wrote `marker` was
/// interrupted and has not been followed by any newer conversation activity.
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
        TranscriptTailEvidence::Activity | TranscriptTailEvidence::Unknown => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                TranscriptTailEvidence::Activity
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
        // A transcript shape this policy does not know (Codex rollout) proves nothing.
        let codex = r#"{"type":"event_msg","payload":{"type":"turn_aborted"}}"#;
        assert_eq!(
            classify_transcript_tail(codex, false),
            TranscriptTailEvidence::Unknown
        );
        assert_eq!(
            classify_transcript_tail("", false),
            TranscriptTailEvidence::Unknown
        );
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
            TranscriptTailEvidence::Activity,
            TranscriptTailEvidence::Unknown,
        ] {
            assert!(!turn_lease_ended_by_interrupt(&marker, evidence, 5_000));
        }
    }
}

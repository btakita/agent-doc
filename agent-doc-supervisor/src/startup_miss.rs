//! Pure startup-miss and session-log decisions.
//!
//! This module classifies already-loaded supervisor session log content. It
//! does not read or write marker files, session logs, registries, tmux, or
//! process state.

use serde::{Deserialize, Serialize};

const RECENT_SESSION_LOSS_WINDOW_SECS: u64 = 600;
const RECENT_SESSION_LOSS_THRESHOLD: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StartupMiss {
    pub file: String,
    pub pane_id: String,
    pub session_id: String,
    pub harness: String,
    pub timestamp: u64,
    pub origin: StartupMissOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_baseline_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartupMissOrigin {
    FreshStart,
    RoutedTrigger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLogStatus {
    pub latest_start_pane: Option<String>,
    pub latest_start_timestamp: Option<u64>,
    pub latest_run_timestamp: Option<u64>,
    pub latest_run_event: Option<String>,
    pub saw_committed_cycle_after_latest_run: bool,
    pub last_event: Option<String>,
    pub saw_process_exit_after_latest_start: bool,
    pub saw_session_end_after_latest_start: bool,
    pub saw_process_exit_after_latest_run: bool,
    pub saw_session_end_after_latest_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupMissSupersession {
    pub registered_pane: String,
    pub latest_start_pane: String,
    pub latest_start_timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartingPaneRecoveryTarget {
    SamePane,
    DifferentPane(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentSessionLossWindow {
    pub count: usize,
    pub first_timestamp: u64,
    pub last_timestamp: u64,
    pub latest_reason: Option<String>,
}

impl SessionLogStatus {
    fn latest_anchor_timestamp(&self) -> Option<u64> {
        self.latest_run_timestamp.or(self.latest_start_timestamp)
    }

    fn latest_anchor_closed(&self) -> bool {
        if self.latest_run_timestamp.is_some() {
            self.saw_process_exit_after_latest_run || self.saw_session_end_after_latest_run
        } else {
            self.saw_process_exit_after_latest_start || self.saw_session_end_after_latest_start
        }
    }

    pub fn latest_session_open(&self) -> bool {
        self.latest_anchor_timestamp().is_some() && !self.latest_anchor_closed()
    }

    pub fn latest_session_closed(&self) -> bool {
        self.latest_anchor_timestamp().is_some() && self.latest_anchor_closed()
    }
}

/// Build the session-log events that close an open supervisor session after sync
/// proves the registered pane is missing.
///
/// Returns `None` when there is no current log status or the latest session is
/// already closed. Callers own appending the returned events to the durable log.
pub fn missing_pane_session_loss_events(
    status: Option<&SessionLogStatus>,
    pane_id: &str,
    reason: &str,
    last_known_window: Option<&str>,
) -> Option<(String, String)> {
    let status = status?;
    if status.latest_session_closed() {
        return None;
    }

    let mut exit_event =
        format!("supervisor_exit code=missing_pane pane={pane_id} reason={reason}");
    if let Some(window_id) = last_known_window.filter(|window_id| !window_id.is_empty()) {
        exit_event.push_str(&format!(" last_known_window={window_id}"));
    }
    Some((
        exit_event,
        "session_end origin=sync_missing_pane".to_string(),
    ))
}

/// Decide whether a persisted startup-miss marker is superseded by the current
/// registered owner and that owner's latest session-log start.
///
/// Callers supply already-loaded registry/session-log facts so this remains
/// pure. A marker is superseded only when the registered pane differs from the
/// miss pane and the registered session log proves a newer start on that same
/// registered pane.
pub fn registered_start_supersedes_miss(
    miss: &StartupMiss,
    registered_pane: &str,
    status: Option<&SessionLogStatus>,
) -> Option<StartupMissSupersession> {
    if registered_pane == miss.pane_id {
        return None;
    }
    let status = status?;
    let latest_start_timestamp = status.latest_start_timestamp?;
    let latest_start_pane = status.latest_start_pane.as_ref()?;
    if latest_start_pane != registered_pane || latest_start_timestamp <= miss.timestamp {
        return None;
    }

    Some(StartupMissSupersession {
        registered_pane: registered_pane.to_string(),
        latest_start_pane: latest_start_pane.clone(),
        latest_start_timestamp,
    })
}

pub fn is_harness_run_start_event(event: &str) -> bool {
    matches!(
        event.split_whitespace().next(),
        Some(token) if token.ends_with("_start") || token.ends_with("_restart")
    )
}

pub fn is_session_loss_event(event: &str) -> bool {
    event.starts_with("supervisor_exit code=missing_pane ")
}

pub fn event_reason(event: &str) -> Option<String> {
    event
        .split_whitespace()
        .find_map(|part| part.strip_prefix("reason=").map(ToOwned::to_owned))
}

fn event_from_log_line(line: &str) -> &str {
    line.split_once("] ")
        .map(|(_, event)| event)
        .unwrap_or(line)
        .trim()
}

fn timestamp_from_log_line(line: &str) -> Option<u64> {
    line.strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .and_then(|(ts, _)| agent_doc_log_time::parse_log_timestamp(ts))
}

pub fn format_timestamp(epoch_secs: u64) -> String {
    agent_doc_log_time::format_log_timestamp(epoch_secs)
}

pub fn session_log_status_from_content(content: &str) -> Option<SessionLogStatus> {
    let mut saw_start = false;
    let mut latest_start_pane = None;
    let mut latest_start_timestamp = None;
    let mut latest_run_timestamp = None;
    let mut latest_run_event = None;
    let mut saw_committed_cycle_after_latest_run = false;
    let mut last_event = None;
    let mut saw_process_exit_after_latest_start = false;
    let mut saw_session_end_after_latest_start = false;
    let mut saw_process_exit_after_latest_run = false;
    let mut saw_session_end_after_latest_run = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let event = event_from_log_line(line);
        let timestamp = timestamp_from_log_line(line);

        if event.starts_with("session_start ") {
            saw_start = true;
            latest_start_timestamp = timestamp;
            latest_start_pane = event
                .split_whitespace()
                .find_map(|part| part.strip_prefix("pane=").map(ToOwned::to_owned));
            latest_run_timestamp = None;
            latest_run_event = None;
            saw_committed_cycle_after_latest_run = false;
            last_event = Some(event.to_string());
            saw_process_exit_after_latest_start = false;
            saw_session_end_after_latest_start = false;
            saw_process_exit_after_latest_run = false;
            saw_session_end_after_latest_run = false;
            continue;
        }

        if !saw_start {
            continue;
        }

        if is_harness_run_start_event(event) {
            latest_run_timestamp = timestamp.or(latest_start_timestamp);
            latest_run_event = Some(event.to_string());
            saw_committed_cycle_after_latest_run = false;
            last_event = Some(event.to_string());
            saw_process_exit_after_latest_run = false;
            saw_session_end_after_latest_run = false;
            continue;
        }

        last_event = Some(event.to_string());
        if event.starts_with("document_cycle phase=committed ") && latest_run_timestamp.is_some() {
            saw_committed_cycle_after_latest_run = true;
        }
        if event.contains("_exit code=") {
            saw_process_exit_after_latest_start = true;
            if latest_run_timestamp.is_some() {
                saw_process_exit_after_latest_run = true;
            }
        }
        if event
            .split_whitespace()
            .next()
            .is_some_and(|token| token == "session_end")
        {
            saw_session_end_after_latest_start = true;
            if latest_run_timestamp.is_some() {
                saw_session_end_after_latest_run = true;
            }
        }
    }

    saw_start.then_some(SessionLogStatus {
        latest_start_pane,
        latest_start_timestamp,
        latest_run_timestamp,
        latest_run_event,
        saw_committed_cycle_after_latest_run,
        last_event,
        saw_process_exit_after_latest_start,
        saw_session_end_after_latest_start,
        saw_process_exit_after_latest_run,
        saw_session_end_after_latest_run,
    })
}

pub fn session_log_has_event_after_latest_start(
    content: &str,
    event_prefix: &str,
    matches_event: impl Fn(&str) -> bool,
) -> bool {
    let mut found_after_latest_start = false;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let event = event_from_log_line(line);
        if event.starts_with("session_start ") {
            found_after_latest_start = false;
            continue;
        }
        if event.starts_with("agent_restart_performed ") {
            found_after_latest_start = false;
            continue;
        }
        if event.starts_with(event_prefix) && matches_event(event) {
            found_after_latest_start = true;
        }
    }
    found_after_latest_start
}

pub fn session_log_diagnostic(status: &SessionLogStatus) -> String {
    let latest_start = status
        .latest_run_event
        .as_deref()
        .map(|event| {
            format!(
                "latest harness run `{event}` on pane={}",
                status.latest_start_pane.as_deref().unwrap_or("<unknown>")
            )
        })
        .unwrap_or_else(|| {
            status
                .latest_start_pane
                .as_deref()
                .map(|pane| format!("latest session_start pane={pane}"))
                .unwrap_or_else(|| "latest session_start pane=<unknown>".to_string())
        });
    if status.latest_session_open() {
        format!("{latest_start}; session log still has no later child exit or session_end")
    } else if status.latest_session_closed() {
        format!("{latest_start}; session log recorded a later child exit/session_end")
    } else {
        latest_start
    }
}

pub fn latest_open_run_timestamp(status: &SessionLogStatus) -> Option<u64> {
    if status.latest_session_open() {
        status.latest_anchor_timestamp()
    } else {
        None
    }
}

pub fn dispatch_only_requires_ready_probe(
    status: Option<&SessionLogStatus>,
    pane: &str,
    harness_binary: &str,
) -> bool {
    let Some(status) = status else {
        return false;
    };
    if !status.latest_session_open()
        || status.latest_start_pane.as_deref() != Some(pane)
        || status.saw_committed_cycle_after_latest_run
    {
        return false;
    }

    status
        .latest_run_event
        .as_deref()
        .and_then(|event| event.split_whitespace().next())
        .is_some_and(|token| {
            token == format!("{harness_binary}_start")
                || token == format!("{harness_binary}_restart")
        })
}

fn starting_pane_generation_changed(
    initial_status: Option<&SessionLogStatus>,
    current_status: &SessionLogStatus,
    pane: &str,
) -> bool {
    if current_status.latest_start_pane.as_deref() != Some(pane)
        || !current_status.latest_session_open()
    {
        return false;
    }

    let Some(initial_status) = initial_status else {
        return false;
    };

    current_status.latest_start_timestamp != initial_status.latest_start_timestamp
        || current_status.latest_run_timestamp != initial_status.latest_run_timestamp
        || current_status.latest_run_event != initial_status.latest_run_event
}

pub fn starting_pane_recovery_target(
    initial_status: Option<&SessionLogStatus>,
    current_status: Option<&SessionLogStatus>,
    current_pane: &str,
    registered_pane: Option<&str>,
) -> Option<StartingPaneRecoveryTarget> {
    let current_status = current_status?;

    if let Some(registered_pane) = registered_pane
        && registered_pane != current_pane
        && current_status.latest_start_pane.as_deref() == Some(registered_pane)
        && current_status.latest_session_open()
    {
        return Some(StartingPaneRecoveryTarget::DifferentPane(
            registered_pane.to_string(),
        ));
    }

    if starting_pane_generation_changed(initial_status, current_status, current_pane) {
        return Some(StartingPaneRecoveryTarget::SamePane);
    }

    None
}

pub fn latest_log_anchor(status: &SessionLogStatus) -> String {
    status
        .latest_run_event
        .as_deref()
        .map(|event| {
            format!(
                "latest_run={} pane={}",
                event,
                status.latest_start_pane.as_deref().unwrap_or("?")
            )
        })
        .unwrap_or_else(|| {
            format!(
                "latest session_start pane={}",
                status.latest_start_pane.as_deref().unwrap_or("?")
            )
        })
}

pub fn latest_log_outcome(status: &SessionLogStatus) -> &'static str {
    if status.latest_session_open() {
        "open"
    } else if status.latest_session_closed() {
        "closed"
    } else {
        "unknown"
    }
}

pub fn latest_log_last_event(status: &SessionLogStatus) -> &str {
    status.last_event.as_deref().unwrap_or("?")
}

pub fn latest_registry_rebind_successor(status: &SessionLogStatus) -> Option<&str> {
    let event = status.last_event.as_deref()?;
    if !event.starts_with("session_end origin=registry_rebind ") {
        return None;
    }
    event
        .split_whitespace()
        .find_map(|part| part.strip_prefix("next_pane="))
        .filter(|pane| !pane.is_empty())
}

pub fn unresolved_startup_miss_blocks_autostart(
    registered_pane: Option<&str>,
    pane_alive: bool,
    miss: Option<&StartupMiss>,
) -> bool {
    pane_alive && registered_pane.is_some_and(|pane| miss.is_some_and(|miss| miss.pane_id == pane))
}

pub fn passive_autostart_skip_reason(
    unresolved_startup_miss: bool,
    status: Option<&SessionLogStatus>,
    live_registry_rebind_successor: Option<&str>,
) -> Option<String> {
    if unresolved_startup_miss {
        return Some("startup-miss is still unresolved for this document".to_string());
    }

    let status = status?;
    if !status.latest_session_closed() {
        return Some(format!(
            "latest session log is still open or ambiguous (last_event={})",
            latest_log_last_event(status)
        ));
    }

    let last_event = latest_log_last_event(status);
    if last_event.starts_with("session_end origin=registry_rebind ")
        && let Some(successor) = live_registry_rebind_successor
    {
        return Some(format!(
            "latest session ended via registry_rebind and successor pane {successor} is still alive (last_event={last_event})"
        ));
    }

    None
}

pub fn recent_session_loss_window_at(
    content: &str,
    now_epoch_secs: u64,
) -> Option<RecentSessionLossWindow> {
    let cutoff = now_epoch_secs.saturating_sub(RECENT_SESSION_LOSS_WINDOW_SECS);
    let mut count = 0usize;
    let mut first_timestamp = None;
    let mut last_timestamp = None;
    let mut latest_reason = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let event = event_from_log_line(line);
        let Some(timestamp) = timestamp_from_log_line(line) else {
            continue;
        };

        if timestamp < cutoff || timestamp > now_epoch_secs || !is_session_loss_event(event) {
            continue;
        }

        count += 1;
        first_timestamp.get_or_insert(timestamp);
        last_timestamp = Some(timestamp);
        latest_reason = event_reason(event);
    }

    if count < RECENT_SESSION_LOSS_THRESHOLD {
        return None;
    }

    Some(RecentSessionLossWindow {
        count,
        first_timestamp: first_timestamp.unwrap_or(now_epoch_secs),
        last_timestamp: last_timestamp.unwrap_or(now_epoch_secs),
        latest_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_status(
        pane: &str,
        start_timestamp: u64,
        run_timestamp: u64,
        run_event: &str,
    ) -> SessionLogStatus {
        SessionLogStatus {
            latest_start_pane: Some(pane.to_string()),
            latest_start_timestamp: Some(start_timestamp),
            latest_run_timestamp: Some(run_timestamp),
            latest_run_event: Some(run_event.to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some(run_event.to_string()),
            saw_process_exit_after_latest_start: false,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        }
    }

    #[test]
    fn harness_run_start_classification_accepts_start_and_restart() {
        assert!(is_harness_run_start_event("codex_start mode=fresh"));
        assert!(is_harness_run_start_event("claude_restart mode=continue"));
        assert!(!is_harness_run_start_event("codex_exit code=0"));
    }

    #[test]
    fn event_reason_parses_reason_field() {
        assert_eq!(
            event_reason("supervisor_exit code=missing_pane pane=%1 reason=registered_pane_dead"),
            Some("registered_pane_dead".to_string())
        );
        assert_eq!(event_reason("session_end origin=manual"), None);
    }

    #[test]
    fn missing_pane_session_loss_events_close_open_status() {
        let status = open_status("%61", 1, 2, "codex_start mode=fresh restart_count=0");
        let events = missing_pane_session_loss_events(
            Some(&status),
            "%61",
            "registered_pane_missing",
            Some("@9"),
        )
        .expect("open status should produce closeout events");

        assert_eq!(
            events.0,
            "supervisor_exit code=missing_pane pane=%61 reason=registered_pane_missing last_known_window=@9"
        );
        assert_eq!(events.1, "session_end origin=sync_missing_pane");
    }

    #[test]
    fn missing_pane_session_loss_events_skip_closed_or_missing_status() {
        let mut closed = open_status("%61", 1, 2, "codex_start mode=fresh restart_count=0");
        closed.saw_process_exit_after_latest_run = true;

        assert!(
            missing_pane_session_loss_events(Some(&closed), "%61", "registered_pane_missing", None)
                .is_none()
        );
        assert!(
            missing_pane_session_loss_events(None, "%61", "registered_pane_missing", None)
                .is_none()
        );
    }

    #[test]
    fn registered_start_supersedes_stale_startup_miss() {
        let miss = StartupMiss {
            file: "tasks/owned.md".to_string(),
            pane_id: "%401".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 5,
            origin: StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };
        let status = open_status("%408", 10, 11, "codex_start mode=fresh restart_count=0");

        let supersession = registered_start_supersedes_miss(&miss, "%408", Some(&status))
            .expect("newer registered pane start should supersede stale miss");

        assert_eq!(supersession.registered_pane, "%408");
        assert_eq!(supersession.latest_start_pane, "%408");
        assert_eq!(supersession.latest_start_timestamp, 10);
    }

    #[test]
    fn registered_start_supersession_requires_different_pane_and_newer_matching_start() {
        let miss = StartupMiss {
            file: "tasks/owned.md".to_string(),
            pane_id: "%401".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 5,
            origin: StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };

        let same_pane_status = open_status("%401", 10, 11, "codex_start mode=fresh");
        assert!(registered_start_supersedes_miss(&miss, "%401", Some(&same_pane_status)).is_none());

        let stale_status = open_status("%408", 5, 6, "codex_start mode=fresh");
        assert!(registered_start_supersedes_miss(&miss, "%408", Some(&stale_status)).is_none());

        let wrong_pane_status = open_status("%999", 10, 11, "codex_start mode=fresh");
        assert!(
            registered_start_supersedes_miss(&miss, "%408", Some(&wrong_pane_status)).is_none()
        );

        assert!(registered_start_supersedes_miss(&miss, "%408", None).is_none());
    }

    #[test]
    fn unresolved_startup_miss_blocks_only_matching_alive_pane() {
        let miss = StartupMiss {
            file: "tasks/owned.md".to_string(),
            pane_id: "%42".to_string(),
            session_id: "associated-supervisor".to_string(),
            harness: "codex".to_string(),
            timestamp: 5,
            origin: StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };

        assert!(unresolved_startup_miss_blocks_autostart(
            Some("%42"),
            true,
            Some(&miss)
        ));
        assert!(!unresolved_startup_miss_blocks_autostart(
            Some("%42"),
            false,
            Some(&miss)
        ));
        assert!(!unresolved_startup_miss_blocks_autostart(
            Some("%43"),
            true,
            Some(&miss)
        ));
        assert!(!unresolved_startup_miss_blocks_autostart(
            Some("%42"),
            true,
            None
        ));
    }

    #[test]
    fn passive_autostart_skip_reason_reports_unresolved_miss() {
        assert_eq!(
            passive_autostart_skip_reason(true, None, None),
            Some("startup-miss is still unresolved for this document".to_string())
        );
    }

    #[test]
    fn passive_autostart_skip_reason_blocks_open_latest_session() {
        let status = session_log_status_from_content(
            "[1] session_start file=test.md pane=%52 session=s generation=1\n\
             [2] codex_start mode=fresh restart_count=0\n",
        )
        .expect("status");

        assert_eq!(
            passive_autostart_skip_reason(false, Some(&status), None),
            Some(
                "latest session log is still open or ambiguous (last_event=codex_start mode=fresh restart_count=0)"
                    .to_string()
            )
        );
    }

    #[test]
    fn starting_pane_recovery_target_follows_same_file_handoff() {
        let initial = open_status("%151", 10, 11, "codex_start mode=fresh restart_count=0");
        let handed_off = open_status("%183", 20, 21, "codex_start mode=fresh restart_count=0");

        assert_eq!(
            starting_pane_recovery_target(Some(&initial), Some(&handed_off), "%151", Some("%183")),
            Some(StartingPaneRecoveryTarget::DifferentPane(
                "%183".to_string()
            ))
        );
    }

    #[test]
    fn starting_pane_recovery_target_retries_same_pane_after_new_generation() {
        let initial = open_status("%151", 10, 11, "codex_start mode=fresh restart_count=0");
        let restarted = open_status(
            "%151",
            12,
            13,
            "codex_start mode=fresh_restart restart_count=1",
        );

        assert_eq!(
            starting_pane_recovery_target(Some(&initial), Some(&restarted), "%151", Some("%151")),
            Some(StartingPaneRecoveryTarget::SamePane)
        );
    }

    #[test]
    fn starting_pane_recovery_target_ignores_unchanged_open_start() {
        let initial = open_status("%151", 10, 11, "codex_start mode=fresh restart_count=0");

        assert_eq!(
            starting_pane_recovery_target(Some(&initial), Some(&initial), "%151", Some("%151")),
            None
        );
    }

    #[test]
    fn passive_autostart_skip_reason_blocks_live_registry_rebind_successor() {
        let status = session_log_status_from_content(
            "[1] session_start file=test.md pane=%52 session=s generation=1\n\
             [2] codex_start mode=fresh restart_count=0\n\
             [3] session_end origin=registry_rebind next_pane=%77\n",
        )
        .expect("status");

        assert_eq!(
            passive_autostart_skip_reason(false, Some(&status), Some("%77")),
            Some(
                "latest session ended via registry_rebind and successor pane %77 is still alive (last_event=session_end origin=registry_rebind next_pane=%77)"
                    .to_string()
            )
        );
    }

    #[test]
    fn passive_autostart_skip_reason_allows_closed_without_live_successor() {
        let status = session_log_status_from_content(
            "[1] session_start file=test.md pane=%52 session=s generation=1\n\
             [2] codex_start mode=fresh restart_count=0\n\
             [3] codex_exit code=0 restart_count=0\n",
        )
        .expect("status");

        assert_eq!(
            passive_autostart_skip_reason(false, Some(&status), None),
            None
        );
        assert_eq!(passive_autostart_skip_reason(false, None, None), None);
    }

    #[test]
    fn status_reports_open_latest_run() {
        let status = session_log_status_from_content(
            "[1] session_start file=test.md pane=%41 session=s generation=1\n\
             [2] ipc_started project_root=/tmp/project\n\
             [3] codex_start mode=fresh restart_count=0\n",
        )
        .expect("status");

        assert_eq!(status.latest_start_pane.as_deref(), Some("%41"));
        assert_eq!(status.latest_start_timestamp, Some(1));
        assert_eq!(status.latest_run_timestamp, Some(3));
        assert_eq!(
            status.latest_run_event.as_deref(),
            Some("codex_start mode=fresh restart_count=0")
        );
        assert!(status.latest_session_open());
        assert!(!status.latest_session_closed());
        assert_eq!(latest_open_run_timestamp(&status), Some(3));
        assert_eq!(
            session_log_diagnostic(&status),
            "latest harness run `codex_start mode=fresh restart_count=0` on pane=%41; session log still has no later child exit or session_end"
        );
    }

    #[test]
    fn status_reopens_after_child_restart() {
        let status = session_log_status_from_content(
            "[1] session_start file=test.md pane=%52 session=s generation=1\n\
             [2] codex_start mode=fresh restart_count=0\n\
             [3] codex_exit code=0 restart_count=0\n\
             [4] codex_restart mode=continue restart_count=1\n",
        )
        .expect("status");

        assert_eq!(status.latest_run_timestamp, Some(4));
        assert_eq!(
            status.latest_run_event.as_deref(),
            Some("codex_restart mode=continue restart_count=1")
        );
        assert!(status.latest_session_open());
        assert!(!status.latest_session_closed());
    }

    #[test]
    fn status_tracks_committed_cycle_and_closure_after_latest_run() {
        let status = session_log_status_from_content(
            "[1] session_start file=test.md pane=%52 session=s generation=1\n\
             [2] codex_start mode=fresh restart_count=0\n\
             [3] document_cycle phase=committed cycle=cycle-1 event=commit_success\n\
             [4] codex_exit code=0 restart_count=0\n",
        )
        .expect("status");

        assert!(status.saw_committed_cycle_after_latest_run);
        assert_eq!(
            status.last_event.as_deref(),
            Some("codex_exit code=0 restart_count=0")
        );
        assert!(status.latest_session_closed());
    }

    #[test]
    fn dispatch_only_ready_probe_requires_open_matching_harness_run() {
        let status = session_log_status_from_content(
            "[1] session_start file=test.md pane=%42 session=s generation=1\n\
             [2] codex_start mode=fresh restart_count=0\n",
        )
        .expect("status");

        assert!(dispatch_only_requires_ready_probe(
            Some(&status),
            "%42",
            "codex"
        ));
        assert!(!dispatch_only_requires_ready_probe(
            Some(&status),
            "%43",
            "codex"
        ));
        assert!(!dispatch_only_requires_ready_probe(
            Some(&status),
            "%42",
            "claude"
        ));
        assert!(!dispatch_only_requires_ready_probe(None, "%42", "codex"));
    }

    #[test]
    fn dispatch_only_ready_probe_ignores_committed_or_closed_runs() {
        let committed = session_log_status_from_content(
            "[1] session_start file=test.md pane=%42 session=s generation=1\n\
             [2] codex_start mode=fresh restart_count=0\n\
             [3] document_cycle phase=committed cycle=cycle-1 event=commit_success\n",
        )
        .expect("status");
        assert!(!dispatch_only_requires_ready_probe(
            Some(&committed),
            "%42",
            "codex"
        ));

        let closed = session_log_status_from_content(
            "[1] session_start file=test.md pane=%42 session=s generation=1\n\
             [2] codex_restart mode=continue restart_count=1\n\
             [3] codex_exit code=0 restart_count=1\n",
        )
        .expect("status");
        assert!(!dispatch_only_requires_ready_probe(
            Some(&closed),
            "%42",
            "codex"
        ));
    }

    #[test]
    fn event_after_latest_start_resets_on_new_start_or_restart_handoff() {
        let content = "[1] session_start file=test.md pane=%41 session=s generation=1\n\
             [2] codex_capability_proof status=proven\n\
             [3] session_start file=test.md pane=%42 session=s generation=2\n";
        assert!(!session_log_has_event_after_latest_start(
            content,
            "codex_capability_proof status=proven",
            |_| true
        ));

        let content = "[1] session_start file=test.md pane=%41 session=s generation=1\n\
             [2] codex_capability_proof status=proven\n\
             [3] agent_restart_performed old_harness=claude new_harness=codex\n";
        assert!(!session_log_has_event_after_latest_start(
            content,
            "codex_capability_proof status=proven",
            |_| true
        ));
    }

    #[test]
    fn latest_registry_rebind_successor_projects_next_pane() {
        let status = session_log_status_from_content(
            "[1] session_start file=test.md pane=%52 session=s generation=1\n\
             [2] codex_start mode=fresh restart_count=0\n\
             [3] session_end origin=registry_rebind pane=%52 next_pane=%84 generation=1 next_generation=2\n",
        )
        .expect("status");

        assert!(status.latest_session_closed());
        assert_eq!(latest_registry_rebind_successor(&status), Some("%84"));
        assert_eq!(latest_log_outcome(&status), "closed");
        assert_eq!(
            latest_log_anchor(&status),
            "latest_run=codex_start mode=fresh restart_count=0 pane=%52"
        );
        assert_eq!(
            latest_log_last_event(&status),
            "session_end origin=registry_rebind pane=%52 next_pane=%84 generation=1 next_generation=2"
        );
    }

    #[test]
    fn recent_session_loss_window_requires_multiple_recent_losses() {
        let content = concat!(
            "[100] supervisor_exit code=missing_pane pane=%41 reason=registered_pane_missing\n",
            "[200] supervisor_exit code=missing_pane pane=%42 reason=registered_pane_dead\n",
            "[250] pane_death_detected pane=%42 status=9 cycle_phase=preflight_started\n",
            "[900] supervisor_exit code=missing_pane pane=%43 reason=registered_pane_missing\n",
        );

        let recent = recent_session_loss_window_at(content, 260)
            .expect("two recent session losses should trip the guard");
        assert_eq!(recent.count, 2);
        assert_eq!(recent.first_timestamp, 100);
        assert_eq!(recent.last_timestamp, 200);
        assert_eq!(
            recent.latest_reason.as_deref(),
            Some("registered_pane_dead")
        );

        assert!(
            recent_session_loss_window_at(content, 1000).is_none(),
            "old session-loss events outside the guard window should not trip it"
        );
    }

    #[test]
    fn format_timestamp_renders_utc_iso8601() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00Z");
    }
}

//! Pure session-accretion facts, thresholds, and decisions.
//!
//! This crate owns deterministic policy only. Callers provide already-collected
//! document/log/frontmatter facts; effectful IO remains in orchestration.

use serde::{Deserialize, Serialize};

pub const RECENT_WINDOW_SECS: u64 = 30 * 60;
pub const WARN_EXCHANGE_LINES: usize = 160;
// Block thresholds are intentionally high so session-accretion never reaches
// `block` during a normal multi-cycle queue drain (operator directive: the
// queue must not stall by default). `warn` still surfaces a heads-up; only
// genuine crash indicators (restart churn + startup-miss) block by default.
pub const BLOCK_EXCHANGE_LINES: usize = 800;
pub const WARN_RESPONSE_SECTIONS: usize = 8;
pub const BLOCK_RESPONSE_SECTIONS: usize = 40;
pub const WARN_RECENT_COMMITTED_CYCLES: usize = 6;
pub const BLOCK_RECENT_COMMITTED_CYCLES: usize = 60;
pub const WARN_RECENT_NOOP_CLOSEOUTS: usize = 2;
pub const BLOCK_RECENT_NOOP_CLOSEOUTS: usize = 20;
pub const WARN_RESTART_EVENTS: usize = 2;
pub const BLOCK_RESTART_EVENTS: usize = 3;
pub const RECENT_SESSION_LOSS_WARN: usize = 2;
pub const POST_COMPACTION_NOOP_GRACE_SECS: u64 = RECENT_WINDOW_SECS;

/// Built-in default for the editor `/clear` opt-in threshold when neither the
/// document frontmatter nor the project config sets one.
pub const DEFAULT_CLEAR_THRESHOLD: u8 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionAccretionLevel {
    #[default]
    Healthy,
    Warn,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionAccretionReport {
    pub level: SessionAccretionLevel,
    pub exchange_lines: usize,
    pub response_sections: usize,
    pub recent_committed_cycles: usize,
    pub recent_noop_closeouts: usize,
    pub recent_restart_count: usize,
    pub recent_session_loss_count: usize,
    pub startup_miss_active: bool,
    /// Resolved editor `/clear` opt-in threshold (context-usage %, 0-100) for
    /// this document. Editors compare live context usage against this value.
    #[serde(default)]
    pub clear_threshold: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guidance: Vec<String>,
}

impl SessionAccretionReport {
    pub fn is_healthy(&self) -> bool {
        self.level == SessionAccretionLevel::Healthy
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAccretionInput {
    /// Display-ready document path or label for operator guidance.
    pub document: String,
    pub exchange_lines: usize,
    pub response_sections: usize,
    pub recent_committed_cycles: usize,
    pub recent_noop_closeouts: usize,
    pub recent_restart_count: usize,
    pub recent_session_loss_count: usize,
    pub startup_miss_active: bool,
    pub clear_threshold: u8,
    pub auto_compact_opt_in: bool,
    pub queue_active: bool,
}

impl Default for SessionAccretionInput {
    fn default() -> Self {
        Self {
            document: String::new(),
            exchange_lines: 0,
            response_sections: 0,
            recent_committed_cycles: 0,
            recent_noop_closeouts: 0,
            recent_restart_count: 0,
            recent_session_loss_count: 0,
            startup_miss_active: false,
            clear_threshold: DEFAULT_CLEAR_THRESHOLD,
            auto_compact_opt_in: false,
            queue_active: false,
        }
    }
}

pub fn evaluate_session_accretion(input: SessionAccretionInput) -> SessionAccretionReport {
    let mut reasons = Vec::new();
    if input.exchange_lines >= WARN_EXCHANGE_LINES
        || input.response_sections >= WARN_RESPONSE_SECTIONS
    {
        reasons.push(format!(
            "exchange has grown to {} lines across {} response sections",
            input.exchange_lines, input.response_sections
        ));
    }
    if input.recent_committed_cycles >= WARN_RECENT_COMMITTED_CYCLES {
        reasons.push(format!(
            "document closed {} cycles in the last {} minutes ({} no-op closeouts)",
            input.recent_committed_cycles,
            RECENT_WINDOW_SECS / 60,
            input.recent_noop_closeouts
        ));
    } else if input.recent_noop_closeouts >= WARN_RECENT_NOOP_CLOSEOUTS {
        reasons.push(format!(
            "document hit {} no-op closeouts in the last {} minutes",
            input.recent_noop_closeouts,
            RECENT_WINDOW_SECS / 60
        ));
    }
    if input.recent_restart_count >= WARN_RESTART_EVENTS {
        reasons.push(format!(
            "session log recorded {} restart-heavy events in the last {} minutes",
            input.recent_restart_count,
            RECENT_WINDOW_SECS / 60
        ));
    }
    if input.startup_miss_active {
        reasons.push("an unresolved startup-miss marker is still active".to_string());
    }
    if input.recent_session_loss_count >= RECENT_SESSION_LOSS_WARN {
        reasons.push(format!(
            "session lost {} pane(s) recently enough to trip the restart-loss window",
            input.recent_session_loss_count
        ));
    }

    let block_for_exchange = input.exchange_lines >= BLOCK_EXCHANGE_LINES
        || input.response_sections >= BLOCK_RESPONSE_SECTIONS;
    let block_for_closeout_churn = input.recent_committed_cycles >= BLOCK_RECENT_COMMITTED_CYCLES
        && input.recent_noop_closeouts >= BLOCK_RECENT_NOOP_CLOSEOUTS;
    let block_for_restart_churn = input.recent_restart_count >= BLOCK_RESTART_EVENTS
        && (input.startup_miss_active
            || input.recent_session_loss_count >= RECENT_SESSION_LOSS_WARN);
    let warn = !reasons.is_empty();
    let level = if block_for_exchange || block_for_closeout_churn || block_for_restart_churn {
        SessionAccretionLevel::Block
    } else if warn {
        SessionAccretionLevel::Warn
    } else {
        SessionAccretionLevel::Healthy
    };

    let mut guidance = Vec::new();
    if !matches!(level, SessionAccretionLevel::Healthy) {
        if input.exchange_lines >= WARN_EXCHANGE_LINES
            || input.response_sections >= WARN_RESPONSE_SECTIONS
            || input.recent_committed_cycles >= WARN_RECENT_COMMITTED_CYCLES
            || input.recent_noop_closeouts >= WARN_RECENT_NOOP_CLOSEOUTS
        {
            guidance.push(compaction_guidance(
                &input.document,
                input.auto_compact_opt_in,
                input.queue_active,
            ));
        }
        if input.recent_restart_count >= WARN_RESTART_EVENTS
            || input.startup_miss_active
            || input.recent_session_loss_count >= RECENT_SESSION_LOSS_WARN
        {
            guidance.push(restart_or_drain_guidance(input.queue_active));
        }
        if guidance.is_empty() {
            guidance.push(
                "Inspect the per-document churn signals before launching another large turn."
                    .to_string(),
            );
        }
    }

    SessionAccretionReport {
        level,
        exchange_lines: input.exchange_lines,
        response_sections: input.response_sections,
        recent_committed_cycles: input.recent_committed_cycles,
        recent_noop_closeouts: input.recent_noop_closeouts,
        recent_restart_count: input.recent_restart_count,
        recent_session_loss_count: input.recent_session_loss_count,
        startup_miss_active: input.startup_miss_active,
        clear_threshold: input.clear_threshold,
        reasons,
        guidance,
    }
}

/// Accretion severity label used in operator-facing prompt/preflight text.
pub fn level_label(level: SessionAccretionLevel) -> &'static str {
    match level {
        SessionAccretionLevel::Healthy => "healthy",
        SessionAccretionLevel::Warn => "warn",
        SessionAccretionLevel::Block => "block",
    }
}

/// Resolve the product-wide opt-in for pre-emptive queue context resets.
///
/// Document frontmatter has precedence over project config. Missing values
/// default to off so a queue drain never interleaves `/clear` without explicit
/// operator/project opt-in.
pub fn resolve_queue_context_reset_opt_in(
    frontmatter_flag: Option<bool>,
    project_flag: Option<bool>,
) -> bool {
    frontmatter_flag.or(project_flag).unwrap_or(false)
}

/// Resolve and clamp the editor `/clear` context-usage threshold.
///
/// Document frontmatter has precedence over project config, then the built-in
/// default. Values above 100 are clamped because callers compare percentages.
pub fn resolve_clear_threshold(
    frontmatter_threshold: Option<u8>,
    project_threshold: Option<u8>,
) -> u8 {
    frontmatter_threshold
        .or(project_threshold)
        .unwrap_or(DEFAULT_CLEAR_THRESHOLD)
        .min(100)
}

/// Context-reset reason for a recent exchange compaction that happened after
/// the last tracked `/clear`.
pub fn context_reset_reason_for_recent_compaction(
    recent_exchange_compaction_timestamp: Option<u64>,
    last_context_clear_at: Option<u64>,
) -> Option<String> {
    if let Some(compaction_ts) = recent_exchange_compaction_timestamp
        && last_context_clear_at.unwrap_or(0) < compaction_ts
    {
        return Some(
            "exchange was compacted after the last tracked context clear; compaction shrinks the document but not the already-loaded conversation"
                .to_string(),
        );
    }
    None
}

/// Context-reset reason for the current session-accretion report.
pub fn context_reset_reason_for_report(report: &SessionAccretionReport) -> Option<String> {
    if report.is_healthy() {
        return None;
    }

    Some(format!(
        "session accretion is {} (exchange_lines={}, response_sections={}, recent_committed_cycles={}, recent_noop_closeouts={})",
        level_label(report.level),
        report.exchange_lines,
        report.response_sections,
        report.recent_committed_cycles,
        report.recent_noop_closeouts
    ))
}

/// Queue-aware restart guidance.
///
/// While a queue is actively draining, guidance must not ask the agent to stop
/// and defer work to a later fresh cycle.
pub fn restart_or_drain_guidance(queue_active: bool) -> String {
    if queue_active {
        "Queue is actively draining — do NOT stop to restart or defer the remaining items: \
         keep finalizing and looping. The supervisor recycles onto a fresh binary and \
         /clears agent context between items at idle boundaries (#drain-no-defer), so \
         accretion/restart churn resets without stalling the drain."
            .to_string()
    } else {
        "Restart cleanly from the current committed boundary before continuing.".to_string()
    }
}

/// Queue-aware compaction guidance for an over-accreted exchange.
///
/// On an active queue, this must not surface "ask the user before compacting"
/// guidance because a self-driving queue is meant to run unattended.
pub fn compaction_guidance(
    document: &str,
    auto_compact_opt_in: bool,
    queue_active: bool,
) -> String {
    if auto_compact_opt_in {
        format!("Run `agent-doc compact {document} --commit` before another large turn.")
    } else if queue_active {
        "Exchange is large, but an `agent:queue` is active — do NOT stall the queue to ask about compacting. Compact only with an explicit `agent_doc_auto_compact` opt-in (frontmatter or `.agent-doc/config.toml`); otherwise keep draining and note the size in one line of the response."
            .to_string()
    } else {
        format!(
            "Exchange is large; ask the user before compacting. Auto-compact requires an explicit `agent_doc_auto_compact` opt-in in frontmatter or `.agent-doc/config.toml` (currently off). If the user approves, run `agent-doc compact {document} --commit`."
        )
    }
}

pub fn exchange_metrics(content: &str) -> (usize, usize) {
    let exchange = agent_doc_element::element::parse(content)
        .ok()
        .and_then(|components| {
            components
                .into_iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.content(content).to_string())
        })
        .unwrap_or_else(|| content.to_string());
    let exchange_lines = exchange.lines().count();
    let response_sections = exchange
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("### Re:") || trimmed.starts_with("## Assistant")
        })
        .count();
    (exchange_lines, response_sections)
}

/// Deterministic session-log event classifier for restart-heavy churn.
pub fn is_restart_churn_event(event: &str) -> bool {
    event.contains("fresh_restart")
        || event.starts_with("auto_trigger_timeout ")
        || event.starts_with("startup_miss")
        || event.contains("ctrl_d")
        || event.contains("Ctrl-D")
}

pub fn recent_restart_count_from_session_log(content: &str, now: u64) -> usize {
    let window_start = now.saturating_sub(RECENT_WINDOW_SECS);
    let mut count = 0;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let timestamp = line
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
            .and_then(|(ts, _)| agent_doc_log_time::parse_log_timestamp(ts));
        let Some(timestamp) = timestamp else {
            continue;
        };
        if timestamp < window_start {
            continue;
        }
        let event = line
            .split_once("] ")
            .map(|(_, event)| event)
            .unwrap_or(line)
            .trim();
        if is_restart_churn_event(event) {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_metrics_uses_exchange_component_when_present() {
        let content = "outside\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: one\n\
            body\n\
            ## Assistant\n\
            done\n\
            <!-- /agent:exchange -->\n\
            outside\n";

        let (lines, responses) = exchange_metrics(content);

        assert_eq!(lines, 4);
        assert_eq!(responses, 2);
    }

    #[test]
    fn evaluate_warns_on_large_exchange() {
        let report = evaluate_session_accretion(SessionAccretionInput {
            document: "session.md".to_string(),
            exchange_lines: WARN_EXCHANGE_LINES,
            response_sections: 1,
            ..Default::default()
        });

        assert_eq!(report.level, SessionAccretionLevel::Warn);
        assert!(report.reasons[0].contains("exchange has grown"));
        assert!(
            report
                .guidance
                .iter()
                .any(|line| line.contains("ask the user before compacting"))
        );
    }

    #[test]
    fn evaluate_does_not_ask_to_compact_while_queue_active() {
        let report = evaluate_session_accretion(SessionAccretionInput {
            document: "session.md".to_string(),
            exchange_lines: WARN_EXCHANGE_LINES,
            queue_active: true,
            ..Default::default()
        });

        assert_eq!(report.level, SessionAccretionLevel::Warn);
        assert!(
            report
                .guidance
                .iter()
                .any(|line| line.contains("do NOT stall the queue")),
            "got {:?}",
            report.guidance
        );
        assert!(
            !report
                .guidance
                .iter()
                .any(|line| line.contains("ask the user")),
            "got {:?}",
            report.guidance
        );
    }

    #[test]
    fn evaluate_blocks_on_restart_churn_with_active_startup_miss() {
        let report = evaluate_session_accretion(SessionAccretionInput {
            document: "session.md".to_string(),
            recent_restart_count: BLOCK_RESTART_EVENTS,
            startup_miss_active: true,
            ..Default::default()
        });

        assert_eq!(report.level, SessionAccretionLevel::Block);
        assert!(
            report
                .guidance
                .iter()
                .any(|line| line.contains("Restart cleanly"))
        );
    }

    #[test]
    fn compaction_guidance_precedence_opt_in_beats_queue_active() {
        let g = compaction_guidance("/tmp/doc.md", true, true);
        assert!(g.starts_with("Run `agent-doc compact"), "got {g}");

        let g = compaction_guidance("/tmp/doc.md", false, true);
        assert!(g.contains("do NOT stall the queue"), "got {g}");

        let g = compaction_guidance("/tmp/doc.md", false, false);
        assert!(g.contains("ask the user before compacting"), "got {g}");
    }

    #[test]
    fn context_reset_opt_in_prefers_frontmatter_then_project_then_off() {
        assert!(!resolve_queue_context_reset_opt_in(None, None));
        assert!(resolve_queue_context_reset_opt_in(None, Some(true)));
        assert!(!resolve_queue_context_reset_opt_in(Some(false), Some(true)));
        assert!(resolve_queue_context_reset_opt_in(Some(true), Some(false)));
    }

    #[test]
    fn clear_threshold_prefers_frontmatter_then_project_then_default_and_clamps() {
        assert_eq!(resolve_clear_threshold(None, None), DEFAULT_CLEAR_THRESHOLD);
        assert_eq!(resolve_clear_threshold(None, Some(65)), 65);
        assert_eq!(resolve_clear_threshold(Some(70), Some(65)), 70);
        assert_eq!(resolve_clear_threshold(Some(150), None), 100);
        assert_eq!(resolve_clear_threshold(None, Some(150)), 100);
    }

    #[test]
    fn context_reset_reason_for_recent_compaction_requires_clear_after_compaction() {
        let reason = context_reset_reason_for_recent_compaction(Some(20), None)
            .expect("missing clear should require reset after compaction");
        assert!(reason.contains("exchange was compacted"), "got {reason}");
        assert!(context_reset_reason_for_recent_compaction(Some(20), Some(19)).is_some());
        assert!(context_reset_reason_for_recent_compaction(Some(20), Some(20)).is_none());
        assert!(context_reset_reason_for_recent_compaction(None, Some(1)).is_none());
    }

    #[test]
    fn context_reset_reason_for_report_formats_unhealthy_accretion_only() {
        let healthy = SessionAccretionReport::default();
        assert!(context_reset_reason_for_report(&healthy).is_none());

        let warn = SessionAccretionReport {
            level: SessionAccretionLevel::Warn,
            exchange_lines: 170,
            response_sections: 9,
            recent_committed_cycles: 2,
            recent_noop_closeouts: 1,
            ..Default::default()
        };
        let reason = context_reset_reason_for_report(&warn).expect("warn report needs reason");
        assert!(reason.contains("session accretion is warn"), "{reason}");
        assert!(reason.contains("exchange_lines=170"), "{reason}");
        assert!(reason.contains("response_sections=9"), "{reason}");
    }

    #[test]
    fn restart_or_drain_guidance_is_queue_aware() {
        assert!(restart_or_drain_guidance(false).contains("Restart cleanly"));

        let draining = restart_or_drain_guidance(true);
        assert!(draining.contains("do NOT stop"), "got: {draining}");
        assert!(draining.contains("#drain-no-defer"), "got: {draining}");
        assert!(!draining.contains("Restart cleanly"), "got: {draining}");
    }

    #[test]
    fn restart_churn_classifier_matches_known_restart_events() {
        for event in [
            "fresh_restart",
            "session fresh_restart from supervisor",
            "auto_trigger_timeout session idle",
            "startup_miss pending launch",
            "received ctrl_d from pane",
            "received Ctrl-D from pane",
        ] {
            assert!(is_restart_churn_event(event), "event should churn: {event}");
        }
    }

    #[test]
    fn restart_churn_classifier_rejects_non_churn_event() {
        assert!(!is_restart_churn_event("commit completed normally"));
    }

    #[test]
    fn recent_restart_count_filters_session_log_by_window_and_churn_signature() {
        let now = agent_doc_log_time::parse_log_timestamp("2026-06-30T12:30:00Z").unwrap();
        let recent = "2026-06-30T12:29:00Z";
        let old = "2026-06-30T11:00:00Z";
        let content = format!(
            "\n\
             [{recent}] fresh_restart reason=recycle\n\
             [{recent}] auto_trigger_timeout pane=%1\n\
             [{recent}] route_submit_ok\n\
             [{old}] startup_miss pane=%1\n\
             malformed without timestamp\n\
             [] ctrl_d\n\
             [{recent}] received Ctrl-D from pane\n"
        );

        assert_eq!(recent_restart_count_from_session_log(&content, now), 3);
    }
}

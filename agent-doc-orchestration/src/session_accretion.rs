//! # Module: session_accretion
//!
//! ## Spec
//! - Summarizes per-document context accretion from local document state and
//!   existing binary-owned logs, without replaying whole transcripts.
//! - Measures exchange growth (`exchange_lines`, `response_sections`), recent
//!   closeout churn (`recent_committed_cycles`, `recent_noop_closeouts`), and
//!   restart churn (`recent_restart_count`, `recent_session_loss_count`,
//!   `startup_miss_active`).
//! - Returns a deterministic `SessionAccretionReport` with `healthy`, `warn`,
//!   or `block` severity plus bounded recovery guidance.
//! - `warn` means the session is accreting context and should surface compact
//!   or restart guidance before another expensive turn.
//! - `block` means the orchestration layer should fail closed unless the user
//!   is already taking an explicit compact/recovery path.
//! - Successful exchange compaction records a per-document recovery marker so
//!   closeout-churn metrics only count cycles that happened after the latest
//!   compact, instead of trapping the same document in a compact-then-block loop.
//!
//! ## Agentic Contracts
//! - Uses only local file state plus `.agent-doc/logs/*` and persisted
//!   startup-miss/session-loss state.
//! - Missing logs or missing session metadata degrade to zero-valued metrics.
//! - Guidance stays bounded and operational: compact the exchange, or restart
//!   cleanly from the current committed boundary.
//!
//! ## Evals
//! - `inspect_warns_on_large_exchange`
//! - `inspect_warns_on_repeated_noop_closeouts`
//! - `inspect_blocks_on_restart_heavy_churn_with_active_startup_miss`
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const RECENT_WINDOW_SECS: u64 = 30 * 60;
const WARN_EXCHANGE_LINES: usize = 160;
// Block thresholds are intentionally high so session-accretion never reaches
// `block` during a normal multi-cycle queue drain (operator directive: the
// queue must not stall by default). `warn` still surfaces a heads-up; only
// genuine crash indicators (restart churn + startup-miss) block by default.
const BLOCK_EXCHANGE_LINES: usize = 800;
const WARN_RESPONSE_SECTIONS: usize = 8;
const BLOCK_RESPONSE_SECTIONS: usize = 40;
const WARN_RECENT_COMMITTED_CYCLES: usize = 6;
const BLOCK_RECENT_COMMITTED_CYCLES: usize = 60;
const WARN_RECENT_NOOP_CLOSEOUTS: usize = 2;
const BLOCK_RECENT_NOOP_CLOSEOUTS: usize = 20;
const WARN_RESTART_EVENTS: usize = 2;
const BLOCK_RESTART_EVENTS: usize = 3;
const RECENT_SESSION_LOSS_WARN: usize = 2;
const POST_COMPACTION_NOOP_GRACE_SECS: u64 = RECENT_WINDOW_SECS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RecentExchangeCompaction {
    file: String,
    timestamp: u64,
}

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
    /// Resolved editor `/clear` opt-in threshold (context-usage %, 0–100) for
    /// this document (`#clear-opt-in-threshold`). The editor compares its live
    /// context-usage percentage against this value to decide a pre-emptive clear.
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

pub fn inspect(file: &Path) -> Result<SessionAccretionReport> {
    let content = std::fs::read_to_string(file)?;
    inspect_at(file, &content, current_epoch_secs())
}

pub fn inspect_with_context(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<SessionAccretionReport> {
    let content = std::fs::read_to_string(file)?;
    inspect_at_with_context(file, &content, current_epoch_secs(), rc)
}

pub fn record_recent_exchange_compaction(file: &Path) -> Result<()> {
    let Some(path) = recent_exchange_compaction_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let marker = RecentExchangeCompaction {
        file: canonical.display().to_string(),
        timestamp: current_epoch_secs(),
    };
    let json = serde_json::to_string_pretty(&marker)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn recent_exchange_compaction_timestamp(file: &Path) -> Result<Option<u64>> {
    recent_exchange_compaction_timestamp_at(file, current_epoch_secs())
}

fn recent_exchange_compaction_timestamp_at(file: &Path, now: u64) -> Result<Option<u64>> {
    Ok(load_recent_exchange_compaction(file)?
        .map(|marker| marker.timestamp)
        .filter(|timestamp| now.saturating_sub(*timestamp) <= POST_COMPACTION_NOOP_GRACE_SECS))
}

/// Whether the supervisor idle-queue watch is allowed to pre-emptively
/// interleave a context-clear (`/clear`) before a queue head
/// (`#nm1x-no-preempt-clear`). Off by default: a frontmatter
/// `agent_doc_queue_context_reset: true` takes precedence, then the project
/// config `.agent-doc/config.toml`. Without an explicit opt-in the watch never
/// fires a pre-emptive `/clear`, so a manual `Run Agent Doc` or an auto-loop
/// drain does not churn the session or hit `/clear` rejected mid-turn.
pub fn queue_context_reset_opted_in(file: &Path) -> bool {
    if let Ok(content) = std::fs::read_to_string(file)
        && let Ok((fm, _)) = crate::frontmatter::parse(&content)
        && let Some(flag) = fm.queue_context_reset
    {
        return flag;
    }
    crate::project_config_io::load_project_for_doc(file)
        .agent_doc_queue_context_reset
        .unwrap_or(false)
}

/// Built-in default for the editor `/clear` opt-in threshold when neither the
/// document frontmatter nor the project config sets one.
pub const DEFAULT_CLEAR_THRESHOLD: u8 = 50;

/// Resolve the context-usage percentage (0–100) at or above which an opted-in
/// editor should pre-emptively run `/clear` (`#clear-opt-in-threshold`).
///
/// Resolution order mirrors [`queue_context_reset_opted_in`]: a per-document
/// frontmatter `agent_doc_clear_threshold` takes precedence, then the project
/// config `.agent-doc/config.toml`, then [`DEFAULT_CLEAR_THRESHOLD`]. The value
/// is clamped to `0..=100`. The binary owns this threshold so every editor and
/// harness shares the same gate; supported transcript readers compare live
/// context usage against it and fail safe when no reliable percentage is known.
pub fn clear_threshold_for_doc(file: &Path) -> u8 {
    if let Ok(content) = std::fs::read_to_string(file)
        && let Ok((fm, _)) = crate::frontmatter::parse(&content)
        && let Some(threshold) = fm.clear_threshold
    {
        return threshold.min(100);
    }
    crate::project_config_io::load_project_for_doc(file)
        .agent_doc_clear_threshold
        .unwrap_or(DEFAULT_CLEAR_THRESHOLD)
        .min(100)
}

pub fn queue_context_reset_reason(
    file: &Path,
    last_context_clear_at: Option<u64>,
) -> Result<Option<String>> {
    if let Some(compaction_ts) = recent_exchange_compaction_timestamp(file)?
        && last_context_clear_at.unwrap_or(0) < compaction_ts
    {
        return Ok(Some(
            "exchange was compacted after the last tracked context clear; compaction shrinks the document but not the already-loaded conversation"
                .to_string(),
        ));
    }

    let report = inspect(file)?;
    if report.is_healthy() {
        return Ok(None);
    }

    Ok(Some(format!(
        "session accretion is {} (exchange_lines={}, response_sections={}, recent_committed_cycles={}, recent_noop_closeouts={})",
        level_label(report.level),
        report.exchange_lines,
        report.response_sections,
        report.recent_committed_cycles,
        report.recent_noop_closeouts
    )))
}

/// Accretion-driven context-reset reason, gated on the
/// `agent_doc_queue_context_reset` opt-in (`#nm1x-codex-clear-parity`).
///
/// `#nm1x-no-preempt-clear` scoped the off-by-default opt-in to the supervisor
/// idle-queue watch; this helper extends the same gate to the Codex Stop-hook
/// continuation instructions and the `run.rs` queue-continuation fresh-session
/// decision so the no-pre-emptive-clear policy is product-wide. Without an
/// explicit opt-in (frontmatter or `.agent-doc/config.toml`), every accretion
/// path returns `None`, so a manual `Run Agent Doc` / auto-loop drain never
/// interleaves a pre-emptive `/clear`. Deferred *operator* clears stay a
/// separate live path.
pub fn queue_context_reset_reason_if_opted_in(
    file: &Path,
    last_context_clear_at: Option<u64>,
) -> Result<Option<String>> {
    if !queue_context_reset_opted_in(file) {
        return Ok(None);
    }
    queue_context_reset_reason(file, last_context_clear_at)
}

/// Compaction guidance line for an over-accreted exchange.
///
/// Queue-aware (`#no-compact-prompt-during-queue-drain`): while an `agent:queue`
/// is actively draining, the agent must NOT stall the queue to ask the user about
/// compacting — a self-driving queue is meant to run unattended, so a compaction
/// question blocks the very work the user queued. On an active queue, compaction
/// happens only via an explicit `agent_doc_auto_compact` opt-in; otherwise the
/// agent keeps draining and notes the size in one line. Off the queue (idle /
/// user-driven turn), the agent asks before compacting as before.
/// `#drain-no-defer` — restart/accretion guidance is queue-aware. While a queue is
/// actively draining, do NOT tell the agent to stop and restart "from the current
/// committed boundary" — that stalls the drain and is exactly the "defer to a fresh
/// cycle" anti-pattern. The supervisor recycles onto a fresh binary and `/clear`s
/// agent context between items at idle boundaries, so accretion/restart churn resets
/// without the agent stopping. Off the queue (idle / user-driven turn) the normal
/// clean-restart guidance applies.
fn restart_or_drain_guidance(queue_active: bool) -> String {
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

fn compaction_guidance(file: &Path, auto_compact_opt_in: bool, queue_active: bool) -> String {
    if auto_compact_opt_in {
        format!(
            "Run `agent-doc compact {} --commit` before another large turn.",
            file.display()
        )
    } else if queue_active {
        "Exchange is large, but an `agent:queue` is active — do NOT stall the queue to ask about compacting. Compact only with an explicit `agent_doc_auto_compact` opt-in (frontmatter or `.agent-doc/config.toml`); otherwise keep draining and note the size in one line of the response."
            .to_string()
    } else {
        format!(
            "Exchange is large; ask the user before compacting. Auto-compact requires an explicit `agent_doc_auto_compact` opt-in in frontmatter or `.agent-doc/config.toml` (currently off). If the user approves, run `agent-doc compact {} --commit`.",
            file.display()
        )
    }
}

fn level_label(level: SessionAccretionLevel) -> &'static str {
    match level {
        SessionAccretionLevel::Healthy => "healthy",
        SessionAccretionLevel::Warn => "warn",
        SessionAccretionLevel::Block => "block",
    }
}

fn inspect_at(file: &Path, content: &str, now: u64) -> Result<SessionAccretionReport> {
    let (exchange_lines, response_sections) = exchange_metrics(content);
    let (recent_committed_cycles, recent_noop_closeouts) = recent_cycle_metrics(file, now)?;
    let startup_miss_active = crate::startup_miss::load(file)?.is_some();
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let parsed_frontmatter =
        crate::frontmatter::parse_for_file_with_context(content, file, &rc).ok();
    let session_id = parsed_frontmatter
        .as_ref()
        .and_then(|(fm, _)| fm.session.as_ref().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty());
    let auto_compact_opt_in = parsed_frontmatter
        .as_ref()
        .map(|(fm, _)| fm.auto_compact.is_some())
        .unwrap_or(false)
        || rc.project_config().agent_doc_auto_compact.is_some();
    let queue_active = parsed_frontmatter
        .as_ref()
        .map(|(fm, _)| fm.queue_active == Some(true))
        .unwrap_or(false);
    let recent_restart_count = session_id
        .as_deref()
        .map(|session_id| recent_restart_metrics(file, session_id, now))
        .transpose()?
        .unwrap_or(0);
    let recent_session_loss_count = session_id
        .as_deref()
        .and_then(|session_id| {
            crate::startup_miss::recent_session_loss_window(file, session_id)
                .ok()
                .flatten()
                .map(|window| window.count)
        })
        .unwrap_or(0);

    let mut reasons = Vec::new();
    if exchange_lines >= WARN_EXCHANGE_LINES || response_sections >= WARN_RESPONSE_SECTIONS {
        reasons.push(format!(
            "exchange has grown to {} lines across {} response sections",
            exchange_lines, response_sections
        ));
    }
    if recent_committed_cycles >= WARN_RECENT_COMMITTED_CYCLES {
        reasons.push(format!(
            "document closed {} cycles in the last {} minutes ({} no-op closeouts)",
            recent_committed_cycles,
            RECENT_WINDOW_SECS / 60,
            recent_noop_closeouts
        ));
    } else if recent_noop_closeouts >= WARN_RECENT_NOOP_CLOSEOUTS {
        reasons.push(format!(
            "document hit {} no-op closeouts in the last {} minutes",
            recent_noop_closeouts,
            RECENT_WINDOW_SECS / 60
        ));
    }
    if recent_restart_count >= WARN_RESTART_EVENTS {
        reasons.push(format!(
            "session log recorded {} restart-heavy events in the last {} minutes",
            recent_restart_count,
            RECENT_WINDOW_SECS / 60
        ));
    }
    if startup_miss_active {
        reasons.push("an unresolved startup-miss marker is still active".to_string());
    }
    if recent_session_loss_count >= RECENT_SESSION_LOSS_WARN {
        reasons.push(format!(
            "session lost {} pane(s) recently enough to trip the restart-loss window",
            recent_session_loss_count
        ));
    }

    let block_for_exchange =
        exchange_lines >= BLOCK_EXCHANGE_LINES || response_sections >= BLOCK_RESPONSE_SECTIONS;
    let block_for_closeout_churn = recent_committed_cycles >= BLOCK_RECENT_COMMITTED_CYCLES
        && recent_noop_closeouts >= BLOCK_RECENT_NOOP_CLOSEOUTS;
    let block_for_restart_churn = recent_restart_count >= BLOCK_RESTART_EVENTS
        && (startup_miss_active || recent_session_loss_count >= RECENT_SESSION_LOSS_WARN);
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
        if exchange_lines >= WARN_EXCHANGE_LINES
            || response_sections >= WARN_RESPONSE_SECTIONS
            || recent_committed_cycles >= WARN_RECENT_COMMITTED_CYCLES
            || recent_noop_closeouts >= WARN_RECENT_NOOP_CLOSEOUTS
        {
            guidance.push(compaction_guidance(file, auto_compact_opt_in, queue_active));
        }
        if recent_restart_count >= WARN_RESTART_EVENTS
            || startup_miss_active
            || recent_session_loss_count >= RECENT_SESSION_LOSS_WARN
        {
            guidance.push(restart_or_drain_guidance(queue_active));
        }
        if guidance.is_empty() {
            guidance.push(
                "Inspect the per-document churn signals before launching another large turn."
                    .to_string(),
            );
        }
    }

    Ok(SessionAccretionReport {
        level,
        exchange_lines,
        response_sections,
        recent_committed_cycles,
        recent_noop_closeouts,
        recent_restart_count,
        recent_session_loss_count,
        startup_miss_active,
        clear_threshold: clear_threshold_for_doc(file),
        reasons,
        guidance,
    })
}

fn inspect_at_with_context(
    file: &Path,
    content: &str,
    now: u64,
    rc: &crate::graph::RunContext,
) -> Result<SessionAccretionReport> {
    let (exchange_lines, response_sections) = exchange_metrics(content);
    let (recent_committed_cycles, recent_noop_closeouts) = recent_cycle_metrics(file, now)?;
    let startup_miss_active = crate::startup_miss::load(file)?.is_some();
    let parsed_frontmatter =
        crate::frontmatter::parse_for_file_with_context(content, file, rc).ok();
    let session_id = parsed_frontmatter
        .as_ref()
        .and_then(|(fm, _)| fm.session.as_ref().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty());
    let auto_compact_opt_in = parsed_frontmatter
        .as_ref()
        .map(|(fm, _)| fm.auto_compact.is_some())
        .unwrap_or(false)
        || rc.project_config().agent_doc_auto_compact.is_some();
    let queue_active = parsed_frontmatter
        .as_ref()
        .map(|(fm, _)| fm.queue_active == Some(true))
        .unwrap_or(false);
    let recent_restart_count = session_id
        .as_deref()
        .map(|session_id| recent_restart_metrics(file, session_id, now))
        .transpose()?
        .unwrap_or(0);
    let recent_session_loss_count = session_id
        .as_deref()
        .and_then(|session_id| {
            crate::startup_miss::recent_session_loss_window(file, session_id)
                .ok()
                .flatten()
                .map(|window| window.count)
        })
        .unwrap_or(0);

    let mut reasons = Vec::new();
    if exchange_lines >= WARN_EXCHANGE_LINES || response_sections >= WARN_RESPONSE_SECTIONS {
        reasons.push(format!(
            "exchange has grown to {} lines across {} response sections",
            exchange_lines, response_sections
        ));
    }
    if recent_committed_cycles >= WARN_RECENT_COMMITTED_CYCLES {
        reasons.push(format!(
            "document closed {} cycles in the last {} minutes ({} no-op closeouts)",
            recent_committed_cycles,
            RECENT_WINDOW_SECS / 60,
            recent_noop_closeouts
        ));
    } else if recent_noop_closeouts >= WARN_RECENT_NOOP_CLOSEOUTS {
        reasons.push(format!(
            "document hit {} no-op closeouts in the last {} minutes",
            recent_noop_closeouts,
            RECENT_WINDOW_SECS / 60
        ));
    }
    if recent_restart_count >= WARN_RESTART_EVENTS {
        reasons.push(format!(
            "session log recorded {} restart-heavy events in the last {} minutes",
            recent_restart_count,
            RECENT_WINDOW_SECS / 60
        ));
    }
    if startup_miss_active {
        reasons.push("an unresolved startup-miss marker is still active".to_string());
    }
    if recent_session_loss_count >= RECENT_SESSION_LOSS_WARN {
        reasons.push(format!(
            "session lost {} pane(s) recently enough to trip the restart-loss window",
            recent_session_loss_count
        ));
    }

    let block_for_exchange =
        exchange_lines >= BLOCK_EXCHANGE_LINES || response_sections >= BLOCK_RESPONSE_SECTIONS;
    let block_for_closeout_churn = recent_committed_cycles >= BLOCK_RECENT_COMMITTED_CYCLES
        && recent_noop_closeouts >= BLOCK_RECENT_NOOP_CLOSEOUTS;
    let block_for_restart_churn = recent_restart_count >= BLOCK_RESTART_EVENTS
        && (startup_miss_active || recent_session_loss_count >= RECENT_SESSION_LOSS_WARN);
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
        if exchange_lines >= WARN_EXCHANGE_LINES
            || response_sections >= WARN_RESPONSE_SECTIONS
            || recent_committed_cycles >= WARN_RECENT_COMMITTED_CYCLES
            || recent_noop_closeouts >= WARN_RECENT_NOOP_CLOSEOUTS
        {
            guidance.push(compaction_guidance(file, auto_compact_opt_in, queue_active));
        }
        if recent_restart_count >= WARN_RESTART_EVENTS
            || startup_miss_active
            || recent_session_loss_count >= RECENT_SESSION_LOSS_WARN
        {
            guidance.push(restart_or_drain_guidance(queue_active));
        }
        if guidance.is_empty() {
            guidance.push(
                "Inspect the per-document churn signals before launching another large turn."
                    .to_string(),
            );
        }
    }

    Ok(SessionAccretionReport {
        level,
        exchange_lines,
        response_sections,
        recent_committed_cycles,
        recent_noop_closeouts,
        recent_restart_count,
        recent_session_loss_count,
        startup_miss_active,
        clear_threshold: clear_threshold_for_doc(file),
        reasons,
        guidance,
    })
}

fn exchange_metrics(content: &str) -> (usize, usize) {
    let exchange = crate::component::parse(content)
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

fn recent_cycle_metrics(file: &Path, now: u64) -> Result<(usize, usize)> {
    let Some(path) = cycles_log_path(file)? else {
        return Ok((0, 0));
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok((0, 0));
    };
    let Some(relative_file) = relative_file_key(file) else {
        return Ok((0, 0));
    };
    let window_start = now.saturating_sub(RECENT_WINDOW_SECS);
    let recent_compaction_timestamp = recent_exchange_compaction_timestamp_at(file, now)?;
    let post_compaction_noop_grace_until = recent_compaction_timestamp
        .filter(|timestamp| now.saturating_sub(*timestamp) <= POST_COMPACTION_NOOP_GRACE_SECS)
        .map(|timestamp| timestamp.saturating_add(POST_COMPACTION_NOOP_GRACE_SECS));
    let mut committed = 0;
    let mut noops = 0;

    for line in content.lines() {
        let Ok(entry) = serde_json::from_str::<crate::ops_log::CycleEntry>(line) else {
            continue;
        };
        if entry.file != relative_file {
            continue;
        }
        let Some(timestamp) = crate::ops_log::parse_log_timestamp(&entry.timestamp) else {
            continue;
        };
        if timestamp < window_start {
            continue;
        }
        if recent_compaction_timestamp.is_some_and(|compact_ts| timestamp <= compact_ts) {
            continue;
        }
        if entry.op == "commit_noop"
            && post_compaction_noop_grace_until.is_some_and(|grace_until| timestamp <= grace_until)
        {
            continue;
        }
        match entry.op.as_str() {
            "commit" | "commit_noop" => committed += 1,
            _ => {}
        }
        if entry.op == "commit_noop" {
            noops += 1;
        }
    }

    Ok((committed, noops))
}

fn recent_restart_metrics(file: &Path, session_id: &str, now: u64) -> Result<usize> {
    let Some(path) = session_log_path(file, session_id)? else {
        return Ok(0);
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(0);
    };
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
            .and_then(|(ts, _)| crate::ops_log::parse_log_timestamp(ts));
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
    Ok(count)
}

fn is_restart_churn_event(event: &str) -> bool {
    event.contains("fresh_restart")
        || event.starts_with("auto_trigger_timeout ")
        || event.starts_with("startup_miss")
        || event.contains("ctrl_d")
        || event.contains("Ctrl-D")
}

fn cycles_log_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(root.join(".agent-doc/logs/cycles.jsonl")))
}

fn recent_exchange_compaction_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(&canonical)?;
    Ok(Some(
        root.join(".agent-doc/state/session-accretion-compaction")
            .join(format!("{hash}.json")),
    ))
}

fn load_recent_exchange_compaction(file: &Path) -> Result<Option<RecentExchangeCompaction>> {
    let Some(path) = recent_exchange_compaction_path(file)? else {
        return Ok(None);
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(None);
    };
    let marker: RecentExchangeCompaction = serde_json::from_str(&content)?;
    Ok(Some(marker))
}

fn session_log_path(file: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(
        root.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
    ))
}

fn relative_file_key(file: &Path) -> Option<String> {
    let canonical = file.canonicalize().ok()?;
    let root = crate::snapshot::find_project_root(&canonical)?;
    Some(
        canonical
            .strip_prefix(&root)
            .unwrap_or(&canonical)
            .to_string_lossy()
            .to_string(),
    )
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn setup_doc(content: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        (dir, doc)
    }

    fn write_cycles_log(doc: &Path, entries: &[crate::ops_log::CycleEntry]) {
        let log_path = doc.parent().unwrap().join(".agent-doc/logs/cycles.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(log_path).unwrap();
        for entry in entries {
            writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
        }
    }

    fn write_session_log(doc: &Path, session_id: &str, lines: &[String]) {
        let log_path = doc
            .parent()
            .unwrap()
            .join(".agent-doc/logs")
            .join(format!("{session_id}.log"));
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        std::fs::write(log_path, lines.join("\n")).unwrap();
    }

    #[test]
    fn inspect_warns_on_large_exchange() {
        let exchange_lines = (0..170)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n"
        );
        let (_dir, doc) = setup_doc(&content);

        let report = inspect(&doc).unwrap();

        assert_eq!(report.level, SessionAccretionLevel::Warn);
        assert!(report.exchange_lines >= 170);
        assert!(
            report
                .guidance
                .iter()
                .any(|line| line.contains("agent-doc compact")),
            "expected compact guidance, got {:?}",
            report.guidance
        );
        assert!(
            report.guidance.iter().any(
                |line| line.contains("ask the user") && line.contains("agent_doc_auto_compact")
            ),
            "expected opt-in gated compact guidance when agent_doc_auto_compact is unset, got {:?}",
            report.guidance
        );
        assert!(
            !report
                .guidance
                .iter()
                .any(|line| line.starts_with("Run `agent-doc compact")),
            "auto-compact must be off by default; should not emit imperative compact guidance without opt-in, got {:?}",
            report.guidance
        );
    }

    #[test]
    fn queue_context_reset_opt_in_defaults_off_and_honors_frontmatter() {
        // `#nm1x-no-preempt-clear`: the supervisor idle-queue watch consults this
        // opt-in before signalling an accretion-driven context reset. It must
        // default off so a manual `Run Agent Doc` / auto-loop drain never fires a
        // pre-emptive `/clear`, and honor an explicit frontmatter opt-in.
        let off = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nx\n<!-- /agent:exchange -->\n";
        let (_dir_off, doc_off) = setup_doc(off);
        assert!(
            !queue_context_reset_opted_in(&doc_off),
            "opt-in defaults off"
        );

        let on = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\nagent_doc_queue_context_reset: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nx\n<!-- /agent:exchange -->\n";
        let (_dir_on, doc_on) = setup_doc(on);
        assert!(
            queue_context_reset_opted_in(&doc_on),
            "frontmatter opt-in honored"
        );
    }

    #[test]
    fn clear_threshold_defaults_to_50_and_honors_frontmatter() {
        // #clear-opt-in-threshold: the editor /clear opt-in threshold resolves
        // from frontmatter, then project config, then the built-in default of 50.
        let base = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nx\n<!-- /agent:exchange -->\n";
        let (_dir_d, doc_d) = setup_doc(base);
        assert_eq!(
            clear_threshold_for_doc(&doc_d),
            DEFAULT_CLEAR_THRESHOLD,
            "unconfigured threshold defaults to 50"
        );

        let fm70 = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nagent_doc_clear_threshold: 70\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nx\n<!-- /agent:exchange -->\n";
        let (_dir_70, doc_70) = setup_doc(fm70);
        assert_eq!(
            clear_threshold_for_doc(&doc_70),
            70,
            "frontmatter agent_doc_clear_threshold is honored"
        );

        // An out-of-range value is clamped to 100.
        let fm_over = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nagent_doc_clear_threshold: 150\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nx\n<!-- /agent:exchange -->\n";
        let (_dir_o, doc_o) = setup_doc(fm_over);
        assert_eq!(
            clear_threshold_for_doc(&doc_o),
            100,
            "threshold clamps to 100"
        );
    }

    #[test]
    fn accretion_report_surfaces_clear_threshold() {
        // The resolved threshold is surfaced on the SessionAccretionReport so the
        // editor (which reads preflight output) can compare its live context %.
        let fm = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nagent_doc_clear_threshold: 65\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nx\n<!-- /agent:exchange -->\n";
        let (_dir, doc) = setup_doc(fm);
        let report = inspect(&doc).unwrap();
        assert_eq!(
            report.clear_threshold, 65,
            "report carries the resolved threshold"
        );
    }

    #[test]
    fn queue_context_reset_reason_warns_on_large_exchange() {
        let exchange_lines = (0..170)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n"
        );
        let (_dir, doc) = setup_doc(&content);

        let reason = queue_context_reset_reason(&doc, None)
            .unwrap()
            .expect("large exchange should require fresh queue context");

        assert!(reason.contains("session accretion is warn"), "{reason}");
        assert!(reason.contains("exchange_lines="), "{reason}");
    }

    #[test]
    fn queue_context_reset_reason_if_opted_in_gates_codex_and_run_paths() {
        // `#nm1x-codex-clear-parity`: the Codex Stop-hook continuation
        // instructions and the run-path queue-continuation fresh-session decision
        // route through this gated helper. A large/unhealthy exchange that
        // `queue_context_reset_reason` would flag must still return `None` without
        // the `agent_doc_queue_context_reset` opt-in, so neither path fires a
        // pre-emptive `/clear` by default. An explicit opt-in re-enables it.
        let exchange_lines = (0..170)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let off = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n"
        );
        let (_dir_off, doc_off) = setup_doc(&off);
        // The raw reason still flags the unhealthy exchange...
        assert!(
            queue_context_reset_reason(&doc_off, None)
                .unwrap()
                .is_some()
        );
        // ...but the gated helper suppresses it without the opt-in.
        assert!(
            queue_context_reset_reason_if_opted_in(&doc_off, None)
                .unwrap()
                .is_none(),
            "context reset must default off for Codex/run paths"
        );

        let on = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\nagent_doc_queue_context_reset: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n"
        );
        let (_dir_on, doc_on) = setup_doc(&on);
        let reason = queue_context_reset_reason_if_opted_in(&doc_on, None)
            .unwrap()
            .expect("explicit opt-in re-enables the accretion-driven reset");
        assert!(reason.contains("session accretion is warn"), "{reason}");
    }

    #[test]
    fn queue_context_reset_reason_requires_clear_after_exchange_compaction() {
        let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n";
        let (_dir, doc) = setup_doc(content);
        assert!(queue_context_reset_reason(&doc, None).unwrap().is_none());

        record_recent_exchange_compaction(&doc).unwrap();
        let reason = queue_context_reset_reason(&doc, None)
            .unwrap()
            .expect("recent compaction should require one fresh context");
        assert!(reason.contains("exchange was compacted"), "{reason}");
        assert!(reason.contains("already-loaded conversation"), "{reason}");

        let compaction_ts = recent_exchange_compaction_timestamp(&doc)
            .unwrap()
            .expect("compaction timestamp should be recorded");
        assert!(
            queue_context_reset_reason(&doc, Some(compaction_ts))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn inspect_emits_imperative_compact_guidance_when_auto_compact_opt_in() {
        let exchange_lines = (0..170)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nagent_doc_auto_compact: 180\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n"
        );
        let (_dir, doc) = setup_doc(&content);

        let report = inspect(&doc).unwrap();

        assert_eq!(report.level, SessionAccretionLevel::Warn);
        assert!(
            report
                .guidance
                .iter()
                .any(|line| line.starts_with("Run `agent-doc compact")),
            "expected imperative compact guidance when frontmatter opts in, got {:?}",
            report.guidance
        );
        assert!(
            !report
                .guidance
                .iter()
                .any(|line| line.contains("ask the user")),
            "opt-in caller should not get the gated phrasing, got {:?}",
            report.guidance
        );
    }

    #[test]
    fn inspect_does_not_ask_to_compact_while_queue_active() {
        // #no-compact-prompt-during-queue-drain: a self-draining `agent:queue`
        // must not be stalled by a compaction question — the binary surfaces
        // don't-stall guidance instead of "ask the user".
        let exchange_lines = (0..170)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n"
        );
        let (_dir, doc) = setup_doc(&content);

        let report = inspect(&doc).unwrap();

        assert!(
            report
                .guidance
                .iter()
                .any(|line| line.contains("do NOT stall the queue")),
            "active queue must get the don't-stall guidance, got {:?}",
            report.guidance
        );
        assert!(
            !report
                .guidance
                .iter()
                .any(|line| line.contains("ask the user")),
            "active queue must NOT be told to ask the user about compacting, got {:?}",
            report.guidance
        );
    }

    #[test]
    fn compaction_guidance_precedence_opt_in_beats_queue_active() {
        // An explicit auto-compact opt-in still wins over the queue-active branch.
        let g = compaction_guidance(Path::new("/tmp/doc.md"), true, true);
        assert!(g.starts_with("Run `agent-doc compact"), "got {g}");
        // Queue active, no opt-in → don't-stall guidance.
        let g = compaction_guidance(Path::new("/tmp/doc.md"), false, true);
        assert!(g.contains("do NOT stall the queue"), "got {g}");
        // Idle (no queue), no opt-in → ask the user.
        let g = compaction_guidance(Path::new("/tmp/doc.md"), false, false);
        assert!(g.contains("ask the user before compacting"), "got {g}");
    }

    #[test]
    fn inspect_emits_imperative_compact_guidance_when_project_config_opt_in() {
        let exchange_lines = (0..170)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n"
        );
        let (dir, doc) = setup_doc(&content);
        let config_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "agent_doc_auto_compact = 180\n",
        )
        .unwrap();

        let report = inspect(&doc).unwrap();

        assert_eq!(report.level, SessionAccretionLevel::Warn);
        assert!(
            report
                .guidance
                .iter()
                .any(|line| line.starts_with("Run `agent-doc compact")),
            "expected imperative compact guidance when project config opts in, got {:?}",
            report.guidance
        );
    }

    #[test]
    fn inspect_warns_on_repeated_noop_closeouts() {
        let now = current_epoch_secs();
        let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n";
        let (_dir, doc) = setup_doc(content);
        write_cycles_log(
            &doc,
            &[
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(10).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(20).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        let report = inspect(&doc).unwrap();

        assert_eq!(report.level, SessionAccretionLevel::Warn);
        assert_eq!(report.recent_noop_closeouts, 2);
        assert!(
            report
                .reasons
                .iter()
                .any(|reason| reason.contains("no-op closeouts")),
            "expected noop-closeout reason, got {:?}",
            report.reasons
        );
    }

    #[test]
    fn inspect_ignores_closeout_churn_before_recent_exchange_compaction() {
        let now = current_epoch_secs();
        let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\nCompacted.\n<!-- /agent:exchange -->\n";
        let (_dir, doc) = setup_doc(content);
        write_cycles_log(
            &doc,
            &[
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(120).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(110).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(100).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(90).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(80).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(70).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(60).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(50).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(40).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "session.md".to_string(),
                    timestamp: now.saturating_sub(30).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        record_recent_exchange_compaction(&doc).unwrap();

        let report = inspect(&doc).unwrap();

        assert_eq!(report.level, SessionAccretionLevel::Healthy);
        assert_eq!(report.recent_committed_cycles, 0);
        assert_eq!(report.recent_noop_closeouts, 0);
        assert!(
            report.reasons.is_empty(),
            "compaction recovery should clear closeout-churn reasons: {:?}",
            report.reasons
        );
    }

    #[test]
    fn inspect_blocks_on_restart_heavy_churn_with_active_startup_miss() {
        let now = 8_000;
        let session_id = "session-123";
        let content = format!(
            "---\nagent_doc_session: {session_id}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n"
        );
        let (_dir, doc) = setup_doc(&content);
        write_session_log(
            &doc,
            session_id,
            &[
                format!("[{}] session_start pane=%1", now - 120),
                format!(
                    "[{}] codex_start mode=fresh_restart restart_count=1",
                    now - 90
                ),
                format!(
                    "[{}] route_cycle_start_retry_fresh_restart_not_ready pane=%1",
                    now - 60
                ),
                format!(
                    "[{}] startup_miss_skip_autostart file=session.md pane=%1",
                    now - 30
                ),
            ],
        );
        crate::startup_miss::record(
            &doc,
            "%1",
            session_id,
            "codex",
            crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            None,
        )
        .unwrap();

        let report = inspect_at(&doc, &content, now).unwrap();

        assert_eq!(report.level, SessionAccretionLevel::Block);
        assert!(report.startup_miss_active);
        assert!(report.recent_restart_count >= 3);
        assert!(
            report
                .guidance
                .iter()
                .any(|line| line.contains("Restart cleanly")),
            "expected restart guidance, got {:?}",
            report.guidance
        );
    }

    #[test]
    fn restart_or_drain_guidance_is_queue_aware() {
        // #drain-no-defer: off the queue, the normal clean-restart guidance applies.
        assert!(restart_or_drain_guidance(false).contains("Restart cleanly"));
        // While a queue is draining, never tell the agent to stop/restart/defer — the
        // supervisor /clears + recycles between items instead.
        let draining = restart_or_drain_guidance(true);
        assert!(draining.contains("do NOT stop"), "got: {draining}");
        assert!(draining.contains("#drain-no-defer"), "got: {draining}");
        assert!(!draining.contains("Restart cleanly"), "got: {draining}");
    }
}

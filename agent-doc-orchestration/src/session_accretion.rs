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
#[cfg(test)]
use agent_doc_session_accretion::{
    DEFAULT_CLEAR_THRESHOLD, SessionAccretionLevel, compaction_guidance, restart_or_drain_guidance,
};
use agent_doc_session_accretion::{
    POST_COMPACTION_NOOP_GRACE_SECS, RECENT_WINDOW_SECS, SessionAccretionInput,
    SessionAccretionReport, context_reset_reason_for_recent_compaction,
    context_reset_reason_for_report, evaluate_session_accretion, exchange_metrics,
    recent_restart_count_from_session_log, resolve_clear_threshold,
    resolve_queue_context_reset_opt_in,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RecentExchangeCompaction {
    file: String,
    timestamp: u64,
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
    let frontmatter_flag = if let Ok(content) = std::fs::read_to_string(file)
        && let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(&content)
    {
        fm.queue_context_reset
    } else {
        None
    };
    let project_flag =
        agent_doc_project_config_io::load_project_for_doc(file).agent_doc_queue_context_reset;
    resolve_queue_context_reset_opt_in(frontmatter_flag, project_flag)
}

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
    let frontmatter_threshold = if let Ok(content) = std::fs::read_to_string(file)
        && let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(&content)
    {
        fm.clear_threshold
    } else {
        None
    };
    let project_threshold =
        agent_doc_project_config_io::load_project_for_doc(file).agent_doc_clear_threshold;
    resolve_clear_threshold(frontmatter_threshold, project_threshold)
}

pub fn queue_context_reset_reason(
    file: &Path,
    last_context_clear_at: Option<u64>,
) -> Result<Option<String>> {
    if let Some(reason) = context_reset_reason_for_recent_compaction(
        recent_exchange_compaction_timestamp(file)?,
        last_context_clear_at,
    ) {
        return Ok(Some(reason));
    }

    let report = inspect(file)?;
    Ok(context_reset_reason_for_report(&report))
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

fn inspect_at(file: &Path, content: &str, now: u64) -> Result<SessionAccretionReport> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    inspect_at_with_context(file, content, now, &rc)
}

fn inspect_at_with_context(
    file: &Path,
    content: &str,
    now: u64,
    rc: &crate::graph::RunContext,
) -> Result<SessionAccretionReport> {
    Ok(evaluate_session_accretion(session_accretion_input(
        file, content, now, rc,
    )?))
}

fn session_accretion_input(
    file: &Path,
    content: &str,
    now: u64,
    rc: &crate::graph::RunContext,
) -> Result<SessionAccretionInput> {
    let (exchange_lines, response_sections) = exchange_metrics(content);
    let (recent_committed_cycles, recent_noop_closeouts) = recent_cycle_metrics(file, now)?;
    let startup_miss_active = crate::startup_miss::load(file)?.is_some();
    let parsed_frontmatter =
        crate::frontmatter_io::parse_for_file_with_context(content, file, rc).ok();
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

    Ok(SessionAccretionInput {
        document: file.display().to_string(),
        exchange_lines,
        response_sections,
        recent_committed_cycles,
        recent_noop_closeouts,
        recent_restart_count,
        recent_session_loss_count,
        startup_miss_active,
        clear_threshold: clear_threshold_for_doc(file),
        auto_compact_opt_in,
        queue_active,
    })
}

fn recent_cycle_metrics(file: &Path, now: u64) -> Result<(usize, usize)> {
    let Some(path) = cycles_log_path(file)? else {
        return Ok((0, 0));
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
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
        let Some(timestamp) = agent_doc_log_time::parse_log_timestamp(&entry.timestamp) else {
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
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(0);
    };
    Ok(recent_restart_count_from_session_log(&content, now))
}

fn cycles_log_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(root.join(".agent-doc/logs/cycles.jsonl")))
}

fn recent_exchange_compaction_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
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
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
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
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(
        root.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
    ))
}

fn relative_file_key(file: &Path) -> Option<String> {
    let canonical = file.canonicalize().ok()?;
    let root = agent_doc_fs::find_project_root(&canonical)?;
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
        let g = compaction_guidance("/tmp/doc.md", true, true);
        assert!(g.starts_with("Run `agent-doc compact"), "got {g}");
        // Queue active, no opt-in → don't-stall guidance.
        let g = compaction_guidance("/tmp/doc.md", false, true);
        assert!(g.contains("do NOT stall the queue"), "got {g}");
        // Idle (no queue), no opt-in → ask the user.
        let g = compaction_guidance("/tmp/doc.md", false, false);
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
            agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
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
        // owner turn should keep draining in pane instead.
        let draining = restart_or_drain_guidance(true);
        assert!(draining.contains("do NOT stop"), "got: {draining}");
        assert!(draining.contains("#drain-no-defer"), "got: {draining}");
        assert!(!draining.contains("Restart cleanly"), "got: {draining}");
    }
}

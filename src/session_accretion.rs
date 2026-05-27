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
const BLOCK_EXCHANGE_LINES: usize = 240;
const WARN_RESPONSE_SECTIONS: usize = 8;
const BLOCK_RESPONSE_SECTIONS: usize = 12;
const WARN_RECENT_COMMITTED_CYCLES: usize = 6;
const BLOCK_RECENT_COMMITTED_CYCLES: usize = 10;
const WARN_RECENT_NOOP_CLOSEOUTS: usize = 2;
const BLOCK_RECENT_NOOP_CLOSEOUTS: usize = 3;
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

fn inspect_at(file: &Path, content: &str, now: u64) -> Result<SessionAccretionReport> {
    let (exchange_lines, response_sections) = exchange_metrics(content);
    let (recent_committed_cycles, recent_noop_closeouts) = recent_cycle_metrics(file, now)?;
    let startup_miss_active = crate::startup_miss::load(file)?.is_some();
    let parsed_frontmatter = crate::frontmatter::parse_for_file(content, file).ok();
    let session_id = parsed_frontmatter
        .as_ref()
        .and_then(|(fm, _)| fm.session.as_ref().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty());
    let auto_compact_opt_in = parsed_frontmatter
        .as_ref()
        .map(|(fm, _)| fm.auto_compact.is_some())
        .unwrap_or(false)
        || crate::project_config::load_project_for_doc(file)
            .agent_doc_auto_compact
            .is_some();
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
            if auto_compact_opt_in {
                guidance.push(format!(
                    "Run `agent-doc compact {} --commit` before another large turn.",
                    file.display()
                ));
            } else {
                guidance.push(format!(
                    "Exchange is large; ask the user before compacting. Auto-compact requires an explicit `agent_doc_auto_compact` opt-in in frontmatter or `.agent-doc/config.toml` (currently off). If the user approves, run `agent-doc compact {} --commit`.",
                    file.display()
                ));
            }
        }
        if recent_restart_count >= WARN_RESTART_EVENTS
            || startup_miss_active
            || recent_session_loss_count >= RECENT_SESSION_LOSS_WARN
        {
            guidance.push(
                "Restart cleanly from the current committed boundary before continuing."
                    .to_string(),
            );
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
    let recent_compaction_timestamp = load_recent_exchange_compaction(file)?
        .map(|marker| marker.timestamp)
        .filter(|timestamp| *timestamp >= window_start);
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
        let Some(timestamp) = entry.timestamp.parse::<u64>().ok() else {
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
            .and_then(|(ts, _)| ts.parse::<u64>().ok());
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
}

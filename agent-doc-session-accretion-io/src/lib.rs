//! Session-accretion filesystem adapters.

use anyhow::Result;
use std::path::{Path, PathBuf};

use agent_doc_session_accretion::{
    POST_COMPACTION_NOOP_GRACE_SECS, RECENT_WINDOW_SECS, SessionAccretionInput,
    SessionAccretionReport, context_reset_reason_for_recent_compaction,
    context_reset_reason_for_report, evaluate_session_accretion, exchange_metrics,
    recent_restart_count_from_session_log, resolve_clear_threshold,
    resolve_queue_context_reset_opt_in,
};

/// Build a session-accretion report for a concrete document from local file
/// state and binary-owned logs.
pub fn inspect(file: &Path) -> Result<SessionAccretionReport> {
    let content = std::fs::read_to_string(file)?;
    inspect_at(file, &content, current_epoch_secs())
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
/// `agent_doc_queue_context_reset` opt-in.
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
    Ok(evaluate_session_accretion(session_accretion_input(
        file, content, now,
    )?))
}

fn session_accretion_input(file: &Path, content: &str, now: u64) -> Result<SessionAccretionInput> {
    let (exchange_lines, response_sections) = exchange_metrics(content);
    let (recent_committed_cycles, recent_noop_closeouts) = recent_cycle_metrics(file, now)?;
    let startup_miss_active =
        agent_doc_supervisor_io::startup_miss::load_startup_miss(file)?.is_some();
    let parsed_frontmatter = agent_doc_frontmatter_io::session::parse_for_file(content, file).ok();
    let session_id = parsed_frontmatter
        .as_ref()
        .and_then(|(fm, _)| fm.session.as_ref().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty());
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    let auto_compact_opt_in = parsed_frontmatter
        .as_ref()
        .map(|(fm, _)| fm.auto_compact.is_some())
        .unwrap_or(false)
        || project_config.agent_doc_auto_compact.is_some();
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
            agent_doc_supervisor_io::startup_miss::recent_session_loss_window(file, session_id)
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

const RECENT_EXCHANGE_COMPACTION_SCOPE: &str = "recent_exchange_compaction";

/// Record that the binary compacted this document's exchange recently.
pub fn record_recent_exchange_compaction(file: &Path) -> Result<()> {
    let Some((root, canonical, document_hash)) = recent_exchange_compaction_scope(file)? else {
        return Ok(());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_coordination_lease_in_db(
        &conn,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: RECENT_EXCHANGE_COMPACTION_SCOPE.to_string(),
            scope_id: document_hash,
            holder: canonical.display().to_string(),
            holder_pid: None,
            heartbeat_secs: current_epoch_secs(),
        },
    )
}

/// Return a recent exchange-compaction timestamp when the state projection is inside
/// the post-compaction no-op grace window.
pub fn recent_exchange_compaction_timestamp(file: &Path) -> Result<Option<u64>> {
    recent_exchange_compaction_timestamp_at(file, current_epoch_secs())
}

pub fn recent_exchange_compaction_timestamp_at(file: &Path, now: u64) -> Result<Option<u64>> {
    Ok(
        load_recent_exchange_compaction_timestamp(file)?.filter(|timestamp| {
            now.saturating_sub(*timestamp)
                <= agent_doc_session_accretion::POST_COMPACTION_NOOP_GRACE_SECS
        }),
    )
}

pub fn cycles_log_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(root.join(".agent-doc/logs/cycles.jsonl")))
}

pub fn session_log_path(file: &Path, session_id: &str) -> Result<Option<PathBuf>> {
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

pub fn relative_file_key(file: &Path) -> Option<String> {
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

pub fn recent_cycle_metrics(file: &Path, now: u64) -> Result<(usize, usize)> {
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
        let Ok(entry) = serde_json::from_str::<agent_doc_ops_log_io::CycleEntry>(line) else {
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

pub fn recent_restart_metrics(file: &Path, session_id: &str, now: u64) -> Result<usize> {
    let Some(path) = session_log_path(file, session_id)? else {
        return Ok(0);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(0);
    };
    Ok(recent_restart_count_from_session_log(&content, now))
}

/// Whether queue context reset is enabled by document frontmatter or project config.
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

/// Resolve the context usage percentage threshold for a pre-emptive context reset.
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

fn recent_exchange_compaction_scope(file: &Path) -> Result<Option<(PathBuf, PathBuf, String)>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    Ok(Some((root, canonical, hash)))
}

fn load_recent_exchange_compaction_timestamp(file: &Path) -> Result<Option<u64>> {
    let Some((root, _, document_hash)) = recent_exchange_compaction_scope(file)? else {
        return Ok(None);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    Ok(
        agent_doc_sqlite::state_store::load_coordination_lease_from_db(
            &conn,
            RECENT_EXCHANGE_COMPACTION_SCOPE,
            &document_hash,
        )?
        .map(|lease| lease.heartbeat_secs),
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
    use agent_doc_session_accretion::SessionAccretionLevel;
    use std::io::Write;

    fn setup_doc(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, content).unwrap();
        (dir, doc)
    }

    #[test]
    fn recent_exchange_compaction_state_roundtrips_without_sidecar() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();

        assert!(
            recent_exchange_compaction_timestamp(&doc)
                .unwrap()
                .is_none()
        );
        record_recent_exchange_compaction(&doc).unwrap();
        assert!(
            recent_exchange_compaction_timestamp(&doc)
                .unwrap()
                .is_some()
        );
        assert!(
            !dir.path()
                .join(".agent-doc/state/session-accretion-compaction")
                .exists()
        );
    }

    #[test]
    fn recent_exchange_compaction_state_expires() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        record_recent_exchange_compaction(&doc).unwrap();
        let timestamp = recent_exchange_compaction_timestamp(&doc)
            .unwrap()
            .expect("compaction state should be recent");

        let expired_at =
            timestamp + agent_doc_session_accretion::POST_COMPACTION_NOOP_GRACE_SECS + 1;
        assert!(
            recent_exchange_compaction_timestamp_at(&doc, expired_at)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn accretion_log_paths_resolve_under_project_logs() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("nested/session.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();

        assert_eq!(
            cycles_log_path(&doc).unwrap(),
            Some(dir.path().join(".agent-doc/logs/cycles.jsonl"))
        );
        assert_eq!(
            session_log_path(&doc, "session-1").unwrap(),
            Some(dir.path().join(".agent-doc/logs/session-1.log"))
        );
        assert_eq!(
            relative_file_key(&doc),
            Some("nested/session.md".to_string())
        );
    }

    fn write_cycles_log(doc: &Path, entries: &[agent_doc_ops_log_io::CycleEntry]) {
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
    fn recent_cycle_metrics_counts_current_doc_commits_and_noops() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        let other = dir.path().join("other.md");
        std::fs::write(&other, "body").unwrap();
        let now = 1_700_000_000;
        write_cycles_log(
            &doc,
            &[
                agent_doc_ops_log_io::CycleEntry {
                    timestamp: (now - 10).to_string(),
                    file: "session.md".to_string(),
                    op: "commit".to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    timestamp: (now - 5).to_string(),
                    file: "session.md".to_string(),
                    op: "commit_noop".to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    timestamp: (now - 5).to_string(),
                    file: "other.md".to_string(),
                    op: "commit_noop".to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        assert_eq!(recent_cycle_metrics(&doc, now).unwrap(), (2, 1));
    }

    #[test]
    fn recent_cycle_metrics_ignores_noops_before_recent_compaction_grace() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        let now = 1_700_000_000;
        write_cycles_log(
            &doc,
            &[
                agent_doc_ops_log_io::CycleEntry {
                    timestamp: (now - 20).to_string(),
                    file: "session.md".to_string(),
                    op: "commit_noop".to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    timestamp: (now - 5).to_string(),
                    file: "session.md".to_string(),
                    op: "commit_noop".to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );
        record_recent_exchange_compaction(&doc).unwrap();

        assert_eq!(recent_cycle_metrics(&doc, now).unwrap(), (0, 0));
    }

    #[test]
    fn recent_restart_metrics_reads_session_log() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        let now = 1_700_000_000;
        let log_path = dir.path().join(".agent-doc/logs/session-1.log");
        std::fs::write(
            log_path,
            format!(
                "[{}] fresh_restart reason=recycle\n[{}] auto_trigger_timeout pane=%1\n[{}] ordinary line\n",
                now - 3,
                now - 2,
                now - 1
            ),
        )
        .unwrap();

        assert_eq!(recent_restart_metrics(&doc, "session-1", now).unwrap(), 2);
    }

    #[test]
    fn queue_context_reset_prefers_frontmatter_then_project_config() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_queue_context_reset = true\n",
        )
        .unwrap();
        let doc_off = dir.path().join("off.md");
        std::fs::write(
            &doc_off,
            "---\nagent_doc_queue_context_reset: false\n---\nbody\n",
        )
        .unwrap();
        let doc_project = dir.path().join("project.md");
        std::fs::write(&doc_project, "---\n---\nbody\n").unwrap();

        assert!(!queue_context_reset_opted_in(&doc_off));
        assert!(queue_context_reset_opted_in(&doc_project));
    }

    #[test]
    fn clear_threshold_prefers_frontmatter_then_project_config_and_clamps() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_clear_threshold = 120\n",
        )
        .unwrap();
        let doc_frontmatter = dir.path().join("frontmatter.md");
        std::fs::write(
            &doc_frontmatter,
            "---\nagent_doc_clear_threshold: 70\n---\nbody\n",
        )
        .unwrap();
        let doc_project = dir.path().join("project.md");
        std::fs::write(&doc_project, "---\n---\nbody\n").unwrap();

        assert_eq!(clear_threshold_for_doc(&doc_frontmatter), 70);
        assert_eq!(clear_threshold_for_doc(&doc_project), 100);
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
    }

    #[test]
    fn queue_context_reset_reason_if_opted_in_gates_context_reset() {
        let exchange_lines = (0..170)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let off = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n"
        );
        let (_dir_off, doc_off) = setup_doc(&off);
        assert!(
            queue_context_reset_reason(&doc_off, None)
                .unwrap()
                .is_some()
        );
        assert!(
            queue_context_reset_reason_if_opted_in(&doc_off, None)
                .unwrap()
                .is_none()
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
    fn inspect_uses_project_config_auto_compact_opt_in() {
        let exchange_lines = (0..170)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange_lines}\n<!-- /agent:exchange -->\n"
        );
        let (dir, doc) = setup_doc(&content);
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
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
            "expected imperative compact guidance, got {:?}",
            report.guidance
        );
    }

    #[test]
    fn inspect_blocks_on_restart_heavy_churn_with_active_startup_miss() {
        let now = 8_000;
        let session_id = "session-123";
        let content = format!(
            "---\nagent_doc_session: {session_id}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n"
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
        agent_doc_supervisor_io::startup_miss::record_startup_miss(
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
    }
}

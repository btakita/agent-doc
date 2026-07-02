//! Session-accretion filesystem adapters.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use agent_doc_session_accretion::{
    POST_COMPACTION_NOOP_GRACE_SECS, RECENT_WINDOW_SECS, recent_restart_count_from_session_log,
    resolve_clear_threshold, resolve_queue_context_reset_opt_in,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RecentExchangeCompaction {
    file: String,
    timestamp: u64,
}

/// Record that the binary compacted this document's exchange recently.
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

/// Return a recent exchange-compaction timestamp when the marker is still inside
/// the post-compaction no-op grace window.
pub fn recent_exchange_compaction_timestamp(file: &Path) -> Result<Option<u64>> {
    recent_exchange_compaction_timestamp_at(file, current_epoch_secs())
}

pub fn recent_exchange_compaction_timestamp_at(file: &Path, now: u64) -> Result<Option<u64>> {
    Ok(load_recent_exchange_compaction(file)?
        .map(|marker| marker.timestamp)
        .filter(|timestamp| {
            now.saturating_sub(*timestamp)
                <= agent_doc_session_accretion::POST_COMPACTION_NOOP_GRACE_SECS
        }))
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

fn recent_exchange_compaction_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
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

    #[test]
    fn recent_exchange_compaction_marker_roundtrips() {
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
    }

    #[test]
    fn recent_exchange_compaction_marker_expires() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        record_recent_exchange_compaction(&doc).unwrap();
        let timestamp = recent_exchange_compaction_timestamp(&doc)
            .unwrap()
            .expect("marker should be recent");

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
}

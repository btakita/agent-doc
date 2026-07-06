//! Ops-log read/write adapters.

use agent_doc_turn::op_log::{
    IPC_PROOF_INSUFFICIENT_EVENT, is_write_completed_commit_missing_event, strip_timestamp_prefix,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Structured cycle log entry for reproducible operation tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleEntry {
    /// Operation type (e.g., "write_inline", "write_template", "write_stream", "commit").
    pub op: String,
    /// Document path (relative to project root).
    pub file: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Git commit hash after the operation (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    /// SHA256 of the snapshot content after the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    /// SHA256 of the document file content after the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
}

/// Maximum size (bytes) an individual best-effort log (`ops.log`,
/// `cycles.jsonl`) may reach before it is rotated aside.
pub const LOG_ROTATE_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default)]
pub struct OpsLogTracking<'a> {
    pub doc_stem: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub turn_id: Option<&'a str>,
}

/// Process-local cache of `agent_doc_session` keyed by canonical document path.
static SESSION_ID_CACHE: LazyLock<Mutex<HashMap<PathBuf, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Best-effort `agent_doc_session` for `file`, cached per process.
fn cached_session_id(file: &Path) -> Option<String> {
    let key = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if let Ok(cache) = SESSION_ID_CACHE.lock()
        && let Some(sid) = cache.get(&key)
    {
        return Some(sid.clone());
    }
    let content = std::fs::read_to_string(file).ok()?;
    let session = agent_doc_frontmatter::frontmatter::parse(&content)
        .ok()?
        .0
        .session?;
    if session.is_empty() {
        return None;
    }
    if let Ok(mut cache) = SESSION_ID_CACHE.lock() {
        cache.insert(key, session.clone());
    }
    Some(session)
}

/// Append a timestamped line to `.agent-doc/logs/ops.log`.
///
/// Best-effort: silently returns on I/O errors. Each line includes document,
/// session, and turn attribution when those facts are available.
pub fn log_op(file: &Path, message: &str) {
    let _ = try_log_op(file, message);
}

fn try_log_op(file: &Path, message: &str) -> Option<()> {
    let canonical = file.canonicalize().ok()?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let doc_stem = file.file_stem().and_then(|n| n.to_str());
    let session = cached_session_id(file);
    let turn = agent_doc_cycle_state_io::load_closeout_projection(file)
        .ok()
        .flatten()
        .and_then(|projection| projection.cycle_id)
        .or_else(|| {
            agent_doc_cycle_state_io::load_with_closeout_projection(file)
                .ok()
                .flatten()
                .map(|cs| cs.cycle_id)
        });
    append_ops_log_at_project(
        &project_root,
        message,
        OpsLogTracking {
            doc_stem,
            session_id: session.as_deref(),
            turn_id: turn.as_deref(),
        },
    )
}

/// Append a structured cycle entry to `.agent-doc/logs/cycles.jsonl`.
///
/// Best-effort: silently returns on I/O errors.
pub fn log_cycle(
    file: &Path,
    op: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) {
    let _ = append_cycle_log_for_file(file, op, snapshot_content, file_content);
}

/// Best-effort size-based rotation for an append-only log.
fn rotate_log_if_oversized(log_path: &Path, max_bytes: u64) {
    let len = match std::fs::metadata(log_path) {
        Ok(meta) => meta.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            eprintln!("[ops-log] stat of {} failed: {e}", log_path.display());
            return;
        }
    };
    if len < max_bytes {
        return;
    }
    let Some(name) = log_path.file_name().and_then(|n| n.to_str()) else {
        eprintln!(
            "[ops-log] cannot rotate {}: non-UTF-8 file name",
            log_path.display()
        );
        return;
    };
    let rotated = log_path.with_file_name(format!("{name}.1"));
    if let Err(e) = std::fs::rename(log_path, &rotated) {
        eprintln!(
            "[ops-log] rotation of {} -> {} failed: {e}",
            log_path.display(),
            rotated.display()
        );
    }
}

fn logs_dir(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc/logs")
}

pub fn append_ops_log_at_project(
    project_root: &Path,
    message: &str,
    tracking: OpsLogTracking<'_>,
) -> Option<()> {
    let logs_dir = logs_dir(project_root);
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join("ops.log");
    rotate_log_if_oversized(&log_path, LOG_ROTATE_MAX_BYTES);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()?;
    let suffix = agent_doc_log_time::format_ops_log_tracking_suffix(
        tracking.doc_stem,
        tracking.session_id,
        tracking.turn_id,
    );
    let line = agent_doc_log_time::format_ops_log_line(
        agent_doc_log_time::current_epoch_secs(),
        message,
        &suffix,
    );
    writeln!(f, "{line}").ok()
}

fn git_head_hash(file: &Path) -> Option<String> {
    agent_doc_git_io::revision::last_commit_hash(file)
        .ok()
        .flatten()
}

pub fn append_cycle_log_for_file(
    file: &Path,
    op: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Option<()> {
    let canonical = file.canonicalize().ok()?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let relative = canonical
        .strip_prefix(&project_root)
        .unwrap_or(&canonical)
        .to_string_lossy()
        .to_string();

    let entry = CycleEntry {
        op: op.to_string(),
        file: relative,
        timestamp: agent_doc_log_time::current_log_timestamp(),
        commit_hash: git_head_hash(file),
        snapshot_hash: snapshot_content.map(agent_doc_hash::content_hash),
        file_hash: file_content.map(agent_doc_hash::content_hash),
    };
    append_cycle_entry_at_project(&project_root, &entry)
}

pub fn append_cycle_entry_at_project(project_root: &Path, entry: &CycleEntry) -> Option<()> {
    let logs_dir = logs_dir(project_root);
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join("cycles.jsonl");
    rotate_log_if_oversized(&log_path, LOG_ROTATE_MAX_BYTES);
    let json = serde_json::to_string(entry).ok()?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()?;
    writeln!(f, "{json}").ok()
}

fn read_optional_text(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn ops_log_context_for_file(file: &Path) -> Result<Option<(PathBuf, String, String)>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(None);
    };
    let log_path = project_root.join(".agent-doc/logs/ops.log");
    let Some(content) = read_optional_text(&log_path)? else {
        return Ok(None);
    };
    Ok(Some((
        canonical.clone(),
        file.display().to_string(),
        content,
    )))
}

/// Return the message portion of the last non-empty line in `ops.log`,
/// stripped of the timestamp prefix.
pub fn last_ops_event(file: &Path) -> Result<Option<String>> {
    let Some((canonical, requested_display, content)) = ops_log_context_for_file(file)? else {
        return Ok(None);
    };
    let canonical_display = canonical.display().to_string();
    let last = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !is_read_only_document_resolution_event(strip_timestamp_prefix(line)))
        .rfind(|line| {
            line.contains(&format!("file={canonical_display}"))
                || line.contains(&format!("file={requested_display}"))
        })
        .or_else(|| {
            content
                .lines()
                .filter(|line| !line.trim().is_empty())
                .filter(|line| {
                    !is_read_only_document_resolution_event(strip_timestamp_prefix(line))
                })
                .rfind(|_| true)
        })
        .map(|line| strip_timestamp_prefix(line).to_string());
    Ok(last)
}

fn is_read_only_document_resolution_event(event: &str) -> bool {
    event.starts_with("realtime_doc_resolve ")
        || event.starts_with("realtime_doc_resolve_crdt_error ")
        || event.starts_with("crdt_current_text_unavailable ")
        || event.starts_with("document_model_ensure_start ")
        || event.starts_with("document_model_ensure_publish_requested ")
        || event.starts_with("document_model_ensure_failed ")
}

pub fn latest_ipc_proof_diagnostic(file: &Path) -> Result<Option<String>> {
    let Some((canonical, requested_display, content)) = ops_log_context_for_file(file)? else {
        return Ok(None);
    };
    let canonical_display = canonical.display().to_string();
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .map(strip_timestamp_prefix)
        .find(|event| {
            event.starts_with(IPC_PROOF_INSUFFICIENT_EVENT)
                && (event.contains(&format!("file={canonical_display}"))
                    || event.contains(&format!("file={requested_display}")))
        })
        .map(str::to_string))
}

pub fn latest_ipc_proof_diagnostic_hint(file: &Path) -> Result<Option<String>> {
    Ok(latest_ipc_proof_diagnostic(file)?
        .map(|event| format!("latest IPC proof diagnostic: {event}")))
}

pub fn detect_write_completed_commit_missing(file: &Path) -> Result<Option<String>> {
    Ok(last_ops_event(file)?.filter(|event| is_write_completed_commit_missing_event(event)))
}

pub fn latest_unclosed_write_completed_commit_missing(file: &Path) -> Result<Option<String>> {
    let Some((canonical, requested_display, content)) = ops_log_context_for_file(file)? else {
        return Ok(None);
    };
    let canonical_display = canonical.display().to_string();
    for event in content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .map(strip_timestamp_prefix)
        .filter(|event| {
            event.contains(&format!("file={canonical_display}"))
                || event.contains(&format!("file={requested_display}"))
        })
    {
        if is_write_completed_commit_missing_event(event) {
            return Ok(Some(event.to_string()));
        }
        if event.starts_with("commit_success ")
            || event.starts_with("repair_commit_boundary_recovered ")
        {
            return Ok(None);
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_project(root: &Path) -> PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "---\n---\n").unwrap();
        doc
    }

    #[test]
    fn rotate_log_moves_oversized_file_to_backup() {
        let tmp = tempfile::TempDir::new().unwrap();
        let log_path = tmp.path().join("ops.log");
        std::fs::write(&log_path, "0123456789").unwrap();

        rotate_log_if_oversized(&log_path, 10);

        assert!(!log_path.exists());
        let backup = tmp.path().join("ops.log.1");
        assert!(backup.exists());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "0123456789");
    }

    #[test]
    fn rotate_log_leaves_small_file_in_place() {
        let tmp = tempfile::TempDir::new().unwrap();
        let log_path = tmp.path().join("ops.log");
        std::fs::write(&log_path, "small").unwrap();

        rotate_log_if_oversized(&log_path, 64);

        assert!(log_path.exists());
        assert!(!tmp.path().join("ops.log.1").exists());
        assert_eq!(std::fs::read_to_string(&log_path).unwrap(), "small");
    }

    #[test]
    fn rotate_log_replaces_existing_backup() {
        let tmp = tempfile::TempDir::new().unwrap();
        let log_path = tmp.path().join("ops.log");
        let backup = tmp.path().join("ops.log.1");
        std::fs::write(&backup, "stale-backup").unwrap();
        std::fs::write(&log_path, "fresh-oversized").unwrap();

        rotate_log_if_oversized(&log_path, 1);

        assert!(!log_path.exists());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "fresh-oversized");
    }

    #[test]
    fn rotate_log_absent_file_is_noop() {
        let tmp = tempfile::TempDir::new().unwrap();
        let log_path = tmp.path().join("ops.log");

        rotate_log_if_oversized(&log_path, 10);

        assert!(!log_path.exists());
        assert!(!tmp.path().join("ops.log.1").exists());
    }

    #[test]
    fn append_ops_log_at_project_creates_timestamped_line() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();

        append_ops_log_at_project(
            tmp.path(),
            "test_event file=test.md",
            OpsLogTracking {
                doc_stem: Some("test"),
                session_id: Some("session-1"),
                turn_id: Some("turn-1"),
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        let line = content.lines().next().unwrap();
        assert!(line.contains("test_event file=test.md"));
        assert!(line.contains("doc=test"));
        assert!(line.contains("session=session-1"));
        assert!(line.contains("turn=turn-1"));
        let inner = line
            .strip_prefix('[')
            .and_then(|r| r.split_once(']'))
            .map(|(ts, _)| ts)
            .expect("bracketed timestamp");
        assert!(agent_doc_log_time::parse_log_timestamp(inner).is_some());
    }

    #[test]
    fn log_op_turn_tracking_prefers_latest_projection_over_stale_cycle_sidecar() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        let content = "---\nagent_doc_session: session-1\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();

        let first =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        let sidecar_path = std::fs::read_dir(tmp.path().join(".agent-doc/state/cycles"))
            .unwrap()
            .next()
            .expect("cycle state sidecar")
            .unwrap()
            .path();
        let stale_first_sidecar = std::fs::read(&sidecar_path).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(2));
        let second =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        assert_ne!(first.cycle_id, second.cycle_id);
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        std::fs::write(&sidecar_path, stale_first_sidecar).unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .cycle_id,
            first.cycle_id
        );

        log_op(&doc, "projection_turn_event");

        let content = std::fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        let line = content
            .lines()
            .find(|line| line.contains("projection_turn_event"))
            .expect("ops-log event");
        assert!(line.contains(&format!("turn={}", second.cycle_id)));
    }

    #[test]
    fn append_cycle_log_for_file_writes_jsonl() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "content").unwrap();

        append_cycle_log_for_file(&doc, "write_inline", Some("snapshot"), Some("content")).unwrap();

        let content =
            std::fs::read_to_string(tmp.path().join(".agent-doc/logs/cycles.jsonl")).unwrap();
        let entry: serde_json::Value =
            serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(entry["op"], "write_inline");
        assert!(entry["file"].as_str().unwrap().contains("session.md"));
        assert!(entry["snapshot_hash"].is_string());
        assert!(entry["file_hash"].is_string());
    }

    #[test]
    fn last_ops_event_missing_log_returns_none() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        assert!(last_ops_event(&doc).unwrap().is_none());
    }

    #[test]
    fn last_ops_event_empty_log_returns_none() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        std::fs::write(tmp.path().join(".agent-doc/logs/ops.log"), "\n\n").unwrap();
        assert!(last_ops_event(&doc).unwrap().is_none());
    }

    #[test]
    fn last_ops_event_returns_final_event_stripped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        std::fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] preflight_diff_start file=x\n[101] ipc_write_consumed file=x patches=1\n",
        )
        .unwrap();

        assert_eq!(
            last_ops_event(&doc).unwrap().unwrap(),
            "ipc_write_consumed file=x patches=1"
        );
    }

    #[test]
    fn last_ops_event_prefers_matching_file_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let other = tmp.path().join("other.md");
        std::fs::write(&other, "body").unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            format!(
                "[100] ipc_write_consumed file={} patches=1\n[101] preflight_diff_start file={}\n",
                doc.display(),
                other.display()
            ),
        )
        .unwrap();

        assert_eq!(
            last_ops_event(&doc).unwrap().unwrap(),
            format!("ipc_write_consumed file={} patches=1", doc.display())
        );
    }

    #[test]
    fn last_ops_event_ignores_read_only_document_resolution_telemetry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        std::fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            format!(
                "[100] ipc_write_consumed file={} patches=1\n[101] realtime_doc_resolve authority=disk reason=editor_absent file={}\n",
                doc.display(),
                doc.display()
            ),
        )
        .unwrap();

        assert_eq!(
            last_ops_event(&doc).unwrap().unwrap(),
            format!("ipc_write_consumed file={} patches=1", doc.display())
        );
    }

    #[test]
    fn latest_ipc_proof_diagnostic_prefers_matching_file_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        let other = tmp.path().join("other.md");
        std::fs::write(&other, "body").unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            format!(
                "[100] ipc_proof_insufficient file={} invariant=no_ack recovery=retry_without_disk_write\n[101] ipc_proof_insufficient file={} invariant=missing_response_probe recovery=retry_without_disk_write\n",
                other.display(),
                doc.display()
            ),
        )
        .unwrap();

        let diagnostic = latest_ipc_proof_diagnostic(&doc).unwrap().unwrap();
        assert!(diagnostic.contains("invariant=missing_response_probe"));
        assert!(diagnostic.contains("recovery=retry_without_disk_write"));
    }

    #[test]
    fn detect_write_completed_commit_missing_returns_last_write_event() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        std::fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            "[100] snapshot_saved_file_ipc file=x snap_len=10\n",
        )
        .unwrap();

        assert_eq!(
            detect_write_completed_commit_missing(&doc)
                .unwrap()
                .unwrap(),
            "snapshot_saved_file_ipc file=x snap_len=10"
        );
    }

    #[test]
    fn latest_unclosed_write_completed_commit_missing_skips_read_diagnostics() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = make_project(tmp.path());
        std::fs::write(
            tmp.path().join(".agent-doc/logs/ops.log"),
            format!(
                "[100] ipc_write_consumed file={} patches=1\n[101] realtime_doc_resolve authority=disk file={}\n",
                doc.display(),
                doc.display()
            ),
        )
        .unwrap();

        assert_eq!(
            latest_unclosed_write_completed_commit_missing(&doc)
                .unwrap()
                .unwrap(),
            format!("ipc_write_consumed file={} patches=1", doc.display())
        );
    }
}

//! # Module: ops_log
//!
//! ## Spec
//! - `log_op(file, message)` appends a timestamped text line to
//!   `.agent-doc/logs/ops.log`. Best-effort: silently returns on I/O errors.
//!   Each line is `[<iso-ts>] <message> doc=<stem>[ session=<id>][ turn=<cycle_id>]`
//!   (`#opslogtrack`): the doc/session/turn attribution suffix is **appended**
//!   (never prepended) so interleaved entries from multiple documents in one
//!   project ops.log are traceable without disturbing positional message
//!   parsers or `contains()`-based marker scans. `doc` is derived from the path
//!   (no I/O); `session` (cached per process from `agent_doc_session`
//!   frontmatter) and `turn` (cycle-state `cycle_id`) are best-effort.
//! - `log_cycle(file, entry)` appends a structured JSON line to
//!   `.agent-doc/logs/cycles.jsonl` for reproducible operation tracking.
//!   Each entry records the operation type, git commit hash, snapshot content
//!   hash, and document content hash — enough to reconstruct any cycle.
//! - `CycleEntry` is serializable and contains: `op`, `file`, `timestamp`,
//!   `commit_hash`, `snapshot_hash`, `file_hash`.
//!
//! ## Agentic Contracts
//! - Both log functions are best-effort and never panic or return errors.
//! - `cycles.jsonl` uses newline-delimited JSON (one object per line) for
//!   easy parsing by both humans and tools.
//! - `commit_hash` and `snapshot_hash` together identify the exact cycle state.
//!   `agent-doc replay` can reconstruct any cycle from these references.
//!
//! ## Evals
//! - `log_op_creates_file_and_appends`: two calls → two lines in ops.log
//! - `log_op_no_panic_on_missing_project_root`: no .agent-doc dir → silent noop
//! - `log_cycle_writes_jsonl`: cycle entry → valid JSON line in cycles.jsonl
//! - `log_cycle_appends_multiple`: multiple entries → multiple lines

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use crate::graph::RunContext;

/// Process-local cache of `agent_doc_session` keyed by canonical document path
/// (`#opslogtrack`). The session id is immutable for a document's lifetime, so
/// parsing the frontmatter once and caching it avoids a YAML parse on every
/// best-effort `ops.log` line.
static SESSION_ID_CACHE: LazyLock<Mutex<HashMap<PathBuf, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Best-effort `agent_doc_session` for `file`, cached per process
/// (`#opslogtrack`). Prefers a cached [`RunContext`] document body to avoid a
/// re-read; falls back to reading the file. Returns `None` when the document has
/// no `agent_doc_session` frontmatter (or cannot be read).
fn cached_session_id(file: &Path, rc: Option<&RunContext>) -> Option<String> {
    let key = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if let Ok(cache) = SESSION_ID_CACHE.lock()
        && let Some(sid) = cache.get(&key)
    {
        return Some(sid.clone());
    }
    let content = match rc {
        Some(rc) => rc.doc_content(),
        None => std::fs::read_to_string(file).ok()?,
    };
    let session = agent_doc_core::frontmatter::parse(&content)
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

/// Build the `ops.log` tracking suffix (`#opslogtrack`) appended to each line so
/// interleaved entries from multiple documents in one project `ops.log` are
/// attributable: ` doc=<stem> session=<id> turn=<cycle_id>`.
///
/// Appended (not prepended) so it never disturbs positional message parsers
/// (for example `gate_verify`'s `strip_prefix("s760 clear-decision ")`) and so
/// `contains()`-based ops.log marker scans keep matching. `doc` is always
/// present (derived from the path, no I/O); `session` and `turn` are best-effort
/// and omitted when unavailable.
fn ops_log_tracking_suffix(file: &Path, rc: Option<&RunContext>) -> String {
    let mut suffix = String::new();
    if let Some(stem) = file.file_stem().and_then(|n| n.to_str()) {
        suffix.push_str(" doc=");
        suffix.push_str(stem);
    }
    if let Some(session) = cached_session_id(file, rc) {
        suffix.push_str(" session=");
        suffix.push_str(&session);
    }
    let turn = match rc {
        Some(rc) => rc.cycle_state().map(|cs| cs.cycle_id.clone()),
        None => crate::cycle_state::load(file)
            .ok()
            .flatten()
            .map(|cs| cs.cycle_id),
    };
    if let Some(turn) = turn.filter(|t| !t.is_empty()) {
        suffix.push_str(" turn=");
        suffix.push_str(&turn);
    }
    suffix
}

/// Append a timestamped log line to `.agent-doc/logs/ops.log`.
///
/// Finds the project root by walking up from `file` (`fs_util::find_project_root`).
/// Best-effort: silently returns on any I/O error.
pub fn log_op(file: &Path, message: &str) {
    log_op_with_context(file, message, None);
}

/// Append a timestamped log line using an optional cached [`RunContext`].
///
/// Callers that already have a context avoid re-canonicalizing the document and
/// walking ancestors to find `.agent-doc/`.
pub fn log_op_with_context(file: &Path, message: &str, rc: Option<&RunContext>) {
    let _ = try_log_op(file, message, rc);
}

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

/// Compute SHA256 hex hash of content.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Get the current git HEAD commit hash for a file.
fn git_head_hash(file: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%H", "--"])
        .arg(file)
        .output()
        .ok()?;
    if output.status.success() {
        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if hash.is_empty() { None } else { Some(hash) }
    } else {
        None
    }
}

/// Human-readable log timestamp helpers (`#opslogts`) live in `agent-doc-core`
/// so both this crate's writers and `agent-doc-core::gate_verify`'s ops.log
/// scanner share one implementation.
pub use agent_doc_core::log_time::{format_log_timestamp, parse_log_timestamp};

/// Get the current timestamp in ISO 8601 (UTC) format.
fn iso_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_log_timestamp(now)
}

/// Append a structured cycle entry to `.agent-doc/logs/cycles.jsonl`.
///
/// Best-effort: silently returns on any I/O error.
pub fn log_cycle(
    file: &Path,
    op: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) {
    let _ = try_log_cycle(file, op, snapshot_content, file_content);
}

fn try_log_cycle(
    file: &Path,
    op: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Option<()> {
    let canonical = file.canonicalize().ok()?;
    let project_root = crate::fs_util::find_project_root(&canonical)?;
    let logs_dir = project_root.join(".agent-doc/logs");
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join("cycles.jsonl");

    let relative = canonical
        .strip_prefix(&project_root)
        .unwrap_or(&canonical)
        .to_string_lossy()
        .to_string();

    let entry = CycleEntry {
        op: op.to_string(),
        file: relative,
        timestamp: iso_timestamp(),
        commit_hash: git_head_hash(file),
        snapshot_hash: snapshot_content.map(content_hash),
        file_hash: file_content.map(content_hash),
    };

    let json = serde_json::to_string(&entry).ok()?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()?;
    writeln!(f, "{}", json).ok()
}

fn try_log_op(file: &Path, message: &str, rc: Option<&RunContext>) -> Option<()> {
    let project_root = match rc {
        Some(rc) => rc.project_root()?,
        None => {
            let canonical = file.canonicalize().ok()?;
            crate::fs_util::find_project_root(&canonical)?
        }
    };
    let logs_dir = project_root.join(".agent-doc/logs");
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join("ops.log");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()?;
    // `#opslogtrack`: append doc/session/turn attribution so interleaved entries
    // from multiple documents in one project ops.log are traceable.
    let suffix = ops_log_tracking_suffix(file, rc);
    writeln!(f, "[{}] {}{}", format_log_timestamp(ts), message, suffix).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn log_op_creates_file_and_appends() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_root = tmp.path();

        // Create project root marker (.agent-doc dir) and the file
        fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc_path = project_root.join("test.md");
        fs::write(&doc_path, "test").unwrap();

        log_op(&doc_path, "test_event file=test.md");
        log_op(&doc_path, "second_event file=test.md");

        let log_path = project_root.join(".agent-doc/logs/ops.log");
        assert!(log_path.exists(), "ops.log should be created");

        let content = fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "should have 2 log lines");
        assert!(
            lines[0].contains("test_event"),
            "first line should contain message"
        );
        assert!(
            lines[1].contains("second_event"),
            "second line should contain message"
        );

        // `#opslogts` — entries carry a human-readable ISO-8601 UTC timestamp
        // in `[...]`, preserving the `[` prefix + `] ` separator that readers
        // split on.
        assert!(
            lines[0].starts_with('['),
            "should start with timestamp bracket"
        );
        assert!(
            lines[0].contains("] "),
            "should have ] separator after timestamp"
        );
        let inner = lines[0]
            .strip_prefix('[')
            .and_then(|r| r.split_once(']'))
            .map(|(ts, _)| ts)
            .expect("bracketed timestamp");
        assert!(
            inner.contains('T') && inner.ends_with('Z'),
            "ops.log timestamp should be ISO-8601 UTC, got {inner:?}"
        );
        assert!(
            parse_log_timestamp(inner).is_some(),
            "the ops.log timestamp must round-trip through parse_log_timestamp"
        );
    }

    #[test]
    fn log_op_with_context_uses_cached_project_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_root = tmp.path();
        fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc_path = project_root.join("test.md");
        fs::write(&doc_path, "test").unwrap();
        let rc = crate::graph::RunContext::new(doc_path.clone());

        assert!(!rc.is_project_root_cached());
        log_op_with_context(&doc_path, "context_event file=test.md", Some(&rc));
        assert!(rc.is_project_root_cached());

        let log_path = project_root.join(".agent-doc/logs/ops.log");
        let content = fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("context_event file=test.md"));
    }

    #[test]
    fn log_cycle_writes_jsonl() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_root = tmp.path();
        fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc_path = project_root.join("test.md");
        fs::write(&doc_path, "content").unwrap();

        log_cycle(&doc_path, "write_inline", Some("snapshot"), Some("content"));

        let log_path = project_root.join(".agent-doc/logs/cycles.jsonl");
        assert!(log_path.exists(), "cycles.jsonl should be created");

        let content = fs::read_to_string(&log_path).unwrap();
        let entry: serde_json::Value =
            serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(entry["op"], "write_inline");
        assert!(entry["file"].as_str().unwrap().contains("test.md"));
        assert!(entry["snapshot_hash"].is_string());
        assert!(entry["file_hash"].is_string());
    }

    #[test]
    fn log_cycle_appends_multiple() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_root = tmp.path();
        fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc_path = project_root.join("test.md");
        fs::write(&doc_path, "content").unwrap();

        log_cycle(&doc_path, "write_inline", Some("snap1"), Some("file1"));
        log_cycle(&doc_path, "commit", Some("snap2"), Some("file2"));

        let log_path = project_root.join(".agent-doc/logs/cycles.jsonl");
        let content = fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let entry1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let entry2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(entry1["op"], "write_inline");
        assert_eq!(entry2["op"], "commit");
    }

    #[test]
    fn log_op_appends_doc_and_session_tracking_suffix() {
        // `#opslogtrack`: each ops.log line gains a ` doc=<stem> session=<id>`
        // suffix so interleaved multi-document project logs are attributable.
        let tmp = tempfile::TempDir::new().unwrap();
        let project_root = tmp.path();
        fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc_path = project_root.join("plan.md");
        fs::write(
            &doc_path,
            "---\nagent_doc_session: sess-abc-123\n---\n\nbody\n",
        )
        .unwrap();

        // Cache must start empty so the first call exercises the frontmatter parse.
        SESSION_ID_CACHE.lock().unwrap().clear();
        log_op(&doc_path, "write_authority action=routed file=plan.md");

        let log_path = project_root.join(".agent-doc/logs/ops.log");
        let content = fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();

        // doc + session are appended.
        assert!(line.contains("doc=plan"), "must carry doc stem: {line:?}");
        assert!(
            line.contains("session=sess-abc-123"),
            "must carry session id: {line:?}"
        );
        // The original message stays at the FRONT of the body (after `] `) so
        // positional parsers (e.g. gate_verify strip_prefix) and contains()
        // marker scans keep working.
        let body = line.split_once("] ").map(|(_, m)| m).unwrap();
        assert!(
            body.starts_with("write_authority action=routed"),
            "message must precede the tracking suffix: {body:?}"
        );
        assert!(
            body.contains("write_authority action=routed"),
            "marker scan substring must survive: {line:?}"
        );
    }

    #[test]
    fn log_op_no_panic_on_missing_project_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("orphan.md");
        fs::write(&doc_path, "test").unwrap();

        // No .agent-doc dir — should silently return without panic
        log_op(&doc_path, "should_not_crash");

        // Verify no log was created
        assert!(!tmp.path().join(".agent-doc/logs/ops.log").exists());
    }
}

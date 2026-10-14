//! Durable, process-targeted duplicate-supervisor retirement requests (GH #243).
//!
//! A route may discover that another proven supervisor still owns the same
//! document while that duplicate is mid-turn, holds typed input, or is visible
//! to a tmux client. The route cannot safely terminate it. Instead it records a
//! request addressed to the exact supervisor process and pane. That supervisor's
//! existing idle watcher consumes the request only after reaching a safe
//! boundary and revalidating the authoritative owner.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use agent_doc_fs::{document_state_hash, find_project_root};

const CONTROL_DIR: &str = ".agent-doc/control";
const REQUEST_SUFFIX: &str = "duplicate-supervisor-retirement.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateRetirementRequest {
    pub pane: String,
    pub supervisor_pid: u32,
    pub process_start_ticks: u64,
    pub authoritative_owner: String,
    pub window: String,
    pub event: String,
}

fn request_path(file: &Path, supervisor_pid: u32) -> Result<PathBuf> {
    let hash = document_state_hash(file)?;
    let canonical = file.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root
        .join(CONTROL_DIR)
        .join(format!("{hash}.{supervisor_pid}.{REQUEST_SUFFIX}")))
}

#[cfg(unix)]
fn process_start_ticks(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    // Fields after `comm` begin at proc field 3 (`state`); starttime is field
    // 22, therefore the twentieth token in this suffix.
    after_comm.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(not(unix))]
fn process_start_ticks(_pid: u32) -> Option<u64> {
    None
}

fn parse_string(value: &serde_json::Value, field: &str) -> Option<String> {
    value.get(field)?.as_str().map(str::to_string)
}

fn parse_request(body: &str) -> Option<DuplicateRetirementRequest> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    Some(DuplicateRetirementRequest {
        pane: parse_string(&value, "pane")?,
        supervisor_pid: value.get("supervisor_pid")?.as_u64()?.try_into().ok()?,
        process_start_ticks: value.get("process_start_ticks")?.as_u64()?,
        authoritative_owner: parse_string(&value, "authoritative_owner")?,
        window: parse_string(&value, "window")?,
        event: parse_string(&value, "event")?,
    })
}

/// Persist a safe-boundary retirement request for the exact proven supervisor.
/// The `/proc` start tick prevents a later process that reuses the pid from
/// consuming an old request.
pub fn request_duplicate_retirement(
    file: &Path,
    pane: &str,
    supervisor_pid: u32,
    authoritative_owner: &str,
    window: &str,
    event: &str,
) -> Result<()> {
    let start_ticks = process_start_ticks(supervisor_pid)
        .with_context(|| format!("failed to identify duplicate supervisor pid {supervisor_pid}"))?;
    let path = request_path(file, supervisor_pid)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::json!({
        "version": 1,
        "pane": pane,
        "supervisor_pid": supervisor_pid,
        "process_start_ticks": start_ticks,
        "authoritative_owner": authoritative_owner,
        "window": window,
        "event": event,
    });
    std::fs::write(path, serde_json::to_vec(&body)?)?;
    Ok(())
}

/// Read the retirement request only when it still names this exact process and
/// pane. Malformed, stale, and pid-reused requests fail closed.
pub fn duplicate_retirement_request_for_current_process(
    file: &Path,
    pane: &str,
) -> Option<DuplicateRetirementRequest> {
    let pid = std::process::id();
    let body = std::fs::read_to_string(request_path(file, pid).ok()?).ok()?;
    let request = parse_request(&body)?;
    (request.supervisor_pid == pid
        && request.pane == pane
        && process_start_ticks(pid) == Some(request.process_start_ticks))
    .then_some(request)
}

/// Remove a consumed, superseded, or stale request for `supervisor_pid`.
pub fn clear_duplicate_retirement_request(file: &Path, supervisor_pid: u32) {
    if let Ok(path) = request_path(file, supervisor_pid) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn request_is_consumed_only_by_the_exact_process_and_pane() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "# Session\n").unwrap();
        let pid = std::process::id();

        request_duplicate_retirement(&file, "%7", pid, "%2", "@3", "route_duplicate_owner")
            .unwrap();

        assert!(duplicate_retirement_request_for_current_process(&file, "%8").is_none());
        let request = duplicate_retirement_request_for_current_process(&file, "%7").unwrap();
        assert_eq!(request.authoritative_owner, "%2");
        assert_eq!(request.window, "@3");
        assert_eq!(request.event, "route_duplicate_owner");

        let path = request_path(&file, pid).unwrap();
        let mut stale: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stale["process_start_ticks"] = serde_json::json!(request.process_start_ticks + 1);
        std::fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();
        assert!(duplicate_retirement_request_for_current_process(&file, "%7").is_none());

        clear_duplicate_retirement_request(&file, pid);
        assert!(duplicate_retirement_request_for_current_process(&file, "%7").is_none());
    }
}

//! Direct queue-mutation ownership coordinated through project `state.db`.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

const QUEUE_EDIT_OWNER_SCOPE: &str = "queue_mutation";
const DEFAULT_QUEUE_EDIT_OWNER_TTL_SECS: u64 = 15;
const QUEUE_EDIT_OWNER_TTL_SECS_ENV: &str = "AGENT_DOC_QUEUE_EDIT_OWNER_TTL_SECS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEditOwnerLease {
    pub holder_pid: u32,
    pub heartbeat_secs: u64,
}

pub fn queue_edit_owner_ttl() -> Duration {
    let secs = std::env::var(QUEUE_EDIT_OWNER_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_QUEUE_EDIT_OWNER_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn state_location(file: &str) -> Result<(std::path::PathBuf, String)> {
    let file = Path::new(file);
    let root = agent_doc_fs::find_project_root_canonical(file)
        .or_else(|| agent_doc_fs::find_project_root(file))
        .with_context(|| format!("no .agent-doc project root for {}", file.display()))?;
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    Ok((root, document_hash))
}

pub fn refresh_queue_edit_owner_lease(file: &str, pid: u32) -> Result<()> {
    let (project_root, document_hash) = state_location(file)?;
    agent_doc_controller_io::project_controller::upsert_coordination_lease(
        &project_root,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: QUEUE_EDIT_OWNER_SCOPE.to_string(),
            scope_id: document_hash,
            holder: pid.to_string(),
            holder_pid: Some(pid),
            heartbeat_secs: now_secs(),
        },
    )
}

pub fn read_queue_edit_owner_lease(file: &str) -> Option<QueueEditOwnerLease> {
    let (project_root, document_hash) = state_location(file).ok()?;
    let lease = agent_doc_controller_io::project_controller::load_coordination_lease(
        &project_root,
        QUEUE_EDIT_OWNER_SCOPE,
        &document_hash,
    )
    .ok()??;
    Some(QueueEditOwnerLease {
        holder_pid: lease.holder_pid?,
        heartbeat_secs: lease.heartbeat_secs,
    })
}

pub fn clear_queue_edit_owner_lease(file: &str) {
    let result = state_location(file).and_then(|(project_root, document_hash)| {
        agent_doc_controller_io::project_controller::clear_coordination_lease(
            &project_root,
            QUEUE_EDIT_OWNER_SCOPE,
            &document_hash,
        )
        .map(|_| ())
    });
    if let Err(err) = result {
        eprintln!("[agent-doc] warning: failed to clear queue-mutation lease: {err}");
    }
}

pub fn foreign_queue_edit_in_flight_with(
    lease: Option<&QueueEditOwnerLease>,
    self_pid: u32,
    now: u64,
    ttl: Duration,
    pid_is_live: impl Fn(u32) -> bool,
) -> Option<u32> {
    let lease = lease?;
    if lease.holder_pid == self_pid
        || !agent_doc_lease::timestamp_is_fresh(lease.heartbeat_secs, now, ttl)
        || !pid_is_live(lease.holder_pid)
    {
        return None;
    }
    Some(lease.holder_pid)
}

pub fn foreign_queue_edit_in_flight(file: &str) -> Option<u32> {
    let lease = read_queue_edit_owner_lease(file)?;
    foreign_queue_edit_in_flight_with(
        Some(&lease),
        std::process::id(),
        now_secs(),
        queue_edit_owner_ttl(),
        pid_is_live,
    )
}

#[cfg(unix)]
fn pid_is_live(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_is_live(_pid: u32) -> bool {
    true
}

pub struct QueueEditGuard {
    file: String,
}

impl QueueEditGuard {
    pub fn acquire(file: &Path) -> Self {
        let file = file.to_string_lossy().to_string();
        if let Err(err) = refresh_queue_edit_owner_lease(&file, std::process::id()) {
            eprintln!("[agent-doc] warning: failed to acquire queue-mutation lease: {err}");
        }
        Self { file }
    }
}

impl Drop for QueueEditGuard {
    fn drop(&mut self) {
        clear_queue_edit_owner_lease(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_uses_state_db_and_never_creates_a_sidecar() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_str = file.to_string_lossy().to_string();
        {
            let _guard = QueueEditGuard::acquire(&file);
            assert_eq!(
                read_queue_edit_owner_lease(&file_str).unwrap().holder_pid,
                std::process::id()
            );
            assert!(!dir.path().join(".agent-doc/queue-edit-owner").exists());
        }
        assert!(read_queue_edit_owner_lease(&file_str).is_none());
    }

    #[test]
    fn only_foreign_live_fresh_holder_blocks() {
        let lease = QueueEditOwnerLease {
            holder_pid: 999,
            heartbeat_secs: 1_000,
        };
        assert_eq!(
            foreign_queue_edit_in_flight_with(
                Some(&lease),
                1,
                1_005,
                Duration::from_secs(15),
                |_| true,
            ),
            Some(999)
        );
        assert_eq!(
            foreign_queue_edit_in_flight_with(
                Some(&lease),
                1,
                2_000,
                Duration::from_secs(15),
                |_| true,
            ),
            None
        );
    }
}

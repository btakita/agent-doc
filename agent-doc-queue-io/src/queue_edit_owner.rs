//! Direct queue-mutation ownership coordinated through project `state.db`.

use std::fs::{File, OpenOptions};
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

fn refresh_queue_edit_owner_lease(file: &str, pid: u32) -> Result<()> {
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

fn open_queue_apply_lock(file: &str) -> Result<File> {
    let (project_root, document_hash) = state_location(file)?;
    let lock_dir = project_root.join(".agent-doc/locks/queue-apply");
    std::fs::create_dir_all(&lock_dir).with_context(|| {
        format!(
            "failed to create queue apply lock directory {}",
            lock_dir.display()
        )
    })?;
    let lock_path = lock_dir.join(format!("{document_hash}.lock"));
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open queue apply lock {}", lock_path.display()))
}

enum QueueApplyLockAttempt {
    Acquired(File),
    Contended,
}

fn try_acquire_queue_apply_lock(file: &str) -> Result<QueueApplyLockAttempt> {
    use fs2::FileExt as _;

    let lock = open_queue_apply_lock(file)?;
    match lock.try_lock_exclusive() {
        Ok(()) => Ok(QueueApplyLockAttempt::Acquired(lock)),
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Ok(QueueApplyLockAttempt::Contended)
        }
        Err(error) => Err(error).context("failed to acquire queue apply lock"),
    }
}

/// Atomically reserve queue-maintenance ownership for a child that has been
/// spawned but has not entered its mutation body yet.
///
/// Competing admissions may both create a short-lived child process, but only
/// one PID can win this SQLite compare-and-set.  Every child verifies this
/// reservation before doing work.
pub fn try_reserve_queue_edit_owner(file: &str, token: &str, pid: u32) -> Result<bool> {
    let (project_root, document_hash) = state_location(file)?;
    let now = now_secs();
    let eligible_at_or_before = now.saturating_sub(queue_edit_owner_ttl().as_secs());
    agent_doc_controller_io::project_controller::claim_coordination_lease_if_expired(
        &project_root,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: QUEUE_EDIT_OWNER_SCOPE.to_string(),
            scope_id: document_hash,
            holder: token.to_string(),
            holder_pid: Some(pid),
            heartbeat_secs: now,
        },
        eligible_at_or_before,
    )
}

/// Refresh only the lease still owned by `token`; a superseded worker cannot
/// overwrite the heartbeat or identity of its replacement.
pub fn refresh_queue_edit_owner_lease_if_holder(file: &str, token: &str, pid: u32) -> Result<bool> {
    let (project_root, document_hash) = state_location(file)?;
    agent_doc_controller_io::project_controller::refresh_coordination_lease_if_holder(
        &project_root,
        QUEUE_EDIT_OWNER_SCOPE,
        &document_hash,
        token,
        pid,
        now_secs(),
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

pub fn clear_queue_edit_owner_lease_if_holder(file: &str, token: &str) {
    let result = state_location(file).and_then(|(project_root, document_hash)| {
        agent_doc_controller_io::project_controller::clear_coordination_lease_if_holder(
            &project_root,
            QUEUE_EDIT_OWNER_SCOPE,
            &document_hash,
            token,
        )
    });
    if let Err(err) = result {
        eprintln!("[agent-doc] warning: failed to clear owned queue-mutation lease: {err}");
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

#[derive(Debug)]
pub struct QueueEditGuard {
    file: String,
    holder: String,
    _apply_lock: File,
}

impl QueueEditGuard {
    pub fn ensure_protects(&self, file: &Path) -> Result<()> {
        let (_, held_hash) = state_location(&self.file)?;
        let (_, requested_hash) = state_location(&file.to_string_lossy())?;
        if held_hash != requested_hash {
            anyhow::bail!(
                "queue apply guard for {} cannot protect {}",
                self.file,
                file.display()
            );
        }
        Ok(())
    }

    pub fn acquire(file: &Path) -> Result<Self> {
        Self::try_acquire(file)?.with_context(|| {
            format!(
                "queue mutation deferred: another owner is applying maintenance for {}; retry after it completes",
                file.display()
            )
        })
    }

    pub fn try_acquire(file: &Path) -> Result<Option<Self>> {
        let file = file.to_string_lossy().to_string();
        // This flock is the apply exclusion. The SQLite lease makes ownership
        // observable and coalesces dispatch, while the OS lock closes the
        // check-then-effect race: no direct queue mutation can overwrite the
        // lease or enter its apply body until the current guard drops.
        let apply_lock = match try_acquire_queue_apply_lock(&file)? {
            QueueApplyLockAttempt::Acquired(lock) => lock,
            QueueApplyLockAttempt::Contended => return Ok(None),
        };
        let holder_pid = std::process::id();
        refresh_queue_edit_owner_lease(&file, holder_pid)?;
        Ok(Some(Self {
            file,
            holder: holder_pid.to_string(),
            _apply_lock: apply_lock,
        }))
    }

    /// Adopt a reservation published for this process before it was allowed to
    /// enter the maintenance body.
    pub fn adopt_reserved(file: &Path, token: &str) -> Result<Option<Self>> {
        let file = file.to_string_lossy().to_string();
        let apply_lock = match try_acquire_queue_apply_lock(&file)? {
            QueueApplyLockAttempt::Acquired(lock) => lock,
            QueueApplyLockAttempt::Contended => return Ok(None),
        };
        let holder_pid = std::process::id();
        if !refresh_queue_edit_owner_lease_if_holder(&file, token, holder_pid)? {
            return Ok(None);
        }
        Ok(Some(Self {
            file,
            holder: token.to_string(),
            _apply_lock: apply_lock,
        }))
    }
}

impl Drop for QueueEditGuard {
    fn drop(&mut self) {
        clear_queue_edit_owner_lease_if_holder(&self.file, &self.holder);
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
            let _guard = QueueEditGuard::acquire(&file).unwrap();
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

    #[test]
    fn only_one_concurrent_background_maintenance_body_is_admitted() {
        use std::sync::{Arc, Barrier};

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = Arc::new(file.to_string_lossy().to_string());
        let barrier = Arc::new(Barrier::new(8));
        let body_entries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut contenders = Vec::new();
        for offset in 0..8u32 {
            let file = Arc::clone(&file);
            let barrier = Arc::clone(&barrier);
            let body_entries = Arc::clone(&body_entries);
            contenders.push(std::thread::spawn(move || {
                barrier.wait();
                let pid = 40_000 + offset;
                if try_reserve_queue_edit_owner(&file, &format!("contender-{pid}"), pid).unwrap() {
                    body_entries.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }));
        }
        for contender in contenders {
            contender.join().unwrap();
        }
        assert_eq!(body_entries.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn superseded_worker_cannot_refresh_or_clear_its_successor() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();
        let token = "reserved-worker-token";
        assert!(try_reserve_queue_edit_owner(&file_text, token, std::process::id()).unwrap());
        let guard = QueueEditGuard::adopt_reserved(&file, token)
            .unwrap()
            .expect("reserved worker owns the lease");

        refresh_queue_edit_owner_lease(&file_text, 55_001).unwrap();
        assert!(
            !refresh_queue_edit_owner_lease_if_holder(&file_text, token, std::process::id())
                .unwrap(),
            "superseded worker must not revive its token"
        );
        drop(guard);

        let successor = read_queue_edit_owner_lease(&file_text).expect("successor lease remains");
        assert_eq!(successor.holder_pid, 55_001);
    }

    #[test]
    fn direct_edit_cannot_supersede_an_active_apply_guard() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();
        let token = "active-apply-token";
        assert!(try_reserve_queue_edit_owner(&file_text, token, std::process::id()).unwrap());
        let apply = QueueEditGuard::adopt_reserved(&file, token)
            .unwrap()
            .expect("reserved worker owns the apply lock");

        let direct = QueueEditGuard::acquire(&file).unwrap_err();
        assert!(direct.to_string().contains("queue mutation deferred"));
        assert!(
            refresh_queue_edit_owner_lease_if_holder(&file_text, token, std::process::id())
                .unwrap(),
            "rejected direct edit must not overwrite the active token",
        );

        drop(apply);
        QueueEditGuard::acquire(&file).expect("direct edit enters after apply guard drops");
    }
}

//! Direct queue-mutation ownership coordinated through project `state.db`.

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub(crate) const QUEUE_EDIT_OWNER_SCOPE: &str =
    agent_doc_controller_io::project_controller::QUEUE_APPLY_LEASE_SCOPE;
const QUEUE_MAINTENANCE_REQUEST_PREFIX: &str = "queue_maintenance_request:";
const DEFAULT_QUEUE_EDIT_OWNER_TTL_SECS: u64 = 15;
const QUEUE_EDIT_OWNER_TTL_SECS_ENV: &str = "AGENT_DOC_QUEUE_EDIT_OWNER_TTL_SECS";
/// Detached preflight maintenance reservations are identifiable so an
/// immediately-following foreground transaction can wait briefly for their
/// authoritative cut instead of failing the admitted turn or superseding an
/// active effect family.
pub const BACKGROUND_QUEUE_EDIT_OWNER_PREFIX: &str = "background-queue-maintenance:";
const DIRECT_QUEUE_EDIT_OWNER_PREFIX: &str = "direct-";
const FOREGROUND_BACKGROUND_WAIT: Duration = Duration::from_secs(4);
pub(crate) const RESPONSE_BACKGROUND_WAIT: Duration = Duration::from_secs(30);
const FOREGROUND_BACKGROUND_POLL: Duration = Duration::from_millis(10);
/// The longest a foreground acquirer waits on a held apply flock that names no
/// lease holder: an owner's clear-then-unlock (whose own clear retries for up
/// to 500 ms) or lock-then-publish step, not an apply body.
const QUEUE_APPLY_HANDOFF_GRACE: Duration = Duration::from_secs(2);
/// How long an operator-invoked repair waits for the
/// live apply owner before reporting the deferral. A closeout on a saturated
/// host held the flock for 160 s (sdk-dev.md 2026-11-22: `commit_latency
/// total_ms=160021`), and every `repair --session-check-heal` in that window
/// failed at once with `queue mutation deferred`, so the pending self-heal it
/// was asked to apply never ran.
pub const OPERATOR_REPAIR_OWNER_WAIT: Duration = Duration::from_secs(180);
const OPERATOR_REPAIR_OWNER_POLL: Duration = Duration::from_millis(100);
/// dogfood `maintlockhold`: the longest an operator-initiated mutation (an editor
/// command such as `Run Agent Doc`, or an operator queue CLI) waits behind a
/// background maintenance owner that keeps proving progress. The generic
/// foreground grace ([`FOREGROUND_BACKGROUND_WAIT`]) assumed a short detached
/// apply; controller-owned maintenance on a ~1MB document held the claim for
/// 42-185 s, so `editor_route` failed with `queue mutation deferred`
/// (haiven-dev sdk-dev.md, 2026-11-23 22:48:22Z). The wait only extends while
/// the owner's lease heartbeat stays fresh and its pid is live; a wedged owner
/// still fails the mutation after the generic grace.
pub const OPERATOR_BACKGROUND_MAINTENANCE_WAIT: Duration = Duration::from_secs(90);

thread_local! {
    static OPERATOR_BACKGROUND_PROGRESS_CEILING: std::cell::Cell<Option<Duration>> =
        const { std::cell::Cell::new(None) };
}

/// Marks the current thread as running an operator-initiated mutation
/// (dogfood `maintlockhold`): every [`QueueEditGuard::acquire`] on this thread waits
/// behind progressing background maintenance (bounded by
/// [`OPERATOR_BACKGROUND_MAINTENANCE_WAIT`]) instead of failing after the
/// generic grace. Hooks with tight harness timeouts never enter it, so their
/// fail-fast deferral is unchanged.
#[must_use = "the scope ends when the value is dropped"]
pub struct OperatorMutationWaitScope {
    previous: Option<Duration>,
}

impl OperatorMutationWaitScope {
    pub fn enter() -> Self {
        let previous = OPERATOR_BACKGROUND_PROGRESS_CEILING
            .with(|cell| cell.replace(Some(OPERATOR_BACKGROUND_MAINTENANCE_WAIT)));
        Self { previous }
    }
}

impl Drop for OperatorMutationWaitScope {
    fn drop(&mut self) {
        OPERATOR_BACKGROUND_PROGRESS_CEILING.with(|cell| cell.set(self.previous));
    }
}

pub(crate) fn operator_background_progress_ceiling() -> Option<Duration> {
    OPERATOR_BACKGROUND_PROGRESS_CEILING.with(std::cell::Cell::get)
}

/// Whether a background lease currently proves progress: a fresh heartbeat
/// from a live pid.
fn background_owner_progressing(lease: &QueueEditOwnerLease) -> bool {
    lease.holder.starts_with(BACKGROUND_QUEUE_EDIT_OWNER_PREFIX)
        && agent_doc_lease::timestamp_is_fresh(
            lease.heartbeat_secs,
            now_secs(),
            queue_edit_owner_ttl(),
        )
        && pid_is_live(lease.holder_pid)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEditOwnerLease {
    pub holder: String,
    pub holder_pid: u32,
    pub heartbeat_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueMaintenanceRequest {
    pub admitted_authority_hash: String,
    pub commit_if_mutated: bool,
    #[serde(skip)]
    pub generation: u64,
    #[serde(skip)]
    payload: String,
}

pub(crate) fn maintenance_request_location(
    file: &Path,
) -> Result<(std::path::PathBuf, String, String)> {
    let (root, document_hash) = state_location(&file.to_string_lossy())?;
    let key = format!("{QUEUE_MAINTENANCE_REQUEST_PREFIX}{document_hash}");
    Ok((root, document_hash, key))
}

impl QueueMaintenanceRequest {
    /// The exact persisted payload; with `generation` it is the request row's
    /// version that the durable maintenance claim compares and consumes.
    pub(crate) fn payload(&self) -> &str {
        &self.payload
    }
}

pub fn publish_queue_maintenance_request(
    file: &Path,
    admitted_authority_hash: &str,
    commit_if_mutated: bool,
) -> Result<QueueMaintenanceRequest> {
    let (root, _, key) = maintenance_request_location(file)?;
    let mut request = QueueMaintenanceRequest {
        admitted_authority_hash: admitted_authority_hash.to_string(),
        commit_if_mutated,
        generation: 0,
        payload: String::new(),
    };
    request.payload = serde_json::to_string(&request)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    request.generation =
        agent_doc_sqlite::state_store::upsert_latest_project_runtime_request_in_db(
            &conn,
            &key,
            &request.payload,
        )?;
    Ok(request)
}

pub fn load_queue_maintenance_request(file: &Path) -> Result<Option<QueueMaintenanceRequest>> {
    let (root, _, key) = maintenance_request_location(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let Some((payload, generation)) =
        agent_doc_sqlite::state_store::load_project_runtime_state_with_generation_from_db(
            &conn, &key,
        )?
    else {
        return Ok(None);
    };
    let mut request: QueueMaintenanceRequest = serde_json::from_str(&payload)?;
    request.generation = generation;
    request.payload = payload;
    Ok(Some(request))
}

pub fn queue_maintenance_request_pending(file: &Path) -> bool {
    load_queue_maintenance_request(file)
        .ok()
        .flatten()
        .is_some()
}

/// Who will run a pending admitted maintenance request (`#deadmaintworker`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueMaintenanceRequestOwner {
    /// No admitted request is pending.
    NoRequest,
    /// A live process owns the request: it holds the apply flock, or a live
    /// background reservation is about to adopt it.
    Live { holder_pid: Option<u32> },
    /// The request is pending and nothing will run it. The apply flock is
    /// free and the lease is absent, names a dead PID, or is a direct token
    /// whose transaction already released the flock. Only a new owner that
    /// reserves, adopts, and runs the request settles it.
    Orphaned {
        dead_holder: Option<QueueEditOwnerLease>,
    },
}

/// Classify the owner of the pending maintenance request without mutating
/// anything. A detached worker holds the apply flock for its whole run, so a
/// free flock with no live background reservation means its worker is gone:
/// it crashed, panicked, or returned an error after it claimed the request.
pub fn queue_maintenance_request_owner(file: &Path) -> Result<QueueMaintenanceRequestOwner> {
    if load_queue_maintenance_request(file)?.is_none() {
        return Ok(QueueMaintenanceRequestOwner::NoRequest);
    }
    // Probe the flock first: a worker that adopts after this probe has
    // already refreshed its lease, so the read below sees it as live.
    let in_flight = queue_apply_in_flight(file)?;
    let lease = read_queue_edit_owner_lease(&file.to_string_lossy());
    if in_flight {
        return Ok(QueueMaintenanceRequestOwner::Live {
            holder_pid: lease.map(|lease| lease.holder_pid),
        });
    }
    Ok(match lease {
        Some(lease)
            if !lease.holder.starts_with(DIRECT_QUEUE_EDIT_OWNER_PREFIX)
                && pid_is_live(lease.holder_pid) =>
        {
            QueueMaintenanceRequestOwner::Live {
                holder_pid: Some(lease.holder_pid),
            }
        }
        dead_holder => QueueMaintenanceRequestOwner::Orphaned { dead_holder },
    })
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
        .or_else(|| agent_doc_fs::find_project_root(file));
    #[cfg(test)]
    let root = root.or_else(|| file.parent().map(Path::to_path_buf));
    let root =
        root.with_context(|| format!("no .agent-doc project root for {}", file.display()))?;
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    Ok((root, document_hash))
}

#[cfg(test)]
fn publish_queue_edit_owner_lease(file: &str, holder: &str, pid: u32) -> Result<()> {
    let (project_root, document_hash) = state_location(file)?;
    agent_doc_controller_io::project_controller::upsert_coordination_lease(
        &project_root,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: QUEUE_EDIT_OWNER_SCOPE.to_string(),
            scope_id: document_hash,
            holder: holder.to_string(),
            holder_pid: Some(pid),
            heartbeat_secs: now_secs(),
        },
    )
}

/// Publish `holder` unless `keep_existing` accepts the current row, in one
/// state.db round trip. Returns the kept row, if any.
fn publish_queue_edit_owner_lease_unless(
    file: &str,
    holder: &str,
    pid: u32,
    keep_existing: impl FnOnce(&agent_doc_sqlite::state_store::CoordinationLeaseRecord) -> bool,
) -> Result<Option<agent_doc_sqlite::state_store::CoordinationLeaseRecord>> {
    let (project_root, document_hash) = state_location(file)?;
    agent_doc_controller_io::project_controller::publish_coordination_lease_unless(
        &project_root,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: QUEUE_EDIT_OWNER_SCOPE.to_string(),
            scope_id: document_hash,
            holder: holder.to_string(),
            holder_pid: Some(pid),
            heartbeat_secs: now_secs(),
        },
        keep_existing,
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

/// Whether another thread/process currently holds the document's apply lock.
/// This closes the observability gap before that owner publishes its SQLite
/// lease and is safe for session-check/closeout probes (no mutation).
pub fn queue_apply_in_flight(file: &Path) -> Result<bool> {
    match try_acquire_queue_apply_lock(&file.to_string_lossy())? {
        QueueApplyLockAttempt::Contended => Ok(true),
        QueueApplyLockAttempt::Acquired(lock) => {
            fs2::FileExt::unlock(&lock).context("failed to release queue apply probe lock")?;
            Ok(false)
        }
    }
}

/// Atomically reserve queue-maintenance ownership for a child that has been
/// spawned but has not entered its mutation body yet.
///
/// Competing admissions may both create a short-lived child process, but only
/// one PID can win this SQLite compare-and-set.  Every child verifies this
/// reservation before doing work.
pub fn try_reserve_queue_edit_owner(file: &str, token: &str, pid: u32) -> Result<bool> {
    // The flock is the authoritative apply exclusion. Probe and hold it across
    // stale-lease recovery plus the reservation CAS: a long-running direct
    // apply has no heartbeat thread, so heartbeat age alone must never allow a
    // detached worker to replace its still-live ownership.
    let reservation_lock = match try_acquire_queue_apply_lock(file)? {
        QueueApplyLockAttempt::Acquired(lock) => lock,
        QueueApplyLockAttempt::Contended => {
            // A published holder means an active apply and coalesces normally.
            // No holder while the flock is still held is the successful-Drop
            // handoff window: the predecessor conditionally cleared its token
            // before unlocking. Wait briefly for that unlock so the one
            // successor dispatch is not lost as a false coalesce.
            if read_queue_edit_owner_lease(file).is_some() {
                return Ok(false);
            }
            let deadline = std::time::Instant::now() + Duration::from_millis(250);
            loop {
                std::thread::sleep(Duration::from_millis(2));
                match try_acquire_queue_apply_lock(file)? {
                    QueueApplyLockAttempt::Acquired(lock) => break lock,
                    QueueApplyLockAttempt::Contended => {
                        if read_queue_edit_owner_lease(file).is_some()
                            || std::time::Instant::now() >= deadline
                        {
                            return Ok(false);
                        }
                    }
                }
            }
        }
    };
    if let Some(existing) = read_queue_edit_owner_lease(file)
        && (existing.holder.starts_with(DIRECT_QUEUE_EDIT_OWNER_PREFIX)
            || !pid_is_live(existing.holder_pid))
    {
        // Holding the flock proves that a direct mutation no longer owns its
        // apply transaction, even when its long-lived controller PID remains
        // alive. Every non-direct token is a pre-apply reservation and must be
        // preserved until adoption (including older/custom reservation names).
        clear_queue_edit_owner_lease_if_holder(file, &existing.holder);
    }
    let (project_root, document_hash) = state_location(file)?;
    let now = now_secs();
    let eligible_at_or_before = agent_doc_lease::expiration_cutoff(now, queue_edit_owner_ttl());
    let reserved =
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
        )?;
    fs2::FileExt::unlock(&reservation_lock)
        .context("failed to release queue reservation probe lock")?;
    Ok(reserved)
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
        holder: lease.holder,
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
    let result = clear_queue_edit_owner_lease_if_holder_result(file, token).map(|_| ());
    if let Err(err) = result {
        eprintln!("[agent-doc] warning: failed to clear owned queue-mutation lease: {err}");
    }
}

fn clear_queue_edit_owner_lease_if_holder_result(file: &str, token: &str) -> Result<bool> {
    state_location(file).and_then(|(project_root, document_hash)| {
        agent_doc_controller_io::project_controller::clear_coordination_lease_if_holder(
            &project_root,
            QUEUE_EDIT_OWNER_SCOPE,
            &document_hash,
            token,
        )
    })
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

/// Who holds the queue apply lease, in operator words.
/// The deferral used to say only "another owner is applying maintenance", which
/// read as background maintenance even when the holder was a slow foreground
/// closeout, so nobody could tell what to wait for.
pub fn describe_queue_edit_owner(lease: Option<&QueueEditOwnerLease>, now: u64) -> String {
    let Some(lease) = lease else {
        return "holder: an owner between lease and unlock".to_string();
    };
    let kind = if lease.holder.starts_with(BACKGROUND_QUEUE_EDIT_OWNER_PREFIX) {
        "background queue maintenance"
    } else if lease.holder.starts_with(DIRECT_QUEUE_EDIT_OWNER_PREFIX) {
        "a foreground queue apply (closeout, write, queue, or repair)"
    } else {
        "a queue apply owner"
    };
    format!(
        "holder: {kind}, pid {}, last heartbeat {}s ago",
        lease.holder_pid,
        agent_doc_lease::timestamp_age_secs(lease.heartbeat_secs, now)
    )
}

fn queue_mutation_deferred_error(file: &Path) -> anyhow::Error {
    let lease = read_queue_edit_owner_lease(&file.to_string_lossy());
    anyhow::anyhow!(
        "queue mutation deferred: another owner is applying maintenance for {} ({}); retry after it completes",
        file.display(),
        describe_queue_edit_owner(lease.as_ref(), now_secs()),
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
    apply_lock: Option<File>,
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
        Self::acquire_with_background_wait(file, FOREGROUND_BACKGROUND_WAIT)
    }

    /// Acquire the apply capability for the response transaction that follows
    /// an admitted preflight.
    ///
    /// Admission deliberately dispatches comprehensive maintenance after it
    /// emits JSON. On a saturated host that child can take longer than the
    /// short generic mutation grace period merely to start. A response owns
    /// the admitted turn, so it waits for that exact background effect family
    /// instead of failing the turn spuriously. Direct/foreign mutations still
    /// fail closed immediately after the generic grace period, and a dead
    /// worker is recovered by `try_acquire` rather than consuming this bound.
    pub fn acquire_for_response(file: &Path) -> Result<Self> {
        Self::acquire_with_background_wait(file, RESPONSE_BACKGROUND_WAIT)
    }

    /// Acquire the apply capability after this command's failed closeout was
    /// proven committed by another owner (`#resumemaintrace`).
    ///
    /// The owner that committed the cycle (typically the route-owned
    /// supervisor's captured-finalize resume) is a direct holder that still
    /// runs its post-commit tail under the apply flock for a few seconds.
    /// `acquire` fails a direct holder closed after the short handoff grace,
    /// which made `respond` replace a superseded closeout error with `queue
    /// mutation deferred` and exit nonzero for a committed turn. This waits for
    /// any holder, direct or background, up to `wait`, and only then reports
    /// the same deferral.
    pub fn acquire_after_committed_closeout(file: &Path, wait: Duration) -> Result<Self> {
        Self::acquire_waiting_for_any_holder(file, wait, FOREGROUND_BACKGROUND_POLL)
    }

    pub(crate) fn acquire_waiting_for_any_holder(
        file: &Path,
        wait: Duration,
        poll: Duration,
    ) -> Result<Self> {
        let deadline = std::time::Instant::now() + wait;
        loop {
            if let Some(guard) = Self::try_acquire(file)? {
                return Ok(guard);
            }
            if std::time::Instant::now() >= deadline {
                return Err(queue_mutation_deferred_error(file));
            }
            std::thread::sleep(poll);
        }
    }

    /// Acquire the apply capability for an operator-invoked repair.
    ///
    /// `repair --session-check-heal` is the command `session-check` names for a
    /// pending self-heal. It used to take [`Self::acquire`], which fails a live
    /// foreground holder after the short handoff grace, so while one slow
    /// closeout held the flock every heal attempt failed and the heal stayed
    /// pending. Wait for whichever live owner holds it, bounded by `wait`
    /// (the command passes [`OPERATOR_REPAIR_OWNER_WAIT`]), and say once what
    /// is being waited on.
    pub fn acquire_for_operator_repair(file: &Path, wait: Duration) -> Result<Self> {
        Self::acquire_for_operator_repair_bounded(file, wait, OPERATOR_REPAIR_OWNER_POLL)
    }

    pub(crate) fn acquire_for_operator_repair_bounded(
        file: &Path,
        wait: Duration,
        poll: Duration,
    ) -> Result<Self> {
        if let Some(guard) = Self::try_acquire(file)? {
            return Ok(guard);
        }
        let lease = read_queue_edit_owner_lease(&file.to_string_lossy());
        eprintln!(
            "[repair] waiting up to {}s for the live queue apply owner of {} to finish ({})",
            wait.as_secs(),
            file.display(),
            describe_queue_edit_owner(lease.as_ref(), now_secs()),
        );
        Self::acquire_waiting_for_any_holder(file, wait, poll)
    }

    fn acquire_with_background_wait(file: &Path, wait: Duration) -> Result<Self> {
        Self::acquire_with_background_progress_wait(
            file,
            wait,
            QUEUE_APPLY_HANDOFF_GRACE,
            operator_background_progress_ceiling(),
            |_| {},
        )
    }

    /// `after_contended_observation` sees the lease read after each contended
    /// attempt, before the wait-or-fail decision. Tests use it to hold the
    /// flock until the acquirer has provably observed a given state.
    #[cfg(test)]
    pub(crate) fn acquire_with_background_wait_observed(
        file: &Path,
        wait: Duration,
        handoff_grace: Duration,
        after_contended_observation: impl FnMut(Option<&QueueEditOwnerLease>),
    ) -> Result<Self> {
        Self::acquire_with_background_progress_wait(
            file,
            wait,
            handoff_grace,
            None,
            after_contended_observation,
        )
    }

    /// The foreground acquisition loop. `progress_ceiling` (dogfood `maintlockhold`)
    /// lets an operator-initiated mutation keep waiting behind a background
    /// owner for as long as that owner proves progress, up to the ceiling from
    /// the first contended observation; each progressing observation re-arms
    /// the generic `wait`, so an owner whose heartbeat goes stale still fails
    /// the mutation within `wait`.
    pub(crate) fn acquire_with_background_progress_wait(
        file: &Path,
        wait: Duration,
        handoff_grace: Duration,
        progress_ceiling: Option<Duration>,
        mut after_contended_observation: impl FnMut(Option<&QueueEditOwnerLease>),
    ) -> Result<Self> {
        let started = std::time::Instant::now();
        let mut background_deadline: Option<std::time::Instant> = None;
        let mut handoff_deadline = None;
        let mut waited_behind_progress = false;
        loop {
            if let Some(guard) = Self::try_acquire(file)? {
                if waited_behind_progress {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "queue_apply_waited_behind_background_maintenance file={} waited_ms={} (maintlockhold)",
                            file.display(),
                            started.elapsed().as_millis(),
                        ),
                    );
                }
                return Ok(guard);
            }
            let lease = read_queue_edit_owner_lease(&file.to_string_lossy());
            after_contended_observation(lease.as_ref());
            if let Some(background) = lease
                .as_ref()
                .filter(|lease| lease.holder.starts_with(BACKGROUND_QUEUE_EDIT_OWNER_PREFIX))
            {
                let now = std::time::Instant::now();
                let first = *background_deadline.get_or_insert(now + wait);
                if let Some(ceiling) = progress_ceiling
                    && background_owner_progressing(background)
                {
                    let extended = (now + wait).min(started + ceiling);
                    if extended > first {
                        waited_behind_progress = true;
                        background_deadline = Some(extended);
                    }
                }
            }
            // GH #412: a held flock with no lease row is
            // an owner between its lease and its unlock. Drop clears the token
            // before unlocking, completing a maintenance request consumes the
            // lease before the guard drops, and a reservation probe or direct
            // acquirer locks before it publishes. Bailing on the first such
            // observation failed the admitted Stop-hook commit right after the
            // detached worker finished. Wait through it, bounded by one grace
            // that never re-arms: a lease that appears meanwhile decides as
            // before (background waits, a foreign direct owner fails closed).
            if lease.is_none() {
                handoff_deadline.get_or_insert_with(|| std::time::Instant::now() + handoff_grace);
            }
            let now = std::time::Instant::now();
            let in_handoff = lease.is_none() && handoff_deadline.is_some_and(|end| now < end);
            if in_handoff || background_deadline.is_some_and(|deadline| now < deadline) {
                // A preflight admission's detached maintenance owns no user
                // response. Let its short apply finish, then admit the
                // foreground transaction on the resulting authoritative cut.
                // A missing lease after first observation is the guard Drop
                // handoff window; keep polling until the flock is released.
                std::thread::sleep(FOREGROUND_BACKGROUND_POLL);
                continue;
            }
            return Err(queue_mutation_deferred_error(file));
        }
    }

    pub fn try_acquire(file: &Path) -> Result<Option<Self>> {
        Self::try_acquire_with_hook(file, || {})
    }

    /// Acquire the apply capability for short controller background work
    /// (`#q-9f8f5f60c2c3`, the lifecycle-projection worker).
    ///
    /// The guard holds a `background-queue-maintenance:` lease, so a foreground
    /// mutation that arrives meanwhile waits behind it (up to its own
    /// background wait) instead of failing closed the way it does behind a
    /// direct holder. This waits up to `wait` for any current holder, then
    /// reports the usual deferral.
    pub fn acquire_background(file: &Path, label: &str, wait: Duration) -> Result<Self> {
        let file_text = file.to_string_lossy().to_string();
        let pid = std::process::id();
        let token = format!(
            "{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}{label}:{}",
            new_direct_queue_edit_holder(pid).trim_start_matches(DIRECT_QUEUE_EDIT_OWNER_PREFIX)
        );
        let deadline = std::time::Instant::now() + wait;
        loop {
            if try_reserve_queue_edit_owner(&file_text, &token, pid)? {
                if let Some(guard) = Self::adopt_reserved(file, &token)? {
                    return Ok(guard);
                }
                clear_queue_edit_owner_lease_if_holder(&file_text, &token);
            }
            if std::time::Instant::now() >= deadline {
                return Err(queue_mutation_deferred_error(file));
            }
            std::thread::sleep(FOREGROUND_BACKGROUND_POLL);
        }
    }

    fn try_acquire_with_hook(
        file: &Path,
        after_lock_before_lease: impl FnOnce(),
    ) -> Result<Option<Self>> {
        let file = file.to_string_lossy().to_string();
        // This flock is the apply exclusion. The SQLite lease makes ownership
        // observable and coalesces dispatch, while the OS lock closes the
        // check-then-effect race: no direct queue mutation can overwrite the
        // lease or enter its apply body until the current guard drops.
        let apply_lock = match try_acquire_queue_apply_lock(&file)? {
            QueueApplyLockAttempt::Acquired(lock) => lock,
            QueueApplyLockAttempt::Contended => return Ok(None),
        };
        after_lock_before_lease();
        // A detached worker reserves before it can acquire the flock. Do not
        // overwrite that live reservation in the short lock-before-adoption
        // window: doing so would make the sole worker fail adoption and lose
        // maintenance. A direct holder, however, can only own the transaction
        // while it owns this same flock. Once we acquired the flock, any direct
        // token is stale even when its long-lived controller PID is still live
        // (for example after an RPC task dropped its guard). Keeping that token
        // would wedge every later mutation behind a nonexistent apply owner.
        //
        // `#admissionleasechurn`: observe, decide, and replace in one state.db
        // transaction. Replacing a stale row is the same upsert as publishing
        // over an empty one, so the separate read and clear round trips bought
        // nothing but connections and an extra commit.
        let holder_pid = std::process::id();
        let holder = new_direct_queue_edit_holder(holder_pid);
        let kept = publish_queue_edit_owner_lease_unless(&file, &holder, holder_pid, |existing| {
            !existing.holder.starts_with(DIRECT_QUEUE_EDIT_OWNER_PREFIX)
                && existing.holder_pid.is_some_and(pid_is_live)
        })?;
        if kept.is_some() {
            fs2::FileExt::unlock(&apply_lock)
                .context("failed to release queue apply lock after live reservation")?;
            return Ok(None);
        }
        Ok(Some(Self {
            file,
            holder,
            apply_lock: Some(apply_lock),
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
            apply_lock: Some(apply_lock),
        }))
    }

    /// Leave the durable reservation behind when a detached worker fails.
    /// Dropping this guard still releases the OS apply exclusion. The failed
    /// worker's PID then makes the incomplete attempt immediately recoverable
    /// after process exit, while session-check remains fail-closed meanwhile.
    pub fn retain_incomplete_attempt_on_drop(&mut self) {
        self.holder.clear();
    }

    /// The coordination-lease holder token this guard owns (empty once the
    /// lease was consumed or deliberately retained).
    pub(crate) fn holder(&self) -> &str {
        &self.holder
    }

    /// The durable maintenance claim consumed this guard's lease row together
    /// with its request; Drop must not try to clear it again.
    ///
    /// `#answerheadmaintrace`: release the apply flock now as well. The guard
    /// owns nothing more, and a held flock with no lease is the handoff window
    /// every foreground acquirer waits through for at most
    /// `QUEUE_APPLY_HANDOFF_GRACE`. Holding it until the guard drops kept the
    /// detached worker's teardown (`retains_work`'s `state.db` open, with its
    /// 30 s busy timeout, and the heartbeat join) inside that window, so under
    /// `state.db` contention an admitted response failed after the grace.
    pub(crate) fn release_consumed_apply(&mut self) {
        self.holder.clear();
        drop(self.apply_lock.take());
    }

    fn release_with_handoff_hook(&mut self, after_clear_before_unlock: impl FnOnce()) {
        // Successful owners clear their exact token while still holding the
        // authoritative flock. A successor therefore sees either the live
        // predecessor token, or the short no-token handoff window that
        // reservation waits through. Incomplete/error owners leave the token
        // durable and only release the flock.
        if !self.holder.is_empty() {
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            loop {
                // One conditional DELETE is the whole release. Once it
                // commits, the row is not this guard's whether or not it
                // matched (tokens are unique, so nobody republishes ours), so
                // the re-read that used to follow it cost a connection and
                // proved nothing (`#admissionleasechurn`).
                match clear_queue_edit_owner_lease_if_holder_result(&self.file, &self.holder) {
                    Ok(_) => break,
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => {
                        eprintln!(
                            "[agent-doc] warning: failed to clear owned queue-mutation lease before apply unlock: {error}"
                        );
                        break;
                    }
                }
            }
            self.holder.clear();
        }
        after_clear_before_unlock();
        drop(self.apply_lock.take());
    }
}

fn new_direct_queue_edit_holder(pid: u32) -> String {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{DIRECT_QUEUE_EDIT_OWNER_PREFIX}{pid}-{nanos}-{sequence}")
}

impl Drop for QueueEditGuard {
    fn drop(&mut self) {
        self.release_with_handoff_hook(|| {});
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

    /// `#admissionleasechurn`: one guard acquire/release is exactly one
    /// publish round trip and one clear round trip. Preflight admission runs
    /// several guard cycles, and each extra state.db connection cost a
    /// directory sync and (as the last connection) a WAL checkpoint. The
    /// previous protocol read the lease, published it, cleared it, then re-read
    /// it: 4 connections per cycle, 5 with 3 lease writes when a stale direct
    /// token had to be cleared first.
    #[test]
    fn a_guard_cycle_is_one_publish_and_one_clear_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();
        // Converge the schema outside the measured window.
        assert!(read_queue_edit_owner_lease(&file_text).is_none());

        for stale_direct_token in [false, true] {
            if stale_direct_token {
                publish_queue_edit_owner_lease(
                    &file_text,
                    "direct-stale-after-dropped-guard",
                    std::process::id(),
                )
                .unwrap();
            }
            let before = agent_doc_sqlite::state_store::state_db_io_counters();
            let guard = QueueEditGuard::try_acquire(&file)
                .unwrap()
                .expect("an uncontended flock admits the guard");
            let acquired = agent_doc_sqlite::state_store::state_db_io_counters().since(before);
            drop(guard);
            let cycle = agent_doc_sqlite::state_store::state_db_io_counters().since(before);
            assert_eq!(
                (acquired.writable_opens, acquired.lease_writes),
                (1, 1),
                "acquire must observe, decide, and publish in one round trip \
                 (stale direct token present: {stale_direct_token})"
            );
            assert_eq!(
                (cycle.writable_opens, cycle.lease_writes),
                (2, 2),
                "a guard cycle is one publish and one clear round trip \
                 (stale direct token present: {stale_direct_token})"
            );
            assert!(read_queue_edit_owner_lease(&file_text).is_none());
        }
    }

    #[test]
    fn a_live_reservation_is_kept_without_a_lease_write() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();
        let reservation = format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}live-reservation");
        publish_queue_edit_owner_lease(&file_text, &reservation, std::process::id()).unwrap();

        let before = agent_doc_sqlite::state_store::state_db_io_counters();
        assert!(
            QueueEditGuard::try_acquire(&file).unwrap().is_none(),
            "a live pre-apply reservation keeps the apply slot"
        );
        let io = agent_doc_sqlite::state_store::state_db_io_counters().since(before);
        assert_eq!((io.writable_opens, io.lease_writes), (1, 0));
        assert_eq!(
            read_queue_edit_owner_lease(&file_text).unwrap().holder,
            reservation
        );
    }

    #[test]
    fn flock_free_direct_token_from_a_live_controller_is_recovered() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();

        publish_queue_edit_owner_lease(
            &file_text,
            "direct-live-controller-stale",
            std::process::id(),
        )
        .unwrap();
        let guard = QueueEditGuard::try_acquire(&file)
            .unwrap()
            .expect("the free flock proves the direct token is stale");
        assert_ne!(
            read_queue_edit_owner_lease(&file_text).unwrap().holder,
            "direct-live-controller-stale"
        );
        drop(guard);

        publish_queue_edit_owner_lease(
            &file_text,
            "direct-live-controller-stale",
            std::process::id(),
        )
        .unwrap();
        assert!(
            try_reserve_queue_edit_owner(
                &file_text,
                "queue-maintenance-background-successor",
                std::process::id(),
            )
            .unwrap(),
            "a detached successor must not be lost behind a flock-free direct token",
        );
        clear_queue_edit_owner_lease_if_holder(
            &file_text,
            "queue-maintenance-background-successor",
        );
    }

    #[test]
    fn only_foreign_live_fresh_holder_blocks() {
        let lease = QueueEditOwnerLease {
            holder: "holder-999".to_string(),
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
                let pid = std::process::id();
                if try_reserve_queue_edit_owner(&file, &format!("contender-{offset}"), pid).unwrap()
                {
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

        publish_queue_edit_owner_lease(&file_text, "successor-token", 55_001).unwrap();
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

    /// The deferral names the live holder, and an
    /// operator repair waits out a slow foreground closeout instead of failing
    /// while it holds the flock (sdk-dev.md 2026-11-22: a 160 s commit made
    /// every `repair --session-check-heal` fail at once).
    #[test]
    fn operator_repair_waits_for_a_live_foreground_holder_and_deferral_names_it() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();

        let closeout = QueueEditGuard::acquire(&file).unwrap();
        let refused = QueueEditGuard::acquire(&file).unwrap_err().to_string();
        assert!(refused.contains("queue mutation deferred"), "{refused}");
        assert!(
            refused.contains("a foreground queue apply")
                && refused.contains(&format!("pid {}", std::process::id())),
            "the deferral names who holds the flock: {refused}"
        );
        let bounded = QueueEditGuard::acquire_for_operator_repair_bounded(
            &file,
            Duration::from_millis(50),
            Duration::from_millis(5),
        )
        .unwrap_err()
        .to_string();
        assert!(bounded.contains("queue mutation deferred"), "{bounded}");

        let holder = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(closeout);
        });
        let repair = QueueEditGuard::acquire_for_operator_repair_bounded(
            &file,
            Duration::from_secs(20),
            Duration::from_millis(5),
        )
        .expect("the repair runs once the closeout releases the flock");
        holder.join().unwrap();
        drop(repair);
    }

    #[test]
    fn describe_queue_edit_owner_distinguishes_background_from_foreground() {
        let background = QueueEditOwnerLease {
            holder: format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}abc"),
            holder_pid: 7,
            heartbeat_secs: 100,
        };
        let foreground = QueueEditOwnerLease {
            holder: format!("{DIRECT_QUEUE_EDIT_OWNER_PREFIX}abc"),
            holder_pid: 8,
            heartbeat_secs: 100,
        };
        assert_eq!(
            describe_queue_edit_owner(Some(&background), 130),
            "holder: background queue maintenance, pid 7, last heartbeat 30s ago"
        );
        assert!(describe_queue_edit_owner(Some(&foreground), 100).contains("foreground"));
        assert!(describe_queue_edit_owner(None, 100).contains("between lease and unlock"));
    }

    /// `#resumemaintrace`: the owner that committed a cycle `respond` failed to
    /// close is a direct holder in its post-commit tail. `acquire` refuses it
    /// after the short handoff grace (the respond error), while the
    /// committed-closeout acquisition waits for it to release, within a bound.
    #[test]
    fn committed_closeout_acquire_waits_out_a_direct_post_commit_holder() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();

        let committing_owner = QueueEditGuard::acquire(&file).unwrap();
        assert!(
            QueueEditGuard::acquire(&file)
                .unwrap_err()
                .to_string()
                .contains("queue mutation deferred"),
            "precondition: a live direct holder fails the ordinary acquire closed"
        );
        let bounded = QueueEditGuard::acquire_waiting_for_any_holder(
            &file,
            Duration::from_millis(50),
            Duration::from_millis(5),
        )
        .unwrap_err();
        assert!(
            bounded.to_string().contains("queue mutation deferred"),
            "the wait is bounded: {bounded:#}"
        );

        let (released_tx, released_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(committing_owner);
            released_tx.send(()).unwrap();
        });
        let guard =
            QueueEditGuard::acquire_after_committed_closeout(&file, Duration::from_secs(20))
                .expect("waits for the committing owner to release");
        released_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the holder released the flock the guard now holds");
        holder.join().unwrap();
        assert_eq!(
            read_queue_edit_owner_lease(&file.to_string_lossy())
                .unwrap()
                .holder_pid,
            std::process::id()
        );
        drop(guard);
    }

    #[test]
    fn drop_clears_while_locked_and_hands_the_sole_successor_dispatch_through() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();
        let mut first = QueueEditGuard::acquire(&file).unwrap();
        let successor_file = file.clone();
        let successor_file_text = file_text.to_string();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let mut successor = None;

        first.release_with_handoff_hook(|| {
            successor = Some(std::thread::spawn(move || {
                started_tx.send(()).unwrap();
                assert!(
                    try_reserve_queue_edit_owner(
                        &successor_file_text,
                        "drop-handoff-successor",
                        std::process::id(),
                    )
                    .unwrap()
                );
                QueueEditGuard::adopt_reserved(&successor_file, "drop-handoff-successor")
                    .unwrap()
                    .expect("the sole successor adopts after Drop unlocks")
            }));
            started_rx.recv().unwrap();
            assert!(
                read_queue_edit_owner_lease(&file_text).is_none(),
                "successful Drop must clear its token before releasing the flock"
            );
        });

        let successor = successor.unwrap().join().unwrap();
        assert_eq!(
            read_queue_edit_owner_lease(&file_text).unwrap().holder,
            "drop-handoff-successor",
            "successor reservation/body was lost during Drop handoff",
        );
        drop(successor);
        assert!(read_queue_edit_owner_lease(&file_text).is_none());
    }

    #[test]
    fn direct_guard_cannot_overwrite_a_live_pre_spawn_reservation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();
        let token = "reserved-before-child-spawn";
        assert!(try_reserve_queue_edit_owner(&file_text, token, std::process::id()).unwrap());

        assert!(
            QueueEditGuard::try_acquire(&file).unwrap().is_none(),
            "direct mutation overwrote the sole reserved maintenance worker"
        );
        assert_eq!(
            read_queue_edit_owner_lease(&file_text).unwrap().holder,
            token
        );

        let worker = QueueEditGuard::adopt_reserved(&file, token)
            .unwrap()
            .expect("reserved worker still adopts after the rejected direct edit");
        drop(worker);
        assert!(read_queue_edit_owner_lease(&file_text).is_none());
    }

    #[test]
    fn foreground_guard_waits_for_background_apply_and_uses_its_resulting_cut() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();
        let token = format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}reserved-child");
        assert!(try_reserve_queue_edit_owner(&file_text, &token, std::process::id()).unwrap());

        let background = QueueEditGuard::adopt_reserved(&file, &token)
            .unwrap()
            .expect("background worker adopts the reservation");
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            drop(background);
        });

        let started = std::time::Instant::now();
        let foreground = QueueEditGuard::acquire(&file)
            .expect("foreground transaction enters after background apply completes");
        assert!(
            started.elapsed() >= Duration::from_millis(40),
            "foreground must not preempt the active background effect family"
        );
        release.join().unwrap();
        drop(foreground);
        assert!(read_queue_edit_owner_lease(&file_text).is_none());
    }

    /// Hold `file`'s apply flock as a background maintenance owner for `hold`
    /// while a heartbeat thread keeps its lease fresh (or, with
    /// `stale_heartbeat`, pins the lease heartbeat in the past).
    fn hold_as_background_owner(
        file: &Path,
        token: &str,
        hold: Duration,
        stale_heartbeat: bool,
    ) -> std::thread::JoinHandle<()> {
        let file_text = file.to_string_lossy().to_string();
        assert!(try_reserve_queue_edit_owner(&file_text, token, std::process::id()).unwrap());
        let background = QueueEditGuard::adopt_reserved(file, token)
            .unwrap()
            .expect("background worker adopts the reservation");
        if stale_heartbeat {
            let (project_root, document_hash) = state_location(&file_text).unwrap();
            agent_doc_controller_io::project_controller::upsert_coordination_lease(
                &project_root,
                &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
                    scope_kind: QUEUE_EDIT_OWNER_SCOPE.to_string(),
                    scope_id: document_hash,
                    holder: token.to_string(),
                    holder_pid: Some(std::process::id()),
                    heartbeat_secs: now_secs() - 3600,
                },
            )
            .unwrap();
        }
        let token = token.to_string();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            while started.elapsed() < hold {
                if !stale_heartbeat {
                    refresh_queue_edit_owner_lease_if_holder(
                        &file_text,
                        &token,
                        std::process::id(),
                    )
                    .unwrap();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(background);
        })
    }

    /// dogfood `maintlockhold`: an editor `Run Agent Doc` (editor_route) failed with
    /// `queue mutation deferred` because controller maintenance held the apply
    /// claim longer than the fixed foreground grace. An operator-initiated
    /// mutation waits behind a background owner that keeps proving progress
    /// (fresh heartbeat, live pid) and then applies on its resulting cut.
    #[test]
    fn operator_mutation_waits_behind_progressing_background_maintenance() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy().to_string();
        let token = format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}slow-controller-run");
        let generic_wait = Duration::from_millis(60);
        let holder = hold_as_background_owner(&file, &token, Duration::from_millis(400), false);

        let started = std::time::Instant::now();
        let foreground = QueueEditGuard::acquire_with_background_progress_wait(
            &file,
            generic_wait,
            QUEUE_APPLY_HANDOFF_GRACE,
            Some(Duration::from_secs(10)),
            |_| {},
        )
        .expect("an operator mutation waits out progressing maintenance instead of failing");
        assert!(
            started.elapsed() > generic_wait * 3,
            "the wait outlived the generic grace: {:?}",
            started.elapsed()
        );
        holder.join().unwrap();
        assert!(
            read_queue_edit_owner_lease(&file_text)
                .unwrap()
                .holder
                .starts_with(DIRECT_QUEUE_EDIT_OWNER_PREFIX)
        );
        drop(foreground);
    }

    /// The progress wait is not a blank cheque: a background owner whose
    /// heartbeat went stale fails the operator mutation after the generic
    /// grace, and without the operator scope the generic grace still applies.
    #[test]
    fn progress_wait_never_extends_for_a_stale_owner_or_outside_operator_scope() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let generic_wait = Duration::from_millis(60);

        let token = format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}wedged-run");
        let holder = hold_as_background_owner(&file, &token, Duration::from_millis(600), true);
        let refused = QueueEditGuard::acquire_with_background_progress_wait(
            &file,
            generic_wait,
            QUEUE_APPLY_HANDOFF_GRACE,
            Some(Duration::from_secs(10)),
            |_| {},
        )
        .unwrap_err();
        assert!(refused.to_string().contains("queue mutation deferred"));
        holder.join().unwrap();

        let token = format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}progressing-run");
        let holder = hold_as_background_owner(&file, &token, Duration::from_millis(600), false);
        let refused = QueueEditGuard::acquire_with_background_progress_wait(
            &file,
            generic_wait,
            QUEUE_APPLY_HANDOFF_GRACE,
            None,
            |_| {},
        )
        .unwrap_err();
        assert!(refused.to_string().contains("queue mutation deferred"));
        holder.join().unwrap();
    }

    /// The operator scope is what `acquire` consults, and it is scoped: it
    /// restores the enclosing value on drop.
    #[test]
    fn operator_mutation_scope_is_thread_scoped_and_restored() {
        assert_eq!(operator_background_progress_ceiling(), None);
        {
            let _outer = OperatorMutationWaitScope::enter();
            assert_eq!(
                operator_background_progress_ceiling(),
                Some(OPERATOR_BACKGROUND_MAINTENANCE_WAIT)
            );
            {
                let _inner = OperatorMutationWaitScope::enter();
            }
            assert_eq!(
                operator_background_progress_ceiling(),
                Some(OPERATOR_BACKGROUND_MAINTENANCE_WAIT)
            );
            std::thread::spawn(|| assert_eq!(operator_background_progress_ceiling(), None))
                .join()
                .unwrap();
        }
        assert_eq!(operator_background_progress_ceiling(), None);
    }

    /// GH #412: the Stop hook's commit failed with "queue mutation deferred"
    /// because it first observed the apply flock in an owner's handoff window,
    /// lease row already gone and flock not yet released, and bailed at once.
    /// The owner here holds that exact state until the acquirer has observed
    /// it, so the outcome does not depend on timing.
    #[test]
    fn foreground_guard_waits_through_a_locked_no_lease_handoff_window() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy().to_string();
        let mut owner = QueueEditGuard::acquire(&file).unwrap();
        let (observed_tx, observed_rx) = std::sync::mpsc::channel();
        let mut foreground = None;

        owner.release_with_handoff_hook(|| {
            assert!(
                read_queue_edit_owner_lease(&file_text).is_none(),
                "the owner is in its clear-then-unlock handoff window"
            );
            let foreground_file = file.clone();
            foreground = Some(std::thread::spawn(move || {
                QueueEditGuard::acquire_with_background_wait_observed(
                    &foreground_file,
                    FOREGROUND_BACKGROUND_WAIT,
                    QUEUE_APPLY_HANDOFF_GRACE,
                    |lease| {
                        let _ = observed_tx.send(lease.cloned());
                    },
                )
            }));
            assert_eq!(
                observed_rx.recv().unwrap(),
                None,
                "the acquirer first sees the held flock with no lease"
            );
        });

        let foreground = foreground
            .unwrap()
            .join()
            .unwrap()
            .expect("a no-lease handoff window must not fail the foreground transaction");
        assert!(
            read_queue_edit_owner_lease(&file_text)
                .unwrap()
                .holder
                .starts_with(DIRECT_QUEUE_EDIT_OWNER_PREFIX)
        );
        drop(foreground);
        assert!(read_queue_edit_owner_lease(&file_text).is_none());
    }

    /// The handoff grace only covers a flock that names no holder: a live
    /// direct owner still fails a foreground acquirer on its first
    /// observation, and a no-lease flock that outlives the grace fails it too.
    #[test]
    fn handoff_grace_never_waits_on_a_direct_owner_or_past_its_bound() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy().to_string();

        let direct = QueueEditGuard::acquire(&file).unwrap();
        let mut observations = Vec::new();
        let refused = QueueEditGuard::acquire_with_background_wait_observed(
            &file,
            FOREGROUND_BACKGROUND_WAIT,
            QUEUE_APPLY_HANDOFF_GRACE,
            |lease| observations.push(lease.cloned()),
        )
        .unwrap_err();
        assert!(refused.to_string().contains("queue mutation deferred"));
        assert_eq!(observations.len(), 1, "a direct owner fails closed at once");
        assert!(
            observations[0]
                .as_ref()
                .is_some_and(|lease| lease.holder.starts_with(DIRECT_QUEUE_EDIT_OWNER_PREFIX))
        );

        let mut direct = direct;
        direct.release_with_handoff_hook(|| {
            let mut stuck_observations = 0;
            let refused = QueueEditGuard::acquire_with_background_wait_observed(
                &file,
                FOREGROUND_BACKGROUND_WAIT,
                Duration::from_millis(30),
                |lease| {
                    assert!(lease.is_none());
                    stuck_observations += 1;
                },
            )
            .unwrap_err();
            assert!(refused.to_string().contains("queue mutation deferred"));
            assert!(
                stuck_observations > 1,
                "the handoff window is polled before the bound fails it"
            );
        });
        assert!(read_queue_edit_owner_lease(&file_text).is_none());
    }

    /// `#answerheadmaintrace`: the handoff grace covers a flock that names no
    /// holder, never a direct owner. A direct mutation first observed between
    /// its flock and its token publish (inside the grace) still fails the
    /// foreground transaction closed as soon as its token appears.
    #[test]
    fn direct_owner_observed_before_its_token_publish_still_fails_closed() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy().to_string();
        let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
        let publish = std::sync::Arc::new(std::sync::Barrier::new(2));
        let release = std::sync::Arc::new(std::sync::Barrier::new(2));
        let worker_file = file.clone();
        let (worker_entered, worker_publish, worker_release) = (
            std::sync::Arc::clone(&entered),
            std::sync::Arc::clone(&publish),
            std::sync::Arc::clone(&release),
        );
        let worker = std::thread::spawn(move || {
            let guard = QueueEditGuard::try_acquire_with_hook(&worker_file, || {
                worker_entered.wait();
                worker_publish.wait();
            })
            .unwrap()
            .expect("direct worker acquired the apply lock");
            worker_release.wait();
            drop(guard);
        });

        entered.wait();
        let mut observations = Vec::new();
        let refused = QueueEditGuard::acquire_with_background_wait_observed(
            &file,
            RESPONSE_BACKGROUND_WAIT,
            QUEUE_APPLY_HANDOFF_GRACE,
            |lease| {
                observations.push(lease.cloned());
                if lease.is_none() {
                    publish.wait();
                    while read_queue_edit_owner_lease(&file_text).is_none() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            },
        )
        .unwrap_err();
        assert!(refused.to_string().contains("queue mutation deferred"));
        assert_eq!(observations.len(), 2, "{observations:?}");
        assert_eq!(
            observations[0], None,
            "first observed inside the handoff window"
        );
        assert!(
            observations[1]
                .as_ref()
                .is_some_and(|lease| lease.holder.starts_with(DIRECT_QUEUE_EDIT_OWNER_PREFIX)),
            "{observations:?}"
        );
        release.wait();
        worker.join().unwrap();
        assert!(read_queue_edit_owner_lease(&file_text).is_none());
    }

    #[test]
    fn expired_heartbeat_cannot_supersede_a_live_direct_apply_lock() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy().to_string();
        let direct = QueueEditGuard::acquire(&file).unwrap();
        let original = read_queue_edit_owner_lease(&file_text).unwrap();

        // Age the observable lease beyond every TTL without sleeping. The
        // still-held flock remains the authoritative proof that apply is live.
        let (project_root, document_hash) = state_location(&file_text).unwrap();
        agent_doc_controller_io::project_controller::upsert_coordination_lease(
            &project_root,
            &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
                scope_kind: QUEUE_EDIT_OWNER_SCOPE.to_string(),
                scope_id: document_hash,
                holder: original.holder.clone(),
                holder_pid: Some(std::process::id()),
                heartbeat_secs: 0,
            },
        )
        .unwrap();

        assert!(
            !try_reserve_queue_edit_owner(&file_text, "replacement", std::process::id()).unwrap(),
            "heartbeat expiry must not overwrite a live apply lock"
        );
        assert_eq!(
            read_queue_edit_owner_lease(&file_text).unwrap().holder,
            original.holder
        );

        drop(direct);
        assert!(
            try_reserve_queue_edit_owner(&file_text, "successor-after-release", std::process::id())
                .unwrap()
        );
        let successor = QueueEditGuard::adopt_reserved(&file, "successor-after-release")
            .unwrap()
            .expect("one successor adopts after the direct apply releases");
        drop(successor);
        assert!(read_queue_edit_owner_lease(&file_text).is_none());
    }

    #[test]
    fn apply_probe_observes_lock_before_sqlite_lease_publish() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
        let release = std::sync::Arc::new(std::sync::Barrier::new(2));
        let worker_file = file.clone();
        let worker_entered = std::sync::Arc::clone(&entered);
        let worker_release = std::sync::Arc::clone(&release);
        let worker = std::thread::spawn(move || {
            let guard = QueueEditGuard::try_acquire_with_hook(&worker_file, || {
                worker_entered.wait();
                worker_release.wait();
            })
            .unwrap()
            .expect("worker acquired apply lock");
            drop(guard);
        });

        entered.wait();
        assert!(queue_apply_in_flight(&file).unwrap());
        assert!(
            read_queue_edit_owner_lease(&file.to_string_lossy()).is_none(),
            "barrier must be before SQLite lease publication"
        );
        release.wait();
        worker.join().unwrap();
        assert!(!queue_apply_in_flight(&file).unwrap());
    }

    #[test]
    fn dead_crashed_attempt_is_durable_then_promptly_redispatched() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();
        let crashed = "crashed-worker-token";
        let dead_pid = 4_000_000;
        assert!(try_reserve_queue_edit_owner(&file_text, crashed, dead_pid).unwrap());
        let incomplete = read_queue_edit_owner_lease(&file_text).unwrap();
        assert_eq!(incomplete.holder, crashed);
        assert_eq!(incomplete.holder_pid, dead_pid);
        assert!(!queue_apply_in_flight(&file).unwrap());

        assert!(
            try_reserve_queue_edit_owner(&file_text, "recovery-worker", std::process::id())
                .unwrap(),
            "dead fresh attempt must be recovered without waiting for TTL"
        );
        assert_eq!(
            read_queue_edit_owner_lease(&file_text).unwrap().holder,
            "recovery-worker"
        );
    }

    /// `#deadmaintworker`: a pending request is owned only while a live
    /// process holds the flock or a live background reservation will adopt
    /// it. A worker that died after claiming leaves its token behind with a
    /// dead PID; that request is orphaned until a new owner runs it.
    #[test]
    fn pending_request_owner_distinguishes_a_live_worker_from_a_dead_one() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_text = file.to_string_lossy();
        assert_eq!(
            queue_maintenance_request_owner(&file).unwrap(),
            QueueMaintenanceRequestOwner::NoRequest
        );

        publish_queue_maintenance_request(&file, "hash-a", false).unwrap();
        assert_eq!(
            queue_maintenance_request_owner(&file).unwrap(),
            QueueMaintenanceRequestOwner::Orphaned { dead_holder: None },
            "a request nobody reserved is orphaned"
        );

        let dead = format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}dead-worker");
        assert!(try_reserve_queue_edit_owner(&file_text, &dead, 4_000_000).unwrap());
        match queue_maintenance_request_owner(&file).unwrap() {
            QueueMaintenanceRequestOwner::Orphaned {
                dead_holder: Some(lease),
            } => {
                assert_eq!(lease.holder, dead);
                assert_eq!(lease.holder_pid, 4_000_000);
            }
            other => panic!("a dead worker's request must be orphaned, got {other:?}"),
        }

        let live = format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}live-worker");
        assert!(try_reserve_queue_edit_owner(&file_text, &live, std::process::id()).unwrap());
        assert_eq!(
            queue_maintenance_request_owner(&file).unwrap(),
            QueueMaintenanceRequestOwner::Live {
                holder_pid: Some(std::process::id())
            },
            "a live reservation about to adopt owns the request"
        );
        let guard = QueueEditGuard::adopt_reserved(&file, &live)
            .unwrap()
            .expect("the live worker adopts");
        assert!(matches!(
            queue_maintenance_request_owner(&file).unwrap(),
            QueueMaintenanceRequestOwner::Live { .. }
        ));
        drop(guard);
    }

    #[test]
    fn latest_maintenance_request_survives_coalescing_and_completes_atomically() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body-a").unwrap();
        let file_text = file.to_string_lossy();

        let first = publish_queue_maintenance_request(&file, "hash-a", false).unwrap();
        let duplicate = publish_queue_maintenance_request(&file, "hash-a", false).unwrap();
        assert_eq!(duplicate.generation, first.generation);

        let token = format!("{BACKGROUND_QUEUE_EDIT_OWNER_PREFIX}latest-request-test");
        assert!(try_reserve_queue_edit_owner(&file_text, &token, std::process::id()).unwrap());
        let mut guard = QueueEditGuard::adopt_reserved(&file, &token)
            .unwrap()
            .expect("reserved incumbent");

        let successor = publish_queue_maintenance_request(&file, "hash-b", true).unwrap();
        assert!(successor.generation > first.generation);
        assert_eq!(
            load_queue_maintenance_request(&file).unwrap(),
            Some(successor.clone())
        );
        let claim =
            crate::queue_maintenance_claim::QueueMaintenanceClaim::reconnect(&file).unwrap();
        claim.claim_request(&first).unwrap();
        assert!(
            !claim.complete(&mut guard, &first).unwrap(),
            "the incumbent must not consume a newer coalesced generation"
        );
        assert_eq!(
            read_queue_edit_owner_lease(&file_text).unwrap().holder,
            token
        );
        claim.claim_request(&successor).unwrap();
        assert!(claim.complete(&mut guard, &successor).unwrap());
        assert!(load_queue_maintenance_request(&file).unwrap().is_none());
        assert!(read_queue_edit_owner_lease(&file_text).is_none());
    }
}

//! Bounded, named registry lock wait (`#registrylockbound`).
//!
//! `tmux_router::RegistryLock::acquire` blocks in `flock` with no limit. A
//! start, sync or claim queued behind a slow holder therefore hung silently
//! until the editor killed it, and nothing said which lock or which process it
//! was waiting on. At IDE restart every reopened tab ran its own start, so all
//! of them queued behind one holder.
//!
//! [`acquire`] waits for the same lock file with a non-blocking probe:
//!
//! - It gives up after [`registry_lock_wait_bound`] (default
//!   [`DEFAULT_REGISTRY_LOCK_WAIT`], `AGENT_DOC_REGISTRY_LOCK_WAIT_MS`
//!   overrides) with an error that names the lock file, what the caller was
//!   about to do, and the holder's pid and command line (`/proc/locks`).
//! - While it waits it reports `[progress] phase=registry_lock_wait` about
//!   once a second, so an editor that extends its wait on progress
//!   (`#starttimeoutload`) keeps waiting for a lock that is moving and can say
//!   where it stopped when it is not.
//!
//! Once the probe sees the lock free it takes the real
//! `tmux_router::RegistryLock`, so the lock file, the in-process reentrancy
//! check and the guard's release are exactly tmux-router's. Between the probe's
//! release and that `flock` another waiter can win the lock; the wait is then
//! that holder's critical section (registry writes are short). The bound
//! applies to the case that hung: a holder that does not let go.
//!
//! Every acquisition is counted per thread
//! ([`registry_lock_acquisitions_on_this_thread`]) so a test can pin how many
//! times a code path takes the lock (`#startupcriticalpath`), by count and not
//! by wall clock.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::cell::Cell;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tmux_router::RegistryLock;

/// How long [`acquire`] waits for a held registry lock by default.
pub const DEFAULT_REGISTRY_LOCK_WAIT: Duration = Duration::from_secs(30);

/// Overrides [`DEFAULT_REGISTRY_LOCK_WAIT`] in milliseconds.
pub const AGENT_DOC_REGISTRY_LOCK_WAIT_MS_ENV: &str = "AGENT_DOC_REGISTRY_LOCK_WAIT_MS";

/// The progress phase reported while waiting.
pub const REGISTRY_LOCK_WAIT_PHASE: &str = "registry_lock_wait";

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
const FIRST_BACKOFF: Duration = Duration::from_millis(5);
const MAX_BACKOFF: Duration = Duration::from_millis(100);

thread_local! {
    static HELD_ON_THIS_THREAD: Cell<bool> = const { Cell::new(false) };
    static ACQUISITIONS_ON_THIS_THREAD: Cell<u64> = const { Cell::new(0) };
}

/// The held registry lock. Dropping it releases the lock.
#[derive(Debug)]
#[must_use = "the registry lock is held only while the guard is alive"]
pub struct BoundedRegistryLock {
    _inner: RegistryLock,
}

impl Drop for BoundedRegistryLock {
    fn drop(&mut self) {
        HELD_ON_THIS_THREAD.set(false);
    }
}

/// How many registry locks this thread has taken through [`acquire`].
pub fn registry_lock_acquisitions_on_this_thread() -> u64 {
    ACQUISITIONS_ON_THIS_THREAD.get()
}

/// The configured wait bound.
pub fn registry_lock_wait_bound() -> Duration {
    registry_lock_wait_bound_from(
        std::env::var(AGENT_DOC_REGISTRY_LOCK_WAIT_MS_ENV)
            .ok()
            .as_deref(),
    )
}

fn registry_lock_wait_bound_from(value: Option<&str>) -> Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_REGISTRY_LOCK_WAIT)
}

/// The lock file tmux-router's `RegistryLock` takes for `registry_path`.
pub fn registry_lock_file_path(registry_path: &Path) -> PathBuf {
    let sqlite = registry_path.file_name().and_then(|name| name.to_str()) == Some("state.db")
        || matches!(
            registry_path
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("db" | "sqlite" | "sqlite3")
        );
    if sqlite {
        registry_path.with_extension("db.lock")
    } else {
        registry_path.with_extension("json.lock")
    }
}

/// Take the registry lock for `registry_path`, waiting at most
/// [`registry_lock_wait_bound`]. `purpose` names what the caller is about to
/// do; it appears in the error when the wait gives up.
pub fn acquire(registry_path: &Path, purpose: &str) -> Result<BoundedRegistryLock> {
    acquire_within(registry_path, purpose, registry_lock_wait_bound())
}

/// [`acquire`] with an explicit bound.
pub fn acquire_within(
    registry_path: &Path,
    purpose: &str,
    bound: Duration,
) -> Result<BoundedRegistryLock> {
    if HELD_ON_THIS_THREAD.get() {
        // The probe would see our own lock as held and wait out the bound.
        anyhow::bail!(
            "registry lock {} is already held on this thread; refusing a nested acquisition to {purpose}",
            registry_lock_file_path(registry_path).display()
        );
    }
    wait_until_free(registry_path, purpose, bound)?;
    let inner = RegistryLock::acquire(registry_path)
        .with_context(|| format!("failed to take the registry lock to {purpose}"))?;
    HELD_ON_THIS_THREAD.set(true);
    ACQUISITIONS_ON_THIS_THREAD.set(ACQUISITIONS_ON_THIS_THREAD.get() + 1);
    Ok(BoundedRegistryLock { _inner: inner })
}

fn wait_until_free(registry_path: &Path, purpose: &str, bound: Duration) -> Result<()> {
    let lock_path = registry_lock_file_path(registry_path);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let probe = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open lock file {}", lock_path.display()))?;
    let started = Instant::now();
    let mut last_heartbeat: Option<Instant> = None;
    let mut backoff = FIRST_BACKOFF;
    loop {
        match probe.try_lock_exclusive() {
            Ok(()) => {
                probe.unlock().with_context(|| {
                    format!("failed to release probe on {}", lock_path.display())
                })?;
                return Ok(());
            }
            Err(error) if error.kind() == fs2::lock_contended_error().kind() => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to probe registry lock {}", lock_path.display())
                });
            }
        }
        let waited = started.elapsed();
        if waited >= bound {
            anyhow::bail!(
                "{}",
                registry_lock_wait_exceeded_message(
                    &lock_path,
                    purpose,
                    waited,
                    &describe_holders(&lock_path)
                )
            );
        }
        if last_heartbeat.is_none_or(|at| at.elapsed() >= HEARTBEAT_INTERVAL) {
            agent_doc_tmux_io::bootstrap_progress::report_progress_phase(REGISTRY_LOCK_WAIT_PHASE);
            last_heartbeat = Some(Instant::now());
        }
        std::thread::sleep(backoff.min(bound.saturating_sub(waited)));
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// The error a bounded wait gives up with.
pub fn registry_lock_wait_exceeded_message(
    lock_path: &Path,
    purpose: &str,
    waited: Duration,
    holders: &str,
) -> String {
    format!(
        "registry lock {} is still held after waiting {:.1}s to {purpose}; holder: {holders}. \
         Retry once that process finishes; if it is stuck, inspect it before stopping it",
        lock_path.display(),
        waited.as_secs_f64(),
    )
}

fn describe_holders(lock_path: &Path) -> String {
    let pids = lock_file_inode(lock_path)
        .and_then(|inode| {
            std::fs::read_to_string("/proc/locks")
                .ok()
                .map(|locks| flock_holder_pids(&locks, inode))
        })
        .unwrap_or_default();
    if pids.is_empty() {
        return "unknown (no /proc/locks entry)".to_string();
    }
    pids.iter()
        .map(|pid| match process_command_line(*pid) {
            Some(command) => format!("pid {pid} ({command})"),
            None => format!("pid {pid}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(unix)]
fn lock_file_inode(lock_path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(lock_path).ok().map(|meta| meta.ino())
}

#[cfg(not(unix))]
fn lock_file_inode(_lock_path: &Path) -> Option<u64> {
    None
}

/// Pids holding (not waiting for) a `FLOCK` on `inode`, from `/proc/locks`.
///
/// A line is `N: FLOCK ADVISORY WRITE <pid> <maj>:<min>:<inode> 0 EOF`; a
/// blocked waiter's line carries `->` after the ordinal and is skipped.
pub fn flock_holder_pids(proc_locks: &str, inode: u64) -> Vec<u32> {
    let mut pids = proc_locks
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            fields.next()?;
            let kind = fields.next()?;
            if kind != "FLOCK" {
                return None;
            }
            let _advisory = fields.next()?;
            let _mode = fields.next()?;
            let pid = fields.next()?.parse::<u32>().ok()?;
            let file_inode = fields.next()?.rsplit(':').next()?.parse::<u64>().ok()?;
            (file_inode == inode).then_some(pid)
        })
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    pids
}

fn process_command_line(pid: u32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let command = raw
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    if command.is_empty() {
        return None;
    }
    const LIMIT: usize = 160;
    Some(if command.chars().count() > LIMIT {
        format!("{}…", command.chars().take(LIMIT).collect::<String>())
    } else {
        command
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_bound_defaults_and_overrides() {
        assert_eq!(
            registry_lock_wait_bound_from(None),
            DEFAULT_REGISTRY_LOCK_WAIT
        );
        assert_eq!(
            registry_lock_wait_bound_from(Some("nonsense")),
            DEFAULT_REGISTRY_LOCK_WAIT
        );
        assert_eq!(
            registry_lock_wait_bound_from(Some(" 250 ")),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn lock_file_matches_tmux_router() {
        assert_eq!(
            registry_lock_file_path(Path::new("/p/.agent-doc/state.db")),
            PathBuf::from("/p/.agent-doc/state.db.lock")
        );
        assert_eq!(
            registry_lock_file_path(Path::new("/p/sessions.json")),
            PathBuf::from("/p/sessions.json.lock")
        );
    }

    #[test]
    fn proc_locks_names_holders_not_waiters() {
        let locks = "\
1: FLOCK  ADVISORY  WRITE 4242 103:02:777 0 EOF
1: -> FLOCK  ADVISORY  WRITE 5151 103:02:777 0 EOF
2: POSIX  ADVISORY  WRITE 6000 103:02:777 0 EOF
3: FLOCK  ADVISORY  WRITE 7000 103:02:888 0 EOF
";
        assert_eq!(flock_holder_pids(locks, 777), vec![4242]);
        assert_eq!(flock_holder_pids(locks, 999), Vec::<u32>::new());
    }

    /// A held lock is not waited on forever: the wait gives up at its bound
    /// and names the lock, the purpose and this process as the holder.
    #[test]
    fn a_held_lock_gives_up_at_the_bound_and_names_the_holder() {
        let dir = tempfile::tempdir().unwrap();
        let registry = dir.path().join(".agent-doc/state.db");
        std::fs::create_dir_all(registry.parent().unwrap()).unwrap();
        let held = std::thread::scope(|scope| {
            // Hold the lock on another thread (the reentrancy check is per
            // thread) and wait from this one.
            let (locked_tx, locked_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
            let registry_for_holder = registry.clone();
            scope.spawn(move || {
                let _lock = RegistryLock::acquire(&registry_for_holder).unwrap();
                locked_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            });
            locked_rx.recv().unwrap();
            let before = registry_lock_acquisitions_on_this_thread();
            let error = acquire_within(&registry, "test the bound", Duration::from_millis(50))
                .unwrap_err()
                .to_string();
            assert_eq!(registry_lock_acquisitions_on_this_thread(), before);
            release_tx.send(()).unwrap();
            error
        });
        assert!(held.contains("state.db.lock"), "{held}");
        assert!(held.contains("to test the bound"), "{held}");
        #[cfg(target_os = "linux")]
        assert!(
            held.contains(&format!("pid {}", std::process::id())),
            "the holder is this test process: {held}"
        );
        // Released: the next acquisition succeeds and is counted.
        let before = registry_lock_acquisitions_on_this_thread();
        let lock = acquire_within(&registry, "after release", Duration::from_millis(50)).unwrap();
        assert_eq!(registry_lock_acquisitions_on_this_thread(), before + 1);
        let nested = acquire_within(&registry, "nest", Duration::from_secs(5))
            .unwrap_err()
            .to_string();
        assert!(nested.contains("already held on this thread"), "{nested}");
        drop(lock);
        drop(acquire_within(&registry, "again", Duration::from_millis(50)).unwrap());
    }
}

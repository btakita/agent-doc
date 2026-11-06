//! Progress-aware wait for the per-document completion-feedback lock.
//!
//! `#completionlockload`: `queue report-complete`, the completion repairs, and
//! the removed-claim tombstone all serialize on one `flock(2)` per document.
//! A fixed wall-clock budget measured machine load, not a stuck owner: at load
//! average ~122 on 2026-11-05 four one-shot reporters queued behind each other
//! and the last one gave up after 5 s while every owner ahead of it was still
//! finishing normally.
//!
//! The wait is now bounded by two independent limits:
//!
//! - **stall window** — every owner bumps a generation counter stored in the
//!   lock file the moment it acquires. A waiter that sees the counter change
//!   within a window knows the queue ahead of it is draining and keeps
//!   waiting; a window with no handoff means the current owner is stuck, and
//!   the waiter fails fast.
//! - **max wait** — a hard ceiling on the whole wait, so a waiter that keeps
//!   losing the (unfair) `flock` race to newcomers still never blocks forever.
//!
//! Progress is a handoff count, never a duration, so a slow machine extends
//! the wait only while owners keep handing off.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

/// Wait limits for a progress-aware lock acquire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressLockWait {
    /// Longest wait without any observed owner handoff.
    pub stall_window: Duration,
    /// Hard ceiling on the whole wait, progress or not.
    pub max_wait: Duration,
}

/// Production limits for the completion-feedback lock: one owner may hold it
/// for up to 30 s, and the whole queue of owners ahead may take up to 5 min.
pub const COMPLETION_FEEDBACK_LOCK_WAIT: ProgressLockWait = ProgressLockWait {
    stall_window: Duration::from_secs(30),
    max_wait: Duration::from_secs(300),
};

/// One elapsed stall window during which the lock stayed contended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressLockRound {
    /// 1-based count of elapsed windows.
    pub round: u32,
    /// Owner handoffs observed during this window.
    pub handoffs: u64,
    /// Owner handoffs observed since the wait began.
    pub total_handoffs: u64,
    pub waited: Duration,
}

/// Why a progress-aware acquire gave up. Each variant guarantees no lock is
/// held by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressLockRefused {
    /// One whole stall window passed with no owner handoff.
    Stalled {
        rounds: u32,
        total_handoffs: u64,
        waited: Duration,
        stall_window: Duration,
    },
    /// Owners kept handing off, but this waiter never won before the ceiling.
    MaxWait {
        rounds: u32,
        total_handoffs: u64,
        waited: Duration,
        max_wait: Duration,
    },
    /// The lock file could not be opened or locked at all.
    Unavailable(String),
}

impl std::fmt::Display for ProgressLockRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stalled {
                rounds,
                total_handoffs,
                waited,
                stall_window,
            } => write!(
                f,
                "lock owner made no progress for {}ms (stall window; waited {}ms over {rounds} window(s), {total_handoffs} owner handoff(s) observed)",
                stall_window.as_millis(),
                waited.as_millis(),
            ),
            Self::MaxWait {
                rounds,
                total_handoffs,
                waited,
                max_wait,
            } => write!(
                f,
                "lock wait reached its {}ms ceiling while owners kept handing off (waited {}ms over {rounds} window(s), {total_handoffs} owner handoff(s) observed)",
                max_wait.as_millis(),
                waited.as_millis(),
            ),
            Self::Unavailable(reason) => write!(f, "lock unavailable: {reason}"),
        }
    }
}

/// A held completion-feedback lock. Dropping it releases the lock.
#[derive(Debug)]
pub struct CompletionFeedbackLock {
    _file: File,
}

/// Acquire `file`'s completion-feedback lock under
/// [`COMPLETION_FEEDBACK_LOCK_WAIT`]. `action` names the caller's operation
/// for the refusal message ("publish subagent completion feedback for …");
/// `on_wait` sees every contended window so a CLI can print progress.
pub fn acquire_completion_feedback_lock(
    file: &Path,
    action: &str,
    on_wait: impl FnMut(&ProgressLockRound),
) -> Result<CompletionFeedbackLock> {
    acquire_completion_feedback_lock_with(file, action, COMPLETION_FEEDBACK_LOCK_WAIT, on_wait)
}

/// [`acquire_completion_feedback_lock`] with explicit limits (tests).
pub fn acquire_completion_feedback_lock_with(
    file: &Path,
    action: &str,
    wait: ProgressLockWait,
    on_wait: impl FnMut(&ProgressLockRound),
) -> Result<CompletionFeedbackLock> {
    let lock_path = crate::queue_claim::completion_feedback_lock_path(file)?;
    match acquire_progress_lock(&lock_path, wait, || read_generation(&lock_path), on_wait) {
        Ok(file) => Ok(CompletionFeedbackLock { _file: file }),
        Err(refused) => bail!(
            "timed out waiting to {action} for {}: {refused}; no document mutation was attempted",
            file.display(),
        ),
    }
}

/// Acquire an exclusive `flock` on `lock_path`, waiting while the owner
/// generation observed by `observe_generation` keeps changing. On success the
/// generation stored in the lock file is bumped before returning, so every
/// acquire is a handoff visible to the other waiters.
pub fn acquire_progress_lock(
    lock_path: &Path,
    wait: ProgressLockWait,
    mut observe_generation: impl FnMut() -> u64,
    mut on_wait: impl FnMut(&ProgressLockRound),
) -> std::result::Result<File, ProgressLockRefused> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            ProgressLockRefused::Unavailable(format!("create {}: {err}", parent.display()))
        })?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|err| {
            ProgressLockRefused::Unavailable(format!("open {}: {err}", lock_path.display()))
        })?;
    if fs2::FileExt::try_lock_exclusive(&file).is_ok() {
        return Ok(bump_generation(file));
    }

    // Contended. One helper thread blocks in the kernel for the whole wait;
    // the waiter only decides how long to keep listening. If the waiter gives
    // up, the helper's eventual send fails and the file (with its lock) drops
    // at once, so a refused wait never lingers as an owner.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("agent-doc-completion-lock-wait".into())
        .spawn(move || {
            if fs2::FileExt::lock_exclusive(&file).is_ok() {
                let _ = tx.send(file);
            }
        })
        .map_err(|err| ProgressLockRefused::Unavailable(format!("spawn wait thread: {err}")))?;

    let started = Instant::now();
    let mut observed = observe_generation();
    let mut rounds = 0u32;
    let mut total_handoffs = 0u64;
    loop {
        let remaining = wait.max_wait.saturating_sub(started.elapsed());
        let window = wait.stall_window.min(remaining);
        match rx.recv_timeout(window) {
            Ok(file) => return Ok(bump_generation(file)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(ProgressLockRefused::Unavailable(format!(
                    "blocking acquire of {} ended without the lock",
                    lock_path.display()
                )));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        rounds += 1;
        let current = observe_generation();
        let handoffs = current.wrapping_sub(observed);
        observed = current;
        total_handoffs = total_handoffs.saturating_add(handoffs);
        let waited = started.elapsed();
        on_wait(&ProgressLockRound {
            round: rounds,
            handoffs,
            total_handoffs,
            waited,
        });
        if handoffs == 0 {
            return Err(ProgressLockRefused::Stalled {
                rounds,
                total_handoffs,
                waited,
                stall_window: wait.stall_window,
            });
        }
        if waited >= wait.max_wait {
            return Err(ProgressLockRefused::MaxWait {
                rounds,
                total_handoffs,
                waited,
                max_wait: wait.max_wait,
            });
        }
    }
}

/// Owner generation stored in the lock file; a missing, empty, or foreign
/// file reads as generation 0.
pub fn read_generation(lock_path: &Path) -> u64 {
    std::fs::read_to_string(lock_path)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Record one more owner handoff. Called only while the lock is held, so the
/// read-modify-write cannot race another owner. A failed write costs waiters
/// one observed handoff, never the lock itself.
fn bump_generation(mut file: File) -> File {
    let mut text = String::new();
    let current = file
        .seek(SeekFrom::Start(0))
        .and_then(|_| file.read_to_string(&mut text))
        .ok()
        .and_then(|_| text.trim().parse::<u64>().ok())
        .unwrap_or(0);
    // Fixed width: the value never shrinks, so no truncate is needed and a
    // concurrent reader never sees an empty file.
    let next = format!("{:020}\n", current.wrapping_add(1));
    let _ = file
        .seek(SeekFrom::Start(0))
        .and_then(|_| file.write_all(next.as_bytes()));
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Own the lock, waiting in the kernel with no clock deciding.
    ///
    /// A zero window made any transient contention an instant refusal, and an
    /// "uncontended" lock file still has transient contention: another test
    /// in this process spawning a child duplicates every open descriptor
    /// between fork and exec, O_CLOEXEC included, and `flock` belongs to the
    /// open file description, so the duplicate keeps the lock alive after
    /// `drop`. A standalone repro with four spawning threads refused 111514
    /// of 200000 non-blocking acquires and 0 of 200000 blocking ones, which
    /// is why `every_acquire_is_a_visible_handoff` failed with
    /// `Stalled { stall_window: 0ns }`.
    fn hold(lock_path: &Path) -> File {
        acquire_progress_lock(
            lock_path,
            ProgressLockWait {
                stall_window: Duration::MAX,
                max_wait: Duration::MAX,
            },
            || 0,
            |_| {},
        )
        .expect("fixture must own the uncontended lock")
    }

    #[test]
    fn every_acquire_is_a_visible_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("locks/completion.lock");
        assert_eq!(read_generation(&lock_path), 0);
        drop(hold(&lock_path));
        assert_eq!(read_generation(&lock_path), 1);
        drop(hold(&lock_path));
        assert_eq!(read_generation(&lock_path), 2);
    }

    #[test]
    fn a_stalled_owner_refuses_after_exactly_one_window() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("completion.lock");
        let _held = hold(&lock_path);
        let mut rounds = Vec::new();
        let refused = acquire_progress_lock(
            &lock_path,
            ProgressLockWait {
                stall_window: Duration::from_millis(20),
                max_wait: Duration::from_secs(3600),
            },
            || 7,
            |round| rounds.push(*round),
        )
        .unwrap_err();
        let ProgressLockRefused::Stalled {
            rounds: refused_rounds,
            total_handoffs,
            ..
        } = refused
        else {
            panic!("a window without a handoff must refuse as stalled: {refused:?}");
        };
        assert_eq!((refused_rounds, total_handoffs), (1, 0));
        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].handoffs, 0);
    }

    /// The 2026-11-05 failure shape: the owner ahead takes longer than one
    /// window, but owners keep handing off, so the waiter keeps waiting and
    /// wins once the queue drains. The handoffs and the release are driven
    /// from the observer, so the outcome does not depend on machine speed.
    #[test]
    fn a_waiter_outlasts_many_windows_while_owners_hand_off() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("completion.lock");
        let held = Arc::new(Mutex::new(Some(hold(&lock_path))));
        let generation_before = read_generation(&lock_path);
        let observations = Arc::new(Mutex::new(0u64));
        let observer_held = Arc::clone(&held);
        let observer_count = Arc::clone(&observations);
        let mut rounds = Vec::new();
        let file = acquire_progress_lock(
            &lock_path,
            ProgressLockWait {
                stall_window: Duration::from_millis(10),
                max_wait: Duration::from_secs(3600),
            },
            move || {
                let mut count = observer_count.lock().unwrap();
                *count += 1;
                // Observation 1 is the baseline; windows 1-3 each see one
                // simulated handoff, and the owner releases after window 3.
                if *count == 4 {
                    observer_held.lock().unwrap().take();
                }
                *count
            },
            |round| rounds.push(*round),
        )
        .expect("a draining owner queue must never refuse the waiter");
        assert!(held.lock().unwrap().is_none());
        assert!(
            rounds.len() >= 3,
            "the waiter outlasted three windows: {rounds:?}"
        );
        assert!(rounds.iter().all(|round| round.handoffs == 1));
        assert_eq!(
            read_generation(&lock_path),
            generation_before + 1,
            "the waiter's own acquire is the next handoff"
        );
        drop(file);
    }

    #[test]
    fn owners_that_keep_handing_off_still_hit_the_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("completion.lock");
        let _held = hold(&lock_path);
        let mut generation = 0u64;
        let refused = acquire_progress_lock(
            &lock_path,
            ProgressLockWait {
                stall_window: Duration::from_secs(3600),
                max_wait: Duration::from_millis(20),
            },
            || {
                generation += 1;
                generation
            },
            |_| {},
        )
        .unwrap_err();
        assert!(
            matches!(
                refused,
                ProgressLockRefused::MaxWait {
                    rounds: 1,
                    total_handoffs: 1,
                    ..
                }
            ),
            "progress never extends the wait past its ceiling: {refused:?}"
        );
    }

    #[test]
    fn a_refused_waiter_never_lingers_as_an_owner() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("completion.lock");
        let held = hold(&lock_path);
        acquire_progress_lock(
            &lock_path,
            ProgressLockWait {
                stall_window: Duration::from_millis(10),
                max_wait: Duration::from_secs(3600),
            },
            || 0,
            |_| {},
        )
        .unwrap_err();
        drop(held);
        // The refused waiter's helper thread may still win the kernel race
        // once, but it drops the lock at once because nobody is listening; a
        // blocking acquire therefore always completes.
        let (tx, rx) = std::sync::mpsc::channel();
        let path = lock_path.clone();
        std::thread::spawn(move || {
            let file = OpenOptions::new().write(true).open(path).unwrap();
            fs2::FileExt::lock_exclusive(&file).unwrap();
            tx.send(()).unwrap();
        });
        rx.recv().expect("the lock must become free again");
    }
}

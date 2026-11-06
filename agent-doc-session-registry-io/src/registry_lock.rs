//! Bounded, named registry lock wait (`#registrylockbound`,
//! `#tmuxrouterboundedlock`).
//!
//! A start, sync or claim queued behind a slow registry-lock holder used to
//! hang silently in `flock` until the editor killed it, and nothing said which
//! lock or which process it was waiting on. At IDE restart every reopened tab
//! ran its own start, so all of them queued behind one holder.
//!
//! The wait itself is `tmux_router::RegistryLock`'s: every acquisition polls a
//! non-blocking lock on the guard's own file until it succeeds or a bound
//! passes, so the attempt that succeeds is the acquisition (no free-to-taken
//! gap), and gives up with `tmux_router::RegistryLockWaitExceeded`, naming the
//! lock file, the purpose and the holder's pid and command line. This module
//! supplies agent-doc's policy for it:
//!
//! - The bound is [`registry_lock_wait_bound`] (default
//!   [`DEFAULT_REGISTRY_LOCK_WAIT`], `AGENT_DOC_REGISTRY_LOCK_WAIT_MS`
//!   overrides).
//! - While it waits it reports `[progress] phase=registry_lock_wait` about
//!   once a second, so an editor that extends its wait on progress
//!   (`#starttimeoutload`) keeps waiting for a lock that is moving and can say
//!   where it stopped when it is not.
//!
//! [`acquire`] passes that policy with a named purpose.
//! [`install_process_registry_lock_wait`] makes it tmux-router's process-wide
//! policy, so the acquisitions tmux-router takes internally
//! (`tmux_router::prune`, `with_registry`, ...) are bounded and report the
//! same progress.
//!
//! Every acquisition, through any entry point, is counted per thread
//! ([`registry_lock_acquisitions_on_this_thread`]) so a test can pin how many
//! times a code path takes the lock (`#startupcriticalpath`), by count and not
//! by wall clock.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;
use tmux_router::registry::RegistryLockWait;

pub use tmux_router::registry::{
    DEFAULT_REGISTRY_LOCK_WAIT, RegistryLockWaitExceeded,
    registry_lock_acquisitions_on_this_thread, registry_lock_file_path,
};

/// Overrides [`DEFAULT_REGISTRY_LOCK_WAIT`] in milliseconds.
pub const AGENT_DOC_REGISTRY_LOCK_WAIT_MS_ENV: &str = "AGENT_DOC_REGISTRY_LOCK_WAIT_MS";

/// The progress phase reported while waiting.
pub const REGISTRY_LOCK_WAIT_PHASE: &str = "registry_lock_wait";

/// The held registry lock. Dropping it releases the lock.
pub type BoundedRegistryLock = tmux_router::RegistryLock;

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

fn report_registry_lock_wait() {
    agent_doc_tmux_io::bootstrap_progress::report_progress_phase(REGISTRY_LOCK_WAIT_PHASE);
}

/// agent-doc's registry lock wait: `bound`, reporting
/// [`REGISTRY_LOCK_WAIT_PHASE`] progress while it waits.
pub fn registry_lock_wait_policy(bound: Duration) -> RegistryLockWait {
    RegistryLockWait {
        bound,
        heartbeat: Some(report_registry_lock_wait),
    }
}

/// Make agent-doc's wait policy (configured bound, progress heartbeat)
/// tmux-router's process-wide one, so its internal acquisitions are bounded
/// and report progress too. Call once at process start.
pub fn install_process_registry_lock_wait() {
    tmux_router::registry::set_registry_lock_wait(registry_lock_wait_policy(
        registry_lock_wait_bound(),
    ));
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
    tmux_router::RegistryLock::acquire_with(
        registry_path,
        purpose,
        registry_lock_wait_policy(bound),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmux_router::RegistryLock;

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
        let policy = registry_lock_wait_policy(Duration::from_millis(250));
        assert_eq!(policy.bound, Duration::from_millis(250));
        assert!(policy.heartbeat.is_some(), "the wait reports progress");
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
            let error =
                acquire_within(&registry, "test the bound", Duration::from_millis(50)).unwrap_err();
            assert_eq!(registry_lock_acquisitions_on_this_thread(), before);
            release_tx.send(()).unwrap();
            error
        });
        assert!(
            held.downcast_ref::<RegistryLockWaitExceeded>().is_some(),
            "{held:#}"
        );
        let held = held.to_string();
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

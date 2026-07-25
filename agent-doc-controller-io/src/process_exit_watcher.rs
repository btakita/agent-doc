//! Cross-platform OS process-exit watcher (`#s4b-pidfd`, `#live-editor-reactive`).
//!
//! The reactive editor-attachment authority
//! ([`agent_doc_document_realtime::editor_attach`]) needs one signal it cannot get from
//! the editor's own event stream: a **crash**. A crashed editor sends no `deregister`, so
//! without an out-of-band signal a cached "editor attached" would go stale and wrongly
//! hold multi-replica authority, blocking disk commits. This watcher supplies that signal
//! by observing the editor **process** liveness off the hot path and flipping the reactive
//! `alive` cell on exit (crash **or** clean exit).
//!
//! ## Why a portable poller (cross-platform: Linux, macOS, Windows)
//!
//! The plan's north-star primitive is an OS exit *event* (`pidfd` on Linux,
//! `kqueue`/`EVFILT_PROC` on macOS, a wait handle on Windows). Those are three separate
//! unsafe integrations and Linux-only for `pidfd`. Since the authority hot path is already
//! fully reactive (zero syscall per decision — the whole point of S4b), the crash signal
//! only needs to arrive *off* the hot path within a bounded latency. A single portable
//! **liveness poller** — `kill(pid, 0)` on Unix (Linux + macOS), an `OpenProcess` wait on
//! Windows — delivers exactly that on all three platforms with one small code path and no
//! per-pid OS resource to leak (`unwatch` just drops the pid from the polled set). The
//! per-OS *event* primitives remain a latency optimization that can drop in behind the
//! same [`ProcessExitWatcher`] seam later without touching any consumer.
//!
//! The poller runs on a dedicated background thread owned by the project controller; the
//! authority gate never blocks on it. Tests never install this watcher — they install a
//! fake one and drive synthetic exit events (see `editor_attach`'s SimWorld tests).

use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use agent_doc_document_realtime::editor_attach::{ProcessExitWatcher, editor_attach};

/// How often the poller re-checks each watched pid. Bounds crash-detection latency; the
/// authority hot path stays reactive and never waits on this.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// The controller-owned OS process-exit watcher. Installed once on the process-global
/// [`editor_attach`] registry at controller startup.
pub struct OsProcessExitWatcher {
    watched: Arc<Mutex<HashSet<u32>>>,
}

impl OsProcessExitWatcher {
    /// Spawn the background poller thread and return the watcher handle. The thread runs
    /// for the lifetime of the controller process (it exits when the process does).
    pub fn new(project_root: PathBuf) -> Self {
        let watched: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));
        let thread_watched = Arc::clone(&watched);
        thread::Builder::new()
            .name("agent-doc-process-exit-watcher".to_string())
            .spawn(move || run_poll_loop(thread_watched, project_root))
            .expect("spawn agent-doc process-exit watcher thread");
        Self { watched }
    }
}

impl ProcessExitWatcher for OsProcessExitWatcher {
    fn watch(&self, pid: u32) {
        self.watched.lock().insert(pid);
    }

    fn unwatch(&self, pid: u32) {
        self.watched.lock().remove(&pid);
    }
}

/// Install the OS process-exit watcher on the process-global editor-attachment registry.
/// Idempotent-safe to call once at controller startup; after this the registry seeds its
/// reactive authority on attach and the hot path reads pure reactive state.
pub fn install_process_exit_watcher(project_root: PathBuf) {
    editor_attach().install_watcher(Arc::new(OsProcessExitWatcher::new(project_root)));
}

fn run_poll_loop(watched: Arc<Mutex<HashSet<u32>>>, project_root: PathBuf) {
    loop {
        thread::sleep(POLL_INTERVAL);
        // The explicitly-`watch()`ed set is only ever seeded by live attach events
        // in *this* process. It is empty after a fresh controller hydration and can
        // drift from the liveness plane's open-set under a plugin register/deregister
        // storm. Union it with every pid the plane itself still counts as
        // open-and-alive so a crashed editor whose terminal `Alive{false}` was never
        // published — e.g. one restored by durable hydration on controller restart —
        // is still polled for death. Without this the ghost pid stays
        // `pid_alive == true` forever (`pid_alive` presumes alive absent a death
        // fact), holding `live_editors >= 1` and wedging every disk-authority resolve
        // in an unrecoverable authority/disk-divergence loop (`#ghosteditorliveness`).
        let mut candidates: HashSet<u32> = { watched.lock().iter().copied().collect() };
        candidates.extend(plane_open_alive_pids());
        for pid in pids_to_reap(&candidates, process_is_live) {
            // OS-observed exit → drive the reactive `alive` cell closed. Every
            // document that pid owned recomputes to detached.
            editor_attach().process_exited(pid);
            // Sidecar-retirement Phase 3C: also write the reliable-sync
            // `Alive{false}` so the shadow liveness plane cascades the same
            // crash demote and the death fact is durably persisted (survives the
            // next controller hydration).
            crate::project_controller::record_reliable_sync_editor_exit(&project_root, pid as u64);
            watched.lock().remove(&pid);
        }
    }
}

/// Every pid the reliable-sync liveness plane still counts as open-and-alive,
/// across all documents, narrowed to the `u32` OS-pid width. This is the plane's
/// own view of "who might be a live editor" — the exact set whose staleness
/// produces the `#ghosteditorliveness` wedge — so polling it for death closes the
/// gap between the plane's open-set and the watcher's `watch()`ed set. A plane
/// `Pid` (u64) that does not fit `u32` cannot be a real OS pid; it is dropped
/// rather than reaped (conservative: never demote what we cannot prove dead).
fn plane_open_alive_pids() -> Vec<u32> {
    let plane = agent_doc_reliable_sync_io::global_liveness_plane().lock();
    let projection = plane.projection();
    projection
        .all_open_pids()
        .into_iter()
        .filter(|pid| projection.pid_alive(*pid))
        .filter_map(|pid| u32::try_from(pid).ok())
        .collect()
}

/// Pure reap decision: of `candidates`, which pids does OS liveness report as
/// gone? Extracted from [`run_poll_loop`] so the "a hydrated open pid that was
/// never `watch()`ed is still reaped" rule (`#ghosteditorliveness`) is unit
/// testable without a controller, a real thread, or the global plane. The
/// liveness predicate is biased toward *alive* on an ambiguous permission error
/// (see [`process_is_live`]), so only a genuinely-gone pid (ESRCH) is reaped.
fn pids_to_reap(candidates: &HashSet<u32>, is_live: impl Fn(u32) -> bool) -> Vec<u32> {
    candidates
        .iter()
        .copied()
        .filter(|pid| !is_live(*pid))
        .collect()
}

/// Whether `pid` is a currently-live process. Biased toward reporting **alive** on an
/// ambiguous permission error so a live editor is never falsely demoted to disk (the
/// File-Cache-Conflict-avoidance bias, matching the plugin-owner lease predicate).
#[cfg(unix)]
fn process_is_live(pid: u32) -> bool {
    // `kill(pid, 0)` (POSIX; Linux + macOS): 0 ⇒ exists; EPERM ⇒ exists (not permitted);
    // only ESRCH ⇒ no such process.
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Windows liveness via a process handle. A process handle is *signaled* once the process
/// exits, so a zero-millisecond wait that returns `WAIT_OBJECT_0` means it has exited.
#[cfg(windows)]
fn process_is_live(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, GetLastError, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            // Distinguish "process gone" from "no access": access-denied means the
            // process still exists (bias toward alive, avoid demoting a live editor); any
            // other failure (typically invalid-parameter for a dead pid) means gone.
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        let signaled = WaitForSingleObject(handle, 0) == WAIT_OBJECT_0;
        CloseHandle(handle);
        !signaled
    }
}

/// Unknown platform: conservatively report alive so nothing is spuriously demoted (matches
/// the plugin-owner lease predicate's non-Unix fallback). Graceful `deregister` still
/// drives detach cross-platform; only crash auto-detection is unavailable here.
#[cfg(not(any(unix, windows)))]
fn process_is_live(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pids_to_reap_selects_only_dead_pids() {
        // A candidate set mixing a live pid (still-attached editor) and a dead pid
        // (crashed editor whose `Alive{false}` was never published, restored by
        // hydration and hence never `watch()`ed). Only the dead pid is reaped, and
        // the live pid is never demoted (`#ghosteditorliveness`).
        let candidates: HashSet<u32> = [101, 202].into_iter().collect();
        let is_live = |pid: u32| pid == 101;
        let reaped = pids_to_reap(&candidates, is_live);
        assert_eq!(reaped, vec![202], "only the dead pid is reaped: {reaped:?}");
    }

    #[test]
    fn pids_to_reap_reaps_hydrated_ghost_never_watched() {
        // The exact incident shape: the pid was NOT in the `watch()`ed set (that set
        // is empty after hydration) but IS a plane-open candidate, and it is dead.
        // The union feeds it here, and it is reaped so `live_editors` can drop to 0.
        let candidates: HashSet<u32> = [930999].into_iter().collect();
        let reaped = pids_to_reap(&candidates, |_| false);
        assert_eq!(reaped, vec![930999]);
    }

    #[test]
    fn pids_to_reap_keeps_all_live() {
        let candidates: HashSet<u32> = [1, 2, 3].into_iter().collect();
        let reaped = pids_to_reap(&candidates, |_| true);
        assert!(reaped.is_empty(), "no live pid is ever reaped: {reaped:?}");
    }
}

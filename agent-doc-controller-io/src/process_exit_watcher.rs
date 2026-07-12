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

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
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
    pub fn new() -> Self {
        let watched: Arc<Mutex<HashSet<u32>>> = Arc::new(Mutex::new(HashSet::new()));
        let thread_watched = Arc::clone(&watched);
        thread::Builder::new()
            .name("agent-doc-process-exit-watcher".to_string())
            .spawn(move || run_poll_loop(thread_watched))
            .expect("spawn agent-doc process-exit watcher thread");
        Self { watched }
    }
}

impl Default for OsProcessExitWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessExitWatcher for OsProcessExitWatcher {
    fn watch(&self, pid: u32) {
        self.watched
            .lock()
            .expect("process-exit watcher lock")
            .insert(pid);
    }

    fn unwatch(&self, pid: u32) {
        self.watched
            .lock()
            .expect("process-exit watcher lock")
            .remove(&pid);
    }
}

/// Install the OS process-exit watcher on the process-global editor-attachment registry.
/// Idempotent-safe to call once at controller startup; after this the registry seeds its
/// reactive authority on attach and the hot path reads pure reactive state.
pub fn install_process_exit_watcher() {
    editor_attach().install_watcher(Arc::new(OsProcessExitWatcher::new()));
}

fn run_poll_loop(watched: Arc<Mutex<HashSet<u32>>>) {
    loop {
        thread::sleep(POLL_INTERVAL);
        let snapshot: Vec<u32> = {
            watched
                .lock()
                .expect("process-exit watcher lock")
                .iter()
                .copied()
                .collect()
        };
        for pid in snapshot {
            // Check liveness outside the lock (it may syscall).
            if !process_is_live(pid) {
                // OS-observed exit → drive the reactive `alive` cell closed. Every
                // document that pid owned recomputes to detached.
                editor_attach().process_exited(pid);
                // Sidecar-retirement Phase 3C: also write the reliable-sync
                // `Alive{false}` so the shadow liveness plane cascades the same
                // crash demote (no-op unless dual-run is on).
                crate::project_controller::record_reliable_sync_editor_exit(pid as u64);
                watched
                    .lock()
                    .expect("process-exit watcher lock")
                    .remove(&pid);
            }
        }
    }
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

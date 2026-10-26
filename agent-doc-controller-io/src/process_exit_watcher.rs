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
//! ## The polled set is derived, not tracked (`#ghosteditorliveness`)
//!
//! This watcher keeps **no private `watch()`ed set**. Every pid the reactive
//! `editor_attach` authority ever attaches is, by construction, one it read out of the
//! reliable-sync liveness plane's open-set (`mark_editor_attach_open` in
//! `agent-doc-crdt-relay-io` seeds `attach()` *from* `open_pids().filter(pid_alive)`). So
//! the plane's `all_open_pids().filter(pid_alive)` is always a superset of any tally a
//! private `watch()` set could hold — and, unlike a private set, it survives a controller
//! recycle (durable hydration repopulates it) and cannot drift under a register/deregister
//! storm. Deriving the poll candidates directly from that single plane projection is what
//! deletes the drift class: there is no second set to fall behind. `watch`/`unwatch` on this
//! OS impl are therefore inert (the trait still carries them for the SimWorld fake, which
//! records pids for its own assertions).
//!
//! The poller runs on a dedicated background thread owned by the project controller; the
//! authority gate never blocks on it. Tests never install this watcher — they install a
//! fake one and drive synthetic exit events (see `editor_attach`'s SimWorld tests).
//!
//! ## The poller lives exactly as long as its controller's serve loop (`#detachedexitci`)
//!
//! [`install_process_exit_watcher`] returns a [`ProcessExitWatcherLifetime`]; dropping it
//! stops the poller at once. The serve loop holds it until it returns. A production
//! process serves one controller and exits after `serve` returns, so nothing changes
//! there. A test binary serves hundreds of in-process controllers, and the poller used to
//! run until the process exited: every finished controller kept polling the process-global
//! liveness plane and wrote each dead pid it found, including another test's synthetic
//! editor pid, into its own project root. Those writes recreate `.agent-doc` under a root
//! its test is deleting, so `TempDir`'s removal fails with `ENOTEMPTY` and the root
//! survives.
//!
//! ## The poller reaps only its own root's editors (`#exitwatcherrootscope`)
//!
//! Stopping finished pollers is not enough: two live in-process controllers still share
//! the one process-global plane. Each reaped every dead pid in it and wrote the exit,
//! with retention under the dead editor's document hashes, into its own root's
//! `state.db`. One such write, from a controller reaping a sibling test's synthetic
//! editor, deleted that test's `state.db-journal` and failed its commit with
//! `SQLITE_IOERR_DELETE_NOENT`. A pid is now reaped only by the controller whose root is
//! the nearest project root of one of the pid's registered document paths. A pid with
//! no registration names no root, so no poller reaps it; controller hydration still
//! records the deaths of pids its own journal opened. Production is unchanged: a
//! production process serves one root, and editors always send `Register` with
//! `Open`, so every editor pid the plane holds there is registered under that root.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use agent_doc_document_realtime::editor_attach::{ProcessExitWatcher, editor_attach};

/// How often the poller re-checks each candidate pid. Bounds crash-detection latency; the
/// authority hot path stays reactive and never waits on this.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// The controller-owned OS process-exit watcher. Installed once on the process-global
/// [`editor_attach`] registry at controller startup. Stateless: the polled candidate set
/// is derived on each tick from the reliable-sync liveness plane, never from a private
/// tally (`#ghosteditorliveness`; see the module docs). The poller thread's lifetime is
/// owned separately by [`ProcessExitWatcherLifetime`].
pub struct OsProcessExitWatcher;

/// Owns one controller's exit-watcher poller thread (`#detachedexitci`).
///
/// Dropping it disconnects the poller's stop channel, which wakes the poller out of its
/// poll-interval wait and ends the thread before it reaps again. Drop does not join:
/// a reap already in progress finishes its one durable write, and the serve loop
/// must not wait on that.
#[must_use = "dropping the lifetime stops the exit-watcher poller immediately"]
pub struct ProcessExitWatcherLifetime {
    stop: Option<mpsc::Sender<()>>,
    poller: Option<thread::JoinHandle<()>>,
}

impl ProcessExitWatcherLifetime {
    /// Spawn the poller for `project_root`. It runs until the lifetime is dropped.
    fn start(project_root: PathBuf) -> Self {
        let (stop, stopped) = mpsc::channel();
        let poller = thread::Builder::new()
            .name("agent-doc-process-exit-watcher".to_string())
            .spawn(move || run_poll_loop(project_root, stopped))
            .expect("spawn agent-doc process-exit watcher thread");
        Self {
            stop: Some(stop),
            poller: Some(poller),
        }
    }

    /// Stop the poller and wait for its thread to end.
    #[cfg(test)]
    fn stop_and_join(mut self) -> thread::Result<()> {
        drop(self.stop.take());
        self.poller
            .take()
            .expect("the poller handle is taken only here")
            .join()
    }
}

impl Drop for ProcessExitWatcherLifetime {
    fn drop(&mut self) {
        drop(self.stop.take());
        drop(self.poller.take());
    }
}

impl ProcessExitWatcher for OsProcessExitWatcher {
    /// Inert: the poll candidates are derived from the liveness plane, which
    /// `attach()` has already seeded, so there is nothing to record here.
    fn watch(&self, _pid: u32) {}

    /// Inert: the liveness plane's `Close`/`Alive(false)` fact removes a pid from the
    /// derived candidate set on its own; there is no private set to prune.
    fn unwatch(&self, _pid: u32) {}
}

/// Install the OS process-exit watcher on the process-global editor-attachment registry.
/// Idempotent-safe to call once at controller startup; after this the registry seeds its
/// reactive authority on attach and the hot path reads pure reactive state.
///
/// The returned lifetime owns the poller thread. The serve loop holds it until it returns
/// (`#detachedexitci`).
pub fn install_process_exit_watcher(project_root: PathBuf) -> ProcessExitWatcherLifetime {
    let lifetime = ProcessExitWatcherLifetime::start(project_root);
    editor_attach().install_watcher(Arc::new(OsProcessExitWatcher));
    lifetime
}

fn run_poll_loop(project_root: PathBuf, stopped: mpsc::Receiver<()>) {
    loop {
        // The poll-interval wait doubles as the stop signal: dropping the
        // lifetime disconnects the channel and ends the poller at once.
        match stopped.recv_timeout(POLL_INTERVAL) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
        reap_exited_editors_once(&project_root, process_is_live);
    }
}

/// One poll tick for the controller that owns `project_root`: reap every
/// plane-open editor pid that OS liveness reports gone **and** that holds a
/// registration for a document under `project_root` (`#exitwatcherrootscope`).
///
/// The poll candidates are the liveness plane's own open-and-alive set — the
/// single authority. Because `editor_attach` only ever attaches pids it read
/// out of this plane (`mark_editor_attach_open` seeds `attach()` *from*
/// `open_pids().filter(pid_alive)`), the plane is always a superset of any
/// private `watch()` tally. It also survives a controller recycle (durable
/// hydration repopulates it) so a crashed editor whose terminal `Alive{false}`
/// was never published is still polled for death. `pid_alive` presumes alive
/// absent a death fact, so a stale ghost stays `pid_alive == true`, holding
/// `live_editors >= 1` and wedging every disk-authority resolve until this poll
/// reaps it (`#ghosteditorliveness`). Deriving from the plane instead of a
/// private set is what deletes that drift class.
///
/// The plane is process-global, though, and the reap writes into this root's
/// `state.db`. A test binary serves many in-process controllers against one
/// plane, and each used to reap every other test's dead editor pid and write
/// that exit, under the other test's document hash, into its own ledger. One
/// such write deleted a sibling connection's `state.db-journal` and failed its
/// commit with `SQLITE_IOERR_DELETE_NOENT`. So the candidates are narrowed by
/// the registration path, the one liveness fact that names a root. A pid this
/// root does not own is left alone; its own root's controller reaps it. A
/// production process serves one root, and its editors register only that
/// root's documents, so nothing it reaped before is skipped now.
pub(crate) fn reap_exited_editors_once(
    project_root: &Path,
    is_live: impl Fn(u32) -> bool,
) -> Vec<u32> {
    let candidates = plane_open_alive_pid_paths();
    let candidate_pids: HashSet<u32> = candidates.keys().copied().collect();
    let dead = pids_to_reap(&candidate_pids, is_live);
    let reaped = pids_owned_by_root(&dead, &candidates, |path| {
        document_path_belongs_to_root(path, project_root)
    });
    for &pid in &reaped {
        // OS-observed exit → drive the reactive `alive` cell closed. Every
        // document that pid owned recomputes to detached.
        editor_attach().process_exited(pid);
        // Sidecar-retirement Phase 3C: also write the reliable-sync
        // `Alive{false}` so the shadow liveness plane cascades the same
        // crash demote and the death fact is durably persisted (survives the
        // next controller hydration). This also drops the pid from the plane's
        // open-and-alive set, so the next tick no longer lists it as a candidate.
        crate::project_controller::record_reliable_sync_editor_exit(project_root, pid as u64);
    }
    reaped
}

/// Every pid the reliable-sync liveness plane still counts as open-and-alive,
/// across all documents, narrowed to the `u32` OS-pid width, with the paths of
/// the documents it holds registered. This is the plane's own view of "who
/// might be a live editor" — the exact set whose staleness produces the
/// `#ghosteditorliveness` wedge — and it is the watcher's **sole** source of
/// poll candidates: there is no private `watch()`ed set to fall behind it. A
/// plane `Pid` (u64) that does not fit `u32` cannot be a real OS pid; it is
/// dropped rather than reaped (conservative: never demote what we cannot prove
/// dead). The paths are copied out so the root-ownership check, which touches
/// the filesystem, runs after the plane lock is released.
fn plane_open_alive_pid_paths() -> BTreeMap<u32, Vec<String>> {
    let plane = agent_doc_reliable_sync_io::global_liveness_plane().lock();
    let projection = plane.projection();
    projection
        .all_open_pids()
        .into_iter()
        .filter(|pid| projection.pid_alive(*pid))
        .filter_map(|pid| {
            let os_pid = u32::try_from(pid).ok()?;
            let paths = projection
                .registered_document_paths_by_pid(pid)
                .into_values()
                .collect();
            Some((os_pid, paths))
        })
        .collect()
}

/// Pure ownership decision (`#exitwatcherrootscope`): of the `dead` pids, keep
/// those holding at least one registered document whose path `owned` accepts.
/// A pid with no registration names no root, so no controller claims it.
fn pids_owned_by_root(
    dead: &[u32],
    paths_by_pid: &BTreeMap<u32, Vec<String>>,
    owned: impl Fn(&str) -> bool,
) -> Vec<u32> {
    let mut owned_pids: Vec<u32> = dead
        .iter()
        .copied()
        .filter(|pid| {
            paths_by_pid
                .get(pid)
                .is_some_and(|paths| paths.iter().any(|path| owned(path)))
        })
        .collect();
    owned_pids.sort_unstable();
    owned_pids
}

/// Whether the document at `path` belongs to the project rooted at
/// `project_root`: its nearest `.agent-doc` ancestor is that root
/// (`#exitwatcherrootscope`). Nearest, not any: a nested project's documents
/// belong to the nested root, matching how editors route a document to its
/// controller. The comparison also accepts the two directories' canonical
/// forms, so a symlinked spelling of either side still matches.
pub(crate) fn document_path_belongs_to_root(path: &str, project_root: &Path) -> bool {
    let Some(nearest) = agent_doc_project_root_io::project_root_containing(Path::new(path)) else {
        return false;
    };
    nearest == project_root
        || matches!(
            (nearest.canonicalize(), project_root.canonicalize()),
            (Ok(left), Ok(right)) if left == right
        )
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

    /// `#exitwatcherrootscope`: a dead pid is claimed only by the root that
    /// holds one of its registered documents; a pid with no registration names
    /// no root and is claimed by none.
    #[test]
    fn pids_owned_by_root_claims_only_pids_registered_under_the_root() {
        let paths_by_pid = BTreeMap::from([
            (101, vec!["/root-a/plan.md".to_string()]),
            (202, vec!["/root-b/plan.md".to_string()]),
            (
                303,
                vec![
                    "/root-b/notes.md".to_string(),
                    "/root-a/notes.md".to_string(),
                ],
            ),
            (404, Vec::new()),
        ]);
        let under_a = |path: &str| path.starts_with("/root-a/");
        let owned = pids_owned_by_root(&[404, 303, 202, 101, 505], &paths_by_pid, under_a);
        assert_eq!(owned, vec![101, 303]);
    }

    #[test]
    fn document_path_belongs_only_to_its_nearest_root() {
        let outer = tempfile::tempdir().unwrap();
        let nested = outer.path().join("nested");
        std::fs::create_dir_all(outer.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let outer_doc = outer.path().join("plan.md");
        let nested_doc = nested.join("plan.md");
        let outer_doc = outer_doc.to_str().unwrap();
        let nested_doc = nested_doc.to_str().unwrap();
        assert!(document_path_belongs_to_root(outer_doc, outer.path()));
        assert!(!document_path_belongs_to_root(outer_doc, &nested));
        assert!(document_path_belongs_to_root(nested_doc, &nested));
        assert!(
            !document_path_belongs_to_root(nested_doc, outer.path()),
            "a nested project's document belongs to the nested root"
        );
        let stranger = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(stranger.path().join(".agent-doc")).unwrap();
        assert!(!document_path_belongs_to_root(outer_doc, stranger.path()));
    }

    #[test]
    fn os_watcher_holds_no_private_watched_set() {
        // `#ghosteditorliveness` guard: the OS watcher must derive its poll
        // candidates solely from the reliable-sync liveness plane, never from a
        // private `watch()`ed tally that can drift from the plane's open-set. A
        // zero-sized watcher is the structural proof that no such field crept back
        // in. If a future edit reintroduces a `HashSet`/`Mutex` tally as authority,
        // this size assertion fails and forces the derive-from-plane rule back.
        assert_eq!(
            std::mem::size_of::<OsProcessExitWatcher>(),
            0,
            "OsProcessExitWatcher must stay stateless; the poll candidate set is \
             derived from the liveness plane, not from a private watch() tally"
        );
    }

    /// `#detachedexitci`: a finished controller's poller must stop. It used to
    /// run until the process exited, writing other tests' dead editor pids
    /// into a root its own test was deleting.
    #[test]
    fn exit_watcher_poller_stops_when_its_controller_lifetime_ends() {
        let root = tempfile::tempdir().unwrap();
        let lifetime = ProcessExitWatcherLifetime::start(root.path().to_path_buf());
        // Disconnecting the stop channel wakes the poller out of its interval
        // wait, so this join returns without waiting on any clock. A poller
        // with no stop path would never return here.
        lifetime
            .stop_and_join()
            .expect("the exit-watcher poller exits cleanly when its lifetime ends");
    }
}
